use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FolderColumnsConfig {
    // Group 1: Info
    pub file_name: bool,
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
    pub rms_left: bool,
    pub rms_right: bool,

    // Group 7: Ratios
    pub plr: bool,
    pub min_psr: bool,
}

impl Default for FolderColumnsConfig {
    fn default() -> Self {
        Self {
            file_name: true,
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
            rms_left: false,
            rms_right: false,

            plr: false,
            min_psr: false,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub folder_columns: FolderColumnsConfig,
    pub single_rows: SingleFileRowsConfig,
    pub surround_folder_mode: SurroundFolderMode,
    pub include_lfe_in_dr: bool,
    pub max_threads: usize,
    pub charset: CharsetMode,
    pub line_ending: LineEndingMode,
    pub folder_log_pattern: String,
    pub single_log_pattern: String,
    pub save_logs_to_disk: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let max_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            folder_columns: FolderColumnsConfig::default(),
            single_rows: SingleFileRowsConfig::default(),
            surround_folder_mode: SurroundFolderMode::ExpandPerChannel,
            include_lfe_in_dr: false,
            max_threads: max_cores,
            charset: CharsetMode::Utf8,
            line_ending: LineEndingMode::Crlf,
            folder_log_pattern: "{folder_name}_log.txt".to_string(),
            single_log_pattern: "{file_name}_log.txt".to_string(),
            save_logs_to_disk: true,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Option<PathBuf> {
        ProjectDirs::from("org", "TruePeaks", "TruePeaks")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = toml::from_str::<AppConfig>(&contents) {
                    let max_cores = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(4);
                    cfg.max_threads = cfg.max_threads.clamp(1, max_cores);
                    return cfg;
                }
            }
        }
        Self::default()
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
