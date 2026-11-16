use crate::config::{
    AppConfig, CharsetMode, FolderColumnId, FolderColumnsConfig, FolderLogStyle, LineEndingMode,
    SingleFileRowsConfig, SurroundFolderMode,
};
use crate::decoder::cue::is_cue_file;
use crate::decoder::is_supported_audio_file;
use crate::formatter::export::{export_tracks, json_escape, ExportFormat};
use crate::formatter::write_log_file;
use crate::scanner::{spawn_scan_job, ScanEvent, ScanSnapshot, ScanTarget};
use crossbeam_channel::unbounded;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliOutputFormat {
    Text,
    Csv,
    Markdown,
    Json,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum CliAction {
    LaunchGui(Option<ScanTarget>),
    RunHeadless {
        target: ScanTarget,
        config: AppConfig,
        format: CliOutputFormat,
        output_path: Option<PathBuf>,
        show_progress: bool,
    },
    PrintAndExit(i32, String),
}

pub fn target_from_paths(paths: &[PathBuf]) -> Option<ScanTarget> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for p in paths {
        if p.is_dir() {
            dirs.push(p.clone());
        } else if is_supported_audio_file(p) || is_cue_file(p) {
            files.push(p.clone());
        }
    }

    match (dirs.len(), files.len()) {
        (0, 0) => None,
        (0, 1) => Some(ScanTarget::SingleFile(files.remove(0))),
        (1, 0) => Some(ScanTarget::Folder(dirs.remove(0))),
        _ => Some(ScanTarget::Multi { dirs, files }),
    }
}

pub fn parse_cli_args(args: &[String], base_cfg: &AppConfig) -> CliAction {
    parse_cli_args_with_mode(args, base_cfg, false)
}

pub fn parse_cli_args_headless(args: &[String], base_cfg: &AppConfig) -> CliAction {
    parse_cli_args_with_mode(args, base_cfg, true)
}

fn take_opt_val<'a>(
    flag_name: &str,
    inline_val: Option<&'a str>,
    args: &'a [String],
    idx: &mut usize,
) -> Result<&'a str, String> {
    if let Some(v) = inline_val {
        return Ok(v);
    }
    *idx += 1;
    args.get(*idx)
        .map(|s| s.as_str())
        .ok_or_else(|| format!("Missing value after {}", flag_name))
}

