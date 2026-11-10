use crate::decoder::{
    channels_from_wave_mask, default_channel_layout, AudioStreamMetadata, ChunkedDecoder,
    DecodeError, CHUNK_FRAMES,
};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const W64_GUID_RIFF: [u8; 16] = [
    0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00,
];
pub const W64_GUID_WAVE: [u8; 16] = [
    0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
pub const W64_GUID_FMT: [u8; 16] = [
    0x66, 0x6D, 0x74, 0x20, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
pub const W64_GUID_DATA: [u8; 16] = [
    0x64, 0x61, 0x74, 0x61, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcmSampleType {
    Int,
    Float,
}

pub struct W64Rf64Decoder {
    reader: BufReader<File>,
    metadata: AudioStreamMetadata,
    sample_type: PcmSampleType,
    bytes_per_sample: usize,
    frames_remaining: u64,
    raw_buf: Vec<u8>,
}

impl W64Rf64Decoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let file = File::open(path)?;
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut reader = BufReader::with_capacity(64 * 1024, file);

        let mut magic16 = [0u8; 16];
        reader.read_exact(&mut magic16)?;

        if magic16 == W64_GUID_RIFF {
            Self::open_w64(reader, file_len)
        } else if &magic16[0..4] == b"RF64"
            || &magic16[0..4] == b"BW64"
            || &magic16[0..4] == b"RIFF"
        {
            Self::open_rf64_or_riff(reader, magic16, file_len)
        } else {
            Err(DecodeError::Format(
                "Unrecognized Wave64 or RF64/RIFF header".to_string(),
            ))
        }
    }

    fn open_w64(mut reader: BufReader<File>, file_len: u64) -> Result<Self, DecodeError> {
        // After 16-byte RIFF GUID: 8-byte file size + 16-byte WAVE GUID = 24 bytes
        let mut header24 = [0u8; 24];
        reader.read_exact(&mut header24)?;
        let wave_guid = &header24[8..24];
        if wave_guid != W64_GUID_WAVE {
            return Err(DecodeError::Format("Invalid W64 WAVE GUID".to_string()));
        }

        let mut fmt_parsed: Option<(AudioStreamMetadata, PcmSampleType, usize)> = None;
        let mut data_offset_and_size: Option<(u64, u64)> = None;

        loop {
            let chunk_start = reader.stream_position()?;
            let mut chdr = [0u8; 24];
            if reader.read_exact(&mut chdr).is_err() {
                break;
            }
            let guid: [u8; 16] = chdr[0..16].try_into().unwrap();
            let chunk_total_size = u64::from_le_bytes(chdr[16..24].try_into().unwrap());
            if chunk_total_size < 24 {
                break;
            }
            let payload_size = chunk_total_size - 24;

            if guid == W64_GUID_FMT {
                let mut fmt_bytes = vec![0u8; payload_size as usize];
                reader.read_exact(&mut fmt_bytes)?;
                fmt_parsed = Some(parse_wave_fmt(&fmt_bytes)?);
            } else if guid == W64_GUID_DATA {
                let data_pos = reader.stream_position()?;
                data_offset_and_size = Some((data_pos, payload_size));
                break;
            }

            // Advance to next 8-byte aligned chunk
            let aligned_size = (chunk_total_size + 7) & !7;
            let next_pos = chunk_start.saturating_add(aligned_size);
            if file_len > 0 && next_pos >= file_len {
                break;
            }
            reader.seek(SeekFrom::Start(next_pos))?;
        }

        let (mut metadata, sample_type, bytes_per_sample) = fmt_parsed
            .ok_or_else(|| DecodeError::Format("Missing fmt chunk in W64 file".to_string()))?;
        let (data_pos, data_size) = data_offset_and_size
            .ok_or_else(|| DecodeError::Format("Missing data chunk in W64 file".to_string()))?;

        reader.seek(SeekFrom::Start(data_pos))?;
        let frame_bytes = (bytes_per_sample * metadata.channels.len()).max(1) as u64;
        let total_frames = data_size / frame_bytes;
        metadata.total_frames = Some(total_frames);

        Ok(Self {
            reader,
            metadata,
            sample_type,
            bytes_per_sample,
            frames_remaining: total_frames,
            raw_buf: Vec::new(),
        })
    }

    fn open_rf64_or_riff(
        mut reader: BufReader<File>,
        first16: [u8; 16],
        file_len: u64,
    ) -> Result<Self, DecodeError> {
        if &first16[8..12] != b"WAVE" {
            return Err(DecodeError::Format(
                "Missing WAVE signature in RF64/RIFF header".to_string(),
            ));
        }

        // Seek right after "RIFF"/"RF64" + size(4) + "WAVE"(4) = byte 12
        reader.seek(SeekFrom::Start(12))?;

        let mut ds64_data_size: Option<u64> = None;
        let mut fmt_parsed: Option<(AudioStreamMetadata, PcmSampleType, usize)> = None;
        let mut data_offset_and_size: Option<(u64, u64)> = None;

        loop {
            let mut chdr = [0u8; 8];
            if reader.read_exact(&mut chdr).is_err() {
                break;
            }
            let chunk_id = &chdr[0..4];
            let chunk_size32 = u32::from_le_bytes(chdr[4..8].try_into().unwrap()) as u64;
            let payload_start = reader.stream_position()?;

            if chunk_id == b"ds64" || chunk_id == b"JUNK" && ds64_data_size.is_none() {
                if chunk_id == b"ds64" && chunk_size32 >= 28 {
                    let mut ds64_buf = vec![0u8; chunk_size32 as usize];
                    reader.read_exact(&mut ds64_buf)?;
                    let data_sz = u64::from_le_bytes(ds64_buf[8..16].try_into().unwrap());
                    ds64_data_size = Some(data_sz);
                }
            } else if chunk_id == b"fmt " {
                let mut fmt_bytes = vec![0u8; chunk_size32 as usize];
                reader.read_exact(&mut fmt_bytes)?;
                fmt_parsed = Some(parse_wave_fmt(&fmt_bytes)?);
            } else if chunk_id == b"data" {
                let actual_data_size = if chunk_size32 == 0xFFFF_FFFF {
                    ds64_data_size.unwrap_or_else(|| file_len.saturating_sub(payload_start))
                } else if let Some(ds_sz) = ds64_data_size {
                    if ds_sz > 0 {
                        ds_sz
                    } else {
                        chunk_size32
                    }
                } else {
                    chunk_size32
                };
                data_offset_and_size = Some((payload_start, actual_data_size));
                break;
            }

            // Advance to next 2-byte word aligned chunk
            let padded = chunk_size32 + (chunk_size32 & 1);
            let next_pos = payload_start.saturating_add(padded);
            if file_len > 0 && next_pos >= file_len {
                break;
            }
            reader.seek(SeekFrom::Start(next_pos))?;
        }

        let (mut metadata, sample_type, bytes_per_sample) = fmt_parsed
            .ok_or_else(|| DecodeError::Format("Missing fmt chunk in RF64/WAV file".to_string()))?;
        let (data_pos, data_size) = data_offset_and_size.ok_or_else(|| {
            DecodeError::Format("Missing data chunk in RF64/WAV file".to_string())
        })?;

        reader.seek(SeekFrom::Start(data_pos))?;
        let frame_bytes = (bytes_per_sample * metadata.channels.len()).max(1) as u64;
        let total_frames = data_size / frame_bytes;
        metadata.total_frames = Some(total_frames);

        Ok(Self {
            reader,
            metadata,
            sample_type,
            bytes_per_sample,
            frames_remaining: total_frames,
            raw_buf: Vec::new(),
        })
    }
}

fn parse_wave_fmt(
    fmt_bytes: &[u8],
) -> Result<(AudioStreamMetadata, PcmSampleType, usize), DecodeError> {
    if fmt_bytes.len() < 16 {
        return Err(DecodeError::Format("fmt chunk too short".to_string()));
    }
    let format_tag = u16::from_le_bytes([fmt_bytes[0], fmt_bytes[1]]);
    let num_channels = u16::from_le_bytes([fmt_bytes[2], fmt_bytes[3]]) as usize;
    let sample_rate = u32::from_le_bytes(fmt_bytes[4..8].try_into().unwrap());
    let _block_align = u16::from_le_bytes([fmt_bytes[12], fmt_bytes[13]]) as usize;
    let bits_per_sample = u16::from_le_bytes([fmt_bytes[14], fmt_bytes[15]]) as u32;

    if num_channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
        return Err(DecodeError::Format(
            "Invalid channel count, sample rate, or bit depth in fmt chunk".to_string(),
        ));
    }

    let mut effective_format = format_tag;
    let mut valid_bits = bits_per_sample;
    let mut channel_mask = 0u32;

    if format_tag == 0xFFFE && fmt_bytes.len() >= 40 {
        let vbits = u16::from_le_bytes([fmt_bytes[18], fmt_bytes[19]]) as u32;
        if vbits > 0 {
            valid_bits = vbits;
        }
        channel_mask = u32::from_le_bytes(fmt_bytes[20..24].try_into().unwrap());
        effective_format = u16::from_le_bytes([fmt_bytes[24], fmt_bytes[25]]);
    }

    let sample_type = match effective_format {
        1 => PcmSampleType::Int,
        3 => PcmSampleType::Float,
        other => {
            return Err(DecodeError::Format(format!(
                "Unsupported WAVE format tag: 0x{:04X}",
                other
            )))
        }
    };

    let bytes_per_sample = ((bits_per_sample + 7) / 8) as usize;
    let channels = if channel_mask != 0 {
        channels_from_wave_mask(channel_mask, num_channels)
    } else {
        default_channel_layout(num_channels)
    };

    let is_lossless_integer = sample_type == PcmSampleType::Int;
    let word_length = if is_lossless_integer {
        Some(valid_bits)
    } else {
        None
    };

    Ok((
        AudioStreamMetadata {
            sample_rate,
            channels,
            word_length,
            is_lossless_integer,
            total_frames: None,
        },
        sample_type,
        bytes_per_sample,
    ))
}

