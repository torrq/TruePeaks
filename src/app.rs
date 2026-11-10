use crate::config::{
    AppConfig, CharsetMode, FolderColumnsConfig, LineEndingMode, SingleFileRowsConfig,
    SurroundFolderMode, ThemeMode,
};
use crate::decoder::is_supported_audio_file;
use crate::formatter::expand_filename_pattern;
use crate::scanner::{spawn_scan_job, ScanEvent, ScanTarget};
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui::{
    self, Align, Color32, CursorIcon, FontId, Frame, Key, Layout, Margin, Modifiers, RichText,
    Rounding, Sense, Stroke, Vec2,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

const AUDIO_FILE_EXTENSIONS: &[&str] = &[
    "wav", "bwf", "aiff", "aif", "aifc", "w64", "flac", "m4a", "alac", "mp4", "caf", "mp3", "aac",
    "ogg", "oga",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    Appearance,
    FolderLogs,
    SingleLogs,
    Output,
}

impl SettingsSection {
    fn all() -> &'static [SettingsSection] {
        &[
            SettingsSection::Appearance,
            SettingsSection::FolderLogs,
            SettingsSection::SingleLogs,
            SettingsSection::Output,
        ]
    }

    fn label(&self) -> &'static str {
        match self {
            SettingsSection::Appearance => "Appearance & App",
            SettingsSection::FolderLogs => "Folder Logs (DR.txt)",
            SettingsSection::SingleLogs => "Single-File Logs",
            SettingsSection::Output => "Output & Files",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            SettingsSection::Appearance => "Theme, font scale & CPU threads",
            SettingsSection::FolderLogs => "Album columns & surround mode",
            SettingsSection::SingleLogs => "Per-track report metrics",
            SettingsSection::Output => "Auto-save, patterns & encoding",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Palette {
    dark: bool,
    canvas: Color32,
    surface: Color32,
    surface_alt: Color32,
    well: Color32,
    border: Color32,
    accent: Color32,
    accent_hover: Color32,
    text_primary: Color32,
    text_muted: Color32,
    pill_active_fill: Color32,
    pill_active_text: Color32,
    pill_inactive_fill: Color32,
    pill_inactive_text: Color32,
    pill_inactive_border: Color32,
    success: Color32,
    error: Color32,
}

impl Palette {
    fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                dark: true,
                canvas: Color32::from_rgb(17, 19, 25),
                surface: Color32::from_rgb(26, 29, 38),
                surface_alt: Color32::from_rgb(35, 39, 51),
                well: Color32::from_rgb(13, 15, 20),
                border: Color32::from_rgb(48, 53, 68),
                accent: Color32::from_rgb(59, 130, 246),
                accent_hover: Color32::from_rgb(96, 165, 250),
                text_primary: Color32::from_rgb(236, 239, 246),
                text_muted: Color32::from_rgb(154, 163, 180),
                pill_active_fill: Color32::from_rgb(37, 99, 235),
                pill_active_text: Color32::WHITE,
                pill_inactive_fill: Color32::from_rgb(33, 37, 48),
                pill_inactive_text: Color32::from_rgb(190, 196, 210),
                pill_inactive_border: Color32::from_rgb(60, 66, 84),
                success: Color32::from_rgb(74, 222, 128),
                error: Color32::from_rgb(248, 113, 113),
            }
        } else {
            Self {
                dark: false,
                canvas: Color32::from_rgb(244, 246, 250),
                surface: Color32::WHITE,
                surface_alt: Color32::from_rgb(235, 239, 246),
                well: Color32::WHITE,
                border: Color32::from_rgb(214, 220, 232),
                accent: Color32::from_rgb(37, 99, 235),
                accent_hover: Color32::from_rgb(29, 78, 216),
                text_primary: Color32::from_rgb(22, 26, 36),
                text_muted: Color32::from_rgb(92, 101, 118),
                pill_active_fill: Color32::from_rgb(37, 99, 235),
                pill_active_text: Color32::WHITE,
                pill_inactive_fill: Color32::from_rgb(238, 241, 247),
                pill_inactive_text: Color32::from_rgb(55, 65, 81),
                pill_inactive_border: Color32::from_rgb(203, 213, 225),
                success: Color32::from_rgb(21, 128, 61),
                error: Color32::from_rgb(185, 28, 28),
            }
        }
    }
}

fn to_egui_theme_preference(mode: ThemeMode) -> egui::ThemePreference {
    match mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Dark => egui::ThemePreference::Dark,
        ThemeMode::Light => egui::ThemePreference::Light,
    }
}

fn build_visuals(dark: bool) -> egui::Visuals {
    let pal = Palette::for_theme(dark);
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    v.panel_fill = pal.canvas;
    v.window_fill = pal.surface;
    v.faint_bg_color = pal.surface_alt;
    v.extreme_bg_color = pal.well;
    v.code_bg_color = pal.surface_alt;

    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(8.0);
    v.window_stroke = Stroke::new(1.0_f32, pal.border);
    v.slider_trailing_fill = true;
    v.interact_cursor = Some(CursorIcon::PointingHand);

    v.selection.bg_fill = if dark {
        Color32::from_rgba_unmultiplied(59, 130, 246, 95)
    } else {
        Color32::from_rgba_unmultiplied(37, 99, 235, 55)
    };
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);

    let r = Rounding::same(6.0);
    v.widgets.noninteractive.bg_fill = pal.surface;
    v.widgets.noninteractive.weak_bg_fill = pal.surface;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = pal.surface_alt;
    v.widgets.inactive.weak_bg_fill = pal.surface_alt;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    v.widgets.inactive.rounding = r;

    let hover_fill = if dark {
        Color32::from_rgb(47, 53, 68)
    } else {
        Color32::from_rgb(225, 231, 242)
    };
    v.widgets.hovered.bg_fill = hover_fill;
    v.widgets.hovered.weak_bg_fill = hover_fill;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    v.widgets.hovered.rounding = r;

    let active_fill = if dark {
        Color32::from_rgb(56, 63, 82)
    } else {
        Color32::from_rgb(212, 221, 238)
    };
    v.widgets.active.bg_fill = active_fill;
    v.widgets.active.weak_bg_fill = active_fill;
    v.widgets.active.bg_stroke = Stroke::new(1.5_f32, pal.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = active_fill;
    v.widgets.open.weak_bg_fill = active_fill;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    v.widgets.open.rounding = r;

    v
}