fn parse_cli_args_with_mode(
    args: &[String],
    base_cfg: &AppConfig,
    force_headless: bool,
) -> CliAction {
    if args.is_empty() {
        return if force_headless {
            CliAction::PrintAndExit(0, help_text(true))
        } else {
            CliAction::LaunchGui(None)
        };
    }

    let mut headless = force_headless;
    let mut format = CliOutputFormat::Text;
    let mut output_path: Option<PathBuf> = None;
    let mut config = base_cfg.clone();
    // CLI defaults to stdout-only unless --save / --output-dir is explicitly configured
    let mut explicit_save = false;
    let mut show_progress = true;
    let mut print_config_toml = false;
    let mut print_config_json = false;
    let mut raw_paths: Vec<PathBuf> = Vec::new();
    let mut after_double_dash = false;

    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        if after_double_dash {
            raw_paths.push(PathBuf::from(arg));
            i += 1;
            continue;
        }
        if arg == "--" {
            after_double_dash = true;
            i += 1;
            continue;
        }

        let (flag, inline_val): (&str, Option<&str>) = if arg.starts_with("--") {
            if let Some((k, v)) = arg.split_once('=') {
                (k, Some(v))
            } else {
                (arg, None)
            }
        } else if arg.starts_with("-o=") || arg.starts_with("-j=") {
            (&arg[..2], Some(&arg[3..]))
        } else {
            (arg, None)
        };

        macro_rules! need_val {
            () => {
                match take_opt_val(flag, inline_val, args, &mut i) {
                    Ok(v) => v,
                    Err(e) => return CliAction::PrintAndExit(2, e),
                }
            };
        }

        match flag {
            "-h" | "--help" => {
                return CliAction::PrintAndExit(0, help_text(force_headless));
            }
            "--longhelp" | "--long-help" => {
                return CliAction::PrintAndExit(0, long_help_text(force_headless));
            }
            "--help-json" => {
                return CliAction::PrintAndExit(0, help_schema_json());
            }
            "-V" | "--version" => {
                return CliAction::PrintAndExit(
                    0,
                    format!("TruePeaks v{}", env!("CARGO_PKG_VERSION")),
                );
            }
            "--cli" => headless = true,
            "--progress" => {
                headless = true;
                show_progress = true;
            }
            "--no-progress" | "-q" | "--quiet" => {
                headless = true;
                show_progress = false;
            }
            "--print-config" => {
                headless = true;
                print_config_toml = true;
            }
            "--print-config-json" => {
                headless = true;
                print_config_json = true;
            }
            "--no-config" | "--defaults" => {
                headless = true;
                config = AppConfig::default();
            }
            "--config" => {
                headless = true;
                let p = need_val!();
                match AppConfig::load_from_path(Path::new(p)) {
                    Ok(loaded) => config = loaded,
                    Err(e) => return CliAction::PrintAndExit(2, e),
                }
            }
            "--set" => {
                headless = true;
                let kv = need_val!();
                let Some((k, v)) = kv.split_once('=') else {
                    return CliAction::PrintAndExit(
                        2,
                        format!("Invalid --set '{}'. Expected key=value.", kv),
                    );
                };
                let kv_key = k.trim();
                if let Err(e) = config.apply_key_value_override(kv_key, v) {
                    return CliAction::PrintAndExit(2, e);
                }
                if kv_key == "save_logs_to_disk" {
                    explicit_save = true;
                }
            }
            "--json" => {
                headless = true;
                format = CliOutputFormat::Json;
            }
            "--csv" => {
                headless = true;
                format = CliOutputFormat::Csv;
            }
            "--md" | "--markdown" => {
                headless = true;
                format = CliOutputFormat::Markdown;
            }
            "--txt" | "--text" => {
                headless = true;
                format = CliOutputFormat::Text;
            }
            "--format" => {
                headless = true;
                let val = need_val!();
                match val.to_ascii_lowercase().as_str() {
                    "txt" | "text" => format = CliOutputFormat::Text,
                    "csv" => format = CliOutputFormat::Csv,
                    "md" | "markdown" => format = CliOutputFormat::Markdown,
                    "json" => format = CliOutputFormat::Json,
                    other => {
                        return CliAction::PrintAndExit(
                            2,
                            format!(
                                "Unknown --format value '{}'. Use txt, csv, md, or json.",
                                other
                            ),
                        );
                    }
                }
            }
            "--style" => {
                headless = true;
                let val = need_val!();
                let style = match val.to_ascii_lowercase().as_str() {
                    "table" | "truepeaks" => FolderLogStyle::TruePeaksTable,
                    "classic" | "foo_dr" | "foodr" => FolderLogStyle::ClassicFooDr,
                    "compact" | "mastering" => FolderLogStyle::CompactMastering,
                    "json" => FolderLogStyle::Json,
                    other => {
                        return CliAction::PrintAndExit(
                            2,
                            format!(
                                "Unknown --style value '{}'. Use table, classic, compact, or json.",
                                other
                            ),
                        );
                    }
                };
                config.switch_folder_log_style(style);
            }
            "--preset" => {
                headless = true;
                let val = need_val!();
                config.folder_columns = match val.to_ascii_lowercase().as_str() {
                    "standard" | "default" | "table" => FolderColumnsConfig::default(),
                    "classic" | "foo_dr" | "foodr" => FolderColumnsConfig::classic_foo_dr(),
                    "compact" => FolderColumnsConfig::compact_mastering(),
                    "minimal" => FolderColumnsConfig::minimal(),
                    "mastering" => FolderColumnsConfig::mastering(),
                    "all" => FolderColumnsConfig::all_enabled(),
                    "json" => FolderColumnsConfig::json_default(),
                    other => {
                        return CliAction::PrintAndExit(
                            2,
                            format!(
                                "Unknown --preset '{}'. Use standard, classic, compact, minimal, mastering, all, or json.",
                                other
                            ),
                        );
                    }
                };
                config.sync_active_to_profile();
            }
            "--columns" => {
                headless = true;
                let val = need_val!();
                let mut cols = FolderColumnsConfig::empty();
                let mut order = Vec::new();
                for token in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    let Some(cid) = FolderColumnId::from_cli_name(token) else {
                        return CliAction::PrintAndExit(
                            2,
                            format!(
                                "Unknown column '{}'. Run with --help-json for valid column names.",
                                token
                            ),
                        );
                    };
                    cols.set_enabled(cid, true);
                    if !order.contains(&cid) {
                        order.push(cid);
                    }
                }
                if order.is_empty() {
                    return CliAction::PrintAndExit(
                        2,
                        "No valid columns supplied to --columns.".to_string(),
                    );
                }
                cols.column_order = FolderColumnId::order_with_front(&order);
                config.folder_columns = cols;
                config.sync_active_to_profile();
            }
            "--add-column" | "--remove-column" => {
                headless = true;
                let enable = flag == "--add-column";
                let val = need_val!();
                for token in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    let Some(cid) = FolderColumnId::from_cli_name(token) else {
                        return CliAction::PrintAndExit(
                            2,
                            format!("Unknown column '{}'. Run with --help-json for valid names.", token),
                        );
                    };
                    config.folder_columns.set_enabled(cid, enable);
                }
                config.sync_active_to_profile();
            }
            "--single-rows" => {
                headless = true;
                let val = need_val!();
                match val.trim().to_ascii_lowercase().as_str() {
                    "standard" | "default" => config.single_rows = SingleFileRowsConfig::default(),
                    "all" | "diagnostics" => {
                        config.single_rows = SingleFileRowsConfig::all_with_diagnostics();
                    }
                    _ => {
                        let mut rows = SingleFileRowsConfig::none();
                        let mut count = 0usize;
                        for token in val.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                            if let Err(e) = rows.set_row_by_name(token, true) {
                                return CliAction::PrintAndExit(2, e);
                            }
                            count += 1;
                        }
                        if count == 0 {
                            return CliAction::PrintAndExit(
                                2,
                                "No valid single-file rows supplied to --single-rows.".to_string(),
                            );
                        }
                        config.single_rows = rows;
                    }
                }
            }
            "--diagnostics" => {
                headless = true;
                config.single_rows = SingleFileRowsConfig::all_with_diagnostics();
            }
            "--surround" => {
                headless = true;
                let val = need_val!();
                match val.to_ascii_lowercase().as_str() {
                    "expand" | "per_channel" | "all" => {
                        config.surround_folder_mode = SurroundFolderMode::ExpandPerChannel;
                    }
                    "strict" | "2ch" | "stereo" => {
                        config.surround_folder_mode = SurroundFolderMode::StrictTwoChannel;
                    }
                    other => {
                        return CliAction::PrintAndExit(
                            2,
                            format!("Unknown --surround '{}'. Use expand or strict.", other),
                        );
                    }
                }
            }
            "-j" | "--threads" => {
                headless = true;
                let val = need_val!();
                match val.trim().parse::<usize>() {
                    Ok(n) if n >= 1 => {
                        config.max_threads = n;
                        config.sanitize();
                    }
                    _ => {
                        return CliAction::PrintAndExit(
                            2,
                            format!("Expected positive integer for {}, got '{}'", flag, val),
                        );
                    }
                }
            }
            "--max-name-len" => {
                headless = true;
                let val = need_val!();
                match val.trim().parse::<usize>() {
                    Ok(n) => {
                        config.log_filenames.limit_length = true;
                        config.log_filenames.max_length = n.clamp(8, 255);
                    }
                    _ => {
                        return CliAction::PrintAndExit(
                            2,
                            format!("Expected integer (8..=255) for --max-name-len, got '{}'", val),
                        );
                    }
                }
            }
            "--no-max-name-len" | "--no-max-len" => {
                headless = true;
                config.log_filenames.limit_length = false;
            }
            "--ascii" => {
                headless = true;
                config.log_filenames.strip_non_ascii = true;
            }
            "--no-ascii" => {
                headless = true;
                config.log_filenames.strip_non_ascii = false;
            }
            "--charset" => {
                headless = true;
                let val = need_val!();
                config.charset = match val.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
                    "utf8" => CharsetMode::Utf8,
                    "utf8bom" => CharsetMode::Utf8Bom,
                    "utf16le" => CharsetMode::Utf16Le,
                    "utf16be" => CharsetMode::Utf16Be,
                    "win1252" | "windows1252" | "cp1252" => CharsetMode::Windows1252,
                    _ => {
                        return CliAction::PrintAndExit(
                            2,
                            format!(
                                "Unknown --charset '{}'. Use utf8, utf8bom, utf16le, utf16be, or win1252.",
                                val
                            ),
                        );
                    }
                };
            }
            "--line-ending" => {
                headless = true;
                let val = need_val!();
                config.line_ending = match val.to_ascii_lowercase().as_str() {
                    "crlf" | "windows" => LineEndingMode::Crlf,
                    "lf" | "unix" => LineEndingMode::Lf,
                    "cr" | "mac" => LineEndingMode::Cr,
                    other => {
                        return CliAction::PrintAndExit(
                            2,
                            format!("Unknown --line-ending '{}'. Use crlf, lf, or cr.", other),
                        );
                    }
                };
            }
            "--folder-pattern" => {
                headless = true;
                config.folder_log_pattern = need_val!().to_string();
            }
            "--single-pattern" => {
                headless = true;
                config.single_log_pattern = need_val!().to_string();
            }
            "--output-dir" => {
                headless = true;
                config.custom_output_dir = need_val!().to_string();
            }
            "--no-output-dir" => {
                headless = true;
                config.custom_output_dir.clear();
            }
            "--overwrite" => {
                headless = true;
                config.overwrite_existing_logs = true;
            }
            "--no-overwrite" => {
                headless = true;
                config.overwrite_existing_logs = false;
            }
            "-o" | "--output" => {
                headless = true;
                output_path = Some(PathBuf::from(need_val!()));
            }
            "-r" | "--recurse" => {
                config.recurse_subfolders = true;
            }
            "--no-recurse" => {
                headless = true;
                config.recurse_subfolders = false;
            }
            "--lfe" => {
                config.include_lfe_in_dr = true;
            }
            "--no-lfe" => {
                headless = true;
                config.include_lfe_in_dr = false;
            }
            "--tags" => {
                config.use_embedded_tags = true;
            }
            "--no-tags" => {
                headless = true;
                config.use_embedded_tags = false;
            }
            "--save" => {
                explicit_save = true;
                config.save_logs_to_disk = true;
            }
            "--no-save" => {
                explicit_save = true;
                config.save_logs_to_disk = false;
            }
            "--combine" => {
                config.combine_dropped_folders = true;
            }
            "--no-combine" => {
                headless = true;
                config.combine_dropped_folders = false;
            }
            other if other.starts_with('-') => {
                return CliAction::PrintAndExit(
                    2,
                    format!("Unknown flag '{}'. Run with --help for usage.", other),
                );
            }
            path_arg => {
                raw_paths.push(PathBuf::from(path_arg));
            }
        }
        i += 1;
    }

    if print_config_toml || print_config_json {
        if !explicit_save {
            config.save_logs_to_disk = false;
        }
        if print_config_json {
            return CliAction::PrintAndExit(0, format_effective_config_json(&config));
        }
        let toml_out = toml::to_string_pretty(&config)
            .unwrap_or_else(|e| format!("# Serialization error: {}", e));
        return CliAction::PrintAndExit(0, toml_out);
    }

    let target = target_from_paths(&raw_paths);
    if !headless {
        return CliAction::LaunchGui(target);
    }

    let Some(target) = target else {
        return CliAction::PrintAndExit(
            2,
            "No valid audio files, folders, or .cue sheets provided.\n\n".to_string()
                + &help_text(force_headless),
        );
    };

    if !explicit_save {
        config.save_logs_to_disk = false;
    }

    CliAction::RunHeadless {
        target,
        config,
        format,
        output_path,
        show_progress,
    }
}

