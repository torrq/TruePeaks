use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharsetMode {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl CharsetMode {
    pub fn label(&self) -> &'static str {
        match self {
            CharsetMode::Utf8 => "UTF-8 (Default)",
            CharsetMode::Utf8Bom => "UTF-8 with BOM",
            CharsetMode::Utf16Le => "UTF-16 LE",
            CharsetMode::Utf16Be => "UTF-16 BE",
            CharsetMode::Windows1252 => "Windows-1252",
        }
    }

    pub fn all() -> &'static [CharsetMode] {
        &[
            CharsetMode::Utf8,
            CharsetMode::Utf8Bom,
            CharsetMode::Utf16Le,
            CharsetMode::Utf16Be,
            CharsetMode::Windows1252,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEndingMode {
    Crlf,
    Lf,
    Cr,
}

impl LineEndingMode {
    pub fn label(&self) -> &'static str {
        match self {
            LineEndingMode::Crlf => "Windows (CRLF - \\r\\n)",
            LineEndingMode::Lf => "Unix / macOS (LF - \\n)",
            LineEndingMode::Cr => "Classic Mac (CR - \\r)",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LineEndingMode::Crlf => "\r\n",
            LineEndingMode::Lf => "\n",
            LineEndingMode::Cr => "\r",
        }
    }

    pub fn all() -> &'static [LineEndingMode] {
        &[LineEndingMode::Crlf, LineEndingMode::Lf, LineEndingMode::Cr]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurroundFolderMode {
    ExpandPerChannel,
    StrictTwoChannel,
}

impl SurroundFolderMode {
    pub fn label(&self) -> &'static str {
        match self {
            SurroundFolderMode::ExpandPerChannel => {
                "Expand Per-Channel Columns (Default for Surround Folders)"
            }
            SurroundFolderMode::StrictTwoChannel => "Strict 2-Channel (Front L/R Only)",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderLogStyle {
    #[default]
    TruePeaksTable,
    ClassicFooDr,
    CompactMastering,
    Json,
}

impl FolderLogStyle {
    pub fn label(&self) -> &'static str {
        match self {
            FolderLogStyle::TruePeaksTable => "TruePeaks Table (Default)",
            FolderLogStyle::ClassicFooDr => "Classic DR (foo_dr Style)",
            FolderLogStyle::CompactMastering => "Compact Mastering",
            FolderLogStyle::Json => "JSON",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            FolderLogStyle::TruePeaksTable => {
                "Pipe-delimited multi-column spreadsheet table with customizable column selection and order."
            }
            FolderLogStyle::ClassicFooDr => {
                "Classic borderless DR / Peak / RMS / Duration layout with album codec summary and customizable columns."
            }
            FolderLogStyle::CompactMastering => {
                "Concise album summary header with tight space-aligned mastering columns and customizable fields."
            }
            FolderLogStyle::Json => {
                "Machine-readable structured JSON report with album metadata and customizable per-track metrics for scripts and automation."
            }
        }
    }

    pub fn all() -> &'static [FolderLogStyle] {
        &[
            FolderLogStyle::TruePeaksTable,
            FolderLogStyle::ClassicFooDr,
            FolderLogStyle::CompactMastering,
            FolderLogStyle::Json,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FolderColumnId {
    // Group 1: Info
    FileName,
    Duration,
    FileFormat,
    SampleRate,
    WordLength,
    BitsUsed,

    // Group 2: DR
    DrPmf,
    DrLeft,
    DrRight,

    // Group 3: True Peak Level
    MaxTplItu,
    MaxTplLeft,
    MaxTplRight,

    // Group 4: LUFS
    Lufsi,
    Lra,
    MaxMomItu,
    MaxShortTermItu,

    // Group 5: Channel Specific LUFS
    MaxMomLeft,
    MaxMomRight,
    MaxShortTermLeft,
    MaxShortTermRight,

    // Group 6: Old Skool
    MaxSppmJoint,
    MaxSppmLeft,
    MaxSppmRight,
    RmsJoint,
    RmsLeft,
    RmsRight,

    // Group 7: Ratios & Diagnostics
    Plr,
    MinPsr,
    ReplayGainTrack,
    StereoCorrelation,
    TplOversample,
}

impl FolderColumnId {
    pub const ALL: [FolderColumnId; 31] = [
        FolderColumnId::FileName,
        FolderColumnId::Duration,
        FolderColumnId::FileFormat,
        FolderColumnId::SampleRate,
        FolderColumnId::WordLength,
        FolderColumnId::BitsUsed,
        FolderColumnId::DrPmf,
        FolderColumnId::DrLeft,
        FolderColumnId::DrRight,
        FolderColumnId::MaxTplItu,
        FolderColumnId::MaxTplLeft,
        FolderColumnId::MaxTplRight,
        FolderColumnId::Lufsi,
        FolderColumnId::Lra,
        FolderColumnId::MaxMomItu,
        FolderColumnId::MaxShortTermItu,
        FolderColumnId::MaxMomLeft,
        FolderColumnId::MaxMomRight,
        FolderColumnId::MaxShortTermLeft,
        FolderColumnId::MaxShortTermRight,
        FolderColumnId::MaxSppmJoint,
        FolderColumnId::MaxSppmLeft,
        FolderColumnId::MaxSppmRight,
        FolderColumnId::RmsJoint,
        FolderColumnId::RmsLeft,
        FolderColumnId::RmsRight,
        FolderColumnId::Plr,
        FolderColumnId::MinPsr,
        FolderColumnId::ReplayGainTrack,
        FolderColumnId::StereoCorrelation,
        FolderColumnId::TplOversample,
    ];

    pub fn default_order() -> Vec<FolderColumnId> {
        Self::ALL.to_vec()
    }

    pub fn order_with_front(front: &[FolderColumnId]) -> Vec<FolderColumnId> {
        let mut out = Vec::with_capacity(Self::ALL.len());
        for &c in front {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        for c in Self::ALL {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    pub fn label(self) -> &'static str {
        match self {
            FolderColumnId::FileName => "File Name",
            FolderColumnId::Duration => "Duration",
            FolderColumnId::FileFormat => "File Format",
            FolderColumnId::SampleRate => "Sample Rate",
            FolderColumnId::WordLength => "Word Length",
            FolderColumnId::BitsUsed => "Bits Used",
            FolderColumnId::DrPmf => "DR (PMF)",
            FolderColumnId::DrLeft => "DR LEFT",
            FolderColumnId::DrRight => "DR RIGHT",
            FolderColumnId::MaxTplItu => "Max. TPL (ITU)",
            FolderColumnId::MaxTplLeft => "Max. TPL LEFT",
            FolderColumnId::MaxTplRight => "Max. TPL RIGHT",
            FolderColumnId::Lufsi => "Integrated Loudness (ITU)",
            FolderColumnId::Lra => "LRA (ITU)",
            FolderColumnId::MaxMomItu => "Max. Mom. (ITU)",
            FolderColumnId::MaxShortTermItu => "Max. Short-Term (ITU)",
            FolderColumnId::MaxMomLeft => "Max. Mom. LEFT",
            FolderColumnId::MaxMomRight => "Max. Mom. RIGHT",
            FolderColumnId::MaxShortTermLeft => "Max. Short-Term LEFT",
            FolderColumnId::MaxShortTermRight => "Max. Short-Term RIGHT",
            FolderColumnId::MaxSppmJoint => "Max. SPPM (JOINT)",
            FolderColumnId::MaxSppmLeft => "Max. SPPM LEFT",
            FolderColumnId::MaxSppmRight => "Max. SPPM RIGHT",
            FolderColumnId::RmsJoint => "RMS (JOINT)",
            FolderColumnId::RmsLeft => "RMS LEFT",
            FolderColumnId::RmsRight => "RMS RIGHT",
            FolderColumnId::Plr => "PLR (AES)",
            FolderColumnId::MinPsr => "Min. PSR (AES)",
            FolderColumnId::ReplayGainTrack => "ReplayGain (-18)",
            FolderColumnId::StereoCorrelation => "Stereo Corr.",
            FolderColumnId::TplOversample => "TPL Oversample",
        }
    }

    pub fn short_chip_label(self) -> &'static str {
        match self {
            FolderColumnId::Lufsi => "LUFSi (ITU)",
            other => other.label(),
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            FolderColumnId::FileName => "Source audio filename stem (or embedded track title when tags are enabled).",
            FolderColumnId::Duration => "Track duration formatted as M:SS (or H:MM:SS).",
            FolderColumnId::FileFormat => "Audio container/codec extension (for example .flac, .wav, .mp3).",
            FolderColumnId::SampleRate => "Sample rate in kHz (for example 44.1k, 48k, 96k, 192k).",
            FolderColumnId::WordLength => "Nominal container bit depth (for example 16 or 24 bit PCM).",
            FolderColumnId::BitsUsed => "Effective active PCM resolution scanned across all samples (reveals 16-bit audio padded inside a 24-bit file).",
            FolderColumnId::DrPmf => "Rounded integer Pleasurize Music Foundation DR score averaged across active channels.",
            FolderColumnId::DrLeft => "Unrounded left-channel PMF DR (2nd-highest 3 s block peak minus top-20% RMS).",
            FolderColumnId::DrRight => "Unrounded right-channel PMF DR (2nd-highest 3 s block peak minus top-20% RMS).",
            FolderColumnId::MaxTplItu => "Highest oversampled ITU-R BS.1770-4 True Peak across all channels (dBTP).",
            FolderColumnId::MaxTplLeft => "Left-channel oversampled ITU-R BS.1770-4 True Peak (dBTP).",
            FolderColumnId::MaxTplRight => "Right-channel oversampled ITU-R BS.1770-4 True Peak (dBTP).",
            FolderColumnId::Lufsi => "Gated whole-track Integrated Loudness in LUFS per ITU-R BS.1770-4 / EBU R128.",
            FolderColumnId::Lra => "Loudness Range in LU measuring macro-dynamic spread across gated Short-Term windows.",
            FolderColumnId::MaxMomItu => "Peak continuous sample-sliding 400 ms Momentary Loudness across joint weighted channels.",
            FolderColumnId::MaxShortTermItu => "Peak continuous sample-sliding 3.0 s Short-Term Loudness across joint weighted channels.",
            FolderColumnId::MaxMomLeft => "Left-channel continuous 400 ms Momentary peak calibrated with +3.01 dB dual-mono convention.",
            FolderColumnId::MaxMomRight => "Right-channel continuous 400 ms Momentary peak calibrated with +3.01 dB dual-mono convention.",
            FolderColumnId::MaxShortTermLeft => "Left-channel continuous 3.0 s Short-Term peak calibrated with +3.01 dB dual-mono convention.",
            FolderColumnId::MaxShortTermRight => "Right-channel continuous 3.0 s Short-Term peak calibrated with +3.01 dB dual-mono convention.",
            FolderColumnId::MaxSppmJoint => "Highest raw digital Sample Peak Program level across channels without oversampling (dBFS).",
            FolderColumnId::MaxSppmLeft => "Left-channel raw digital Sample Peak magnitude (dBFS).",
            FolderColumnId::MaxSppmRight => "Right-channel raw digital Sample Peak magnitude (dBFS).",
            FolderColumnId::RmsJoint => "Combined multi-channel AES17 sine-normalized RMS power level (dBFS).",
            FolderColumnId::RmsLeft => "Left-channel AES17 sine-normalized RMS across the track (dBFS).",
            FolderColumnId::RmsRight => "Right-channel AES17 sine-normalized RMS across the track (dBFS).",
            FolderColumnId::Plr => "Peak-to-Integrated-Loudness Ratio in LU (reference peak minus LUFSi).",
            FolderColumnId::MinPsr => "Lowest continuous 3.0 s Sample-Peak-to-Short-Term-Loudness ratio (highlights over-compressed sections).",
            FolderColumnId::ReplayGainTrack => "ReplayGain 2.0 track normalization offset in dB relative to -18.0 LUFS (-18.0 - LUFSi).",
            FolderColumnId::StereoCorrelation => "Pearson zero-lag correlation r in [-1.00, +1.00] between Front Left and Front Right channels.",
            FolderColumnId::TplOversample => "ITU-R BS.1770-4 True Peak polyphase oversampling factor (4x < 96 kHz, 2x < 192 kHz, 1x >= 192 kHz).",
        }
    }

    pub fn cli_name(self) -> &'static str {
        match self {
            FolderColumnId::FileName => "file_name",
            FolderColumnId::Duration => "duration",
            FolderColumnId::FileFormat => "file_format",
            FolderColumnId::SampleRate => "sample_rate",
            FolderColumnId::WordLength => "word_length",
            FolderColumnId::BitsUsed => "bits_used",
            FolderColumnId::DrPmf => "dr_pmf",
            FolderColumnId::DrLeft => "dr_left",
            FolderColumnId::DrRight => "dr_right",
            FolderColumnId::MaxTplItu => "max_tpl_itu",
            FolderColumnId::MaxTplLeft => "max_tpl_left",
            FolderColumnId::MaxTplRight => "max_tpl_right",
            FolderColumnId::Lufsi => "lufsi",
            FolderColumnId::Lra => "lra",
            FolderColumnId::MaxMomItu => "max_mom_itu",
            FolderColumnId::MaxShortTermItu => "max_short_term_itu",
            FolderColumnId::MaxMomLeft => "max_mom_left",
            FolderColumnId::MaxMomRight => "max_mom_right",
            FolderColumnId::MaxShortTermLeft => "max_short_term_left",
            FolderColumnId::MaxShortTermRight => "max_short_term_right",
            FolderColumnId::MaxSppmJoint => "max_sppm_joint",
            FolderColumnId::MaxSppmLeft => "max_sppm_left",
            FolderColumnId::MaxSppmRight => "max_sppm_right",
            FolderColumnId::RmsJoint => "rms_joint",
            FolderColumnId::RmsLeft => "rms_left",
            FolderColumnId::RmsRight => "rms_right",
            FolderColumnId::Plr => "plr",
            FolderColumnId::MinPsr => "min_psr",
            FolderColumnId::ReplayGainTrack => "replay_gain_track",
            FolderColumnId::StereoCorrelation => "stereo_correlation",
            FolderColumnId::TplOversample => "tpl_oversample",
        }
    }

    pub fn from_cli_name(raw: &str) -> Option<Self> {
        let norm: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match norm.as_str() {
            "filename" | "track" | "name" => Some(Self::FileName),
            "duration" | "time" => Some(Self::Duration),
            "fileformat" | "format" | "ext" | "codec" => Some(Self::FileFormat),
            "samplerate" | "sr" => Some(Self::SampleRate),
            "wordlength" | "bits" => Some(Self::WordLength),
            "bitsused" => Some(Self::BitsUsed),
            "drpmf" | "dr" => Some(Self::DrPmf),
            "drleft" | "drl" => Some(Self::DrLeft),
            "drright" | "drr" => Some(Self::DrRight),
            "maxtplitu" | "maxtpl" | "tpl" => Some(Self::MaxTplItu),
            "maxtplleft" | "tplleft" => Some(Self::MaxTplLeft),
            "maxtplright" | "tplright" => Some(Self::MaxTplRight),
            "lufsi" | "lufs" | "integrated" => Some(Self::Lufsi),
            "lra" => Some(Self::Lra),
            "maxmomitu" | "maxmom" | "mom" => Some(Self::MaxMomItu),
            "maxshorttermitu" | "maxshortterm" | "shortterm" | "st" => Some(Self::MaxShortTermItu),
            "maxmomleft" | "momleft" => Some(Self::MaxMomLeft),
            "maxmomright" | "momright" => Some(Self::MaxMomRight),
            "maxshorttermleft" | "stleft" => Some(Self::MaxShortTermLeft),
            "maxshorttermright" | "stright" => Some(Self::MaxShortTermRight),
            "maxsppmjoint" | "maxsppm" | "sppm" | "peak" => Some(Self::MaxSppmJoint),
            "maxsppmleft" | "sppmleft" => Some(Self::MaxSppmLeft),
            "maxsppmright" | "sppmright" => Some(Self::MaxSppmRight),
            "rmsjoint" | "rms" => Some(Self::RmsJoint),
            "rmsleft" | "rmsl" => Some(Self::RmsLeft),
            "rmsright" | "rmsr" => Some(Self::RmsRight),
            "plr" => Some(Self::Plr),
            "minpsr" | "psr" => Some(Self::MinPsr),
            "replaygaintrack" | "replaygain" | "rg" => Some(Self::ReplayGainTrack),
            "stereocorrelation" | "stereocorr" | "corr" | "correlation" => {
                Some(Self::StereoCorrelation)
            }
            "tploversample" | "tplos" | "oversample" => Some(Self::TplOversample),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(&self) -> &'static str {
        match self {
            ThemeMode::System => "System Default",
            ThemeMode::Dark => "Dark",
            ThemeMode::Light => "Light",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ThemeMode::System => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::System,
        }
    }

    pub fn all() -> &'static [ThemeMode] {
        &[ThemeMode::System, ThemeMode::Dark, ThemeMode::Light]
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefaultResultsView {
    #[default]
    VisualView,
    TextLog,
}

impl DefaultResultsView {
    pub fn label(self) -> &'static str {
        match self {
            DefaultResultsView::VisualView => "Visual View",
            DefaultResultsView::TextLog => "Text Log",
        }
    }

    pub fn all() -> &'static [DefaultResultsView] {
        &[DefaultResultsView::VisualView, DefaultResultsView::TextLog]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FolderColumnsConfig {
    // Group 1: Info
    pub file_name: bool,
    pub duration: bool,
    pub file_format: bool,
    pub sample_rate: bool,
    pub word_length: bool,
    pub bits_used: bool,

    // Group 2: DR
    pub dr_pmf: bool,
    pub dr_left: bool,
    pub dr_right: bool,

    // Group 3: True Peak Level
    pub max_tpl_itu: bool,
    pub max_tpl_left: bool,
    pub max_tpl_right: bool,

    // Group 4: LUFS
    pub lufsi: bool,
    pub lra: bool,
    pub max_mom_itu: bool,
    pub max_short_term_itu: bool,

    // Group 5: Channel Specific LUFS
    pub max_mom_left: bool,
    pub max_mom_right: bool,
    pub max_short_term_left: bool,
    pub max_short_term_right: bool,

    // Group 6: Old Skool
    pub max_sppm_joint: bool,
    pub max_sppm_left: bool,
    pub max_sppm_right: bool,
    pub rms_joint: bool,
    pub rms_left: bool,
    pub rms_right: bool,

    // Group 7: Ratios & Diagnostics
    pub plr: bool,
    pub min_psr: bool,
    pub replay_gain_track: bool,
    pub stereo_correlation: bool,
    pub tpl_oversample: bool,

    #[serde(default = "FolderColumnId::default_order")]
    pub column_order: Vec<FolderColumnId>,
}

impl Default for FolderColumnsConfig {
    fn default() -> Self {
        Self {
            file_name: true,
            duration: false,
            file_format: false,
            sample_rate: false,
            word_length: false,
            bits_used: false,

            dr_pmf: true,
            dr_left: true,
            dr_right: true,

            max_tpl_itu: true,
            max_tpl_left: true,
            max_tpl_right: true,

            lufsi: true,
            lra: false,
            max_mom_itu: false,
            max_short_term_itu: false,

            max_mom_left: false,
            max_mom_right: false,
            max_short_term_left: false,
            max_short_term_right: false,

            max_sppm_joint: false,
            max_sppm_left: false,
            max_sppm_right: false,
            rms_joint: false,
            rms_left: false,
            rms_right: false,

            plr: false,
            min_psr: false,
            replay_gain_track: false,
            stereo_correlation: false,
            tpl_oversample: false,

            column_order: FolderColumnId::default_order(),
        }
    }
}

impl FolderColumnsConfig {
    pub fn empty() -> Self {
        Self {
            file_name: false,
            dr_pmf: false,
            dr_left: false,
            dr_right: false,
            max_tpl_itu: false,
            max_tpl_left: false,
            max_tpl_right: false,
            lufsi: false,
            ..Self::default()
        }
    }

    /// Default column selection and ordering for a given [`FolderLogStyle`].
    pub fn for_style(style: FolderLogStyle) -> Self {
        match style {
            FolderLogStyle::TruePeaksTable => Self::default(),
            FolderLogStyle::ClassicFooDr => Self::classic_foo_dr(),
            FolderLogStyle::CompactMastering => Self::compact_mastering(),
            FolderLogStyle::Json => Self::json_default(),
        }
    }

    /// JSON default field selection for structured script and automation consumption.
    pub fn json_default() -> Self {
        let front = [
            FolderColumnId::FileName,
            FolderColumnId::Duration,
            FolderColumnId::FileFormat,
            FolderColumnId::SampleRate,
            FolderColumnId::WordLength,
            FolderColumnId::BitsUsed,
            FolderColumnId::DrPmf,
            FolderColumnId::MaxTplItu,
            FolderColumnId::MaxSppmJoint,
            FolderColumnId::RmsJoint,
            FolderColumnId::Lufsi,
            FolderColumnId::Lra,
            FolderColumnId::MinPsr,
            FolderColumnId::Plr,
            FolderColumnId::ReplayGainTrack,
            FolderColumnId::StereoCorrelation,
            FolderColumnId::TplOversample,
        ];
        Self {
            file_name: true,
            duration: true,
            file_format: true,
            sample_rate: true,
            word_length: true,
            bits_used: true,
            dr_pmf: true,
            max_tpl_itu: true,
            max_sppm_joint: true,
            rms_joint: true,
            lufsi: true,
            lra: true,
            min_psr: true,
            plr: true,
            replay_gain_track: true,
            stereo_correlation: true,
            tpl_oversample: true,
            column_order: FolderColumnId::order_with_front(&front),
            ..Self::empty()
        }
    }

    /// Classic `foo_dr` column preset: `DR (PMF)`, `Max. SPPM`, `RMS`, `Duration`, `File Name`.
    pub fn classic_foo_dr() -> Self {
        let front = [
            FolderColumnId::DrPmf,
            FolderColumnId::MaxSppmJoint,
            FolderColumnId::RmsJoint,
            FolderColumnId::Duration,
            FolderColumnId::FileName,
        ];
        Self {
            dr_pmf: true,
            max_sppm_joint: true,
            rms_joint: true,
            duration: true,
            file_name: true,
            column_order: FolderColumnId::order_with_front(&front),
            ..Self::empty()
        }
    }

    /// Compact Mastering column preset: `DR (PMF)`, `Max. SPPM`, `Max. TPL`, `LUFSi`, `Min. PSR`, `Duration`, `File Name`.
    pub fn compact_mastering() -> Self {
        let front = [
            FolderColumnId::DrPmf,
            FolderColumnId::MaxSppmJoint,
            FolderColumnId::MaxTplItu,
            FolderColumnId::Lufsi,
            FolderColumnId::MinPsr,
            FolderColumnId::Duration,
            FolderColumnId::FileName,
        ];
        Self {
            dr_pmf: true,
            max_sppm_joint: true,
            max_tpl_itu: true,
            lufsi: true,
            min_psr: true,
            duration: true,
            file_name: true,
            column_order: FolderColumnId::order_with_front(&front),
            ..Self::empty()
        }
    }

    /// Compact DR-only preset (Track Name, DR PMF, DR L/R, Max TPL, LUFSi).
    pub fn minimal() -> Self {
        Self {
            max_tpl_left: false,
            max_tpl_right: false,
            ..Self::default()
        }
    }

    /// Loudness & dynamics preset focused on modern BS.1770 mastering metrics.
    pub fn mastering() -> Self {
        Self {
            max_tpl_left: false,
            max_tpl_right: false,
            dr_left: false,
            dr_right: false,
            lra: true,
            max_mom_itu: true,
            max_short_term_itu: true,
            plr: true,
            min_psr: true,
            ..Self::default()
        }
    }

    /// Enable every available measurement column in the folder summary table.
    pub fn all_enabled() -> Self {
        Self {
            file_name: true,
            duration: true,
            file_format: true,
            sample_rate: true,
            word_length: true,
            bits_used: true,
            dr_pmf: true,
            dr_left: true,
            dr_right: true,
            max_tpl_itu: true,
            max_tpl_left: true,
            max_tpl_right: true,
            lufsi: true,
            lra: true,
            max_mom_itu: true,
            max_short_term_itu: true,
            max_mom_left: true,
            max_mom_right: true,
            max_short_term_left: true,
            max_short_term_right: true,
            max_sppm_joint: true,
            max_sppm_left: true,
            max_sppm_right: true,
            rms_joint: true,
            rms_left: true,
            rms_right: true,
            plr: true,
            min_psr: true,
            replay_gain_track: true,
            stereo_correlation: true,
            tpl_oversample: true,
            column_order: FolderColumnId::default_order(),
        }
    }

    fn flag_mut(&mut self, col: FolderColumnId) -> &mut bool {
        match col {
            FolderColumnId::FileName => &mut self.file_name,
            FolderColumnId::Duration => &mut self.duration,
            FolderColumnId::FileFormat => &mut self.file_format,
            FolderColumnId::SampleRate => &mut self.sample_rate,
            FolderColumnId::WordLength => &mut self.word_length,
            FolderColumnId::BitsUsed => &mut self.bits_used,
            FolderColumnId::DrPmf => &mut self.dr_pmf,
            FolderColumnId::DrLeft => &mut self.dr_left,
            FolderColumnId::DrRight => &mut self.dr_right,
            FolderColumnId::MaxTplItu => &mut self.max_tpl_itu,
            FolderColumnId::MaxTplLeft => &mut self.max_tpl_left,
            FolderColumnId::MaxTplRight => &mut self.max_tpl_right,
            FolderColumnId::Lufsi => &mut self.lufsi,
            FolderColumnId::Lra => &mut self.lra,
            FolderColumnId::MaxMomItu => &mut self.max_mom_itu,
            FolderColumnId::MaxShortTermItu => &mut self.max_short_term_itu,
            FolderColumnId::MaxMomLeft => &mut self.max_mom_left,
            FolderColumnId::MaxMomRight => &mut self.max_mom_right,
            FolderColumnId::MaxShortTermLeft => &mut self.max_short_term_left,
            FolderColumnId::MaxShortTermRight => &mut self.max_short_term_right,
            FolderColumnId::MaxSppmJoint => &mut self.max_sppm_joint,
            FolderColumnId::MaxSppmLeft => &mut self.max_sppm_left,
            FolderColumnId::MaxSppmRight => &mut self.max_sppm_right,
            FolderColumnId::RmsJoint => &mut self.rms_joint,
            FolderColumnId::RmsLeft => &mut self.rms_left,
            FolderColumnId::RmsRight => &mut self.rms_right,
            FolderColumnId::Plr => &mut self.plr,
            FolderColumnId::MinPsr => &mut self.min_psr,
            FolderColumnId::ReplayGainTrack => &mut self.replay_gain_track,
            FolderColumnId::StereoCorrelation => &mut self.stereo_correlation,
            FolderColumnId::TplOversample => &mut self.tpl_oversample,
        }
    }

    pub fn is_enabled(&self, col: FolderColumnId) -> bool {
        match col {
            FolderColumnId::FileName => self.file_name,
            FolderColumnId::Duration => self.duration,
            FolderColumnId::FileFormat => self.file_format,
            FolderColumnId::SampleRate => self.sample_rate,
            FolderColumnId::WordLength => self.word_length,
            FolderColumnId::BitsUsed => self.bits_used,
            FolderColumnId::DrPmf => self.dr_pmf,
            FolderColumnId::DrLeft => self.dr_left,
            FolderColumnId::DrRight => self.dr_right,
            FolderColumnId::MaxTplItu => self.max_tpl_itu,
            FolderColumnId::MaxTplLeft => self.max_tpl_left,
            FolderColumnId::MaxTplRight => self.max_tpl_right,
            FolderColumnId::Lufsi => self.lufsi,
            FolderColumnId::Lra => self.lra,
            FolderColumnId::MaxMomItu => self.max_mom_itu,
            FolderColumnId::MaxShortTermItu => self.max_short_term_itu,
            FolderColumnId::MaxMomLeft => self.max_mom_left,
            FolderColumnId::MaxMomRight => self.max_mom_right,
            FolderColumnId::MaxShortTermLeft => self.max_short_term_left,
            FolderColumnId::MaxShortTermRight => self.max_short_term_right,
            FolderColumnId::MaxSppmJoint => self.max_sppm_joint,
            FolderColumnId::MaxSppmLeft => self.max_sppm_left,
            FolderColumnId::MaxSppmRight => self.max_sppm_right,
            FolderColumnId::RmsJoint => self.rms_joint,
            FolderColumnId::RmsLeft => self.rms_left,
            FolderColumnId::RmsRight => self.rms_right,
            FolderColumnId::Plr => self.plr,
            FolderColumnId::MinPsr => self.min_psr,
            FolderColumnId::ReplayGainTrack => self.replay_gain_track,
            FolderColumnId::StereoCorrelation => self.stereo_correlation,
            FolderColumnId::TplOversample => self.tpl_oversample,
        }
    }

    pub fn set_enabled(&mut self, col: FolderColumnId, val: bool) {
        let was_enabled = self.is_enabled(col);
        let active_before = self.active_ordered();
        *self.flag_mut(col) = val;

        if !was_enabled && val {
            let mut new_active = active_before;
            if new_active.len() >= 2
                && new_active.last() == Some(&FolderColumnId::FileName)
                && col != FolderColumnId::FileName
            {
                let insert_pos = new_active.len() - 1;
                new_active.insert(insert_pos, col);
            } else {
                new_active.push(col);
            }
            self.column_order = FolderColumnId::order_with_front(&new_active);
        }
    }

    pub fn normalized_order(&self) -> Vec<FolderColumnId> {
        FolderColumnId::order_with_front(&self.column_order)
    }

    pub fn active_ordered(&self) -> Vec<FolderColumnId> {
        self.normalized_order()
            .into_iter()
            .filter(|&c| self.is_enabled(c))
            .collect()
    }

    pub fn move_active_column(&mut self, from_idx: usize, to_idx: usize) -> bool {
        let mut active = self.active_ordered();
        if from_idx >= active.len() || to_idx >= active.len() || from_idx == to_idx {
            return false;
        }
        let item = active.remove(from_idx);
        active.insert(to_idx, item);
        self.column_order = FolderColumnId::order_with_front(&active);
        true
    }

    pub fn reset_order_for_style(&mut self, style: FolderLogStyle) {
        let template = Self::for_style(style);
        self.column_order = template.column_order;
    }

    /// Returns `(active_columns, total_columns)`.
    pub fn active_count(&self) -> (usize, usize) {
        let active = FolderColumnId::ALL
            .iter()
            .filter(|&&c| self.is_enabled(c))
            .count();
        (active, FolderColumnId::ALL.len())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StyleColumnsProfiles {
    pub truepeaks_table: FolderColumnsConfig,
    pub classic_foo_dr: FolderColumnsConfig,
    pub compact_mastering: FolderColumnsConfig,
    pub json: FolderColumnsConfig,
}

impl Default for StyleColumnsProfiles {
    fn default() -> Self {
        Self {
            truepeaks_table: FolderColumnsConfig::default(),
            classic_foo_dr: FolderColumnsConfig::classic_foo_dr(),
            compact_mastering: FolderColumnsConfig::compact_mastering(),
            json: FolderColumnsConfig::json_default(),
        }
    }
}

impl StyleColumnsProfiles {
    pub fn get(&self, style: FolderLogStyle) -> &FolderColumnsConfig {
        match style {
            FolderLogStyle::TruePeaksTable => &self.truepeaks_table,
            FolderLogStyle::ClassicFooDr => &self.classic_foo_dr,
            FolderLogStyle::CompactMastering => &self.compact_mastering,
            FolderLogStyle::Json => &self.json,
        }
    }

    pub fn set(&mut self, style: FolderLogStyle, cfg: FolderColumnsConfig) {
        match style {
            FolderLogStyle::TruePeaksTable => self.truepeaks_table = cfg,
            FolderLogStyle::ClassicFooDr => self.classic_foo_dr = cfg,
            FolderLogStyle::CompactMastering => self.compact_mastering = cfg,
            FolderLogStyle::Json => self.json = cfg,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SingleFileRowsConfig {
    pub max_tpl: bool,
    pub max_sppm: bool,
    pub rms: bool,
    pub max_momentary: bool,
    pub max_short_term: bool,
    pub integrated_loudness: bool,
    pub dr_pmf: bool,
    pub min_psr: bool,
    pub plr: bool,
    pub lra: bool,
    pub replay_gain: bool,
    pub stereo_correlation: bool,
    #[serde(default, alias = "clipping_and_peaks")]
    pub peak_timing_diagnostics: bool,
}

impl Default for SingleFileRowsConfig {
    fn default() -> Self {
        Self {
            max_tpl: true,
            max_sppm: true,
            rms: true,
            max_momentary: true,
            max_short_term: true,
            integrated_loudness: true,
            dr_pmf: true,
            min_psr: true,
            plr: true,
            lra: true,
            replay_gain: false,
            stereo_correlation: false,
            peak_timing_diagnostics: false,
        }
    }
}

impl SingleFileRowsConfig {
    pub fn all_with_diagnostics() -> Self {
        Self {
            replay_gain: true,
            stereo_correlation: true,
            peak_timing_diagnostics: true,
            ..Self::default()
        }
    }

    pub fn none() -> Self {
        Self {
            max_tpl: false,
            max_sppm: false,
            rms: false,
            max_momentary: false,
            max_short_term: false,
            integrated_loudness: false,
            dr_pmf: false,
            min_psr: false,
            plr: false,
            lra: false,
            replay_gain: false,
            stereo_correlation: false,
            peak_timing_diagnostics: false,
        }
    }

    pub fn set_row_by_name(&mut self, name: &str, val: bool) -> Result<(), String> {
        let norm: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match norm.as_str() {
            "maxtpl" | "tpl" => self.max_tpl = val,
            "maxsppm" | "sppm" | "peak" => self.max_sppm = val,
            "rms" => self.rms = val,
            "maxmomentary" | "mom" | "momentary" => self.max_momentary = val,
            "maxshortterm" | "shortterm" | "st" => self.max_short_term = val,
            "integratedloudness" | "lufsi" | "lufs" => self.integrated_loudness = val,
            "drpmf" | "dr" => self.dr_pmf = val,
            "minpsr" | "psr" => self.min_psr = val,
            "plr" => self.plr = val,
            "lra" => self.lra = val,
            "replaygain" | "rg" => self.replay_gain = val,
            "stereocorrelation" | "corr" | "correlation" => self.stereo_correlation = val,
            "peaktimingdiagnostics" | "peaktiming" | "timing" | "diagnostics" => {
                self.peak_timing_diagnostics = val;
            }
            _ => return Err(format!("Unknown single-file row '{}'", name)),
        }
        Ok(())
    }
}

pub const DEFAULT_WINDOW_WIDTH: f32 = 980.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 660.0;
pub const MIN_WINDOW_WIDTH: f32 = 640.0;
pub const MIN_WINDOW_HEIGHT: f32 = 420.0;
pub const DEFAULT_LOG_FILENAME_MAX_LEN: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LogFilenameOptions {
    pub limit_length: bool,
    pub max_length: usize,
    pub strip_non_ascii: bool,
}

impl Default for LogFilenameOptions {
    fn default() -> Self {
        Self {
            limit_length: false,
            max_length: DEFAULT_LOG_FILENAME_MAX_LEN,
            strip_non_ascii: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub folder_log_style: FolderLogStyle,
    pub folder_columns: FolderColumnsConfig,
    pub style_columns: StyleColumnsProfiles,
    pub single_rows: SingleFileRowsConfig,
    pub log_filenames: LogFilenameOptions,
    pub surround_folder_mode: SurroundFolderMode,
    pub include_lfe_in_dr: bool,
    pub recurse_subfolders: bool,
    pub combine_dropped_folders: bool,
    pub use_embedded_tags: bool,
    pub max_threads: usize,
    pub theme: ThemeMode,
    pub default_results_view: DefaultResultsView,
    pub log_font_size: f32,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub window_width: f32,
    pub window_height: f32,
    pub charset: CharsetMode,
    pub line_ending: LineEndingMode,
    pub folder_log_pattern: String,
    pub single_log_pattern: String,
    pub save_logs_to_disk: bool,
    pub overwrite_existing_logs: bool,
    pub custom_output_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        let max_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            folder_log_style: FolderLogStyle::TruePeaksTable,
            folder_columns: FolderColumnsConfig::default(),
            style_columns: StyleColumnsProfiles::default(),
            single_rows: SingleFileRowsConfig::default(),
            log_filenames: LogFilenameOptions::default(),
            surround_folder_mode: SurroundFolderMode::ExpandPerChannel,
            include_lfe_in_dr: false,
            recurse_subfolders: false,
            combine_dropped_folders: false,
            use_embedded_tags: false,
            max_threads: max_cores,
            theme: ThemeMode::System,
            default_results_view: DefaultResultsView::VisualView,
            log_font_size: 13.5,
            window_x: None,
            window_y: None,
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            charset: CharsetMode::Utf8,
            line_ending: LineEndingMode::Crlf,
            folder_log_pattern: "DR.txt".to_string(),
            single_log_pattern: "{file_name}_log.txt".to_string(),
            save_logs_to_disk: true,
            overwrite_existing_logs: false,
            custom_output_dir: String::new(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Option<PathBuf> {
        ProjectDirs::from("org", "TruePeaks", "TruePeaks")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    pub fn sync_active_to_profile(&mut self) {
        self.style_columns
            .set(self.folder_log_style, self.folder_columns.clone());
    }

    pub fn switch_folder_log_style(&mut self, next: FolderLogStyle) {
        if self.folder_log_style != next {
            self.sync_active_to_profile();
            self.folder_log_style = next;
            self.folder_columns = self.style_columns.get(next).clone();
        }
    }

    pub fn has_saved_window_position(&self) -> bool {
        self.window_x.is_some() && self.window_y.is_some()
    }

    pub fn sanitize_window_geometry(&mut self) {
        self.window_width = if self.window_width.is_finite() {
            self.window_width.clamp(MIN_WINDOW_WIDTH, 16_384.0)
        } else {
            DEFAULT_WINDOW_WIDTH
        };
        self.window_height = if self.window_height.is_finite() {
            self.window_height.clamp(MIN_WINDOW_HEIGHT, 16_384.0)
        } else {
            DEFAULT_WINDOW_HEIGHT
        };

        let valid_pos = match (self.window_x, self.window_y) {
            (Some(x), Some(y)) => {
                x.is_finite()
                    && y.is_finite()
                    && (-8192.0..=16_384.0).contains(&x)
                    && (-8192.0..=16_384.0).contains(&y)
            }
            _ => false,
        };
        if !valid_pos {
            self.window_x = None;
            self.window_y = None;
        }
    }

    pub fn sanitize(&mut self) {
        let max_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        self.max_threads = self.max_threads.clamp(1, max_cores);
        self.log_filenames.max_length = self.log_filenames.max_length.clamp(8, 255);
        if !self.log_font_size.is_finite() {
            self.log_font_size = 13.5;
        } else {
            self.log_font_size = self.log_font_size.clamp(10.0, 22.0);
        }
        self.sanitize_window_geometry();
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config '{}': {}", path.display(), e))?;
        let mut cfg = toml::from_str::<AppConfig>(&contents)
            .map_err(|e| format!("Invalid config TOML '{}': {}", path.display(), e))?;
        cfg.sanitize();
        Ok(cfg)
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(cfg) = Self::load_from_path(&path) {
                return cfg;
            }
        }
        Self::default()
    }

    /// Temporarily overrides any configuration field using TOML dot-notation
    /// (never writes changes to disk).
    pub fn apply_key_value_override(&mut self, key: &str, raw_val: &str) -> Result<(), String> {
        let mut root = toml::Value::try_from(self.clone())
            .map_err(|e| format!("Failed to inspect config: {}", e))?;
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() || parts.iter().any(|p| p.trim().is_empty()) {
            return Err(format!("Invalid config key '{}'", key));
        }

        let mut cur = &mut root;
        for part in &parts[..parts.len() - 1] {
            let tbl = cur
                .as_table_mut()
                .ok_or_else(|| format!("Key path '{}' does not address a table", key))?;
            cur = tbl
                .get_mut(*part)
                .ok_or_else(|| format!("Unknown config section '{}' in '{}'", part, key))?;
        }
        let leaf_name = parts[parts.len() - 1];
        let tbl = cur
            .as_table_mut()
            .ok_or_else(|| format!("Key path '{}' does not address a table", key))?;
        let existing = tbl
            .get_mut(leaf_name)
            .ok_or_else(|| format!("Unknown config key '{}'", key))?;

        let new_val = match existing {
            toml::Value::Boolean(_) => toml::Value::Boolean(parse_bool_str(raw_val, key)?),
            toml::Value::Integer(_) => {
                let n = raw_val
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| format!("Expected integer for '{}', got '{}'", key, raw_val))?;
                toml::Value::Integer(n)
            }
            toml::Value::Float(_) => {
                let f = raw_val
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("Expected number for '{}', got '{}'", key, raw_val))?;
                toml::Value::Float(f)
            }
            toml::Value::String(_) => toml::Value::String(raw_val.to_string()),
            toml::Value::Array(_) => {
                let items: Vec<toml::Value> = raw_val
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| toml::Value::String(s.to_string()))
                    .collect();
                toml::Value::Array(items)
            }
            _ => {
                return Err(format!("Config key '{}' cannot be set via a scalar value", key));
            }
        };
        *existing = new_val;

        let mut updated: AppConfig = root
            .try_into()
            .map_err(|e| format!("Invalid value '{}' for '{}': {}", raw_val, key, e))?;
        updated.sanitize();
        if key == "folder_log_style" {
            updated.folder_columns = updated.style_columns.get(updated.folder_log_style).clone();
        } else if key.starts_with("folder_columns.") {
            updated.sync_active_to_profile();
        }
        *self = updated;
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let data = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
            fs::write(path, data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn parse_bool_str(s: &str, key: &str) -> Result<bool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!(
            "Expected boolean (true/false) for '{}', got '{}'",
            key, s
        )),
    }
}
