pub mod symphonia_dec;
pub mod w64_rf64_dec;

use ebur128::Channel as EbuChannel;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const CHUNK_FRAMES: usize = 8192;

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "wav", "bwf", "aiff", "aif", "aifc", "w64", "flac", "m4a", "alac", "mp4", "caf", "mp3", "aac",
    "ogg", "oga",
];

pub fn is_supported_audio_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerChannel {
    FrontLeft,
    FrontRight,
    FrontCenter,
    Lfe,
    LeftSurround,
    RightSurround,
    LeftRearSurround,
    RightRearSurround,
    LeftSideSurround,
    RightSideSurround,
    Aux(usize),
}

impl SpeakerChannel {
    /// Short label used in multichannel (>2ch) tables and layout descriptions
    pub fn short_label(&self) -> String {
        match self {
            SpeakerChannel::FrontLeft => "L".to_string(),
            SpeakerChannel::FrontRight => "R".to_string(),
            SpeakerChannel::FrontCenter => "C".to_string(),
            SpeakerChannel::Lfe => "LFE".to_string(),
            SpeakerChannel::LeftSurround => "Ls".to_string(),
            SpeakerChannel::RightSurround => "Rs".to_string(),
            SpeakerChannel::LeftRearSurround => "Lrs".to_string(),
            SpeakerChannel::RightRearSurround => "Rrs".to_string(),
            SpeakerChannel::LeftSideSurround => "Lss".to_string(),
            SpeakerChannel::RightSideSurround => "Rss".to_string(),
            SpeakerChannel::Aux(idx) => format!("Ch{}", idx + 1),
        }
    }

    /// Single-file log column header ("Left" / "Right" for Stereo, short label for Mono / Surround)
    pub fn single_log_header(&self, total_channels: usize) -> String {
        if total_channels == 2 {
            match self {
                SpeakerChannel::FrontLeft => "Left".to_string(),
                SpeakerChannel::FrontRight => "Right".to_string(),
                other => other.short_label(),
            }
        } else {
            self.short_label()
        }
    }

    /// ITU-R BS.1770-4 channel weighting mapping for `ebur128`
    pub fn ebur128_channel(&self) -> EbuChannel {
        match self {
            SpeakerChannel::FrontLeft => EbuChannel::Left,
            SpeakerChannel::FrontRight => EbuChannel::Right,
            SpeakerChannel::FrontCenter => EbuChannel::Center,
            SpeakerChannel::Lfe => EbuChannel::Unused,
            SpeakerChannel::LeftSurround
            | SpeakerChannel::LeftRearSurround
            | SpeakerChannel::LeftSideSurround => EbuChannel::LeftSurround,
            SpeakerChannel::RightSurround
            | SpeakerChannel::RightRearSurround
            | SpeakerChannel::RightSideSurround => EbuChannel::RightSurround,
            SpeakerChannel::Aux(_) => EbuChannel::Left,
        }
    }

    pub fn is_lfe(&self) -> bool {
        matches!(self, SpeakerChannel::Lfe)
    }

    /// ITU-R BS.1770-4 linear power weight (matching `ebur128`: +1.5 dB ≈ 1.41 on surround channels, 0.0 on LFE)
    pub fn itu_bs1770_weight(&self) -> f64 {
        match self {
            SpeakerChannel::Lfe => 0.0,
            SpeakerChannel::LeftSurround
            | SpeakerChannel::RightSurround
            | SpeakerChannel::LeftRearSurround
            | SpeakerChannel::RightRearSurround
            | SpeakerChannel::LeftSideSurround
            | SpeakerChannel::RightSideSurround => 1.41,
            _ => 1.0,
        }
    }
}

pub fn default_channel_layout(count: usize) -> Vec<SpeakerChannel> {
    match count {
        1 => vec![SpeakerChannel::FrontCenter],
        2 => vec![SpeakerChannel::FrontLeft, SpeakerChannel::FrontRight],
        3 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::FrontCenter,
        ],
        4 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::LeftSurround,
            SpeakerChannel::RightSurround,
        ],
        5 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::FrontCenter,
            SpeakerChannel::LeftSurround,
            SpeakerChannel::RightSurround,
        ],
        6 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::FrontCenter,
            SpeakerChannel::Lfe,
            SpeakerChannel::LeftSurround,
            SpeakerChannel::RightSurround,
        ],
        7 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::FrontCenter,
            SpeakerChannel::Lfe,
            SpeakerChannel::LeftRearSurround,
            SpeakerChannel::RightRearSurround,
            SpeakerChannel::FrontCenter,
        ],
        8 => vec![
            SpeakerChannel::FrontLeft,
            SpeakerChannel::FrontRight,
            SpeakerChannel::FrontCenter,
            SpeakerChannel::Lfe,
            SpeakerChannel::LeftRearSurround,
            SpeakerChannel::RightRearSurround,
            SpeakerChannel::LeftSideSurround,
            SpeakerChannel::RightSideSurround,
        ],
        n => (0..n).map(SpeakerChannel::Aux).collect(),
    }
}