fn format_effective_config_json(cfg: &AppConfig) -> String {
    let active_cols: Vec<String> = cfg
        .folder_columns
        .active_ordered()
        .iter()
        .map(|c| format!("\"{}\"", c.cli_name()))
        .collect();
    format!(
        "{{\n\
  \"folder_log_style\": \"{}\",\n\
  \"active_columns\": [{}],\n\
  \"surround_folder_mode\": \"{}\",\n\
  \"include_lfe_in_dr\": {},\n\
  \"recurse_subfolders\": {},\n\
  \"combine_dropped_folders\": {},\n\
  \"use_embedded_tags\": {},\n\
  \"max_threads\": {},\n\
  \"log_filenames\": {{\n\
    \"limit_length\": {},\n\
    \"max_length\": {},\n\
    \"strip_non_ascii\": {}\n\
  }},\n\
  \"single_rows\": {{\n\
    \"max_tpl\": {},\n\
    \"max_sppm\": {},\n\
    \"rms\": {},\n\
    \"max_momentary\": {},\n\
    \"max_short_term\": {},\n\
    \"integrated_loudness\": {},\n\
    \"dr_pmf\": {},\n\
    \"min_psr\": {},\n\
    \"plr\": {},\n\
    \"lra\": {},\n\
    \"replay_gain\": {},\n\
    \"stereo_correlation\": {},\n\
    \"peak_timing_diagnostics\": {}\n\
  }},\n\
  \"charset\": \"{}\",\n\
  \"line_ending\": \"{}\",\n\
  \"folder_log_pattern\": \"{}\",\n\
  \"single_log_pattern\": \"{}\",\n\
  \"custom_output_dir\": \"{}\",\n\
  \"overwrite_existing_logs\": {},\n\
  \"save_logs_to_disk\": {}\n\
}}",
        json_escape(&format!("{:?}", cfg.folder_log_style)),
        active_cols.join(", "),
        json_escape(&format!("{:?}", cfg.surround_folder_mode)),
        cfg.include_lfe_in_dr,
        cfg.recurse_subfolders,
        cfg.combine_dropped_folders,
        cfg.use_embedded_tags,
        cfg.max_threads,
        cfg.log_filenames.limit_length,
        cfg.log_filenames.max_length,
        cfg.log_filenames.strip_non_ascii,
        cfg.single_rows.max_tpl,
        cfg.single_rows.max_sppm,
        cfg.single_rows.rms,
        cfg.single_rows.max_momentary,
        cfg.single_rows.max_short_term,
        cfg.single_rows.integrated_loudness,
        cfg.single_rows.dr_pmf,
        cfg.single_rows.min_psr,
        cfg.single_rows.plr,
        cfg.single_rows.lra,
        cfg.single_rows.replay_gain,
        cfg.single_rows.stereo_correlation,
        cfg.single_rows.peak_timing_diagnostics,
        json_escape(&format!("{:?}", cfg.charset)),
        json_escape(&format!("{:?}", cfg.line_ending)),
        json_escape(&cfg.folder_log_pattern),
        json_escape(&cfg.single_log_pattern),
        json_escape(&cfg.custom_output_dir),
        cfg.overwrite_existing_logs,
        cfg.save_logs_to_disk,
    )
}