fn configure_theme_styles(ctx: &egui::Context, theme_mode: ThemeMode) {
    ctx.set_visuals_of(egui::Theme::Dark, build_visuals(true));
    ctx.set_visuals_of(egui::Theme::Light, build_visuals(false));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.window_margin = Margin::same(14.0);
        style.spacing.scroll.bar_width = 8.0;
    });
    ctx.set_theme(to_egui_theme_preference(theme_mode));
}

pub struct TruePeaksApp {
    config: AppConfig,
    show_settings: bool,
    settings_section: SettingsSection,

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
    last_was_error: bool,
    last_was_success: bool,
    last_saved_path: Option<PathBuf>,
    log_preview: String,
    copied_at: Option<Instant>,
}

impl TruePeaksApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        configure_theme_styles(&cc.egui_ctx, config.theme);
        Self {
            config,
            show_settings: false,
            settings_section: SettingsSection::Appearance,
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
            last_was_error: false,
            last_was_success: false,
            last_saved_path: None,
            log_preview: String::new(),
            copied_at: None,
        }
    }

    fn apply_and_save_theme(&mut self, ctx: &egui::Context, mode: ThemeMode) {
        self.config.theme = mode;
        ctx.set_theme(to_egui_theme_preference(mode));
        let _ = self.config.save();
    }

    fn pick_and_scan_file(&mut self) {
        if self.is_scanning {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio Files", AUDIO_FILE_EXTENSIONS)
            .pick_file()
        {
            self.start_scan(ScanTarget::SingleFile(path));
        }
    }

    fn pick_and_scan_folder(&mut self) {
        if self.is_scanning {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.start_scan(ScanTarget::Folder(dir));
        }
    }

    fn start_scan(&mut self, target: ScanTarget) {
        if self.is_scanning {
            return;
        }
        let target_label = match &target {
            ScanTarget::SingleFile(p) => p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Single File")
                .to_string(),
            ScanTarget::Folder(p) => p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Album Folder")
                .to_string(),
        };

        let (tx, rx) = unbounded();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.show_settings = false;
        self.is_scanning = true;
        self.scan_rx = Some(rx);
        self.cancel_flag = Some(cancel_flag.clone());
        self.scan_start_instant = Some(Instant::now());
        self.progress_completed = 0;
        self.progress_total = 1;
        self.current_target_label = target_label;
        self.last_file_scanned.clear();
        self.last_saved_path = None;
        self.last_was_error = false;
        self.last_was_success = false;
        self.copied_at = None;
        self.status_message = "Initializing scan...".to_string();

        spawn_scan_job(target, self.config.clone(), cancel_flag, tx);
    }

    fn cancel_active_scan(&mut self) {
        if let Some(ref flag) = self.cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn clear_preview(&mut self) {
        if self.is_scanning {
            return;
        }
        self.log_preview.clear();
        self.last_saved_path = None;
        self.last_was_error = false;
        self.last_was_success = false;
        self.copied_at = None;
        self.current_target_label.clear();
        self.status_message =
            "Drop an audio file or album folder anywhere in the window to begin.".to_string();
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
                        self.status_message =
                            format!("Scanning {} / {} files — {}", completed, total, last_file);
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
                        self.last_was_error = false;
                        self.last_was_success = true;
                        self.status_message = format!(
                            "Completed {} {} in {:.2}s",
                            tracks_count,
                            if tracks_count == 1 { "file" } else { "files" },
                            elapsed_secs
                        );
                        finished = true;
                    }
                    ScanEvent::Cancelled => {
                        self.last_was_error = false;
                        self.last_was_success = false;
                        self.status_message = "Scan cancelled.".to_string();
                        finished = true;
                    }
                    ScanEvent::Error(err) => {
                        self.last_was_error = true;
                        self.last_was_success = false;
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
                    self.last_was_error = true;
                    self.last_was_success = false;
                    self.status_message = format!(
                        "Unsupported file format: {:?}",
                        path.file_name().unwrap_or_default()
                    );
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let open_folder = ctx.input_mut(|i| {
            i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::O)
        });
        let open_file = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O));
        let toggle_settings = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma));
        let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));

        if open_folder {
            self.pick_and_scan_folder();
        } else if open_file {
            self.pick_and_scan_file();
        } else if toggle_settings {
            self.show_settings = !self.show_settings;
        } else if escape {
            if self.is_scanning {
                self.cancel_active_scan();
            } else if self.show_settings {
                self.show_settings = false;
            }
        }
    }

    fn render_top_toolbar(&mut self, ctx: &egui::Context, pal: &Palette) {
        let frame = Frame::none()
            .fill(pal.surface)
            .inner_margin(Margin::symmetric(14.0, 9.0))
            .stroke(Stroke::new(1.0_f32, pal.border));

        egui::TopBottomPanel::top("top_toolbar")
            .frame(frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    paint_waveform_badge(ui, 22.0, pal);
                    ui.label(
                        RichText::new("TruePeaks")
                            .size(18.0)
                            .strong()
                            .color(pal.text_primary),
                    );
                    ui.separator();

                    let can_pick = !self.is_scanning;
                    if ui
                        .add_enabled(
                            can_pick,
                            primary_button("Select Folder...", can_pick, pal)
                                .min_size(Vec2::new(0.0, 28.0)),
                        )
                        .on_hover_text("Analyze an album folder (Ctrl+Shift+O)")
                        .clicked()
                    {
                        self.pick_and_scan_folder();
                    }

                    if ui
                        .add_enabled(
                            can_pick,
                            egui::Button::new("Select File...")
                                .min_size(Vec2::new(0.0, 28.0)),
                        )
                        .on_hover_text("Analyze a single audio file (Ctrl+O)")
                        .clicked()
                    {
                        self.pick_and_scan_file();
                    }

                    if self.is_scanning
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancel Scan").color(pal.error),
                                )
                                .min_size(Vec2::new(0.0, 28.0)),
                            )
                            .on_hover_text("Abort the current analysis (Esc)")
                            .clicked()
                    {
                        self.cancel_active_scan();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.show_settings {
                            if ui
                                .add(
                                    primary_button("← Back to Analyzer", true, pal)
                                        .min_size(Vec2::new(0.0, 28.0)),
                                )
                                .on_hover_text("Return to the analyzer view (Esc or Ctrl+,)")
                                .clicked()
                            {
                                self.show_settings = false;
                            }
                        } else if ui
                            .add(egui::Button::new("Settings").min_size(Vec2::new(0.0, 28.0)))
                            .on_hover_text("Configure columns, theme, and log naming (Ctrl+,)")
                            .clicked()
                        {
                            self.show_settings = true;
                        }

                        let theme_label = match self.config.theme {
                            ThemeMode::System => {
                                let eff = if pal.dark { "Dark" } else { "Light" };
                                format!("Theme: Auto ({})", eff)
                            }
                            ThemeMode::Dark => "Theme: Dark".to_string(),
                            ThemeMode::Light => "Theme: Light".to_string(),
                        };
                        if ui
                            .add(egui::Button::new(theme_label).min_size(Vec2::new(0.0, 28.0)))
                            .on_hover_text("Cycle color theme: System Auto -> Dark -> Light")
                            .clicked()
                        {
                            let next = self.config.theme.next();
                            self.apply_and_save_theme(ctx, next);
                        }

                        if !self.show_settings && !self.log_preview.is_empty() {
                            if ui
                                .add_enabled(
                                    can_pick,
                                    egui::Button::new("Clear").min_size(Vec2::new(0.0, 28.0)),
                                )
                                .on_hover_text("Clear preview and return to start screen")
                                .clicked()
                            {
                                self.clear_preview();
                            }

                            let recently_copied = self
                                .copied_at
                                .map(|t| t.elapsed().as_secs_f32() < 2.0)
                                .unwrap_or(false);
                            if recently_copied {
                                ctx.request_repaint();
                            }
                            let copy_btn = if recently_copied {
                                egui::Button::new(
                                    RichText::new("✓ Copied").color(pal.success),
                                )
                                .stroke(Stroke::new(1.0_f32, pal.success))
                            } else {
                                egui::Button::new("Copy Log")
                            };
                            if ui
                                .add(copy_btn.min_size(Vec2::new(0.0, 28.0)))
                                .on_hover_text("Copy log output to clipboard")
                                .clicked()
                            {
                                ctx.copy_text(self.log_preview.clone());
                                self.copied_at = Some(Instant::now());
                            }
                        }
                    });
                });
            });
    }

    fn render_bottom_status_bar(&self, ctx: &egui::Context, pal: &Palette) {
        let frame = Frame::none()
            .fill(pal.surface)
            .inner_margin(Margin::symmetric(14.0, 9.0))
            .stroke(Stroke::new(1.0_f32, pal.border));

        egui::TopBottomPanel::bottom("bottom_status")
            .frame(frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let dot_color = if self.is_scanning {
                        pal.accent
                    } else if self.last_was_error {
                        pal.error
                    } else if self.last_was_success {
                        pal.success
                    } else {
                        pal.text_muted
                    };

                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.2, dot_color);

                    let msg_color = if self.last_was_error {
                        pal.error
                    } else {
                        pal.text_primary
                    };
                    ui.label(
                        RichText::new(&self.status_message)
                            .size(15.0)
                            .color(msg_color),
                    );

                    if let Some(ref saved) = self.last_saved_path {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Show in Folder").size(13.5),
                                    )
                                    .min_size(Vec2::new(0.0, 24.0)),
                                )
                                .on_hover_text(format!("Reveal {}", saved.display()))
                                .clicked()
                            {
                                reveal_in_file_manager(saved);
                            }

                            let display_name = saved
                                .file_name()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| saved.display().to_string());
                            ui.label(
                                RichText::new(format!("Saved: {}", display_name))
                                    .size(14.5)
                                    .color(pal.success),
                            )
                            .on_hover_text(saved.display().to_string());
                        });
                    }
                });

                if self.is_scanning {
                    ui.add_space(6.0);
                    let frac =
                        (self.progress_completed as f32) / (self.progress_total.max(1) as f32);
                    let elapsed = self
                        .scan_start_instant
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);
                    let prog_label = if self.last_file_scanned.is_empty() {
                        format!(
                            "{} / {} files  ({:.1}s)",
                            self.progress_completed, self.progress_total, elapsed
                        )
                    } else {
                        format!(
                            "{} / {} files  ·  {}  ({:.1}s)",
                            self.progress_completed,
                            self.progress_total,
                            self.last_file_scanned,
                            elapsed
                        )
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_height(22.0)
                            .fill(pal.accent)
                            .text(RichText::new(prog_label).size(13.5)),
                    );
                }
            });
    }

    fn render_central_panel(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.show_settings {
            egui::CentralPanel::default()
                .frame(Frame::none().fill(pal.canvas).inner_margin(Margin::ZERO))
                .show(ctx, |ui| {
                    self.render_settings_view(ui, ctx, pal);
                });
            return;
        }

        let is_hovering_drop =
            !self.is_scanning && ctx.input(|i| !i.raw.hovered_files.is_empty());

        let resp = egui::CentralPanel::default()
            .frame(Frame::none().fill(pal.canvas).inner_margin(Margin::same(14.0)))
            .show(ctx, |ui| {
                if self.log_preview.is_empty() && !self.is_scanning {
                    self.render_empty_drop_zone(ui, pal);
                } else {
                    self.render_log_viewer(ui, pal);
                }
            });

        if is_hovering_drop {
            let rect = resp.response.rect.shrink(8.0);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let tint = Color32::from_rgba_unmultiplied(
                pal.accent.r(),
                pal.accent.g(),
                pal.accent.b(),
                26,
            );
            painter.rect(rect, Rounding::same(12.0), tint, Stroke::new(2.0_f32, pal.accent));

            let badge_center = rect.center();
            let pill_rect = egui::Rect::from_center_size(badge_center, Vec2::new(360.0, 56.0));
            painter.rect(
                pill_rect,
                Rounding::same(12.0),
                pal.surface,
                Stroke::new(1.5_f32, pal.accent),
            );
            painter.text(
                badge_center,
                egui::Align2::CENTER_CENTER,
                "Release to analyze audio file or album folder",
                FontId::proportional(16.0),
                pal.text_primary,
            );
        }
    }

    fn render_empty_drop_zone(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let avail = ui.available_size();
        let card_width = (avail.x - 32.0).clamp(320.0, 620.0);
        let top_pad = ((avail.y - 290.0) * 0.42).max(16.0);

        ui.vertical_centered(|ui| {
            ui.add_space(top_pad);
            Frame::none()
                .fill(pal.surface)
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::symmetric(28.0, 28.0))
                .show(ui, |ui| {
                    ui.set_max_width(card_width);
                    ui.vertical_centered(|ui| {
                        paint_waveform_badge(ui, 44.0, pal);
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Drop an Audio File or Album Folder Here")
                                .size(22.0)
                                .strong()
                                .color(pal.text_primary),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "ITU-R BS.1770-4 True Peak, Loudness (LUFS / LRA), Continuous PSR & PMF Dynamic Range",
                            )
                            .size(13.5)
                            .color(pal.text_muted),
                        );
                        ui.add_space(18.0);

                        ui.horizontal(|ui| {
                            let btn_w = 170.0;
                            let spacing = ui.spacing().item_spacing.x;
                            let pad = ((ui.available_width() - (btn_w * 2.0 + spacing)) * 0.5
                                - spacing)
                                .max(0.0);
                            ui.add_space(pad);
                            if ui
                                .add(
                                    primary_button("Choose Album Folder...", true, pal)
                                        .min_size(Vec2::new(btn_w, 34.0)),
                                )
                                .clicked()
                            {
                                self.pick_and_scan_folder();
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Choose Audio File...").size(14.0),
                                    )
                                    .min_size(Vec2::new(btn_w, 34.0)),
                                )
                                .clicked()
                            {
                                self.pick_and_scan_file();
                            }
                        });

                        ui.add_space(16.0);
                        ui.label(
                            RichText::new(
                                "WAV · BWF · W64 · AIFF · FLAC · ALAC · MP3 · AAC · Ogg   |   Mono–7.1+ · 44.1–384 kHz",
                            )
                            .size(12.0)
                            .color(pal.text_muted),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Ctrl+Shift+O Folder  ·  Ctrl+O File  ·  Ctrl+, Settings")
                                .size(11.5)
                                .color(pal.text_muted),
                        );
                    });
                });
        });
    }

    fn render_log_viewer(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let outer_frame = Frame::none()
            .fill(pal.well)
            .rounding(Rounding::same(8.0))
            .stroke(Stroke::new(1.0_f32, pal.border));

        outer_frame.show(ui, |ui| {
            // Subtle preview header bar (target label + monospace font zoom)
            Frame::none()
                .fill(pal.surface)
                .inner_margin(Margin::symmetric(12.0, 6.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let title = if self.current_target_label.is_empty() {
                            "Analysis Log".to_string()
                        } else {
                            self.current_target_label.clone()
                        };
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .strong()
                                .color(pal.text_primary),
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("A+").size(12.5))
                                        .min_size(Vec2::new(28.0, 22.0)),
                                )
                                .on_hover_text("Increase log font size")
                                .clicked()
                            {
                                self.config.log_font_size =
                                    (self.config.log_font_size + 1.0).min(22.0);
                                let _ = self.config.save();
                            }
                            ui.label(
                                RichText::new(format!("{:.0} pt", self.config.log_font_size))
                                    .size(12.5)
                                    .color(pal.text_muted),
                            );
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("A-").size(12.5))
                                        .min_size(Vec2::new(28.0, 22.0)),
                                )
                                .on_hover_text("Decrease log font size")
                                .clicked()
                            {
                                self.config.log_font_size =
                                    (self.config.log_font_size - 1.0).max(10.0);
                                let _ = self.config.save();
                            }
                        });
                    });
                });

            Frame::none()
                .inner_margin(Margin::same(10.0))
                .show(ui, |ui| {
                    let font_size = self.config.log_font_size;
                    let text_color = pal.text_primary;
                    let mut layouter = |ui: &egui::Ui, string: &str, _wrap_width: f32| {
                        let job = egui::text::LayoutJob::simple(
                            string.to_owned(),
                            FontId::monospace(font_size),
                            text_color,
                            f32::INFINITY,
                        );
                        ui.fonts(|f| f.layout_job(job))
                    };

                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.log_preview.as_str())
                                    .frame(false)
                                    .font(FontId::monospace(font_size))
                                    .layouter(&mut layouter)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                });
        });
    }

    fn render_settings_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
        let mut config_changed = false;
        let full_h = ui.available_height();

        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal_top(|ui| {
            // Left Sidebar Navigation (pinned full height)
            let nav_width = 224.0;
            let (nav_rect, _) = ui.allocate_exact_size(Vec2::new(nav_width, full_h), Sense::hover());
            ui.painter().rect_filled(nav_rect, Rounding::ZERO, pal.surface);
            ui.painter().line_segment(
                [nav_rect.right_top(), nav_rect.right_bottom()],
                Stroke::new(1.0_f32, pal.border),
            );

            let mut nav_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(nav_rect.shrink2(Vec2::new(12.0, 14.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            nav_ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);

            nav_ui.label(
                RichText::new("SETTINGS")
                    .size(11.0)
                    .strong()
                    .color(pal.text_muted),
            );
            nav_ui.add_space(6.0);

            for &sec in SettingsSection::all() {
                let selected = self.settings_section == sec;
                let (btn_rect, resp) =
                    nav_ui.allocate_exact_size(Vec2::new(nav_rect.width() - 24.0, 46.0), Sense::click());
                let fill = if selected {
                    Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), if pal.dark { 42 } else { 26 })
                } else if resp.hovered() {
                    pal.surface_alt
                } else {
                    Color32::TRANSPARENT
                };
                nav_ui.painter().rect_filled(btn_rect, Rounding::same(8.0), fill);
                if selected {
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(btn_rect.left() + 2.0, btn_rect.top() + 8.0),
                        egui::pos2(btn_rect.left() + 5.0, btn_rect.bottom() - 8.0),
                    );
                    nav_ui.painter().rect_filled(bar, Rounding::same(2.0), pal.accent);
                }
                let title_color = if selected {
                    if pal.dark {
                        pal.accent_hover
                    } else {
                        pal.accent
                    }
                } else {
                    pal.text_primary
                };
                nav_ui.painter().text(
                    egui::pos2(btn_rect.left() + 14.0, btn_rect.top() + 14.0),
                    egui::Align2::LEFT_CENTER,
                    sec.label(),
                    FontId::proportional(13.5),
                    title_color,
                );
                nav_ui.painter().text(
                    egui::pos2(btn_rect.left() + 14.0, btn_rect.top() + 32.0),
                    egui::Align2::LEFT_CENTER,
                    sec.description(),
                    FontId::proportional(11.0),
                    pal.text_muted,
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.settings_section = sec;
                }
            }

            // Bottom sidebar actions (Restore Defaults)
            let bottom_size = nav_ui.available_size();
            nav_ui.allocate_ui_with_layout(bottom_size, Layout::bottom_up(Align::Min), |ui| {
                if ui
                    .add_sized(
                        Vec2::new(nav_rect.width() - 24.0, 30.0),
                        egui::Button::new(RichText::new("Restore Defaults").size(12.5)),
                    )
                    .on_hover_text("Reset all settings (theme, columns, rows, naming, and encoding) back to defaults")
                    .clicked()
                {
                    self.config = AppConfig::default();
                    ctx.set_theme(to_egui_theme_preference(self.config.theme));
                    config_changed = true;
                }
                ui.add_space(4.0);
                if let Some(cfg_path) = AppConfig::config_path() {
                    ui.label(
                        RichText::new("Changes save automatically")
                            .size(11.0)
                            .color(pal.text_muted),
                    )
                    .on_hover_text(format!("Config path:\n{}", cfg_path.display()));
                }
            });

            // Right Scrollable Settings Pane
            let content_rect = ui.available_rect_before_wrap();
            let mut content_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            content_ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

            egui::ScrollArea::vertical()
                .id_salt("settings_content_scroll")
                .auto_shrink([false, false])
                .show(&mut content_ui, |ui| {
                    Frame::none()
                        .inner_margin(Margin::symmetric(28.0, 22.0))
                        .show(ui, |ui| {
                            let max_w = ui.available_width().min(760.0);
                            ui.set_width(max_w);

                            match self.settings_section {
                                SettingsSection::Appearance => {
                                    config_changed |= self.render_settings_appearance(ui, ctx, pal);
                                }
                                SettingsSection::FolderLogs => {
                                    config_changed |= self.render_settings_folder_logs(ui, pal);
                                }
                                SettingsSection::SingleLogs => {
                                    config_changed |= self.render_settings_single_logs(ui, pal);
                                }
                                SettingsSection::Output => {
                                    config_changed |= self.render_settings_output(ui, pal);
                                }
                            }
                        });
                });
        });

        if config_changed {
            let _ = self.config.save();
        }
    }

    fn render_settings_appearance(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
    ) -> bool {
        let mut changed = false;
        settings_section_header(
            ui,
            pal,
            "Appearance & App",
            "Customize light/dark colors, log typography, and parallel processing.",
        );

        settings_card(
            ui,
            pal,
            "Theme & Typography",
            "Visual presentation and monospace legibility in the analyzer preview.",
            |ui| {
                let theme_desc = if self.config.theme == ThemeMode::System {
                    match ctx.system_theme() {
                        Some(egui::Theme::Dark) => "Following OS preference (currently Dark)",
                        Some(egui::Theme::Light) => "Following OS preference (currently Light)",
                        None => "Follows operating system Light/Dark mode",
                    }
                } else {
                    "Overrides system dark/light preference"
                };

                setting_row(
                    ui,
                    pal,
                    "Color Theme",
                    theme_desc,
                    "System Default automatically tracks your Windows/macOS/Linux light or dark setting live.",
                    |ui| {
                        ui.horizontal(|ui| {
                            for &mode in ThemeMode::all() {
                                let selected = self.config.theme == mode;
                                if ui
                                    .add(egui::Button::new(mode.label()).selected(selected))
                                    .clicked()
                                {
                                    self.config.theme = mode;
                                    ctx.set_theme(to_egui_theme_preference(mode));
                                    changed = true;
                                }
                            }
                        });
                    },
                );

                ui.separator();

                setting_row(
                    ui,
                    pal,
                    "Log Preview Font Size",
                    "Monospace scale for tables and track reports",
                    "Adjusts font size in the log viewer (10–22 pt). You can also use A- / A+ directly above any log.",
                    |ui| {
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut self.config.log_font_size, 10.0..=22.0)
                                    .suffix(" pt")
                                    .step_by(0.5),
                            )
                            .changed();
                    },
                );
            },
        );

        ui.add_space(12.0);
        let max_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        settings_card(
            ui,
            pal,
            "Performance",
            "Control how many tracks are decoded and analyzed in parallel.",
            |ui| {
                setting_row(
                    ui,
                    pal,
                    "Worker Threads",
                    &format!("Up to {} logical cores detected", max_cores),
                    "Controls the Rayon thread pool size during folder scans. Lowering this keeps background scans gentler on CPU usage.",
                    |ui| {
                        changed |= ui
                            .add(egui::Slider::new(&mut self.config.max_threads, 1..=max_cores))
                            .changed();
                    },
                );
            },
        );

        changed
    }

    fn render_settings_folder_logs(&mut self, ui: &mut egui::Ui, pal: &Palette) -> bool {
        let mut changed = false;
        settings_section_header(
            ui,
            pal,
            "Folder Logs (DR.txt)",
            "Choose which metrics appear in album summary tables and how multichannel files are summarized.",
        );

        let (active_cnt, total_cnt) = self.config.folder_columns.active_count();
        settings_card(
            ui,
            pal,
            "Summary Table Columns",
            "Hover any option for exact standard and DSP details.",
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Presets:").strong().color(pal.text_muted));
                    if ui
                        .button("Standard")
                        .on_hover_text("Classic balanced columns: File Name, DR (PMF/L/R), Max. TPL (ITU/L/R), Integrated Loudness")
                        .clicked()
                    {
                        self.config.folder_columns = FolderColumnsConfig::default();
                        changed = true;
                    }
                    if ui
                        .button("Minimal DR")
                        .on_hover_text("Compact table: File Name, DR (PMF/L/R), Joint Max. TPL, Integrated Loudness")
                        .clicked()
                    {
                        self.config.folder_columns = FolderColumnsConfig::minimal();
                        changed = true;
                    }
                    if ui
                        .button("Mastering")
                        .on_hover_text("Mastering dynamics view: File Name, DR (PMF), Max. TPL, LUFSi, LRA, Max. Momentary, Max. Short-Term, PLR, Min. PSR")
                        .clicked()
                    {
                        self.config.folder_columns = FolderColumnsConfig::mastering();
                        changed = true;
                    }
                    if ui
                        .button("All Columns")
                        .on_hover_text("Enable all 26 available columns")
                        .clicked()
                    {
                        self.config.folder_columns = FolderColumnsConfig::all_enabled();
                        changed = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} / {} active", active_cnt, total_cnt))
                                .size(12.5)
                                .color(pal.text_muted),
                        );
                    });
                });

                ui.add_space(6.0);
                ui.separator();
                let cols = &mut self.config.folder_columns;

                ui.label(RichText::new("Track Info & Bit Depth").strong().size(13.0));
                ui.horizontal_wrapped(|ui| {
                    changed |= pill_toggle_tip(
                        ui,
                        "File Name",
                        &mut cols.file_name,
                        pal,
                        "Source audio filename stem (leftmost table column).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "File Format",
                        &mut cols.file_format,
                        pal,
                        "Audio container/codec extension (for example .flac, .wav, .mp3).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Sample Rate",
                        &mut cols.sample_rate,
                        pal,
                        "Sample rate in kHz (for example 44.1k, 48k, 96k, 192k).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Word Length",
                        &mut cols.word_length,
                        pal,
                        "Nominal container bit depth (for example 16 or 24 bit PCM).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Bits Used",
                        &mut cols.bits_used,
                        pal,
                        "Effective active PCM resolution scanned across all samples (reveals 16-bit audio padded inside a 24-bit file).",
                    );
                });

                ui.add_space(4.0);
                ui.separator();
                ui.label(RichText::new("Dynamic Range & Peaks").strong().size(13.0));
                ui.horizontal_wrapped(|ui| {
                    changed |= pill_toggle_tip(
                        ui,
                        "DR (PMF)",
                        &mut cols.dr_pmf,
                        pal,
                        "Rounded integer Pleasurize Music Foundation DR score averaged across active channels.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "DR LEFT",
                        &mut cols.dr_left,
                        pal,
                        "Unrounded left-channel PMF DR (2nd-highest 3 s block peak minus top-20% RMS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "DR RIGHT",
                        &mut cols.dr_right,
                        pal,
                        "Unrounded right-channel PMF DR (2nd-highest 3 s block peak minus top-20% RMS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. TPL (ITU)",
                        &mut cols.max_tpl_itu,
                        pal,
                        "Highest 4x oversampled ITU-R BS.1770-4 True Peak across all channels (dBTP).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. TPL LEFT",
                        &mut cols.max_tpl_left,
                        pal,
                        "Left-channel 4x oversampled ITU-R BS.1770-4 True Peak (dBTP).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. TPL RIGHT",
                        &mut cols.max_tpl_right,
                        pal,
                        "Right-channel 4x oversampled ITU-R BS.1770-4 True Peak (dBTP).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. SPPM (JOINT)",
                        &mut cols.max_sppm_joint,
                        pal,
                        "Highest raw digital Sample Peak Program level across channels without oversampling (dBFS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. SPPM LEFT",
                        &mut cols.max_sppm_left,
                        pal,
                        "Left-channel raw digital Sample Peak magnitude (dBFS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. SPPM RIGHT",
                        &mut cols.max_sppm_right,
                        pal,
                        "Right-channel raw digital Sample Peak magnitude (dBFS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "RMS LEFT",
                        &mut cols.rms_left,
                        pal,
                        "Left-channel AES17 sine-normalized RMS over 3 s PMF blocks (dBFS).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "RMS RIGHT",
                        &mut cols.rms_right,
                        pal,
                        "Right-channel AES17 sine-normalized RMS over 3 s PMF blocks (dBFS).",
                    );
                });

                ui.add_space(4.0);
                ui.separator();
                ui.label(
                    RichText::new("Loudness (BS.1770-4) & Crest Ratios")
                        .strong()
                        .size(13.0),
                );
                ui.horizontal_wrapped(|ui| {
                    changed |= pill_toggle_tip(
                        ui,
                        "Integrated Loudness (ITU)",
                        &mut cols.lufsi,
                        pal,
                        "Gated whole-track Integrated Loudness in LUFS per ITU-R BS.1770-4 / EBU R128.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "LRA (ITU)",
                        &mut cols.lra,
                        pal,
                        "Loudness Range in LU measuring macro-dynamic spread across gated Short-Term windows.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Mom. (ITU)",
                        &mut cols.max_mom_itu,
                        pal,
                        "Peak continuous sample-sliding 400 ms Momentary Loudness across joint weighted channels.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Short-Term (ITU)",
                        &mut cols.max_short_term_itu,
                        pal,
                        "Peak continuous sample-sliding 3.0 s Short-Term Loudness across joint weighted channels.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Mom. LEFT",
                        &mut cols.max_mom_left,
                        pal,
                        "Left-channel continuous 400 ms Momentary peak calibrated with +3.01 dB dual-mono convention.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Mom. RIGHT",
                        &mut cols.max_mom_right,
                        pal,
                        "Right-channel continuous 400 ms Momentary peak calibrated with +3.01 dB dual-mono convention.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Short-Term LEFT",
                        &mut cols.max_short_term_left,
                        pal,
                        "Left-channel continuous 3.0 s Short-Term peak calibrated with +3.01 dB dual-mono convention.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Max. Short-Term RIGHT",
                        &mut cols.max_short_term_right,
                        pal,
                        "Right-channel continuous 3.0 s Short-Term peak calibrated with +3.01 dB dual-mono convention.",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "PLR (AES)",
                        &mut cols.plr,
                        pal,
                        "Peak-to-Integrated-Loudness Ratio in LU (reference peak minus LUFSi).",
                    );
                    changed |= pill_toggle_tip(
                        ui,
                        "Min. PSR (AES)",
                        &mut cols.min_psr,
                        pal,
                        "Lowest continuous 3.0 s Sample-Peak-to-Short-Term-Loudness ratio (highlights over-compressed sections).",
                    );
                });
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "Multichannel Surround Rules",
            "Configure how >2ch audio (5.1 / 7.1) maps into table columns and Joint DR.",
            |ui| {
                ui.label(RichText::new("Surround Folder Table Mode").strong())
                    .on_hover_text("Determines whether folder tables expand per-channel columns for C, LFE, Ls, Rs, etc., or stay strictly 2-column L/R.");
                changed |= ui
                    .radio_value(
                        &mut self.config.surround_folder_mode,
                        SurroundFolderMode::ExpandPerChannel,
                        SurroundFolderMode::ExpandPerChannel.label(),
                    )
                    .on_hover_text("Automatically adds labeled columns (L, R, C, LFE, Ls, Rs, ...) whenever a scanned folder contains surround tracks.")
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.config.surround_folder_mode,
                        SurroundFolderMode::StrictTwoChannel,
                        SurroundFolderMode::StrictTwoChannel.label(),
                    )
                    .on_hover_text("Keeps legacy Front Left / Front Right columns only in folder logs while Joint ITU loudness/TPL still evaluates all active channels.")
                    .changed();

                ui.add_space(4.0);
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.include_lfe_in_dr,
                    "Include LFE channel in Joint DR (PMF) Average",
                    "Applies to both folder and single-file track DR (off by default to match ITU BS.1770 LFE exclusion)",
                    "When unchecked, dedicated LFE (.1) sub-bass channels are ignored when averaging per-channel DR into Track DR. ITU BS.1770 loudness always excludes LFE.",
                    pal,
                );
            },
        );

        changed
    }

    fn render_settings_single_logs(&mut self, ui: &mut egui::Ui, pal: &Palette) -> bool {
        let mut changed = false;
        settings_section_header(
            ui,
            pal,
            "Single-File Logs",
            "Choose which measurement rows appear when analyzing an individual track.",
        );

        ui.horizontal(|ui| {
            ui.label(RichText::new("Quick actions:").color(pal.text_muted));
            if ui
                .button("Enable All (Default)")
                .on_hover_text("Include all 10 standard single-file analysis rows")
                .clicked()
            {
                self.config.single_rows = SingleFileRowsConfig::default();
                changed = true;
            }
        });
        ui.add_space(6.0);

        let rows = &mut self.config.single_rows;
        settings_card(
            ui,
            pal,
            "Peaks & RMS Level",
            "Sample-accurate, oversampled inter-sample peak, and AES17 power rows.",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.max_tpl,
                    "Max. TPL (True Peak Level - BS.1770)",
                    "4x oversampled inter-sample peak per channel and jointly (dBTP)",
                    "Detects inter-sample overshoots between PCM samples using ITU-R BS.1770 4x oversampling.",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.max_sppm,
                    "Max. SPPM (Sample Peak Program Metering)",
                    "Maximum raw PCM sample magnitude per channel and jointly (dBFS)",
                    "Reports the exact highest sample magnitude present in the decoded waveform.",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.rms,
                    "RMS (AES17)",
                    "Sine-normalized Root Mean Square average level per channel (dBFS)",
                    "Full-scale sine reads -0.00 dBFS (+3.01 dB relative to raw unweighted RMS) averaged across 3 s PMF blocks.",
                    pal,
                );
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "ITU-R BS.1770-4 Loudness & Range",
            "Continuous sample-sliding perceptual K-weighted loudness measurements.",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.max_momentary,
                    "Max. Momentary Loudness (BS.1770)",
                    "Peak K-weighted energy over a continuous 400 ms sliding window",
                    "Evaluates every frame (no 100 ms grid gaps) so fast transient peaks are never skipped.",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.max_short_term,
                    "Max. Short-Term Loudness (BS.1770)",
                    "Peak K-weighted energy over a continuous 3.0 s sliding window",
                    "Evaluates every frame across 3.0 s windows; per-channel values use the +3.01 dB dual-mono convention.",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.integrated_loudness,
                    "Integrated Loudness (LUFS - BS.1770)",
                    "Dual-gated whole-track loudness in LUFS per ITU-R BS.1770-4 / EBU R128",
                    "Applies -70 LUFS absolute and -10 LU relative gating over 400 ms blocks (75% overlap).",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.lra,
                    "LRA (Loudness Range - BS.1770)",
                    "Macro-dynamic loudness variation between 10th and 95th percentiles (LU)",
                    "Quantifies overall level variation across the track using gated 3.0 s Short-Term loudness distributions.",
                    pal,
                );
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "Dynamic Range & Crest Ratios",
            "Track dynamic contrast metrics comparing peaks to loudness.",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.dr_pmf,
                    "DR (PMF Dynamic Range)",
                    "Top-20% RMS to 2nd-highest block peak difference per channel & integer Track DR",
                    "Follows Pleasurize Music Foundation block rules (132,480 frames at 44.1 kHz).",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.min_psr,
                    "Min. PSR (SPPM-to-Short-Term Loudness Ratio)",
                    "Minimum difference across aligned continuous 3.0 s Sample Peak and Short-Term windows",
                    "Gated to active audio above -70 LUFS. Highlights the most dynamically crushed 3-second passage.",
                    pal,
                );
                ui.separator();
                changed |= checkbox_row_tip(
                    ui,
                    &mut rows.plr,
                    "PLR (TP-to-Integrated Loudness Ratio)",
                    "Reference peak headroom minus Integrated Loudness in LU",
                    "Overall track peak-to-loudness headroom indicator (reference peak capped at 0 dBTP when clipping).",
                    pal,
                );
            },
        );

        changed
    }

    fn render_settings_output(&mut self, ui: &mut egui::Ui, pal: &Palette) -> bool {
        let mut changed = false;
        settings_section_header(
            ui,
            pal,
            "Output & Files",
            "Configure disk auto-saving, log filename patterns, character encoding, and line endings.",
        );

        settings_card(
            ui,
            pal,
            "Automatic Saving",
            "Write formatted logs automatically when scans complete.",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.save_logs_to_disk,
                    "Automatically write log files to disk after scan",
                    "Writes folder logs inside the scanned folder and single logs beside the audio file",
                    "Disable this if you only want to view or copy logs inside TruePeaks without creating files on disk.",
                    pal,
                );
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "Filename Patterns",
            "Customize output log names using dynamic tokens.",
            |ui| {
                let folder_preview = expand_filename_pattern(
                    &self.config.folder_log_pattern,
                    "Album Name",
                    None,
                    None,
                    Some(12),
                );
                setting_row(
                    ui,
                    pal,
                    "Folder Log Pattern",
                    &format!("Tokens: {{folder_name}}, {{album_dr}}, {{date}}, {{time}}   ·   Preview: {}", folder_preview),
                    "Filename used for album folder scans (default: DR.txt). Falls back to DR.txt if left empty.",
                    |ui| {
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.config.folder_log_pattern)
                                    .desired_width(220.0),
                            )
                            .changed();
                    },
                );

                ui.separator();

                let single_preview = expand_filename_pattern(
                    &self.config.single_log_pattern,
                    "Album Name",
                    Some("01 - Track Title"),
                    Some(".flac"),
                    Some(12),
                );
                setting_row(
                    ui,
                    pal,
                    "Single-File Log Pattern",
                    &format!("Tokens: {{file_name}}, {{ext}}, {{folder_name}}, {{date}}, {{time}}   ·   Preview: {}", single_preview),
                    "Filename used when scanning an individual audio file (default: {file_name}_log.txt).",
                    |ui| {
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.config.single_log_pattern)
                                    .desired_width(220.0),
                            )
                            .changed();
                    },
                );
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "Encoding & Line Endings",
            "Compatibility options for scripts and external tools.",
            |ui| {
                setting_row(
                    ui,
                    pal,
                    "Character Encoding",
                    "Text byte encoding written to .txt files",
                    "UTF-8 preserves international Unicode track titles cleanly; Windows-1252 targets legacy Windows tools.",
                    |ui| {
                        let resp = egui::ComboBox::from_id_salt("charset_combo")
                            .width(220.0)
                            .selected_text(self.config.charset.label())
                            .show_ui(ui, |ui| {
                                for &mode in CharsetMode::all() {
                                    if ui
                                        .selectable_value(&mut self.config.charset, mode, mode.label())
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        resp.response
                            .on_hover_text("Select output file character encoding.");
                    },
                );

                ui.separator();

                setting_row(
                    ui,
                    pal,
                    "Line Ending Style",
                    "Newline sequence used in exported logs",
                    "CRLF (\\r\\n) is standard on Windows Notepad and Python; LF (\\n) is standard on Linux and macOS.",
                    |ui| {
                        let resp = egui::ComboBox::from_id_salt("line_ending_combo")
                            .width(220.0)
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
                                        changed = true;
                                    }
                                }
                            });
                        resp.response
                            .on_hover_text("Select newline style written to log files.");
                    },
                );
            },
        );

        changed
    }
}

