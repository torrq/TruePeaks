use crate::decoder::{
    default_channel_layout, AudioStreamMetadata, ChunkedDecoder, DecodeError, SpeakerChannel,
    CHUNK_FRAMES,
};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::{AudioBufferRef, Channels, Signal};
use symphonia::core::codecs::{
     CodecParameters, Decoder, DecoderOptions, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC,
    CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24LE,
    CODEC_TYPE_PCM_S32BE, CODEC_TYPE_PCM_S32LE, CODEC_TYPE_PCM_U8,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct SymphoniaDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    metadata: AudioStreamMetadata,
    /// Leftover interleaved f64 samples when a decoded packet exceeds CHUNK_FRAMES
    overflow_interleaved: Vec<f64>,
    overflow_frame_offset: usize,
    eof: bool,
}

impl SymphoniaDecoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let lower = ext.to_ascii_lowercase();
            match lower.as_str() {
                "bwf" => {
                    hint.with_extension("wav");
                }
                "aif" | "aifc" => {
                    hint.with_extension("aiff");
                }
                "alac" => {
                    hint.with_extension("m4a");
                }
                "oga" => {
                    hint.with_extension("ogg");
                }
                other => {
                    hint.with_extension(other);
                }
            }
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| DecodeError::Format(format!("Unsupported container: {}", e)))?;

        let format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| DecodeError::Format("No default audio track found".to_string()))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| DecodeError::Format("Missing sample rate".to_string()))?;

        let channels = map_symphonia_channels(&codec_params);
        let is_lossless_integer = is_lossless_int_codec(&codec_params);
        let word_length = if is_lossless_integer {
            codec_params.bits_per_sample.or(codec_params.bits_per_coded_sample)
        } else {
            None
        };
        let total_frames = codec_params.n_frames;

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| DecodeError::Format(format!("Unsupported codec: {}", e)))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            metadata: AudioStreamMetadata {
                sample_rate,
                channels,
                word_length,
                is_lossless_integer,
                total_frames,
            },
            overflow_interleaved: Vec::new(),
            overflow_frame_offset: 0,
            eof: false,
        })
    }
}

fn is_lossless_int_codec(params: &CodecParameters) -> bool {
    matches!(
        params.codec,
        CODEC_TYPE_PCM_S16LE
            | CODEC_TYPE_PCM_S16BE
            | CODEC_TYPE_PCM_S24LE
            | CODEC_TYPE_PCM_S24BE
            | CODEC_TYPE_PCM_S32LE
            | CODEC_TYPE_PCM_S32BE
            | CODEC_TYPE_PCM_U8
            | CODEC_TYPE_FLAC
            | CODEC_TYPE_ALAC
    )
}

fn map_symphonia_channels(params: &CodecParameters) -> Vec<SpeakerChannel> {
    if let Some(ch_flags) = params.channels {
        let count = ch_flags.count();
        let has_rear = ch_flags.contains(Channels::REAR_LEFT) || ch_flags.contains(Channels::REAR_RIGHT);
        let has_side = ch_flags.contains(Channels::SIDE_LEFT) || ch_flags.contains(Channels::SIDE_RIGHT);
        let mut out = Vec::with_capacity(count);

        let ordered_checks: &[(Channels, SpeakerChannel)] = &[
            (Channels::FRONT_LEFT, SpeakerChannel::FrontLeft),
            (Channels::FRONT_RIGHT, SpeakerChannel::FrontRight),
            (Channels::FRONT_CENTRE, SpeakerChannel::FrontCenter),
            (Channels::LFE1, SpeakerChannel::Lfe),
            (
                Channels::REAR_LEFT,
                if has_rear && has_side {
                    SpeakerChannel::LeftRearSurround
                } else {
                    SpeakerChannel::LeftSurround
                },
            ),
            (
                Channels::REAR_RIGHT,
                if has_rear && has_side {
                    SpeakerChannel::RightRearSurround
                } else {
                    SpeakerChannel::RightSurround
                },
            ),
            (
                Channels::SIDE_LEFT,
                if has_rear && has_side {
                    SpeakerChannel::LeftSideSurround
                } else {
                    SpeakerChannel::LeftSurround
                },
            ),
            (
                Channels::SIDE_RIGHT,
                if has_rear && has_side {
                    SpeakerChannel::RightSideSurround
                } else {
                    SpeakerChannel::RightSurround
                },
            ),
        ];

        for &(flag, speaker) in ordered_checks {
            if ch_flags.contains(flag) {
                out.push(speaker);
            }
        }
        while out.len() < count {
            if count == 1 && out.is_empty() {
                out.push(SpeakerChannel::FrontCenter);
            } else {
                out.push(SpeakerChannel::Aux(out.len()));
            }
        }
        out
    } else {
        default_channel_layout(2)
    }
}

impl ChunkedDecoder for SymphoniaDecoder {
    fn metadata(&self) -> &AudioStreamMetadata {
        &self.metadata
    }