fn help_schema_json() -> String {
    let col_names: Vec<String> = FolderColumnId::ALL
        .iter()
        .map(|c| format!("\"{}\"", c.cli_name()))
        .collect();
    format!(
        "{{\n\
  \"name\": \"truepeaks\",\n\
  \"version\": \"{}\",\n\
  \"config_persistence\": \"read-only (CLI overrides are temporary and never mutate config.toml)\",\n\
  \"exit_codes\": {{ \"0\": \"success\", \"1\": \"analysis or IO error\", \"2\": \"invalid CLI arguments or config override\" }},\n\
  \"streams\": {{ \"stdout\": \"formatted analysis report (--json/--csv/--md/--txt) or --help/--print-config\", \"stderr\": \"live progress (unless --no-progress/--quiet) and errors\" }},\n\
  \"output_formats\": [\"txt\", \"csv\", \"md\", \"json\"],\n\
  \"folder_styles\": [\"table\", \"classic\", \"compact\", \"json\"],\n\
  \"folder_presets\": [\"standard\", \"classic\", \"compact\", \"minimal\", \"mastering\", \"all\", \"json\"],\n\
  \"folder_columns\": [{}],\n\
  \"single_file_rows\": [\"max_tpl\", \"max_sppm\", \"rms\", \"max_momentary\", \"max_short_term\", \"integrated_loudness\", \"dr_pmf\", \"min_psr\", \"plr\", \"lra\", \"replay_gain\", \"stereo_correlation\", \"peak_timing_diagnostics\"],\n\
  \"surround_modes\": [\"expand\", \"strict\"],\n\
  \"charsets\": [\"utf8\", \"utf8bom\", \"utf16le\", \"utf16be\", \"win1252\"],\n\
  \"line_endings\": [\"crlf\", \"lf\", \"cr\"],\n\
  \"override_flags\": [\n\
    \"--config <path>\", \"--no-config\", \"--set <key=value>\",\n\
    \"--format <txt|csv|md|json>\", \"--txt\", \"--csv\", \"--md\", \"--json\",\n\
    \"--style <table|classic|compact|json>\", \"--preset <name>\",\n\
    \"--columns <list>\", \"--add-column <list>\", \"--remove-column <list>\",\n\
    \"--single-rows <standard|all|list>\", \"--diagnostics\",\n\
    \"-r/--recurse\", \"--no-recurse\", \"--combine\", \"--no-combine\",\n\
    \"--lfe\", \"--no-lfe\", \"--tags\", \"--no-tags\",\n\
    \"--surround <expand|strict>\", \"-j/--threads <N>\",\n\
    \"--max-name-len <N>\", \"--no-max-name-len\", \"--ascii\", \"--no-ascii\",\n\
    \"--charset <mode>\", \"--line-ending <mode>\",\n\
    \"--folder-pattern <tpl>\", \"--single-pattern <tpl>\",\n\
    \"--output-dir <dir>\", \"--no-output-dir\",\n\
    \"--overwrite\", \"--no-overwrite\", \"--save\", \"--no-save\",\n\
    \"-o/--output <file>\", \"--progress\", \"--no-progress\", \"-q/--quiet\",\n\
    \"--print-config\", \"--print-config-json\", \"--help-json\"\n\
  ]\n\
}}",
        json_escape(env!("CARGO_PKG_VERSION")),
        col_names.join(", "),
    )
}

