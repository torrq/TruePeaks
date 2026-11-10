use crate::config::{AppConfig, CharsetMode, LineEndingMode, SurroundFolderMode};
use crate::decoder::is_supported_audio_file;
use crate::scanner::{spawn_scan_job, ScanEvent, ScanTarget};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{self, Color32, FontId, RichText, Rounding, Stroke};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    FolderColumns,
    SingleFileRows,
    General,
}

pub struct TruePeaksApp {
    config: AppConfig,
    show_settings: bool,
    settings_tab: SettingsTab,

    // Active scan state
    is_scanning: bool,
    scan_rx: Option<Receiver<ScanEvent>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    scan_start_instant: Option<Instant>,
    progress_completed: usize,
    progress_total: usize,
    current_target_label: String,
    last_file_scanned: String,

    // Output & status
    status_message: String,
    last_saved_path: Option<PathBuf>,
    log_preview: String,
}

impl TruePeaksApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let config = AppConfig::load();
        Self {
            config,
            show_settings: false,
            settings_tab: SettingsTab::FolderColumns,
            is_scanning: false,
            scan_rx: None,
            cancel_flag: None,
            scan_start_instant: None,
            progress_completed: 0,
            progress_total: 0,
            current_target_label: String::new(),
            last_file_scanned: String::new(),
            status_message: "Drop an audio file or album folder anywhere in the window to begin."
                .to_string(),
            last_saved_path: None,
            log_preview: String::new(),
        }
    }

    fn start_scan(&mut self, target: ScanTarget) {
        if self.is_scanning {
            return;
        }
        let (tx, rx) = unbounded();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.is_scanning = true;
        self.scan_rx = Some(rx);
        self.cancel_flag = Some(cancel_flag.clone());
        self.scan_start_instant = Some(Instant::now());
        self.progress_completed = 0;
        self.progress_total = 1;
        self.last_saved_path = None;
        self.status_message = "Initializing scan...".to_string();

        spawn_scan_job(target, self.config.clone(), cancel_flag, tx);
    }

    fn cancel_active_scan(&mut self) {
        if let Some(ref flag) = self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn poll_scan_events(&mut self) {
        let mut finished = false;
        if let Some(ref rx) = self.scan_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    ScanEvent::Started {
                        total_files,
                        target_label,
                    } => {
                        self.progress_total = total_files.max(1);
                        self.progress_completed = 0;
                        self.current_target_label = target_label.clone();
                        self.status_message =
                            format!("Scanning 0 / {} files ({})...", total_files, target_label);
                    }
                    ScanEvent::Progress {
                        completed,
                        total,
                        last_file,
                    } => {
                        self.progress_completed = completed;
                        self.progress_total = total.max(1);
                        self.last_file_scanned = last_file.clone();
                        self.status_message = format!(
                            "Scanning {} / {} files — {}",
                            completed, total, last_file
                        );
                    }
                    ScanEvent::Completed {
                        log_text,
                        saved_path,
                        elapsed_secs,
                        tracks_count,
                    } => {
                        self.log_preview = log_text;
                        self.last_saved_path = saved_path;
                        self.progress_completed = self.progress_total;
                        self.status_message = format!(
                            "Completed {} {} in {:.2}s",
                            tracks_count,
                            if tracks_count == 1 { "file" } else { "files" },
                            elapsed_secs
                        );
                        finished = true;
                    }
                    ScanEvent::Cancelled => {
                        self.status_message = "Scan cancelled.".to_string();
                        finished = true;
                    }
                    ScanEvent::Error(err) => {
                        self.status_message = format!("Error: {}", err);
                        finished = true;
                    }
                }
            }
        }
        if finished {
            self.is_scanning = false;
            self.scan_rx = None;
            self.cancel_flag = None;
            self.scan_start_instant = None;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if self.is_scanning {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(first) = dropped.first() {
            if let Some(ref path) = first.path {
                if path.is_dir() {
                    self.start_scan(ScanTarget::Folder(path.clone()));
                } else if is_supported_audio_file(path) {
                    self.start_scan(ScanTarget::SingleFile(path.clone()));
                } else {
                    self.status_message = format!(
                        "Unsupported file format: {:?}",
                        path.file_name().unwrap_or_default()
                    );
                }
            }
        }
    }

    fn render_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let mut open = self.show_settings;
        let mut config_changed = false;

        egui::Window::new("TruePeaks Settings")
            .open(&mut open)
            .resizable(true)
            .default_width(760.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::FolderColumns,
                        "Folder Log Columns",
                    );
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::SingleFileRows,
                        "Single File Log Rows",
                    );
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::General,
                        "General & Formatting",
                    );
                });
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| match self.settings_tab {
                    SettingsTab::FolderColumns => {
                        ui.label(
                            "Click pill buttons to toggle Folder Log table columns (White = Active, Dark = Inactive):",
                        );
                        ui.add_space(6.0);

                        let cols = &mut self.config.folder_columns;

                        // 1. Info
                        ui.group(|ui| {
                            ui.label(RichText::new("Info").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |= pill_toggle(ui, "File Name", &mut cols.file_name);
                                config_changed |=
                                    pill_toggle(ui, "File Format", &mut cols.file_format);
                                config_changed |=
                                    pill_toggle(ui, "Sample Rate", &mut cols.sample_rate);
                                config_changed |=
                                    pill_toggle(ui, "Word Length", &mut cols.word_length);
                                config_changed |= pill_toggle(ui, "Bits Used", &mut cols.bits_used);
                            });
                        });

                        // 2. DR
                        ui.group(|ui| {
                            ui.label(RichText::new("DR").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |= pill_toggle(ui, "DR (PMF)", &mut cols.dr_pmf);
                                config_changed |= pill_toggle(ui, "DR LEFT", &mut cols.dr_left);
                                config_changed |= pill_toggle(ui, "DR RIGHT", &mut cols.dr_right);
                            });
                        });

                        // 3. True Peak Level
                        ui.group(|ui| {
                            ui.label(RichText::new("True Peak Level").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |=
                                    pill_toggle(ui, "Max. TPL (ITU)", &mut cols.max_tpl_itu);
                                config_changed |=
                                    pill_toggle(ui, "Max. TPL LEFT", &mut cols.max_tpl_left);
                                config_changed |=
                                    pill_toggle(ui, "Max. TPL RIGHT", &mut cols.max_tpl_right);
                            });
                        });

                        // 4. LUFS
                        ui.group(|ui| {
                            ui.label(RichText::new("LUFS").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |=
                                    pill_toggle(ui, "Integrated Loudness (ITU)", &mut cols.lufsi);
                                config_changed |= pill_toggle(ui, "LRA (ITU)", &mut cols.lra);
                                config_changed |=
                                    pill_toggle(ui, "Max. Mom. (ITU)", &mut cols.max_mom_itu);
                                config_changed |= pill_toggle(
                                    ui,
                                    "Max. Short-Term (ITU)",
                                    &mut cols.max_short_term_itu,
                                );
                            });
                        });

                        // 5. Channel Specific LUFS
                        ui.group(|ui| {
                            ui.label(RichText::new("Channel Specific LUFS").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |=
                                    pill_toggle(ui, "Max. Mom. LEFT", &mut cols.max_mom_left);
                                config_changed |=
                                    pill_toggle(ui, "Max. Mom. RIGHT", &mut cols.max_mom_right);
                                config_changed |= pill_toggle(
                                    ui,
                                    "Max. Short-Term LEFT",
                                    &mut cols.max_short_term_left,
                                );
                                config_changed |= pill_toggle(
                                    ui,
                                    "Max. Short-Term RIGHT",
                                    &mut cols.max_short_term_right,
                                );
                            });
                        });

                        // 6. Old Skool
                        ui.group(|ui| {
                            ui.label(RichText::new("Old Skool").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |= pill_toggle(
                                    ui,
                                    "Max. SPPM (JOINT)",
                                    &mut cols.max_sppm_joint,
                                );
                                config_changed |=
                                    pill_toggle(ui, "Max. SPPM LEFT", &mut cols.max_sppm_left);
                                config_changed |=
                                    pill_toggle(ui, "Max. SPPM RIGHT", &mut cols.max_sppm_right);
                                config_changed |= pill_toggle(ui, "RMS LEFT", &mut cols.rms_left);
                                config_changed |= pill_toggle(ui, "RMS RIGHT", &mut cols.rms_right);
                            });
                        });

                        // 7. Ratios
                        ui.group(|ui| {
                            ui.label(RichText::new("Ratios").strong());
                            ui.horizontal_wrapped(|ui| {
                                config_changed |= pill_toggle(ui, "PLR (AES)", &mut cols.plr);
                                config_changed |=
                                    pill_toggle(ui, "Min. PSR (AES)", &mut cols.min_psr);
                            });
                        });

                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.label(RichText::new("Surround (>2ch) Folder Log Mode").strong());
                            if ui
                                .radio_value(
                                    &mut self.config.surround_folder_mode,
                                    SurroundFolderMode::ExpandPerChannel,
                                    SurroundFolderMode::ExpandPerChannel.label(),
                                )
                                .changed()
                            {
                                config_changed = true;
                            }
                            if ui
                                .radio_value(
                                    &mut self.config.surround_folder_mode,
                                    SurroundFolderMode::StrictTwoChannel,
                                    SurroundFolderMode::StrictTwoChannel.label(),
                                )
                                .changed()
                            {
                                config_changed = true;
                            }
                        });
                    }

                    SettingsTab::SingleFileRows => {
                        ui.label("Select rows to include in Single File Analysis Logs:");
                        ui.add_space(6.0);
                        let rows = &mut self.config.single_rows;
                        ui.group(|ui| {
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.max_tpl,
                                    "Max. TPL (True Peak Level - BS.1770)",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.max_sppm,
                                    "Max. SPPM (Sample Peak Program Metering)",
                                )
                                .changed();
                            config_changed |= ui.checkbox(&mut rows.rms, "RMS").changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.max_momentary,
                                    "Max. Momentary Loudness (BS.1770)",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.max_short_term,
                                    "Max. Short-Term Loudness (BS.1770)",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.integrated_loudness,
                                    "Integrated Loudness (LUFS - BS.1770)",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(&mut rows.dr_pmf, "DR (PMF Dynamic Range)")
                                .changed();
                            config_changed |= ui
                                .checkbox(
                                    &mut rows.min_psr,
                                    "Min. PSR (SPPM-to-Short-Term Loudness Ratio)",
                                )
                                .changed();
                            config_changed |= ui
                                .checkbox(&mut rows.plr, "PLR (TP-to-Integrated Loudness Ratio)")
                                .changed();
                            config_changed |= ui
                                .checkbox(&mut rows.lra, "LRA (Loudness Range - BS.1770)")
                                .changed();
                        });
                    }

                    SettingsTab::General => {
                        let max_cores = std::thread::available_parallelism()
                            .map(|n| n.get())
                            .unwrap_or(4);

                        ui.group(|ui| {
                            ui.label(RichText::new("Parallelism & Performance").strong());
                            ui.horizontal(|ui| {
                                ui.label("Max Worker Threads / Cores:");
                                if ui
                                    .add(egui::Slider::new(
                                        &mut self.config.max_threads,
                                        1..=max_cores,
                                    ))
                                    .changed()
                                {
                                    config_changed = true;
                                }
                            });
                        });

                        ui.add_space(6.0);
                        ui.group(|ui| {
                            ui.label(RichText::new("Multichannel Surround DR").strong());
                            if ui
                                .checkbox(
                                    &mut self.config.include_lfe_in_dr,
                                    "Include LFE channel in Joint Multichannel DR (PMF) Average",
                                )
                                .changed()
                            {
                                config_changed = true;
                            }
                        });

                        ui.add_space(6.0);
                        ui.group(|ui| {
                            ui.label(RichText::new("Output File Encoding & Line Endings").strong());
                            ui.horizontal(|ui| {
                                ui.label("Character Encoding (Charset):");
                                egui::ComboBox::from_id_salt("charset_combo")
                                    .selected_text(self.config.charset.label())
                                    .show_ui(ui, |ui| {
                                        for &mode in CharsetMode::all() {
                                            if ui
                                                .selectable_value(
                                                    &mut self.config.charset,
                                                    mode,
                                                    mode.label(),
                                                )
                                                .changed()
                                            {
                                                config_changed = true;
                                            }
                                        }
                                    });
                            });

                            ui.horizontal(|ui| {
                                ui.label("Line Ending (CR Style):");
                                egui::ComboBox::from_id_salt("line_ending_combo")
                                    .selected_text(self.config.line_ending.label())
                                    .show_ui(ui, |ui| {
                                        for &mode in LineEndingMode::all() {
                                            if ui
                                                .selectable_value(
                                                    &mut self.config.line_ending,
                                                    mode,
                                                    mode.label(),
                                                )
                                                .changed()
                                            {
                                                config_changed = true;
                                            }
                                        }
                                    });
                            });
                        });

                        ui.add_space(6.0);
                        ui.group(|ui| {
                            ui.label(RichText::new("Output Filename Syntax").strong());
                            config_changed |= ui
                                .checkbox(
                                    &mut self.config.save_logs_to_disk,
                                    "Automatically write _log.txt files to disk after scan",
                                )
                                .changed();
                            ui.horizontal(|ui| {
                                ui.label("Folder Log Pattern:");
                                if ui
                                    .text_edit_singleline(&mut self.config.folder_log_pattern)
                                    .changed()
                                {
                                    config_changed = true;
                                }
                            });
                            ui.label(
                                RichText::new(
                                    "Tokens: {folder_name}, {album_dr}, {date}, {time}",
                                )
                                .small()
                                .weak(),
                            );
                            ui.horizontal(|ui| {
                                ui.label("Single File Log Pattern:");
                                if ui
                                    .text_edit_singleline(&mut self.config.single_log_pattern)
                                    .changed()
                                {
                                    config_changed = true;
                                }
                            });
                            ui.label(
                                RichText::new(
                                    "Tokens: {file_name}, {ext}, {folder_name}, {date}, {time}",
                                )
                                .small()
                                .weak(),
                            );
                        });
                    }
                });
            });

        self.show_settings = open;
        if config_changed {
            let _ = self.config.save();
        }
    }
}