pub fn channels_from_wave_mask(mask: u32, count: usize) -> Vec<SpeakerChannel> {
    if mask == 0 {
        return default_channel_layout(count);
    }
    let mut out = Vec::with_capacity(count);
    // WAVEFORMATEXTENSIBLE dwChannelMask bit positions:
    // 0x1: FL, 0x2: FR, 0x4: FC, 0x8: LFE, 0x10: BL, 0x20: BR,
    // 0x40: FLC, 0x80: FRC, 0x100: BC, 0x200: SL, 0x400: SR
    let has_back = (mask & 0x30) != 0;
    let has_side = (mask & 0x600) != 0;

    for bit in 0..32u32 {
        let flag = 1u32 << bit;
        if (mask & flag) != 0 {
            let ch = match flag {
                0x1 => SpeakerChannel::FrontLeft,
                0x2 => SpeakerChannel::FrontRight,
                0x4 => SpeakerChannel::FrontCenter,
                0x8 => SpeakerChannel::Lfe,
                0x10 => {
                    if has_back && has_side {
                        SpeakerChannel::LeftRearSurround
                    } else {
                        SpeakerChannel::LeftSurround
                    }
                }
                0x20 => {
                    if has_back && has_side {
                        SpeakerChannel::RightRearSurround
                    } else {
                        SpeakerChannel::RightSurround
                    }
                }
                0x200 => {
                    if has_back && has_side {
                        SpeakerChannel::LeftSideSurround
                    } else {
                        SpeakerChannel::LeftSurround
                    }
                }
                0x400 => {
                    if has_back && has_side {
                        SpeakerChannel::RightSideSurround
                    } else {
                        SpeakerChannel::RightSurround
                    }
                }
                _ => SpeakerChannel::Aux(out.len()),
            };
            out.push(ch);
            if out.len() == count {
                break;
            }
        }
    }
    if out.len() != count {
        default_channel_layout(count)
    } else {
        out
    }
}

pub fn layout_description(channels: &[SpeakerChannel]) -> String {
    let labels: Vec<String> = channels.iter().map(|c| c.short_label()).collect();
    let prefix = match channels.len() {
        1 => "1.0",
        2 => "2.0",
        3 => "3.0",
        4 => "4.0",
        5 => "5.0",
        6 => {
            if channels.iter().any(|c| c.is_lfe()) {
                "5.1"
            } else {
                "6.0"
            }
        }
        8 => {
            if channels.iter().any(|c| c.is_lfe()) {
                "7.1"
            } else {
                "8.0"
            }
        }
        n => return format!("{}ch ({})", n, labels.join(", ")),
    };
    format!("{} ({})", prefix, labels.join(", "))
}

#[derive(Debug, Clone)]
pub struct AudioStreamMetadata {
    pub sample_rate: u32,
    pub channels: Vec<SpeakerChannel>,
    pub word_length: Option<u32>,
    pub is_lossless_integer: bool,
    pub total_frames: Option<u64>,
}

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Format(String),
    Cancelled,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "I/O error: {}", e),
            DecodeError::Format(s) => write!(f, "Format error: {}", s),
            DecodeError::Cancelled => write!(f, "Scan cancelled"),
        }
    }
}

impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        DecodeError::Io(e)
    }
}

pub trait ChunkedDecoder {
    fn metadata(&self) -> &AudioStreamMetadata;
    /// Reads up to `CHUNK_FRAMES` (8192) frames into `out_interleaved` (length = frames * channels),
    /// accumulating raw MSB-aligned integer bit-masks into `bit_mask`.
    /// Returns the number of frames read (`0` on EOF).
    fn next_chunk(
        &mut self,
        out_interleaved: &mut Vec<f64>,
        bit_mask: &mut u32,
    ) -> Result<usize, DecodeError>;
}

pub fn open_audio_file(path: &Path) -> Result<Box<dyn ChunkedDecoder>, DecodeError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let mut header = [0u8; 16];
    if let Ok(mut f) = File::open(path) {
        let _ = f.read(&mut header);
    }

    let is_rf64 = &header[0..4] == b"RF64" || &header[0..4] == b"BW64";
    let is_w64 = header == w64_rf64_dec::W64_GUID_RIFF || ext == "w64";

    if is_rf64 || is_w64 {
        return Ok(Box::new(w64_rf64_dec::W64Rf64Decoder::open(path)?));
    }

    match symphonia_dec::SymphoniaDecoder::open(path) {
        Ok(dec) => Ok(Box::new(dec)),
        Err(e) => {
            if ext == "wav" || ext == "bwf" {
                if let Ok(dec) = w64_rf64_dec::W64Rf64Decoder::open(path) {
                    return Ok(Box::new(dec));
                }
            }
            Err(e)
        }
    }
}