fn help_text(standalone_cli: bool) -> String {
    let usage = if standalone_cli {
        "Usage:\n  truepeaks-cli [OPTIONS] PATH ...\n\n\
Analyzes files, folders, or .cue sheets headlessly using your saved config.toml.\n\
All CLI overrides are temporary and never modify your saved config.toml.\n\n\
CLI & Output Options:\n"
    } else {
        "Usage:\n  truepeaks [OPTIONS] [PATH ...]\n\n\
Passing PATH(s) without headless options opens the GUI and analyzes them immediately.\n\
All CLI overrides are temporary and never modify your saved config.toml.\n\n\
Headless CLI Options:\n\
  --cli                     Run headlessly without opening a window\n"
    };
    format!(
        concat!(
            "TruePeaks v{} — Pure-Rust Loudness, True Peak & Dynamic Range Analyzer\n\n",
            "{}",
            "  --format <fmt>            Output format: txt (default), csv, md, json\n",
            "  --txt | --csv | --md | --json   Shorthand output format flags\n",
            "  --style <style>           Folder log style: table, classic, compact, json\n",
            "  --preset <name>           Column preset: standard, classic, compact, minimal, mastering, all, json\n",
            "  --columns <c1,c2,...>     Custom folder columns & left-to-right order (see --help-json)\n",
            "  --add-column <c1,...>     Enable additional folder column(s)\n",
            "  --remove-column <c1,...>  Disable folder column(s)\n",
            "  --single-rows <spec>      Single-file rows: standard, all, or comma list\n",
            "  --diagnostics             Include ReplayGain, Stereo Corr., and Peak Timing in single logs\n",
            "  -o, --output <path>       Write formatted report to a file instead of stdout\n",
            "  -r, --recurse / --no-recurse    Recurse into subdirectories\n",
            "  --combine / --no-combine        Combine multiple folders into one batch report\n",
            "  --lfe / --no-lfe                Include LFE channel in Joint DR average\n",
            "  --tags / --no-tags              Prefer embedded audio track tags for track names\n",
            "  --surround <expand|strict>      Surround per-channel columns vs strict L/R\n",
            "  -j, --threads <N>               Worker thread pool size\n",
            "  --max-name-len <N> | --no-max-len  Truncate logged track names (8..=255)\n",
            "  --ascii / --no-ascii            Strip non-ASCII characters inside log output\n",
            "  --charset <mode>                utf8, utf8bom, utf16le, utf16be, win1252\n",
            "  --line-ending <mode>            crlf, lf, cr\n",
            "  --folder-pattern <tpl>          Filename template for folder logs\n",
            "  --single-pattern <tpl>          Filename template for single-file logs\n",
            "  --output-dir <dir> | --no-output-dir   Central folder for saved logs\n",
            "  --overwrite / --no-overwrite    Overwrite existing saved log files\n",
            "  --save / --no-save              Save formatted logs to disk during CLI run\n\
\nConfig & Agent Helpers:\n",
            "  --no-config | --defaults        Ignore saved config.toml; use stock defaults\n",
            "  --config <path>                 Load temporary base config from a TOML file\n",
            "  --set <key=value>               Override any config field temporarily (repeatable)\n",
            "  --no-progress | -q, --quiet     Suppress live progress updates on stderr\n",
            "  --print-config                  Print effective resolved TOML config and exit\n",
            "  --print-config-json             Print effective output config as JSON and exit\n",
            "  --help-json                     Print machine-readable CLI schema JSON and exit\n",
            "  --longhelp                      Detailed reference manual for all options, columns & examples\n",
            "  -h, --help                      Show concise help message\n",
            "  -V, --version                   Show version"
        ),
        env!("CARGO_PKG_VERSION"),
        usage
    )
}