fn pill_toggle(ui: &mut egui::Ui, label: &str, active: &mut bool) -> bool {
    let fill = if *active {
        Color32::from_rgb(245, 245, 250)
    } else {
        Color32::from_rgb(42, 44, 50)
    };
    let text_color = if *active {
        Color32::from_rgb(18, 18, 22)
    } else {
        Color32::from_rgb(200, 202, 210)
    };
    let stroke = if *active {
        Stroke::new(1.0_f32, Color32::WHITE)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgb(80, 84, 95))
    };

    let btn = egui::Button::new(RichText::new(label).color(text_color))
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(14.0));

    if ui.add(btn).clicked() {
        *active = !*active;
        true
    } else {
        false
    }
}

impl eframe::App for TruePeaksApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan_events();
        self.handle_dropped_files(ctx);

        if self.is_scanning {
            ctx.request_repaint();
        }

        // Top Toolbar
        egui::TopBottomPanel::top("top_toolbar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("TruePeaks");
                ui.separator();

                let can_pick = !self.is_scanning;
                if ui
                    .add_enabled(can_pick, egui::Button::new("Select File..."))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Audio Files",
                            &[
                                "wav", "bwf", "aiff", "aif", "aifc", "w64", "flac", "m4a", "alac",
                                "mp4", "caf", "mp3", "aac", "ogg", "oga",
                            ],
                        )
                        .pick_file()
                    {
                        self.start_scan(ScanTarget::SingleFile(path));
                    }
                }

                if ui
                    .add_enabled(can_pick, egui::Button::new("Select Folder..."))
                    .clicked()
                {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.start_scan(ScanTarget::Folder(dir));
                    }
                }

                if ui
                    .add_enabled(self.is_scanning, egui::Button::new("Cancel Scan"))
                    .clicked()
                {
                    self.cancel_active_scan();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Settings").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    if !self.log_preview.is_empty() && ui.button("Copy Log").clicked() {
                        ctx.copy_text(self.log_preview.clone());
                    }
                });
            });
            ui.add_space(4.0);
        });

        // Bottom Status & Progress Bar
        egui::TopBottomPanel::bottom("bottom_status").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
                if let Some(ref saved) = self.last_saved_path {
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Saved: {}", saved.display()))
                            .small()
                            .color(Color32::from_rgb(130, 210, 150)),
                    );
                }
            });

            if self.is_scanning {
                let frac = (self.progress_completed as f32) / (self.progress_total.max(1) as f32);
                let elapsed = self
                    .scan_start_instant
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or(0.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .show_percentage()
                        .text(format!(
                            "{} / {} files ({:.1}s)",
                            self.progress_completed, self.progress_total, elapsed
                        )),
                );
            }
            ui.add_space(4.0);
        });

        // Main Central Panel (Drop zone + Monospace Log Preview)
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.log_preview.is_empty() && !self.is_scanning {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.30);
                    ui.label(
                        RichText::new("Drop an Audio File or Album Folder Here")
                            .size(22.0)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Supports WAV, BWF/RF64, Wave64 (.w64), AIFF/AIFC, FLAC, ALAC (.m4a/.alac), MP3, AAC, Ogg Vorbis (Mono to 7.1+ Surround, 44.1kHz - 384kHz)",
                        )
                        .weak(),
                    );
                });
            } else {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.log_preview.as_str())
                                .font(FontId::monospace(12.5))
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
        });

        self.render_settings_window(ctx);
    }
}