impl ChunkedDecoder for W64Rf64Decoder {
    fn metadata(&self) -> &AudioStreamMetadata {
        &self.metadata
    }

    fn next_chunk(
        &mut self,
        out_interleaved: &mut Vec<f64>,
        bit_mask: &mut u32,
    ) -> Result<usize, DecodeError> {
        out_interleaved.clear();
        if self.frames_remaining == 0 {
            return Ok(0);
        }

        let ch_count = self.metadata.channels.len().max(1);
        let frames_to_read = (CHUNK_FRAMES as u64).min(self.frames_remaining) as usize;
        let frame_stride = self.bytes_per_sample * ch_count;
        let bytes_to_read = frames_to_read * frame_stride;

        self.raw_buf.resize(bytes_to_read, 0);
        let mut total_read = 0usize;
        while total_read < bytes_to_read {
            match self.reader.read(&mut self.raw_buf[total_read..]) {
                Ok(0) => break,
                Ok(n) => total_read += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(DecodeError::Io(e)),
            }
        }

        let actual_frames = total_read / frame_stride;
        self.frames_remaining = self.frames_remaining.saturating_sub(actual_frames as u64);
        if actual_frames == 0 {
            self.frames_remaining = 0;
            return Ok(0);
        }

        let total_samples = actual_frames * ch_count;
        out_interleaved.reserve(total_samples);

        match (self.sample_type, self.bytes_per_sample) {
            (PcmSampleType::Int, 1) => {
                let scale = 1.0 / 128.0;
                for chunk in self.raw_buf[..actual_frames * frame_stride].iter() {
                    let signed = (*chunk as i16) - 128;
                    *bit_mask |= (signed as u32) << 24;
                    out_interleaved.push((signed as f64) * scale);
                }
            }
            (PcmSampleType::Int, 2) => {
                let scale = 1.0 / 32768.0;
                for b in self.raw_buf[..actual_frames * frame_stride].chunks_exact(2) {
                    let s = i16::from_le_bytes([b[0], b[1]]);
                    *bit_mask |= (s as i32 as u32) << 16;
                    out_interleaved.push((s as f64) * scale);
                }
            }
            (PcmSampleType::Int, 3) => {
                let scale = 1.0 / 8388608.0;
                for b in self.raw_buf[..actual_frames * frame_stride].chunks_exact(3) {
                    let s32_msb =
                        ((b[0] as i32) << 8) | ((b[1] as i32) << 16) | ((b[2] as i32) << 24);
                    *bit_mask |= s32_msb as u32;
                    let s24 = s32_msb >> 8;
                    out_interleaved.push((s24 as f64) * scale);
                }
            }
            (PcmSampleType::Int, 4) => {
                let scale = 1.0 / 2147483648.0;
                for b in self.raw_buf[..actual_frames * frame_stride].chunks_exact(4) {
                    let s = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    *bit_mask |= s as u32;
                    out_interleaved.push((s as f64) * scale);
                }
            }
            (PcmSampleType::Float, 4) => {
                for b in self.raw_buf[..actual_frames * frame_stride].chunks_exact(4) {
                    let s = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    out_interleaved.push(s as f64);
                }
            }
            (PcmSampleType::Float, 8) => {
                for b in self.raw_buf[..actual_frames * frame_stride].chunks_exact(8) {
                    let s = f64::from_le_bytes(b.try_into().unwrap());
                    out_interleaved.push(s);
                }
            }
            _ => {
                return Err(DecodeError::Format(format!(
                    "Unsupported sample width: {} bytes",
                    self.bytes_per_sample
                )));
            }
        }

        Ok(actual_frames)
    }
}