fn long_help_text(standalone_cli: bool) -> String {
    let bin = if standalone_cli {
        "truepeaks-cli"
    } else {
        "truepeaks --cli"
    };

    let mut cols_section = String::new();
    for col in FolderColumnId::ALL {
        cols_section.push_str(&format!("    {:<20} {}\n", col.cli_name(), col.tooltip()));
    }

    format!(
        concat!(
            "TruePeaks v{} — Detailed CLI Reference (--longhelp)\n",
            "========================================================================\n\n",
            "USAGE & EXECUTION MODES\n",
            "  {} [OPTIONS] [--] PATH ...\n\n",
            "  - Standalone binary (`truepeaks-cli`): Always runs headlessly in the console\n",
            "    and writes the finished report to stdout (or -o <file>). Contains no GUI code.\n",
            "  - Full binary (`truepeaks`): Launches the desktop GUI when invoked with no flags;\n",
            "    switches to headless mode when `--cli` or any headless option is supplied.\n",
            "  - Config persistence: All CLI flags apply temporarily in memory for a single run.\n",
            "    Your saved `config.toml` is NEVER modified on disk.\n",
            "  - Syntax: Both `--flag value` and `--flag=value` are accepted on all options.\n",
            "  - End of options: Use `--` before paths that begin with `-`.\n\n",
            "SUPPORTED INPUTS\n",
            "  Files   .wav, .bwf, .aif, .aifc, .aiff, .w64 (Wave64), .flac, .m4a, .alac,\n",
            "          .mp4, .caf, .mp3, .aac, .ogg, .oga\n",
            "  CD Cue  .cue sheets (single-pass sample-accurate multi-track slicing)\n",
            "  Folders Album folders (scanned in parallel; combine multiple paths or folders)\n\n",
            "OUTPUT FORMATS & REPORT EXPORTS\n",
            "  --format <fmt> | --txt | --csv | --md | --json\n",
            "      txt   Rendered using the active folder/single log formatter (`--style`). Default.\n",
            "      csv   Comma-separated per-track table with ISO-style header row.\n",
            "      md    GitHub-flavored Markdown album summary and aligned track table.\n",
            "      json  Structured JSON exporter schema (`album_dr`, `tracks` array).\n",
            "  -o, --output <file>\n",
            "      Write the formatted report to <file> using `--charset` and `--line-ending`\n",
            "      instead of printing to stdout.\n\n",
            "FOLDER LOG STYLES & PRESETS (`--format txt`)\n",
            "  --style <table|classic|compact|json>\n",
            "      table    Pipe-delimited multi-column spreadsheet (default).\n",
            "      classic  Traditional borderless foo_dr layout + album codec footer.\n",
            "      compact  Space-aligned mastering table with duration-weighted averages.\n",
            "      json     Customizable JSON folder report adhering to active/reordered columns.\n",
            "  --preset <standard|classic|compact|minimal|mastering|all|json>\n",
            "      Apply a built-in column set & order on top of the active style.\n\n",
            "CUSTOM FOLDER COLUMNS & ORDER\n",
            "  --columns <c1,c2,...>\n",
            "      Replace active folder columns and enforce the exact left-to-right order.\n",
            "      Example: --columns file_name,dr_pmf,lufsi,max_tpl_itu,min_psr,replay_gain\n",
            "  --add-column <c1,...> / --remove-column <c1,...>\n",
            "      Add or remove individual columns without discarding your other selections.\n\n",
            "  Available column identifiers:\n",
            "{}\n",
            "SINGLE-FILE REPORT ROWS\n",
            "  --diagnostics\n",
            "      Enable all 3 optional diagnostic rows (ReplayGain, Stereo Corr., Peak Timing).\n",
            "  --single-rows <standard|all|r1,r2,...>\n",
            "      Select exact rows for single-file text reports. Row identifiers:\n",
            "      max_tpl, max_sppm, rms, max_momentary, max_short_term, integrated_loudness,\n",
            "      dr_pmf, min_psr, plr, lra, replay_gain, stereo_correlation,\n",
            "      peak_timing_diagnostics\n\n",
            "SCANNING, DSP & METADATA\n",
            "  -r, --recurse / --no-recurse     Walk subdirectories recursively.\n",
            "  --combine / --no-combine         Merge multiple dropped/passed folders into 1 report.\n",
            "  --lfe / --no-lfe                 Include LFE (.1) channels in Joint PMF DR average.\n",
            "  --tags / --no-tags               Prefer embedded audio tags (TrackNumber - Title).\n",
            "  --surround <expand|strict>       expand: add L/R/C/LFE/Ls/Rs columns for >2ch;\n",
            "                                   strict: keep legacy Front L/R columns only.\n",
            "  -j, --threads <N>                Limit Rayon parallel worker threads (1..=cores).\n\n",
            "NAMING, ENCODING & ON-DISK LOG SAVING\n",
            "  --max-name-len <N>               Truncate logged track names inside reports (8..=255).\n",
            "  --no-max-name-len | --no-max-len Disable track name length truncation.\n",
            "  --ascii / --no-ascii             Strip non-ASCII characters inside log output only.\n",
            "  --charset <mode>                 utf8, utf8bom, utf16le, utf16be, win1252.\n",
            "  --line-ending <mode>             crlf, lf, cr.\n",
            "  --folder-pattern <template>      Filename template for saved folder logs.\n",
            "  --single-pattern <template>      Filename template for saved single-file logs.\n",
            "      Placeholders: {{folder_name}}, {{file_name}}, {{ext}}, {{album_dr}}, {{date}}, {{time}}\n",
            "  --output-dir <dir> | --no-output-dir\n",
            "      Save disk logs into a central directory instead of alongside audio files.\n",
            "  --overwrite / --no-overwrite     Overwrite existing logs instead of numbering _2, _3.\n",
            "  --save / --no-save               Write per-folder/per-file logs to disk during CLI run\n",
            "                                   (off by default in CLI mode).\n\n",
            "CONFIG LOADING, UNIVERSAL OVERRIDES & AGENT HELPERS\n",
            "  --no-config | --defaults         Ignore saved config.toml; start from stock defaults.\n",
            "  --config <path.toml>             Load base config from a custom TOML file.\n",
            "  --set <key=value>                Override any AppConfig field temporarily using TOML\n",
            "                                   dot-notation (repeatable). Unknown keys exit 2.\n",
            "      Examples: --set include_lfe_in_dr=true --set single_rows.replay_gain=true\n",
            "                --set folder_log_pattern={{folder_name}}_DR.txt\n",
            "  --print-config                   Print resolved effective config (TOML) and exit 0.\n",
            "  --print-config-json              Print resolved effective output config (JSON) and exit 0.\n",
            "  --help-json                      Print machine-readable JSON CLI schema and exit 0.\n\n",
            "PROGRESS, STREAMS & EXIT CODES\n",
            "  stdout   Pure formatted report (or --help/--print-config output). Safe for pipes.\n",
            "  stderr   Live scan progress (enabled by default; TTY carriage-return or non-TTY lines)\n",
            "           and error diagnostics. Pass --no-progress or -q/--quiet to silence progress.\n",
            "  Exit 0   Success\n",
            "  Exit 1   Runtime decode, scan, or IO error\n",
            "  Exit 2   Invalid CLI flag, missing argument value, or invalid config override\n\n",
            "EXAMPLES\n",
            "  1. Quick folder scan in default style:\n",
            "     {} ./MyAlbum\n\n",
            "  2. Reproducible quiet JSON export for scripts & AI agents (ignoring user config):\n",
            "     {} --no-config --json -q -r ./Library\n\n",
            "  3. Custom folder JSON report with selected metrics:\n",
            "     {} --style=json --columns=file_name,dr_pmf,lufsi,max_tpl_itu,replay_gain ./MyAlbum\n\n",
            "  4. Recursive batch scan saving classic DR logs into a central folder:\n",
            "     {} --no-config --style classic -r --save --output-dir ./Reports ./Music"
        ),
        env!("CARGO_PKG_VERSION"),
        bin,
        cols_section,
        bin,
        bin,
        bin,
        bin,
    )
}