    fn next_chunk(
        &mut self,
        out_interleaved: &mut Vec<f64>,
        bit_mask: &mut u32,
    ) -> Result<usize, DecodeError> {
        let num_channels = self.metadata.channels.len().max(1);
        out_interleaved.clear();
        let target_samples = CHUNK_FRAMES * num_channels;

        // First drain any buffered overflow frames
        if self.overflow_frame_offset * num_channels < self.overflow_interleaved.len() {
            let start = self.overflow_frame_offset * num_channels;
            let avail = self.overflow_interleaved.len() - start;
            let take = avail.min(target_samples);
            out_interleaved.extend_from_slice(&self.overflow_interleaved[start..start + take]);
            self.overflow_frame_offset += take / num_channels;
            if self.overflow_frame_offset * num_channels >= self.overflow_interleaved.len() {
                self.overflow_interleaved.clear();
                self.overflow_frame_offset = 0;
            }
            if out_interleaved.len() >= target_samples {
                return Ok(out_interleaved.len() / num_channels);
            }
        }

        while !self.eof && out_interleaved.len() < target_samples {
            let packet = match self.format.next_packet() {
                Ok(pkt) => pkt,
                Err(SymphoniaError::IoError(ref err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = match self.decoder.decode(&packet) {
                Ok(buf) => buf,
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::IoError(ref err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.eof = true;
                    break;
                }
                Err(e) => return Err(DecodeError::Format(e.to_string())),
            };

            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }

            // Update channel count if header originally lacked channel info
            let actual_ch = decoded.spec().channels.count();
            if actual_ch != self.metadata.channels.len() && actual_ch > 0 {
                self.metadata.channels = default_channel_layout(actual_ch);
            }
            let ch_count = self.metadata.channels.len();

            // Convert packet frames into interleaved f64 while inspecting raw integer bits
            let mut pkt_samples = Vec::with_capacity(frames * ch_count);
            append_audio_buffer_ref(
                &decoded,
                ch_count,
                self.metadata.is_lossless_integer,
                &mut self.metadata.word_length,
                bit_mask,
                &mut pkt_samples,
            );

            let needed = target_samples - out_interleaved.len();
            if pkt_samples.len() <= needed {
                out_interleaved.extend_from_slice(&pkt_samples);
            } else {
                out_interleaved.extend_from_slice(&pkt_samples[..needed]);
                self.overflow_interleaved.clear();
                self.overflow_interleaved.extend_from_slice(&pkt_samples[needed..]);
                self.overflow_frame_offset = 0;
            }
        }

        let final_ch = self.metadata.channels.len().max(1);
        Ok(out_interleaved.len() / final_ch)
    }
}

fn append_audio_buffer_ref(
    decoded: &AudioBufferRef<'_>,
    ch_count: usize,
    is_lossless_int: bool,
    word_length: &mut Option<u32>,
    bit_mask: &mut u32,
    out: &mut Vec<f64>,
) {
    let frames = decoded.frames();
    match decoded {
        AudioBufferRef::U8(buf) => {
            if is_lossless_int && word_length.is_none() {
                *word_length = Some(8);
            }
            let scale = 1.0 / 128.0;
            for f in 0..frames {
                for c in 0..ch_count {
                    let s = buf.chan(c)[f];
                    let signed = (s as i16) - 128;
                    if is_lossless_int {
                        *bit_mask |= (signed as u32) << 24;
                    }
                    out.push((signed as f64) * scale);
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            if is_lossless_int && word_length.is_none() {
                *word_length = Some(16);
            }
            let scale = 1.0 / 32768.0;
            for f in 0..frames {
                for c in 0..ch_count {
                    let s = buf.chan(c)[f];
                    if is_lossless_int {
                        *bit_mask |= (s as i32 as u32) << 16;
                    }
                    out.push((s as f64) * scale);
                }
            }
        }
        AudioBufferRef::S24(buf) => {
            if is_lossless_int && word_length.is_none() {
                *word_length = Some(24);
            }
            let scale = 1.0 / 8388608.0;
            for f in 0..frames {
                for c in 0..ch_count {
                    let s = buf.chan(c)[f].inner();
                    if is_lossless_int {
                        *bit_mask |= (s as u32) << 8;
                    }
                    out.push((s as f64) * scale);
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            if is_lossless_int && word_length.is_none() {
                *word_length = Some(32);
            }
            let scale = 1.0 / 2147483648.0;
            for f in 0..frames {
                for c in 0..ch_count {
                    let s = buf.chan(c)[f];
                    if is_lossless_int {
                        *bit_mask |= s as u32;
                    }
                    out.push((s as f64) * scale);
                }
            }
        }
        AudioBufferRef::F32(buf) => {
            for f in 0..frames {
                for c in 0..ch_count {
                    out.push(buf.chan(c)[f] as f64);
                }
            }
        }
        AudioBufferRef::F64(buf) => {
            for f in 0..frames {
                for c in 0..ch_count {
                    out.push(buf.chan(c)[f]);
                }
            }
        }
        _ => {}
    }
}