fn settings_section_header(ui: &mut egui::Ui, pal: &Palette, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(20.0).strong().color(pal.text_primary));
    ui.add_space(2.0);
    ui.label(RichText::new(subtitle).size(13.0).color(pal.text_muted));
    ui.add_space(12.0);
}

fn settings_card(
    ui: &mut egui::Ui,
    pal: &Palette,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    Frame::none()
        .fill(pal.surface)
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0_f32, pal.border))
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(15.0).strong().color(pal.text_primary));
            ui.label(RichText::new(subtitle).size(12.0).color(pal.text_muted));
            ui.add_space(10.0);
            add_contents(ui);
        });
}

fn setting_row(
    ui: &mut egui::Ui,
    pal: &Palette,
    title: &str,
    subtitle: &str,
    tooltip: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    let avail_w = ui.available_width();
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width((avail_w - 250.0).max(160.0));
            ui.label(RichText::new(title).strong().color(pal.text_primary))
                .on_hover_text(tooltip);
            ui.label(RichText::new(subtitle).size(12.0).color(pal.text_muted))
                .on_hover_text(tooltip);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            add_control(ui);
        });
    });
}

fn checkbox_row_tip(
    ui: &mut egui::Ui,
    value: &mut bool,
    title: &str,
    subtitle: &str,
    tooltip: &str,
    pal: &Palette,
) -> bool {
    let mut changed = false;
    ui.horizontal_top(|ui| {
        let cb_resp = ui.checkbox(value, "").on_hover_text(tooltip);
        changed |= cb_resp.changed();
        ui.add_space(4.0);
        let text_resp = ui.vertical(|ui| {
            ui.label(RichText::new(title).strong().color(pal.text_primary))
                .on_hover_text(tooltip);
            ui.label(RichText::new(subtitle).size(12.0).color(pal.text_muted))
                .on_hover_text(tooltip);
        });
        if text_resp
            .response
            .interact(Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(tooltip)
            .clicked()
        {
            *value = !*value;
            changed = true;
        }
    });
    changed
}

fn primary_button<'a>(label: &str, enabled: bool, pal: &Palette) -> egui::Button<'a> {
    if enabled {
        egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
            .fill(pal.accent)
            .stroke(Stroke::new(1.0_f32, pal.accent))
    } else {
        egui::Button::new(RichText::new(label).color(pal.text_muted))
            .fill(pal.surface_alt)
            .stroke(Stroke::new(1.0_f32, pal.border))
    }
}

fn paint_waveform_badge(ui: &mut egui::Ui, size: f32, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let corner = (size * 0.24).clamp(4.0, 12.0);
    let bg = Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 36);
    painter.rect(rect, Rounding::same(corner), bg, Stroke::new(1.0_f32, pal.accent));

    // Symmetrical 5-bar peak meter silhouette
    let heights = [0.28_f32, 0.56, 0.72, 0.48, 0.30];
    let bar_w = (size * 0.095).max(2.0);
    let gap = (size * 0.065).max(1.5);
    let total_w = (heights.len() as f32) * bar_w + ((heights.len() - 1) as f32) * gap;
    let start_x = rect.center().x - total_w * 0.5;
    let cy = rect.center().y;

    for (idx, &frac) in heights.iter().enumerate() {
        let x = start_x + (idx as f32) * (bar_w + gap);
        let h = size * frac;
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(x, cy - h * 0.5),
            egui::pos2(x + bar_w, cy + h * 0.5),
        );
        let color = if idx == 2 { pal.accent_hover } else { pal.accent };
        painter.rect_filled(bar_rect, Rounding::same(bar_w * 0.5), color);
    }
}