pub fn attach_parent_console_if_windows() {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn AttachConsole(dwProcessId: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

pub fn execute_headless(
    target: ScanTarget,
    config: AppConfig,
    format: CliOutputFormat,
    output_path: Option<&Path>,
    show_progress: bool,
) -> Result<String, String> {
    let (tx, rx) = unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    spawn_scan_job(target, config.clone(), cancel, tx);

    let stderr_is_tty = show_progress && std::io::stderr().is_terminal();
    let mut last_tty_draw = Instant::now() - Duration::from_secs(1);
    let mut last_pct_step: isize = -1;
    let mut had_tty_line = false;
    let mut result: Option<Result<(String, ScanSnapshot, f64, usize), String>> = None;

    for ev in rx.iter() {
        match ev {
            ScanEvent::Started {
                total_files,
                target_label,
            } => {
                if show_progress {
                    if stderr_is_tty {
                        let msg = format!("\r[  0% | 0/{}] Scanning {}...", total_files, target_label);
                        eprint!("{:<78}", msg);
                        let _ = std::io::stderr().flush();
                        had_tty_line = true;
                        last_tty_draw = Instant::now();
                    } else {
                        eprintln!(
                            "[progress] scanning {} ({} files)",
                            target_label, total_files
                        );
                    }
                }
            }
            ScanEvent::Progress {
                completed,
                total,
                last_file,
            } => {
                if show_progress {
                    let denom = total.max(1);
                    let pct = ((completed.min(denom) * 100) / denom) as isize;
                    if stderr_is_tty {
                        if completed >= denom || last_tty_draw.elapsed() >= Duration::from_millis(60) {
                            let short = if last_file.chars().count() > 42 {
                                let prefix: String = last_file.chars().take(39).collect();
                                format!("{}...", prefix)
                            } else {
                                last_file
                            };
                            let msg = format!(
                                "\r[{:>3}% | {}/{}] {}",
                                pct, completed, denom, short
                            );
                            eprint!("{:<78}", msg);
                            let _ = std::io::stderr().flush();
                            had_tty_line = true;
                            last_tty_draw = Instant::now();
                        }
                    } else {
                        let step = if denom <= 10 { pct } else { pct / 10 };
                        if completed >= denom || step > last_pct_step {
                            last_pct_step = step;
                            eprintln!("[progress] {}/{} ({}%) {}", completed, denom, pct, last_file);
                        }
                    }
                }
            }
            ScanEvent::Completed {
                log_text,
                snapshot,
                elapsed_secs,
                tracks_count,
                ..
            } => {
                result = Some(Ok((log_text, snapshot, elapsed_secs, tracks_count)));
                break;
            }
            ScanEvent::Error(e) => {
                result = Some(Err(e));
                break;
            }
            ScanEvent::Cancelled => {
                result = Some(Err("Scan cancelled".to_string()));
                break;
            }
        }
    }

    if show_progress && had_tty_line {
        eprint!("\r{:<78}\r", "");
        let _ = std::io::stderr().flush();
    }

    let (log_text, snapshot, elapsed_secs, tracks_count) =
        result.unwrap_or_else(|| Err("Analysis worker exited unexpectedly".to_string()))?;

    if show_progress {
        eprintln!(
            "[done] {} {} analyzed in {:.2}s",
            tracks_count,
            if tracks_count == 1 { "file" } else { "files" },
            elapsed_secs
        );
    }

    let rendered = match format {
        CliOutputFormat::Text => log_text,
        CliOutputFormat::Csv => export_tracks(
            &snapshot.title(),
            &snapshot.owned_tracks(),
            ExportFormat::Csv,
            &config.log_filenames,
            config.use_embedded_tags,
            config.surround_folder_mode,
        ),
        CliOutputFormat::Markdown => export_tracks(
            &snapshot.title(),
            &snapshot.owned_tracks(),
            ExportFormat::Markdown,
            &config.log_filenames,
            config.use_embedded_tags,
            config.surround_folder_mode,
        ),
        CliOutputFormat::Json => export_tracks(
            &snapshot.title(),
            &snapshot.owned_tracks(),
            ExportFormat::Json,
            &config.log_filenames,
            config.use_embedded_tags,
            config.surround_folder_mode,
        ),
    };

    if let Some(out) = output_path {
        write_log_file(
            out,
            &rendered,
            config.charset,
            config.line_ending,
            true,
        )?;
    }

    Ok(rendered)
}
