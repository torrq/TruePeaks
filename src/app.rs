use crate::config::{
    AppConfig, CharsetMode, FolderColumnsConfig, FolderLogStyle, LineEndingMode,
    SingleFileRowsConfig, SurroundFolderMode, ThemeMode, DEFAULT_WINDOW_HEIGHT,
    DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
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
use std::time::{Duration, Instant};

const AUDIO_FILE_EXTENSIONS: &[&str] = &[
    "wav", "bwf", "aiff", "aif", "aifc", "w64", "flac", "m4a", "alac", "mp4", "caf", "mp3", "aac",
    "ogg", "oga",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Analyzer,
    Settings,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AboutTab {
    Overview,
    MathDetails,
}

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
            SettingsSection::FolderLogs => "Log style, recursion & columns",
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
    active_view: ActiveView,
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
    last_saved_paths: Vec<PathBuf>,
    log_preview: String,
    copied_at: Option<Instant>,
    about_tab: AboutTab,
    about_url_copied_at: Option<Instant>,

    // Window geometry persistence
    window_frames_seen: u32,
    window_save_pending_at: Option<Instant>,
}

impl TruePeaksApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        configure_theme_styles(&cc.egui_ctx, config.theme);
        Self {
            config,
            active_view: ActiveView::Analyzer,
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
            last_saved_paths: Vec::new(),
            log_preview: String::new(),
            copied_at: None,
            about_tab: AboutTab::Overview,
            about_url_copied_at: None,
            window_frames_seen: 0,
            window_save_pending_at: None,
        }
    }

    fn track_window_geometry(&mut self, ctx: &egui::Context) {
        self.window_frames_seen = self.window_frames_seen.saturating_add(1);
        if self.window_frames_seen < 3 {
            return;
        }

        let vp = ctx.input(|i| i.viewport().clone());
        if vp.minimized.unwrap_or(false)
            || vp.maximized.unwrap_or(false)
            || vp.fullscreen.unwrap_or(false)
        {
            return;
        }

        let mut updated = false;

        if let Some(inner) = vp.inner_rect {
            let new_w = inner.width().round();
            let new_h = inner.height().round();
            if new_w.is_finite()
                && new_h.is_finite()
                && new_w >= MIN_WINDOW_WIDTH
                && new_h >= MIN_WINDOW_HEIGHT
                && ((self.config.window_width - new_w).abs() > 1.0
                    || (self.config.window_height - new_h).abs() > 1.0)
            {
                self.config.window_width = new_w;
                self.config.window_height = new_h;
                updated = true;
            }

            let pos_opt = if cfg!(target_os = "macos") {
                Some(inner.min)
            } else {
                vp.outer_rect.map(|r| r.min)
            };
            if let Some(pos) = pos_opt {
                if pos.x.is_finite()
                    && pos.y.is_finite()
                    && (-8192.0..=16_384.0).contains(&pos.x)
                    && (-8192.0..=16_384.0).contains(&pos.y)
                {
                    let new_x = pos.x.round();
                    let new_y = pos.y.round();
                    let pos_changed = match (self.config.window_x, self.config.window_y) {
                        (Some(ox), Some(oy)) => {
                            (ox - new_x).abs() > 1.0 || (oy - new_y).abs() > 1.0
                        }
                        _ => true,
                    };
                    if pos_changed {
                        self.config.window_x = Some(new_x);
                        self.config.window_y = Some(new_y);
                        updated = true;
                    }
                }
            }
        }

        if updated {
            self.window_save_pending_at = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        if let Some(since) = self.window_save_pending_at {
            if since.elapsed() >= Duration::from_millis(450) {
                self.window_save_pending_at = None;
                let _ = self.config.save();
            }
        }
    }

    fn reset_window_geometry(&mut self, ctx: &egui::Context) {
        self.config.window_x = None;
        self.config.window_y = None;
        self.config.window_width = DEFAULT_WINDOW_WIDTH;
        self.config.window_height = DEFAULT_WINDOW_HEIGHT;
        self.window_save_pending_at = None;
        self.window_frames_seen = 0;

        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
            DEFAULT_WINDOW_WIDTH,
            DEFAULT_WINDOW_HEIGHT,
        )));
        if let Some(mon) = ctx.input(|i| i.viewport().monitor_size) {
            let cx = ((mon.x - DEFAULT_WINDOW_WIDTH) * 0.5).max(0.0);
            let cy = ((mon.y - DEFAULT_WINDOW_HEIGHT) * 0.5).max(0.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(cx, cy)));
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
            ScanTarget::Multi { dirs, files } => {
                if !dirs.is_empty() && files.is_empty() {
                    format!("{} folders", dirs.len())
                } else if dirs.is_empty() {
                    format!("{} files", files.len())
                } else {
                    format!("{} folders, {} files", dirs.len(), files.len())
                }
            }
        };

        let (tx, rx) = unbounded();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_view = ActiveView::Analyzer;
        self.is_scanning = true;
        self.scan_rx = Some(rx);
        self.cancel_flag = Some(cancel_flag.clone());
        self.scan_start_instant = Some(Instant::now());
        self.progress_completed = 0;
        self.progress_total = 1;
        self.current_target_label = target_label;
        self.last_file_scanned.clear();
        self.last_saved_paths.clear();
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
        self.last_saved_paths.clear();
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
                        saved_paths,
                        elapsed_secs,
                        tracks_count,
                        folders_count,
                    } => {
                        self.log_preview = log_text;
                        let saved_cnt = saved_paths.len();
                        self.last_saved_paths = saved_paths;
                        self.progress_completed = self.progress_total;
                        self.last_was_error = false;
                        self.last_was_success = true;
                        let file_word = if tracks_count == 1 { "file" } else { "files" };
                        self.status_message = if folders_count > 1 && saved_cnt > 1 {
                            format!(
                                "Completed {} {} across {} folders ({} logs saved) in {:.2}s",
                                tracks_count, file_word, folders_count, saved_cnt, elapsed_secs
                            )
                        } else if folders_count > 1 {
                            format!(
                                "Completed {} {} across {} folders in {:.2}s",
                                tracks_count, file_word, folders_count, elapsed_secs
                            )
                        } else {
                            format!(
                                "Completed {} {} in {:.2}s",
                                tracks_count, file_word, elapsed_secs
                            )
                        };
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
        if dropped.is_empty() {
            return;
        }

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let mut first_bad: Option<PathBuf> = None;

        for item in dropped {
            if let Some(path) = item.path {
                if path.is_dir() {
                    if !dirs.contains(&path) {
                        dirs.push(path);
                    }
                } else if is_supported_audio_file(&path) {
                    if !files.contains(&path) {
                        files.push(path);
                    }
                } else if first_bad.is_none() {
                    first_bad = Some(path);
                }
            }
        }

        dirs.sort();
        files.sort();

        if dirs.len() == 1 && files.is_empty() {
            self.start_scan(ScanTarget::Folder(dirs.into_iter().next().unwrap()));
        } else if dirs.is_empty() && files.len() == 1 {
            self.start_scan(ScanTarget::SingleFile(files.into_iter().next().unwrap()));
        } else if !dirs.is_empty() || !files.is_empty() {
            self.start_scan(ScanTarget::Multi { dirs, files });
        } else if let Some(bad) = first_bad {
            self.last_was_error = true;
            self.last_was_success = false;
            self.status_message = format!(
                "Unsupported file format: {:?}",
                bad.file_name().unwrap_or_default()
            );
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
            self.active_view = if self.active_view == ActiveView::Settings {
                ActiveView::Analyzer
            } else {
                ActiveView::Settings
            };
        } else if escape {
            if self.is_scanning {
                self.cancel_active_scan();
            } else if self.active_view != ActiveView::Analyzer {
                self.active_view = ActiveView::Analyzer;
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
                    let title_galley = ui.painter().layout_no_wrap(
                        "TruePeaks".to_string(),
                        FontId::proportional(17.5),
                        pal.text_primary,
                    );
                    let badge_sz = 22.0_f32;
                    let pad_x = 7.0_f32;
                    let gap = 7.0_f32;
                    let brand_w = pad_x + badge_sz + gap + title_galley.size().x + pad_x;
                    let (brand_rect, brand_resp) =
                        ui.allocate_exact_size(Vec2::new(brand_w, 30.0), Sense::click());
                    let is_about = self.active_view == ActiveView::About;
                    let brand_fill = if is_about {
                        Color32::from_rgba_unmultiplied(
                            pal.accent.r(),
                            pal.accent.g(),
                            pal.accent.b(),
                            if pal.dark { 42 } else { 24 },
                        )
                    } else if brand_resp.hovered() {
                        pal.surface_alt
                    } else {
                        Color32::TRANSPARENT
                    };
                    let brand_stroke = if is_about {
                        Stroke::new(1.0_f32, pal.accent)
                    } else if brand_resp.hovered() {
                        Stroke::new(1.0_f32, pal.border)
                    } else {
                        Stroke::NONE
                    };
                    ui.painter()
                        .rect(brand_rect, Rounding::same(6.0), brand_fill, brand_stroke);
                    let badge_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            brand_rect.left() + pad_x,
                            brand_rect.center().y - badge_sz * 0.5,
                        ),
                        Vec2::splat(badge_sz),
                    );
                    paint_waveform_badge_in_rect(ui.painter(), badge_rect, pal);
                    ui.painter().galley(
                        egui::pos2(
                            badge_rect.right() + gap,
                            brand_rect.center().y - title_galley.size().y * 0.5,
                        ),
                        title_galley,
                        pal.text_primary,
                    );
                    if brand_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(format!(
                            "About TruePeaks v{} — version, standards & GitHub repository",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .clicked()
                    {
                        if is_about {
                            self.active_view = ActiveView::Analyzer;
                        } else {
                            self.about_tab = AboutTab::Overview;
                            self.active_view = ActiveView::About;
                        }
                    }

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
                        if self.active_view != ActiveView::Analyzer {
                            if ui
                                .add(
                                    primary_button("← Back to Analyzer", true, pal)
                                        .min_size(Vec2::new(0.0, 28.0)),
                                )
                                .on_hover_text("Return to the analyzer view (Esc)")
                                .clicked()
                            {
                                self.active_view = ActiveView::Analyzer;
                            }
                            if self.active_view == ActiveView::About
                                && ui
                                    .add(
                                        egui::Button::new("Settings")
                                            .min_size(Vec2::new(0.0, 28.0)),
                                    )
                                    .on_hover_text("Configure columns, theme, and log naming (Ctrl+,)")
                                    .clicked()
                            {
                                self.active_view = ActiveView::Settings;
                            }
                        } else if ui
                            .add(egui::Button::new("Settings").min_size(Vec2::new(0.0, 28.0)))
                            .on_hover_text("Configure columns, theme, and log naming (Ctrl+,)")
                            .clicked()
                        {
                            self.active_view = ActiveView::Settings;
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

                        if self.active_view == ActiveView::Analyzer && !self.log_preview.is_empty() {
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
                let dot_color = if self.is_scanning {
                    pal.accent
                } else if self.last_was_error {
                    pal.error
                } else if self.last_was_success {
                    pal.success
                } else {
                    pal.text_muted
                };

                if self.is_scanning {
                    let total = self.progress_total.max(1);
                    let completed = self.progress_completed.min(total);
                    let indeterminate = self.progress_completed == 0;
                    let raw_frac = (completed as f32) / (total as f32);
                    let anim_frac = ctx.animate_value_with_time(
                        egui::Id::new("scan_progress_bar"),
                        raw_frac,
                        0.14,
                    );
                    let pct = (raw_frac * 100.0).round().clamp(0.0, 100.0) as u32;
                    let elapsed = self
                        .scan_start_instant
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0);

                    let count_str = format!("{} / {} files", completed, total);
                    let pct_str = format!("{}%", pct);
                    let elapsed_str = format!("{:.1}s", elapsed);
                    let accent_text = if pal.dark {
                        pal.accent_hover
                    } else {
                        pal.accent
                    };
                    let pct_fill = Color32::from_rgba_unmultiplied(
                        pal.accent.r(),
                        pal.accent.g(),
                        pal.accent.b(),
                        if pal.dark { 36 } else { 22 },
                    );

                    let avail_w = ui.available_width();
                    let right_reserve = 195.0_f32;
                    let left_max = (avail_w - right_reserve).max(90.0);
                    let active_label = if self.last_file_scanned.is_empty() {
                        self.status_message.clone()
                    } else {
                        format!("Analyzing: {}", self.last_file_scanned)
                    };

                    ui.horizontal(|ui| {
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.2, dot_color);

                        ui.allocate_ui_with_layout(
                            Vec2::new(left_max, 22.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(active_label)
                                            .size(14.5)
                                            .color(pal.text_primary),
                                    )
                                    .truncate(),
                                );
                            },
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(elapsed_str)
                                    .size(13.0)
                                    .monospace()
                                    .color(pal.text_muted),
                            );
                            Frame::none()
                                .fill(pct_fill)
                                .stroke(Stroke::new(1.0_f32, pal.accent))
                                .rounding(Rounding::same(5.0))
                                .inner_margin(Margin::symmetric(7.0, 1.5))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(pct_str)
                                            .size(12.5)
                                            .strong()
                                            .color(accent_text),
                                    );
                                });
                            ui.label(
                                RichText::new(count_str)
                                    .size(13.5)
                                    .color(pal.text_muted),
                            );
                        });
                    });

                    ui.add_space(6.0);
                    let bar_w = ui.available_width().max(60.0);
                    let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, 9.0), Sense::hover());
                    let rounding = Rounding::same(4.5);
                    let painter = ui.painter();
                    painter.rect(
                        bar_rect,
                        rounding,
                        pal.well,
                        Stroke::new(1.0_f32, pal.border),
                    );

                    let inner = bar_rect.shrink(1.0);
                    if inner.width() > 4.0 && inner.height() > 2.0 {
                        let inner_r = Rounding::same(inner.height() * 0.5);
                        if indeterminate {
                            let t = ctx.input(|i| i.time) as f32;
                            let seg_w = (inner.width() * 0.28).clamp(36.0, 140.0);
                            let travel = inner.width() + seg_w;
                            let head = ((t * 1.15) % 1.0) * travel;
                            let x0 = (inner.left() + head - seg_w).max(inner.left());
                            let x1 = (inner.left() + head).min(inner.right());
                            if x1 > x0 + 2.0 {
                                let seg = egui::Rect::from_min_max(
                                    egui::pos2(x0, inner.top()),
                                    egui::pos2(x1, inner.bottom()),
                                );
                                painter.rect_filled(seg, inner_r, pal.accent);
                            }
                        } else {
                            let fill_w = (inner.width() * anim_frac.clamp(0.0, 1.0)).max(0.0);
                            if fill_w > 2.0 {
                                let fill_rect = egui::Rect::from_min_max(
                                    inner.min,
                                    egui::pos2(
                                        (inner.left() + fill_w).min(inner.right()),
                                        inner.bottom(),
                                    ),
                                );
                                painter.rect_filled(fill_rect, inner_r, pal.accent);
                                if fill_rect.width() > 10.0 && anim_frac < 0.995 {
                                    let tip_rect = egui::Rect::from_min_max(
                                        egui::pos2(
                                            (fill_rect.right() - 8.0).max(fill_rect.left()),
                                            fill_rect.top(),
                                        ),
                                        fill_rect.max,
                                    );
                                    painter.rect_filled(tip_rect, inner_r, pal.accent_hover);
                                }
                            }
                        }
                    }
                } else {
                    ui.horizontal(|ui| {
                        let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.2, dot_color);

                        let msg_color = if self.last_was_error {
                            pal.error
                        } else {
                            pal.text_primary
                        };
                        let right_reserve = if !self.last_saved_paths.is_empty() {
                            280.0_f32
                        } else {
                            16.0_f32
                        };
                        let left_max = (ui.available_width() - right_reserve).max(120.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(left_max, 22.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&self.status_message)
                                            .size(15.0)
                                            .color(msg_color),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&self.status_message);
                            },
                        );

                        if let Some(first_saved) = self.last_saved_paths.first() {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let btn_lbl = if self.last_saved_paths.len() > 1 {
                                    "Show First"
                                } else {
                                    "Show in Folder"
                                };
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new(btn_lbl).size(13.5))
                                            .min_size(Vec2::new(0.0, 24.0)),
                                    )
                                    .on_hover_text(format!("Reveal {}", first_saved.display()))
                                    .clicked()
                                {
                                    reveal_in_file_manager(first_saved);
                                }

                                let saved_label = if self.last_saved_paths.len() > 1 {
                                    format!("Saved: {} log files", self.last_saved_paths.len())
                                } else {
                                    let display_name = first_saved
                                        .file_name()
                                        .and_then(|s| s.to_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| first_saved.display().to_string());
                                    format!("Saved: {}", display_name)
                                };
                                let hover_list = self
                                    .last_saved_paths
                                    .iter()
                                    .map(|p| p.display().to_string())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                ui.label(
                                    RichText::new(saved_label).size(14.5).color(pal.success),
                                )
                                .on_hover_text(hover_list);
                            });
                        }
                    });
                }
            });
    }

    fn render_central_panel(&mut self, ctx: &egui::Context, pal: &Palette) {
        match self.active_view {
            ActiveView::Settings => {
                egui::CentralPanel::default()
                    .frame(Frame::none().fill(pal.canvas).inner_margin(Margin::ZERO))
                    .show(ctx, |ui| {
                        self.render_settings_view(ui, ctx, pal);
                    });
                return;
            }
            ActiveView::About => {
                egui::CentralPanel::default()
                    .frame(Frame::none().fill(pal.canvas).inner_margin(Margin::ZERO))
                    .show(ctx, |ui| {
                        self.render_about_view(ui, ctx, pal);
                    });
                return;
            }
            ActiveView::Analyzer => {}
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
            let pill_rect = egui::Rect::from_center_size(badge_center, Vec2::new(390.0, 56.0));
            painter.rect(
                pill_rect,
                Rounding::same(12.0),
                pal.surface,
                Stroke::new(1.5_f32, pal.accent),
            );
            painter.text(
                badge_center,
                egui::Align2::CENTER_CENTER,
                "Release to analyze audio file(s) or folder(s)",
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
                            RichText::new("Drop Audio File(s) or Album Folder(s) Here")
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

    fn render_about_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
        let repo_url = env!("CARGO_PKG_REPOSITORY");
        let version = env!("CARGO_PKG_VERSION");
        let description = env!("CARGO_PKG_DESCRIPTION");
        let accent_text = if pal.dark { pal.accent_hover } else { pal.accent };

        let max_card_w = match self.about_tab {
            AboutTab::Overview => 620.0,
            AboutTab::MathDetails => 780.0,
        };

        egui::ScrollArea::vertical()
            .id_salt("about_view_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width();
                let avail_h = ui.available_height();
                let card_w = (avail_w - 40.0).clamp(320.0, max_card_w);
                let top_pad = match self.about_tab {
                    AboutTab::Overview => ((avail_h - 500.0) * 0.35).max(20.0),
                    AboutTab::MathDetails => 20.0,
                };

                ui.vertical_centered(|ui| {
                    ui.add_space(top_pad);
                    Frame::none()
                        .fill(pal.surface)
                        .rounding(Rounding::same(12.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .inner_margin(Margin::symmetric(28.0, 24.0))
                        .show(ui, |ui| {
                            ui.set_width(card_w);

                            // Header: Waveform badge + Title + Version pill
                            ui.horizontal(|ui| {
                                paint_waveform_badge(ui, 52.0, pal);
                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("TruePeaks")
                                                .size(24.0)
                                                .strong()
                                                .color(pal.text_primary),
                                        );
                                        let pill_fill = Color32::from_rgba_unmultiplied(
                                            pal.accent.r(),
                                            pal.accent.g(),
                                            pal.accent.b(),
                                            if pal.dark { 36 } else { 22 },
                                        );
                                        Frame::none()
                                            .fill(pill_fill)
                                            .stroke(Stroke::new(1.0_f32, pal.accent))
                                            .rounding(Rounding::same(10.0))
                                            .inner_margin(Margin::symmetric(8.0, 2.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(format!("v{}", version))
                                                        .size(12.5)
                                                        .strong()
                                                        .color(accent_text),
                                                );
                                            });
                                    });
                                    ui.label(
                                        RichText::new(description)
                                            .size(13.5)
                                            .color(pal.text_muted),
                                    );
                                });
                            });

                            ui.add_space(12.0);
                            // Tab strip (starts cleanly on Overview by default)
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(RichText::new("Overview").size(13.5))
                                            .selected(self.about_tab == AboutTab::Overview)
                                            .min_size(Vec2::new(0.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.about_tab = AboutTab::Overview;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("DSP & Math Details").size(13.5),
                                        )
                                        .selected(self.about_tab == AboutTab::MathDetails)
                                        .min_size(Vec2::new(0.0, 28.0)),
                                    )
                                    .on_hover_text(
                                        "Detailed formulas, filter coefficients, and Wikipedia references",
                                    )
                                    .clicked()
                                {
                                    self.about_tab = AboutTab::MathDetails;
                                }
                            });

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(12.0);

                            match self.about_tab {
                                AboutTab::Overview => {
                                    self.render_about_overview(ui, ctx, pal, accent_text, repo_url, version);
                                }
                                AboutTab::MathDetails => {
                                    self.render_about_math_details(ui, ctx, pal, accent_text, repo_url);
                                }
                            }
                        });
                    ui.add_space(20.0);
                });
            });
    }

    fn render_about_overview(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        accent_text: Color32,
        repo_url: &str,
        version: &str,
    ) {
        let license = env!("CARGO_PKG_LICENSE");
        let url_copied = self
            .about_url_copied_at
            .map(|t| t.elapsed().as_secs_f32() < 2.0)
            .unwrap_or(false);
        if url_copied {
            ctx.request_repaint();
        }

        Frame::none()
            .fill(pal.surface_alt)
            .rounding(Rounding::same(8.0))
            .stroke(Stroke::new(1.0_f32, pal.border))
            .inner_margin(Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                about_kv_row(ui, pal, "GitHub", |ui| {
                    ui.horizontal(|ui| {
                        ui.hyperlink_to(
                            RichText::new(repo_url).size(13.5).color(accent_text),
                            repo_url,
                        )
                        .on_hover_text("Open TruePeaks on GitHub in your browser");

                        let copy_lbl = if url_copied {
                            RichText::new("✓ Copied").size(12.0).color(pal.success)
                        } else {
                            RichText::new("Copy").size(12.0)
                        };
                        if ui
                            .add(egui::Button::new(copy_lbl).min_size(Vec2::new(0.0, 22.0)))
                            .on_hover_text("Copy repository URL to clipboard")
                            .clicked()
                        {
                            ctx.copy_text(repo_url.to_string());
                            self.about_url_copied_at = Some(Instant::now());
                        }
                    });
                });

                ui.add_space(6.0);
                let platform = format!(
                    "v{}  ·  {} ({})",
                    version,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                );
                about_kv_row(ui, pal, "Version", |ui| {
                    ui.label(RichText::new(platform).size(13.5).color(pal.text_primary));
                });

                ui.add_space(6.0);
                about_kv_row(ui, pal, "License", |ui| {
                    ui.label(
                        RichText::new(format!("{} License", license))
                            .size(13.5)
                            .color(pal.text_primary),
                    );
                });

                ui.add_space(6.0);
                about_kv_row(ui, pal, "Dynamic Range", |ui| {
                    ui.label(
                        RichText::new("PMF DR (top-20% 3 s RMS blocks, per-channel & joint)")
                            .size(13.0)
                            .color(pal.text_primary),
                    );
                });

                ui.add_space(6.0);
                about_kv_row(ui, pal, "Loudness & Peak", |ui| {
                    ui.label(
                        RichText::new(
                            "ITU-R BS.1770-4 / EBU R128 (48-tap Hann-sinc oversampled True Peak)",
                        )
                        .size(13.0)
                        .color(pal.text_primary),
                    );
                });

                ui.add_space(6.0);
                about_kv_row(ui, pal, "Formats", |ui| {
                    ui.label(
                        RichText::new("WAV, BWF, AIFF, Wave64, FLAC, ALAC, CAF, MP3, AAC, Ogg")
                            .size(13.0)
                            .color(pal.text_primary),
                    );
                });

                if let Some(cfg_path) = AppConfig::config_path() {
                    ui.add_space(6.0);
                    about_kv_row(ui, pal, "Config File", |ui| {
                        ui.horizontal(|ui| {
                            let max_p = (ui.available_width() - 68.0).max(80.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(max_p, 20.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(cfg_path.display().to_string())
                                                .size(12.5)
                                                .color(pal.text_muted),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(cfg_path.display().to_string());
                                },
                            );
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("Reveal").size(12.0))
                                        .min_size(Vec2::new(0.0, 22.0)),
                                )
                                .on_hover_text(format!("Locate {}", cfg_path.display()))
                                .clicked()
                            {
                                reveal_in_file_manager(&cfg_path);
                            }
                        });
                    });
                }
            });

        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("DSP & Math Details →").size(13.5))
                        .min_size(Vec2::new(0.0, 30.0)),
                )
                .on_hover_text("View formulas, DSP pipeline details & Wikipedia links")
                .clicked()
            {
                self.about_tab = AboutTab::MathDetails;
            }

            if ui
                .add(
                    egui::Button::new(RichText::new("Open GitHub ↗").size(13.5))
                        .min_size(Vec2::new(0.0, 30.0)),
                )
                .on_hover_text(repo_url)
                .clicked()
            {
                ctx.open_url(egui::OpenUrl::new_tab(repo_url));
            }

            if ui
                .add(
                    egui::Button::new(RichText::new("Settings...").size(13.5))
                        .min_size(Vec2::new(0.0, 30.0)),
                )
                .clicked()
            {
                self.active_view = ActiveView::Settings;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        primary_button("← Back to Analyzer", true, pal)
                            .min_size(Vec2::new(0.0, 30.0)),
                    )
                    .clicked()
                {
                    self.active_view = ActiveView::Analyzer;
                }
            });
        });
    }

    fn render_about_math_details(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pal: &Palette,
        accent_text: Color32,
        repo_url: &str,
    ) {
        ui.label(
            RichText::new(
                "All DSP metrics run in 64-bit IEEE 754 floating point (f64) across 8,192-frame chunks.",
            )
            .size(13.0)
            .color(pal.text_muted),
        );
        ui.add_space(10.0);

        math_section_card(
            ui,
            pal,
            accent_text,
            "1. PCM Normalization & Effective Bit Depth (Bits Used)",
            "Integer samples normalize symmetrically into [-1, 1) by power-of-2 full-scale divisors. Lossless integers accumulate a 32-bit MSB-aligned bitwise OR mask across the track to detect padded bit depths (such as 16-bit audio stored inside a 24-bit WAV or FLAC container).",
            &[
                "x[n] = (s[n] - 128) / 2^7  [U8]    |    s[n] / 2^(b - 1)  [S16 / S24 / S32]",
                "M    = OR_all( MSB32(s_c[n]) )     [U8 << 24, S16 << 16, S24 << 8, S32]",
                "Bits Used = 0  (if M == 0)   |   32 - tzcnt(M)  (if M != 0)",
            ],
            &[
                ("PCM", "https://en.wikipedia.org/wiki/Pulse-code_modulation"),
                ("Audio Bit Depth", "https://en.wikipedia.org/wiki/Audio_bit_depth"),
                ("Trailing Zero Count", "https://en.wikipedia.org/wiki/Find_first_set"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "2. Sample Peak (SPPM) & Oversampled True Peak (TPL)",
            "SPPM records peak raw sample magnitude in dBFS. True Peak detects inter-sample overshoots per ITU-R BS.1770-4 Annex 2 via a 48-tap Hann-windowed sinc polyphase FIR interpolator adapting factor F to sample rate.",
            &[
                "SPPM_c = 20 * log10( max_n |x_c[n]| ) dBFS    |    SPPM_joint = max_c SPPM_c",
                "w[j] = 0.5 * (1 - cos(2*pi*j / 48)),   h[j] = w[j] * sinc((j - 24) / F)",
                "F = 4x (fs < 96 kHz, 12 taps/phase)  |  2x (96-192 kHz, 24 taps/phase)  |  1x (>= 192 kHz)",
                "TPL_c = 20 * log10( max_k |x_tilde_c[k]| ) dBTP    |    TPL_joint = max_c TPL_c",
            ],
            &[
                ("dBFS", "https://en.wikipedia.org/wiki/DBFS"),
                ("Whittaker-Shannon", "https://en.wikipedia.org/wiki/Whittaker%E2%80%93Shannon_interpolation_formula"),
                ("Hann Window", "https://en.wikipedia.org/wiki/Hann_function"),
                ("Sinc Filter", "https://en.wikipedia.org/wiki/Sinc_function"),
                ("Oversampling", "https://en.wikipedia.org/wiki/Oversampling"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "3. ITU-R BS.1770-4 K-Weighting & Channel Weights",
            "A two-stage bilinear-transformed IIR biquad cascade models acoustic head high-frequency shelving and low-frequency rolloff before applying BS.1770 channel weights (omitting LFE).",
            &[
                "Stage 1 Shelf: f0 = 1681.974 Hz, G = +3.99984 dB, Q = 0.70718, K1 = tan(pi*f0/fs)",
                "Stage 2 High-Pass (RLB): f0 = 38.1355 Hz, Q = 0.50033, b2 = [1, -2, 1]",
                "Channel weights G_c: 1.0 (L, R, C)  |  1.41 (+1.5 dB Surrounds)  |  0.0 (LFE)",
                "Loudness L(z) = -0.691 + 10 * log10(z) LUFS",
            ],
            &[
                ("LUFS / BS.1770", "https://en.wikipedia.org/wiki/LUFS"),
                ("Biquad Filter", "https://en.wikipedia.org/wiki/Digital_biquad_filter"),
                ("Bilinear Transform", "https://en.wikipedia.org/wiki/Bilinear_transform"),
                ("IIR Filter", "https://en.wikipedia.org/wiki/Infinite_impulse_response"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "4. Continuous Sliding Momentary / Short-Term, LUFSi & LRA",
            "Unlike coarse 100 ms hops, Momentary (400 ms) and Short-Term (3.0 s) maxima slide sample-by-sample using a 3.0 s circular buffer with periodic drift resync. Per-channel maxima apply the +3.01 dB (2x) dual-mono convention when C > 1.",
            &[
                "W_M = 4 * round(fs / 10) [400 ms]     |     W_S = 30 * round(fs / 10) [3.0 s]",
                "L_M,joint[n] = -0.691 + 10 * log10( (1/W_M) * sum_c G_c * S_{M,c}[n] )",
                "LUFSi (400 ms, 75% overlap): Gate 1 > -70.0 LUFS, Gate 2 > (L_abs - 10.0 LU)",
                "LRA (EBU Tech 3342): P95(L_S) - P10(L_S) above -70 LUFS & (L_abs - 20.0 LU)",
            ],
            &[
                ("EBU R 128", "https://en.wikipedia.org/wiki/EBU_R_128"),
                ("Circular Buffer", "https://en.wikipedia.org/wiki/Circular_buffer"),
                ("Percentile", "https://en.wikipedia.org/wiki/Percentile"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "5. Continuous Min. PSR & Track PLR",
            "PSR evaluates peak sample amplitude against active Short-Term loudness at every sample frame via an O(1) monotonic decreasing deque, while PLR measures whole-track headroom relative to LUFSi (capping true peaks above 0 dBTP at 0.0).",
            &[
                "P_3s[n] = max_{c, 0 <= i < W_S} |x_c[n - i]|     (trailing 3.0 s monotonic deque)",
                "Min. PSR = 0.691 + 10 * log10( W_S * min_{L_S > -70 LUFS}( P_3s[n]^2 / sum G_c*S_{S,c}[n] ) ) LU",
                "P_ref = 0.0 (if TPL_joint > 0.0) else max(TPL_joint, SPPM_joint)   ->   PLR = P_ref - LUFSi LU",
            ],
            &[
                ("Crest Factor", "https://en.wikipedia.org/wiki/Crest_factor"),
                ("Double-Ended Queue", "https://en.wikipedia.org/wiki/Double-ended_queue"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "6. AES17 RMS, PMF Dynamic Range (DR) & Album Rating",
            "AES17 RMS scales energy by 2.0 so a full-scale sine reads 0.00 dBFS. PMF splits audio into quantized blocks (~3 s), pairs top-20% RMS against the second-loudest block peak per channel, and averages unrounded values across tracks.",
            &[
                "RMS_AES17,c = 10 * log10( (2 / N) * sum x_c[n]^2 ) dBFS   (0 dBFS sine = 0.00 dBFS)",
                "N_blk = 3 * (fs + 60*(fs/44100)) if 44100 | fs  (132,480 @ 44.1k)  else 3 * fs  (144,000 @ 48k)",
                "R_20,c = sqrt( mean of top M = max(1, floor(0.20*K)) block AES17 energies )",
                "P_2,c  = 2nd-highest block peak (or 1st if K = 1);    DR_c = 20 * log10( P_2,c / R_20,c )",
                "DR_track = round( mean_{c in active} DR_c )   |   Album DR = round( mean_tracks DR_raw )",
            ],
            &[
                ("Root Mean Square", "https://en.wikipedia.org/wiki/Root_mean_square"),
                ("Dynamic Range", "https://en.wikipedia.org/wiki/Dynamic_range"),
                ("Loudness War", "https://en.wikipedia.org/wiki/Loudness_war"),
            ],
        );

        ui.add_space(10.0);
        math_section_card(
            ui,
            pal,
            accent_text,
            "7. Combined Multi-Channel RMS & Classic Footer Bitrate",
            "Classic foo_dr logs power-average active non-LFE channel RMS values and estimate album bitrate from total on-disk file sizes and cumulative audio duration.",
            &[
                "RMS_combined = 10 * log10( (1 / |A|) * sum_{c in A} 10^(RMS_AES17,c / 10) ) dB",
                "Bitrate (kbps) = round( 8 * sum(file_bytes) / (1000 * sum(total_frames / fs)) )",
            ],
            &[("Arithmetic Mean", "https://en.wikipedia.org/wiki/Arithmetic_mean")],
        );

        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("← Overview").size(13.5))
                        .min_size(Vec2::new(0.0, 30.0)),
                )
                .clicked()
            {
                self.about_tab = AboutTab::Overview;
            }

            let readme_url = format!("{}/blob/main/README.md", repo_url);
            if ui
                .add(
                    egui::Button::new(RichText::new("Full README on GitHub ↗").size(13.5))
                        .min_size(Vec2::new(0.0, 30.0)),
                )
                .on_hover_text(&readme_url)
                .clicked()
            {
                ctx.open_url(egui::OpenUrl::new_tab(readme_url));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        primary_button("← Back to Analyzer", true, pal)
                            .min_size(Vec2::new(0.0, 30.0)),
                    )
                    .clicked()
                {
                    self.active_view = ActiveView::Analyzer;
                }
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
                    self.reset_window_geometry(ctx);
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

                ui.separator();

                setting_row(
                    ui,
                    pal,
                    "Window Position & Size",
                    "Automatically saved across restarts; reset restores the centered default",
                    "TruePeaks remembers your window size and desktop position in config.toml. Reset restores the default centered window.",
                    |ui| {
                        if ui
                            .button("Reset to Default")
                            .on_hover_text("Restore default centered window size and position")
                            .clicked()
                        {
                            self.reset_window_geometry(ctx);
                            changed = true;
                        }
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
            "Choose folder log format, directory recursion, multi-folder drops, table columns, and surround handling.",
        );

        settings_card(
            ui,
            pal,
            "Folder Log Style",
            "Select the text layout generated for album folder scans.",
            |ui| {
                for &style in FolderLogStyle::all() {
                    let selected = self.config.folder_log_style == style;
                    let tooltip = match style {
                        FolderLogStyle::TruePeaksTable => {
                            "Customizable pipe-delimited multi-column spreadsheet supporting all 26 metrics (DR, TPL, LUFSi, LRA, PSR, PLR, RMS, and per-channel columns)."
                        }
                        FolderLogStyle::ClassicFooDr => {
                            "Traditional Dynamic Range Meter style omitting the app/date header lines. Displays DR, Sample Peak dB, RMS dB, Duration, Track, Official DR value, Samplerate, Channels, Bits per sample, Bitrate, and Codec."
                        }
                        FolderLogStyle::CompactMastering => {
                            "Concise pure-ASCII mastering summary with top album averages (Summary DR, Peak, Avg LUFSi, Max TPL, Tracks & Time) and tightly left-aligned DR, Peak dB, TPL dBTP, LUFSi, PSR, Time, and Track columns."
                        }
                    };
                    if style_option_row(
                        ui,
                        pal,
                        selected,
                        style.label(),
                        style.description(),
                        tooltip,
                    ) {
                        self.config.folder_log_style = style;
                        changed = true;
                    }
                    ui.add_space(4.0);
                }
            },
        );

        ui.add_space(12.0);
        settings_card(
            ui,
            pal,
            "Directory Scanning & Multi-Drop",
            "Control nested subfolder recursion and how multiple dropped folders are saved.",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.recurse_subfolders,
                    "Recurse into subdirectories",
                    "Scan nested subfolders and include all subfolder tables in the log",
                    "Off by default: only analyzes audio files directly inside the selected folder. Enable to recursively scan nested subfolders.",
                    pal,
                );

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.combine_dropped_folders,
                    "Combine multiple dropped folders into one log",
                    "When off (default), dropping multiple folders generates a separate log inside each folder",
                    "Default: dropping multiple album folders creates a separate log (e.g. DR.txt) in each folder and shows all results in the viewer. Turn on to merge them into a single combined batch log.",
                    pal,
                );
            },
        );

        ui.add_space(12.0);
        if self.config.folder_log_style == FolderLogStyle::TruePeaksTable {
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
        } else {
            settings_card(
                ui,
                pal,
                "Summary Table Columns",
                "Column selection applies to the TruePeaks Table (Default) format.",
                |ui| {
                    let note = match self.config.folder_log_style {
                        FolderLogStyle::ClassicFooDr => {
                            "Classic DR (foo_dr Style) uses a fixed column layout (DR, Sample Peak, RMS, Duration, Track) plus codec metadata at the bottom."
                        }
                        FolderLogStyle::CompactMastering => {
                            "Compact Mastering uses a fixed executive layout (DR, Peak dB, TPL dBTP, LUFSi, PSR, Time, Track) with album summary stats above the table."
                        }
                        FolderLogStyle::TruePeaksTable => "",
                    };
                    ui.label(RichText::new(note).size(13.0).color(pal.text_muted));
                    ui.add_space(8.0);
                    if ui
                        .button("Switch to TruePeaks Table to Customize Columns")
                        .clicked()
                    {
                        self.config.folder_log_style = FolderLogStyle::TruePeaksTable;
                        changed = true;
                    }
                },
            );
        }

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

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                ui.add_enabled_ui(self.config.save_logs_to_disk, |ui| {
                    ui.label(
                        RichText::new("If a Log File Already Exists")
                            .strong()
                            .color(pal.text_primary),
                    );
                    ui.add_space(4.0);

                    if style_option_row(
                        ui,
                        pal,
                        !self.config.overwrite_existing_logs,
                        "Keep Both — Save Numbered Copy (Default)",
                        "Preserves existing logs by saving DR_2.txt, DR_3.txt, etc.",
                        "Default behavior: leaves existing log files untouched and appends _2, _3, etc. to new output filenames.",
                    ) && self.config.save_logs_to_disk
                        && self.config.overwrite_existing_logs
                    {
                        self.config.overwrite_existing_logs = false;
                        changed = true;
                    }

                    ui.add_space(4.0);

                    if style_option_row(
                        ui,
                        pal,
                        self.config.overwrite_existing_logs,
                        "Overwrite Existing Log File",
                        "Replaces any existing log file with the same name in place",
                        "Overwrites matching log files on disk directly instead of creating _2, _3 numbered copies.",
                    ) && self.config.save_logs_to_disk
                        && !self.config.overwrite_existing_logs
                    {
                        self.config.overwrite_existing_logs = true;
                        changed = true;
                    }
                });
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
            "Track Filenames Inside Logs",
            "Format how source audio file names appear in folder tables and single-file File Name rows (does not rename saved .txt files).",
            |ui| {
                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.log_filenames.limit_length,
                    "Restrict track filename length in logs",
                    "Off by default — shortens long audio filenames in log text with '...' (suggested default: 64 characters)",
                    "When enabled, track file names inside folder tables and single-file File Name rows are truncated with '...' to stay within the chosen character count.",
                    pal,
                );

                ui.add_space(4.0);
                ui.add_enabled_ui(self.config.log_filenames.limit_length, |ui| {
                    setting_row(
                        ui,
                        pal,
                        "Maximum Filename Characters",
                        "Character limit applied when filename restriction is enabled (suggested: 64)",
                        "Type or drag to set the maximum number of characters shown per audio track filename inside logs (8–255).",
                        |ui| {
                            ui.horizontal(|ui| {
                                if self.config.log_filenames.max_length != 64
                                    && ui
                                        .button("64")
                                        .on_hover_text("Reset character limit to suggested default (64)")
                                        .clicked()
                                {
                                    self.config.log_filenames.max_length = 64;
                                    changed = true;
                                }
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(
                                            &mut self.config.log_filenames.max_length,
                                        )
                                        .range(8..=255)
                                        .suffix(" chars")
                                        .speed(0.5),
                                    )
                                    .changed();
                            });
                        },
                    );
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                changed |= checkbox_row_tip(
                    ui,
                    &mut self.config.log_filenames.strip_non_ascii,
                    "Strip non-ASCII characters from filenames in logs",
                    "Off by default — removes accented, CJK, Cyrillic, or curly punctuation glyphs from logged track names",
                    "When enabled, only standard printable ASCII characters are kept for audio file names inside folder and single-file logs.",
                    pal,
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

fn style_option_row(
    ui: &mut egui::Ui,
    pal: &Palette,
    selected: bool,
    title: &str,
    subtitle: &str,
    tooltip: &str,
) -> bool {
    let fill = if selected {
        Color32::from_rgba_unmultiplied(
            pal.accent.r(),
            pal.accent.g(),
            pal.accent.b(),
            if pal.dark { 30 } else { 18 },
        )
    } else {
        pal.surface_alt
    };
    let stroke = if selected {
        Stroke::new(1.5_f32, pal.accent)
    } else {
        Stroke::new(1.0_f32, pal.border)
    };

    let resp = Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                let c = dot_rect.center();
                if selected {
                    ui.painter()
                        .circle_stroke(c, 7.0, Stroke::new(1.8_f32, pal.accent));
                    ui.painter().circle_filled(c, 3.8, pal.accent);
                } else {
                    ui.painter()
                        .circle_stroke(c, 7.0, Stroke::new(1.2_f32, pal.text_muted));
                }
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(14.0).strong().color(pal.text_primary));
                    ui.label(RichText::new(subtitle).size(12.0).color(pal.text_muted));
                });
            });
        });

    let click_resp = resp
        .response
        .interact(Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tooltip);

    click_resp.clicked()
}

fn about_kv_row(
    ui: &mut egui::Ui,
    pal: &Palette,
    key: &str,
    add_value: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(124.0, 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(
                    RichText::new(key)
                        .size(13.0)
                        .strong()
                        .color(pal.text_muted),
                );
            },
        );
        add_value(ui);
    });
}