fn reveal_in_file_manager(path: &Path) {
    let raw = path.to_string_lossy();
    let clean = raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string();

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", clean))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .args(["-R", &clean])
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = Path::new(&clean)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(clean);
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

fn pill_toggle_tip(
    ui: &mut egui::Ui,
    label: &str,
    active: &mut bool,
    pal: &Palette,
    tooltip: &str,
) -> bool {
    let (fill, text_color, stroke) = if *active {
        (
            pal.pill_active_fill,
            pal.pill_active_text,
            Stroke::new(1.0_f32, pal.pill_active_fill),
        )
    } else {
        (
            pal.pill_inactive_fill,
            pal.pill_inactive_text,
            Stroke::new(1.0_f32, pal.pill_inactive_border),
        )
    };

    let display = if *active {
        format!("✓ {}", label)
    } else {
        label.to_string()
    };

    let btn = egui::Button::new(RichText::new(display).color(text_color))
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(14.0));

    if ui.add(btn).on_hover_text(tooltip).clicked() {
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
        self.handle_shortcuts(ctx);

        if self.is_scanning {
            ctx.request_repaint();
        }

        let pal = Palette::for_theme(ctx.theme() == egui::Theme::Dark);

        self.render_top_toolbar(ctx, &pal);
        self.render_bottom_status_bar(ctx, &pal);
        self.render_central_panel(ctx, &pal);
    }
}