fn math_section_card(
    ui: &mut egui::Ui,
    pal: &Palette,
    accent_text: Color32,
    title: &str,
    summary: &str,
    formulas: &[&str],
    wiki_links: &[(&str, &str)],
) {
    Frame::none()
        .fill(pal.surface_alt)
        .rounding(Rounding::same(8.0))
        .stroke(Stroke::new(1.0_f32, pal.border))
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .size(14.0)
                    .strong()
                    .color(pal.text_primary),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(summary).size(12.5).color(pal.text_muted));
            ui.add_space(8.0);

            Frame::none()
                .fill(pal.well)
                .rounding(Rounding::same(6.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal()
                        .id_salt(title)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for line in formulas {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(*line)
                                            .size(12.0)
                                            .monospace()
                                            .color(pal.text_primary),
                                    )
                                    .extend(),
                                );
                            }
                        });
                });

            if !wiki_links.is_empty() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Wikipedia:")
                            .size(11.5)
                            .strong()
                            .color(pal.text_muted),
                    );
                    for (idx, (label, url)) in wiki_links.iter().enumerate() {
                        if idx > 0 {
                            ui.label(RichText::new("·").size(11.5).color(pal.text_muted));
                        }
                        ui.hyperlink_to(
                            RichText::new(format!("{} ↗", label))
                                .size(11.5)
                                .color(accent_text),
                            *url,
                        )
                        .on_hover_text(*url);
                    }
                });
            }
        });
}

fn paint_waveform_badge(ui: &mut egui::Ui, size: f32, pal: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_waveform_badge_in_rect(ui.painter(), rect, pal);
}

fn paint_waveform_badge_in_rect(painter: &egui::Painter, rect: egui::Rect, pal: &Palette) {
    let size = rect.width().min(rect.height());
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
        self.track_window_geometry(ctx);

        if self.is_scanning {
            ctx.request_repaint();
        }

        let pal = Palette::for_theme(ctx.theme() == egui::Theme::Dark);

        self.render_top_toolbar(ctx, &pal);
        self.render_bottom_status_bar(ctx, &pal);
        self.render_central_panel(ctx, &pal);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.window_save_pending_at.take().is_some() {
            let _ = self.config.save();
        }
    }
}
