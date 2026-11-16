use crate::config::{AppConfig, DefaultResultsView};
use crate::dsp::TrackAnalysis;
use crate::formatter::export::format_comparison_text;
use crate::formatter::folder_log::compute_official_album_dr;
use crate::formatter::{
    format_log_track_name, format_sample_rate, format_timestamp_mmss, normalize_nfc,
};
use crate::math_svg::MathSvgCache;
use crate::scanner::ScanSnapshot;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Frame, Margin, Pos2, Rect, RichText, Rounding,
    Sense, Stroke, Vec2,
};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsViewTab {
    TextLog,
    VisualView,
    CompareAb,
}

impl ResultsViewTab {
    pub fn from_default(pref: DefaultResultsView) -> Self {
        match pref {
            DefaultResultsView::VisualView => Self::VisualView,
            DefaultResultsView::TextLog => Self::TextLog,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualSortColumn {
    Original,
    Name,
    Duration,
    Dr,
    MaxTpl,
    MaxSppm,
    Lufs,
    Lra,
    MinPsr,
    ReplayGain,
    Correlation,
}

pub struct VisualState {
    pub tab: ResultsViewTab,
    pub sort_col: VisualSortColumn,
    pub sort_asc: bool,
    pub selected_track: usize,
    pub compare_a: Option<ScanSnapshot>,
    pub compare_b: Option<ScanSnapshot>,
}

impl Default for VisualState {
    fn default() -> Self {
        Self {
            tab: ResultsViewTab::VisualView,
            sort_col: VisualSortColumn::Original,
            sort_asc: true,
            selected_track: 0,
            compare_a: None,
            compare_b: None,
        }
    }
}

pub struct VisualColors {
    pub dark: bool,
    pub surface: Color32,
    pub surface_alt: Color32,
    pub well: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}

fn format_mmss(secs_f64: f64) -> String {
    let total = secs_f64.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

fn dr_tier_color(dr: i32, pal: &VisualColors) -> Color32 {
    if dr >= 12 {
        pal.success
    } else if dr >= 7 {
        pal.warning
    } else {
        pal.error
    }
}

fn tpl_tier_color(tpl: f64, pal: &VisualColors) -> Color32 {
    if !tpl.is_finite() || tpl <= -1.0 {
        pal.text_primary
    } else if tpl <= 0.0 {
        pal.warning
    } else {
        pal.error
    }
}

fn cmp_f64_opt(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_truncated_text(
    ui: &egui::Ui,
    painter: &egui::Painter,
    left_x: f32,
    center_y: f32,
    max_width: f32,
    text: &str,
    font_id: FontId,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font_id, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(16.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = Pos2::new(left_x, center_y - galley.size().y * 0.5);
    painter.galley(pos, galley, color);
}

#[derive(Clone, Copy)]
struct TableColSpec {
    sort: Option<VisualSortColumn>,
    label: &'static str,
    width: f32,
    right_align: bool,
}

fn render_kpi_row(ui: &mut egui::Ui, items: &[(&str, String, Color32)], avail_w: f32, pal: &VisualColors) {
    if items.is_empty() {
        return;
    }
    let gap = 8.0_f32;
    let count = items.len() as f32;
    if avail_w >= 620.0 {
        let card_w = ((avail_w - gap * (count - 1.0)) / count).max(96.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (idx, (lbl, val, col)) in items.iter().enumerate() {
                // Absorb any fractional remainder in the last card so left/right edges align flush.
                let w = if idx + 1 == items.len() {
                    (avail_w - (card_w + gap) * (items.len() - 1) as f32).max(96.0)
                } else {
                    card_w
                };
                kpi_pill_sized(ui, lbl, val, *col, Some(w), pal);
            }
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
            for (lbl, val, col) in items {
                kpi_pill_sized(ui, lbl, val, *col, None, pal);
            }
        });
    }
}

fn kpi_pill_sized(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    val_color: Color32,
    exact_width: Option<f32>,
    pal: &VisualColors,
) {
    let inner_pad = Vec2::new(12.0, 8.0);
    let mut frame = Frame::none()
        .fill(pal.surface)
        .rounding(Rounding::same(8.0))
        .stroke(Stroke::new(1.0_f32, pal.border))
        .inner_margin(Margin::symmetric(inner_pad.x, inner_pad.y));
    if exact_width.is_some() {
        frame = frame.outer_margin(Margin::ZERO);
    }
    frame.show(ui, |ui| {
        if let Some(w) = exact_width {
            ui.set_width((w - inner_pad.x * 2.0).max(60.0));
        }
        ui.label(RichText::new(label).size(11.0).color(pal.text_muted));
        ui.label(RichText::new(value).size(15.0).strong().color(val_color));
    });
}

pub fn render_visual_results(
    ui: &mut egui::Ui,
    snapshot: &ScanSnapshot,
    config: &AppConfig,
    state: &mut VisualState,
    pal: &VisualColors,
) {
    let tracks = snapshot.collect_tracks();
    if tracks.is_empty() {
        ui.label(RichText::new("No analyzed tracks in this snapshot.").color(pal.text_muted));
        return;
    }

    let owned_tracks: Vec<TrackAnalysis> = tracks.iter().map(|t| (*t).clone()).collect();
    let album_dr = compute_official_album_dr(&owned_tracks);
    let total_secs: f64 = tracks.iter().map(|t| t.duration_secs()).sum();
    let max_tpl = tracks
        .iter()
        .map(|t| t.max_tpl_joint)
        .fold(f64::NEG_INFINITY, f64::max);

    let mut sum_w = 0.0_f64;
    let mut sum_p = 0.0_f64;
    for t in &tracks {
        if t.integrated_lufs.is_finite() {
            let d = t.duration_secs().max(0.1);
            sum_p += d * 10.0_f64.powf(t.integrated_lufs / 10.0);
            sum_w += d;
        }
    }
    let avg_lufs = if sum_w > 0.0 && sum_p > 0.0 {
        Some(10.0 * (sum_p / sum_w).log10())
    } else {
        None
    };

    let viewport_h = ui.available_height();

    egui::ScrollArea::vertical()
        .id_salt("visual_results_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let content_top_y = ui.cursor().top();
            let avail_w = ui.available_width().max(260.0);
            ui.set_width(avail_w);

            // 1. Summary KPI Strip (full width, no FS Clips)
            let mut kpis: Vec<(&str, String, Color32)> = vec![
                ("Official DR", format!("DR{}", album_dr), dr_tier_color(album_dr, pal)),
                (
                    "Tracks / Time",
                    format!("{} · {}", tracks.len(), format_mmss(total_secs)),
                    pal.text_primary,
                ),
            ];
            if let Some(lufs) = avg_lufs {
                kpis.push(("Avg Integrated", format!("{:.2} LUFS", lufs), pal.text_primary));
                kpis.push(("Album ReplayGain", format!("{:+.2} dB", -18.0 - lufs), pal.accent_hover));
            }
            if max_tpl.is_finite() {
                kpis.push((
                    "Max True Peak",
                    format!("{:+.2} dBTP", max_tpl),
                    tpl_tier_color(max_tpl, pal),
                ));
            }
            render_kpi_row(ui, &kpis, avail_w, pal);

            ui.add_space(10.0);

            // 2. Sort Track Order
            let mut order: Vec<usize> = (0..tracks.len()).collect();
            order.sort_by(|&ia, &ib| {
                let a = tracks[ia];
                let b = tracks[ib];
                let ord = match state.sort_col {
                    VisualSortColumn::Original => ia.cmp(&ib),
                    VisualSortColumn::Name => a
                        .display_name(config.use_embedded_tags)
                        .to_lowercase()
                        .cmp(&b.display_name(config.use_embedded_tags).to_lowercase()),
                    VisualSortColumn::Duration => {
                        cmp_f64_opt(Some(a.duration_secs()), Some(b.duration_secs()))
                    }
                    VisualSortColumn::Dr => a
                        .dr_track_raw
                        .partial_cmp(&b.dr_track_raw)
                        .unwrap_or(Ordering::Equal),
                    VisualSortColumn::MaxTpl => cmp_f64_opt(
                        a.max_tpl_joint.is_finite().then_some(a.max_tpl_joint),
                        b.max_tpl_joint.is_finite().then_some(b.max_tpl_joint),
                    ),
                    VisualSortColumn::MaxSppm => cmp_f64_opt(
                        a.max_sppm_joint.is_finite().then_some(a.max_sppm_joint),
                        b.max_sppm_joint.is_finite().then_some(b.max_sppm_joint),
                    ),
                    VisualSortColumn::Lufs => cmp_f64_opt(
                        a.integrated_lufs.is_finite().then_some(a.integrated_lufs),
                        b.integrated_lufs.is_finite().then_some(b.integrated_lufs),
                    ),
                    VisualSortColumn::Lra => a.lra.partial_cmp(&b.lra).unwrap_or(Ordering::Equal),
                    VisualSortColumn::MinPsr => {
                        a.min_psr.partial_cmp(&b.min_psr).unwrap_or(Ordering::Equal)
                    }
                    VisualSortColumn::ReplayGain => {
                        cmp_f64_opt(a.replay_gain_track_db, b.replay_gain_track_db)
                    }
                    VisualSortColumn::Correlation => {
                        cmp_f64_opt(a.stereo_correlation, b.stereo_correlation)
                    }
                };
                if state.sort_asc {
                    ord
                } else {
                    ord.reverse()
                }
            });

            if state.selected_track >= tracks.len() {
                state.selected_track = 0;
            }

            // 3. Responsive Column-Aligned Track Table (11 columns, fills available width cleanly)
            let card_pad = 10.0_f32;
            Frame::none()
                .fill(pal.surface)
                .rounding(Rounding::same(8.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::same(card_pad))
                .show(ui, |ui| {
                    let inner_avail = (avail_w - card_pad * 2.0 - 2.0).max(200.0);
                    ui.set_width(inner_avail);

                    // Gently scale numeric columns when window widens while dedicating extra growth to Track.
                    let num_scale = (inner_avail / 920.0).clamp(1.0, 1.24);
                    let gap = (8.0 * num_scale).round();
                    let pad_x = 10.0_f32;

                    let w_num = (30.0 * num_scale).round();
                    let w_time = (48.0 * num_scale).round();
                    let w_dr = (48.0 * num_scale).round();
                    let w_tpl = (68.0 * num_scale).round();
                    let w_sppm = (62.0 * num_scale).round();
                    let w_lufs = (62.0 * num_scale).round();
                    let w_lra = (52.0 * num_scale).round();
                    let w_psr = (66.0 * num_scale).round();
                    let w_rg = (62.0 * num_scale).round();
                    let w_corr = (56.0 * num_scale).round();

                    let fixed_sum =
                        w_num + w_time + w_dr + w_tpl + w_sppm + w_lufs + w_lra + w_psr + w_rg + w_corr;
                    let total_gaps = gap * 10.0;
                    let w_track = (inner_avail - pad_x * 2.0 - total_gaps - fixed_sum).max(170.0);
                    let table_w = pad_x * 2.0 + fixed_sum + w_track + total_gaps;

                    let cols = [
                        TableColSpec { sort: Some(VisualSortColumn::Original), label: "#", width: w_num, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::Name), label: "Track", width: w_track, right_align: false },
                        TableColSpec { sort: Some(VisualSortColumn::Duration), label: "Time", width: w_time, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::Dr), label: "DR", width: w_dr, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::MaxTpl), label: "Max TPL", width: w_tpl, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::MaxSppm), label: "SPPM", width: w_sppm, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::Lufs), label: "LUFSi", width: w_lufs, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::Lra), label: "LRA", width: w_lra, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::MinPsr), label: "Min PSR", width: w_psr, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::ReplayGain), label: "RG", width: w_rg, right_align: true },
                        TableColSpec { sort: Some(VisualSortColumn::Correlation), label: "Corr", width: w_corr, right_align: true },
                    ];

                    let mut col_x = Vec::with_capacity(cols.len());
                    let mut cur_x = pad_x;
                    for c in cols {
                        col_x.push((cur_x, c.width, c.right_align));
                        cur_x += c.width + gap;
                    }

                    egui::ScrollArea::horizontal()
                        .id_salt("visual_table_hscroll")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;

                            // Header row
                            let hdr_h = 28.0_f32;
                            let (hdr_rect, _) = ui.allocate_exact_size(
                                Vec2::new(table_w, hdr_h),
                                Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                hdr_rect,
                                Rounding {
                                    nw: 5.0,
                                    ne: 5.0,
                                    sw: 0.0,
                                    se: 0.0,
                                },
                                pal.well,
                            );
                            ui.painter().line_segment(
                                [
                                    Pos2::new(hdr_rect.left(), hdr_rect.bottom()),
                                    Pos2::new(hdr_rect.right(), hdr_rect.bottom()),
                                ],
                                Stroke::new(1.0_f32, pal.border),
                            );

                            for (c_idx, c) in cols.iter().enumerate() {
                                let (rx, rw, r_align) = col_x[c_idx];
                                let cell_rect = Rect::from_min_size(
                                    Pos2::new(hdr_rect.left() + rx, hdr_rect.top()),
                                    Vec2::new(rw, hdr_h),
                                );
                                if let Some(sort_col) = c.sort {
                                    let active = state.sort_col == sort_col;
                                    let arrow = if active {
                                        if state.sort_asc { " ^" } else { " v" }
                                    } else {
                                        ""
                                    };
                                    let resp = ui
                                        .interact(
                                            cell_rect,
                                            ui.id().with(("vis_hdr", c_idx)),
                                            Sense::click(),
                                        )
                                        .on_hover_cursor(CursorIcon::PointingHand);
                                    let col_clr = if active {
                                        pal.accent_hover
                                    } else if resp.hovered() {
                                        pal.text_primary
                                    } else {
                                        pal.text_muted
                                    };
                                    let txt = format!("{}{}", c.label, arrow);
                                    let (pos, align) = if r_align {
                                        (
                                            Pos2::new(cell_rect.right(), cell_rect.center().y),
                                            Align2::RIGHT_CENTER,
                                        )
                                    } else {
                                        (
                                            Pos2::new(cell_rect.left(), cell_rect.center().y),
                                            Align2::LEFT_CENTER,
                                        )
                                    };
                                    ui.painter().text(
                                        pos,
                                        align,
                                        txt,
                                        FontId::proportional(11.5),
                                        col_clr,
                                    );
                                    if resp.clicked() {
                                        if state.sort_col == sort_col {
                                            state.sort_asc = !state.sort_asc;
                                        } else {
                                            state.sort_col = sort_col;
                                            state.sort_asc = sort_col == VisualSortColumn::Original
                                                || sort_col == VisualSortColumn::Name;
                                        }
                                    }
                                }
                            }

                            // Data rows
                            let row_h = 27.0_f32;
                            let sel_fill = Color32::from_rgba_unmultiplied(
                                pal.accent.r(),
                                pal.accent.g(),
                                pal.accent.b(),
                                if pal.dark { 42 } else { 28 },
                            );
                            let hover_fill = Color32::from_rgba_unmultiplied(
                                pal.text_primary.r(),
                                pal.text_primary.g(),
                                pal.text_primary.b(),
                                if pal.dark { 14 } else { 10 },
                            );
                            let zebra_fill = if pal.dark {
                                Color32::from_rgba_unmultiplied(0, 0, 0, 72)
                            } else {
                                Color32::from_rgba_unmultiplied(15, 23, 42, 22)
                            };

                            for (vis_idx, orig_idx) in order.into_iter().enumerate() {
                                let t = tracks[orig_idx];
                                let is_sel = state.selected_track == orig_idx;
                                let full_name = t.display_name(config.use_embedded_tags);
                                let name = format_log_track_name(&full_name, &config.log_filenames);

                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    Vec2::new(table_w, row_h),
                                    Sense::click(),
                                );
                                let row_resp = row_resp.on_hover_cursor(CursorIcon::PointingHand);
                                if row_resp.clicked() {
                                    state.selected_track = orig_idx;
                                }
                                if full_name != name || name.len() > 24 {
                                    let _ = row_resp.clone().on_hover_text(&full_name);
                                }

                                let painter = ui.painter();
                                if is_sel {
                                    painter.rect_filled(row_rect, Rounding::same(3.0), sel_fill);
                                    painter.rect_filled(
                                        Rect::from_min_size(row_rect.min, Vec2::new(3.0, row_h)),
                                        Rounding::ZERO,
                                        pal.accent,
                                    );
                                } else if row_resp.hovered() {
                                    painter.rect_filled(row_rect, Rounding::same(3.0), hover_fill);
                                } else if vis_idx % 2 == 1 {
                                    painter.rect_filled(row_rect, Rounding::ZERO, zebra_fill);
                                }

                                let cy = row_rect.center().y;
                                let mono = FontId::monospace(12.0);
                                let prop = FontId::proportional(12.5);

                                // #
                                let (x0, w0, _) = col_x[0];
                                painter.text(
                                    Pos2::new(row_rect.left() + x0 + w0, cy),
                                    Align2::RIGHT_CENTER,
                                    format!("{}", orig_idx + 1),
                                    FontId::monospace(11.5),
                                    if is_sel { pal.text_primary } else { pal.text_muted },
                                );

                                // Track
                                let (x1, w1, _) = col_x[1];
                                paint_truncated_text(
                                    ui,
                                    painter,
                                    row_rect.left() + x1,
                                    cy,
                                    w1,
                                    &name,
                                    prop,
                                    pal.text_primary,
                                );

                                // Time
                                let (x2, w2, _) = col_x[2];
                                painter.text(
                                    Pos2::new(row_rect.left() + x2 + w2, cy),
                                    Align2::RIGHT_CENTER,
                                    format_mmss(t.duration_secs()),
                                    mono.clone(),
                                    pal.text_muted,
                                );

                                // DR
                                let (x3, w3, _) = col_x[3];
                                painter.text(
                                    Pos2::new(row_rect.left() + x3 + w3, cy),
                                    Align2::RIGHT_CENTER,
                                    format!("{:.1}", t.dr_track_raw),
                                    mono.clone(),
                                    dr_tier_color(t.dr_track_rounded, pal),
                                );

                                // Max TPL
                                let (x4, w4, _) = col_x[4];
                                let tpl_txt = if t.max_tpl_joint.is_finite() {
                                    format!("{:+.2}", t.max_tpl_joint)
                                } else {
                                    "-".into()
                                };
                                painter.text(
                                    Pos2::new(row_rect.left() + x4 + w4, cy),
                                    Align2::RIGHT_CENTER,
                                    tpl_txt,
                                    mono.clone(),
                                    tpl_tier_color(t.max_tpl_joint, pal),
                                );

                                // SPPM
                                let (x5, w5, _) = col_x[5];
                                let sppm_txt = if t.max_sppm_joint.is_finite() {
                                    format!("{:.2}", t.max_sppm_joint)
                                } else {
                                    "-".into()
                                };
                                painter.text(
                                    Pos2::new(row_rect.left() + x5 + w5, cy),
                                    Align2::RIGHT_CENTER,
                                    sppm_txt,
                                    mono.clone(),
                                    pal.text_primary,
                                );

                                // LUFSi
                                let (x6, w6, _) = col_x[6];
                                let lufs_txt = if t.integrated_lufs.is_finite() {
                                    format!("{:.2}", t.integrated_lufs)
                                } else {
                                    "-".into()
                                };
                                painter.text(
                                    Pos2::new(row_rect.left() + x6 + w6, cy),
                                    Align2::RIGHT_CENTER,
                                    lufs_txt,
                                    mono.clone(),
                                    pal.text_primary,
                                );

                                // LRA
                                let (x7, w7, _) = col_x[7];
                                painter.text(
                                    Pos2::new(row_rect.left() + x7 + w7, cy),
                                    Align2::RIGHT_CENTER,
                                    format!("{:.2}", t.lra),
                                    mono.clone(),
                                    pal.text_primary,
                                );

                                // Min PSR
                                let (x8, w8, _) = col_x[8];
                                painter.text(
                                    Pos2::new(row_rect.left() + x8 + w8, cy),
                                    Align2::RIGHT_CENTER,
                                    format!("{:.2}", t.min_psr),
                                    mono.clone(),
                                    pal.text_primary,
                                );

                                // RG
                                let (x9, w9, _) = col_x[9];
                                let rg_txt = t
                                    .replay_gain_track_db
                                    .map(|v| format!("{:+.2}", v))
                                    .unwrap_or_else(|| "-".into());
                                painter.text(
                                    Pos2::new(row_rect.left() + x9 + w9, cy),
                                    Align2::RIGHT_CENTER,
                                    rg_txt,
                                    mono.clone(),
                                    pal.text_primary,
                                );

                                // Corr
                                let (x10, w10, _) = col_x[10];
                                let corr_txt = t
                                    .stereo_correlation
                                    .map(|r| format!("{:+.2}", r))
                                    .unwrap_or_else(|| "-".into());
                                painter.text(
                                    Pos2::new(row_rect.left() + x10 + w10, cy),
                                    Align2::RIGHT_CENTER,
                                    corr_txt,
                                    mono,
                                    pal.text_muted,
                                );
                            }
                        });
                });

            ui.add_space(12.0);

            // 4. Selected Track Inspector & Loudness/Peak Sparkline
            if let Some(sel) = tracks.get(state.selected_track) {
                render_track_inspector(
                    ui,
                    sel,
                    avail_w,
                    viewport_h,
                    content_top_y,
                    config,
                    pal,
                );
            }
        });
}

fn render_track_inspector(
    ui: &mut egui::Ui,
    t: &TrackAnalysis,
    avail_w: f32,
    viewport_h: f32,
    content_top_y: f32,
    config: &AppConfig,
    pal: &VisualColors,
) {
    let pad_x = 14.0_f32;
    Frame::none()
        .fill(pal.surface)
        .rounding(Rounding::same(8.0))
        .stroke(Stroke::new(1.0_f32, pal.border))
        .inner_margin(Margin::symmetric(pad_x, 12.0))
        .show(ui, |ui| {
            let inner_w = (avail_w - pad_x * 2.0 - 2.0).max(200.0);
            ui.set_width(inner_w);

            let title = t.display_name(config.use_embedded_tags);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Track Timeline & Diagnostics — {}", title))
                        .size(14.0)
                        .strong()
                        .color(pal.text_primary),
                );
                if let Some(ref tags) = t.tags {
                    if let Some(ref artist) = tags.artist {
                        ui.label(
                            RichText::new(format!("by {}", normalize_nfc(artist)))
                                .size(12.5)
                                .color(pal.text_muted),
                        );
                    }
                }
            });

            ui.add_space(4.0);
            let bits_info = match (t.word_length, t.bits_used) {
                (Some(w), Some(b)) => format!("{} bit ({} used)", w, b),
                (Some(w), None) => format!("{} bit", w),
                _ => "Lossy / Float".to_string(),
            };
            let psr_at = t
                .min_psr_time_secs
                .map(format_timestamp_mmss)
                .unwrap_or_else(|| "-".to_string());
            ui.label(
                RichText::new(format!(
                    "{} · {} ({}) · TPL {}x · Max SPPM @ {} · Min. PSR @ {} · Corr: {}",
                    t.file_format,
                    format_sample_rate(t.sample_rate),
                    bits_info,
                    t.tpl_oversample_factor,
                    format_timestamp_mmss(t.max_sppm_time_secs),
                    psr_at,
                    t.stereo_correlation
                        .map(|r| format!("{:+.3}", r))
                        .unwrap_or_else(|| "-".to_string()),
                ))
                .size(12.0)
                .color(pal.text_muted),
            );

            ui.add_space(8.0);
            let used_above_chart = ui.cursor().top() - content_top_y;
            let chart_h = (viewport_h - used_above_chart - 18.0).max(152.0);
            paint_timeline_chart(ui, t, inner_w, chart_h, pal);
        });
}

fn paint_timeline_chart(
    ui: &mut egui::Ui,
    t: &TrackAnalysis,
    width: f32,
    height: f32,
    pal: &VisualColors,
) {
    let w = width.max(220.0);
    let h = height.max(152.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());

    let painter = ui.painter();
    painter.rect(rect, Rounding::same(6.0), pal.well, Stroke::new(1.0_f32, pal.border));

    if t.timeline.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Track too short for timeline envelope",
            FontId::proportional(12.0),
            pal.text_muted,
        );
        return;
    }

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, rect.top() + 24.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 22.0),
    );
    let total_t = t.duration_secs().max(0.01) as f32;
    let min_db = -54.0_f32;
    let max_db = 1.0_f32;
    let y_for_db = |db: f32| -> f32 {
        let clamped = db.clamp(min_db, max_db);
        let frac = (clamped - min_db) / (max_db - min_db);
        plot.bottom() - frac * plot.height()
    };
    let x_for_t = |sec: f32| -> f32 {
        let frac = (sec / total_t).clamp(0.0, 1.0);
        plot.left() + frac * plot.width()
    };

    // Legend cleanly in top header strip (above plot area)
    painter.text(
        Pos2::new(rect.right() - 10.0, rect.top() + 6.0),
        Align2::RIGHT_TOP,
        "Blue: Sample Peak   Amber: Short-Term LUFS   Red: Max Peak   Amber Pin: Min PSR",
        FontId::proportional(10.5),
        pal.text_muted,
    );

    // Subtle horizontal dB grid lines
    let muted_grid =
        Color32::from_rgba_unmultiplied(pal.text_muted.r(), pal.text_muted.g(), pal.text_muted.b(), 45);
    let mut grid_lines = vec![
        (0.0, Color32::from_rgba_unmultiplied(pal.error.r(), pal.error.g(), pal.error.b(), 95), "0 dBFS"),
        (-14.0, Color32::from_rgba_unmultiplied(pal.success.r(), pal.success.g(), pal.success.b(), 75), "-14 LUFS"),
        (-30.0, Color32::from_rgba_unmultiplied(pal.text_muted.r(), pal.text_muted.g(), pal.text_muted.b(), 55), "-30"),
    ];
    if h >= 230.0 {
        grid_lines.push((-6.0, muted_grid, "-6"));
        grid_lines.push((-20.0, muted_grid, "-20"));
        grid_lines.push((-42.0, muted_grid, "-42"));
    }
    for (db, color, lbl) in grid_lines {
        let y = y_for_db(db);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0_f32, color));
        painter.text(
            Pos2::new(plot.left() + 3.0, y - 2.0),
            Align2::LEFT_BOTTOM,
            lbl,
            FontId::monospace(9.5),
            pal.text_muted,
        );
    }

    // Peak SPPM polyline & Short-Term LUFS polyline
    let sppm_pts: Vec<Pos2> = t
        .timeline
        .iter()
        .map(|pt| Pos2::new(x_for_t(pt.time_secs), y_for_db(pt.sppm_db)))
        .collect();
    let lufs_pts: Vec<Pos2> = t
        .timeline
        .iter()
        .map(|pt| Pos2::new(x_for_t(pt.time_secs), y_for_db(pt.short_term_lufs)))
        .collect();

    let sppm_color = pal.accent_hover;
    let lufs_color = Color32::from_rgb(245, 166, 35);
    painter.add(egui::Shape::line(sppm_pts, Stroke::new(1.6_f32, sppm_color)));
    painter.add(egui::Shape::line(lufs_pts, Stroke::new(1.8_f32, lufs_color)));

    // Pin markers for Max SPPM and Min PSR timestamps
    let peak_x = x_for_t(t.max_sppm_time_secs as f32);
    painter.line_segment(
        [Pos2::new(peak_x, plot.top()), Pos2::new(peak_x, plot.bottom())],
        Stroke::new(1.2_f32, pal.error),
    );
    if let Some(psr_t) = t.min_psr_time_secs {
        let px = x_for_t(psr_t as f32);
        painter.line_segment(
            [Pos2::new(px, plot.top()), Pos2::new(px, plot.bottom())],
            Stroke::new(1.2_f32, pal.warning),
        );
    }

    // Interactive hover readout
    if let Some(hover) = resp.hover_pos() {
        if plot.contains(hover) {
            let frac = ((hover.x - plot.left()) / plot.width().max(1.0)).clamp(0.0, 1.0);
            let hover_t = frac * total_t;
            if let Some(nearest) = t.timeline.iter().min_by(|a, b| {
                (a.time_secs - hover_t)
                    .abs()
                    .partial_cmp(&(b.time_secs - hover_t).abs())
                    .unwrap_or(Ordering::Equal)
            }) {
                let x = hover.x;
                painter.line_segment(
                    [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                    Stroke::new(1.0_f32, pal.text_primary),
                );
                let psr_s = if nearest.psr_db.is_finite() {
                    format!("{:.1} LU", nearest.psr_db)
                } else {
                    "-".into()
                };
                let tip = format!(
                    "{}  |  SPPM: {:.1} dBFS  |  ST: {:.1} LUFS  |  PSR: {}",
                    format_timestamp_mmss(nearest.time_secs as f64),
                    nearest.sppm_db,
                    nearest.short_term_lufs,
                    psr_s
                );
                painter.text(
                    Pos2::new(plot.left() + 4.0, rect.bottom() - 4.0),
                    Align2::LEFT_BOTTOM,
                    tip,
                    FontId::monospace(10.5),
                    pal.text_primary,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_compare_view(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    current: Option<&ScanSnapshot>,
    recent: &[ScanSnapshot],
    config: &AppConfig,
    state: &mut VisualState,
    pal: &VisualColors,
    _cache: &mut MathSvgCache,
) {
    let avail_w = ui.available_width().max(260.0);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        if let Some(cur) = current {
            if ui
                .add(
                    egui::Button::new(format!("Set Current as A ({})", cur.title()))
                        .min_size(Vec2::new(0.0, 26.0)),
                )
                .clicked()
            {
                state.compare_a = Some(cur.clone());
            }
            if ui
                .add(
                    egui::Button::new(format!("Set Current as B ({})", cur.title()))
                        .min_size(Vec2::new(0.0, 26.0)),
                )
                .clicked()
            {
                state.compare_b = Some(cur.clone());
            }
        }
        if state.compare_a.is_some()
            && state.compare_b.is_some()
            && ui
                .add(egui::Button::new("Swap A <-> B").min_size(Vec2::new(0.0, 26.0)))
                .clicked()
        {
            std::mem::swap(&mut state.compare_a, &mut state.compare_b);
        }
        if recent.len() >= 2
            && (state.compare_a.is_none() || state.compare_b.is_none())
            && ui
                .add(
                    egui::Button::new(format!(
                        "Compare Most Recent Two ({} vs {})",
                        recent[1].title(),
                        recent[0].title()
                    ))
                    .min_size(Vec2::new(0.0, 26.0)),
                )
                .clicked()
        {
            state.compare_a = Some(recent[1].clone());
            state.compare_b = Some(recent[0].clone());
        }
    });

    ui.add_space(8.0);
    let (Some(a), Some(b)) = (&state.compare_a, &state.compare_b) else {
        Frame::none()
            .fill(pal.surface)
            .rounding(Rounding::same(8.0))
            .stroke(Stroke::new(1.0_f32, pal.border))
            .inner_margin(Margin::same(16.0))
            .show(ui, |ui| {
                ui.set_width((avail_w - 34.0).max(200.0));
                ui.label(
                    RichText::new("A/B Release Comparison")
                        .size(15.0)
                        .strong()
                        .color(pal.text_primary),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Scan two releases or tracks, click 'Set Current as A' on the baseline release, then 'Set Current as B' on the alternate release to compare DR, LUFSi, True Peak, and PSR deltas side-by-side.",
                    )
                    .size(13.0)
                    .color(pal.text_muted),
                );
            });
        return;
    };

    let tracks_a = a.owned_tracks();
    let tracks_b = b.owned_tracks();
    let dr_a = compute_official_album_dr(&tracks_a);
    let dr_b = compute_official_album_dr(&tracks_b);
    let delta_dr = dr_b - dr_a;

    egui::ScrollArea::vertical()
        .id_salt("compare_ab_vscroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let v_avail = ui.available_width().max(260.0);
            ui.set_width(v_avail);

            let cmp_kpis = vec![
                (format!("Baseline A: {}", a.title()), format!("DR{}", dr_a), dr_tier_color(dr_a, pal)),
                (format!("Alternate B: {}", b.title()), format!("DR{}", dr_b), dr_tier_color(dr_b, pal)),
                (
                    "Album DR Delta (B - A)".to_string(),
                    format!("{:+}", delta_dr),
                    if delta_dr > 0 {
                        pal.success
                    } else if delta_dr < 0 {
                        pal.error
                    } else {
                        pal.text_primary
                    },
                ),
            ];
            let gap = 8.0_f32;
            if v_avail >= 640.0 {
                let card_w = ((v_avail - gap * 2.0) / 3.0).max(120.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (idx, (lbl, val, col)) in cmp_kpis.iter().enumerate() {
                        let w = if idx == 2 {
                            (v_avail - (card_w + gap) * 2.0).max(120.0)
                        } else {
                            card_w
                        };
                        kpi_pill_sized(ui, lbl, val, *col, Some(w), pal);
                    }
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                    for (lbl, val, col) in &cmp_kpis {
                        kpi_pill_sized(ui, lbl, val, *col, None, pal);
                    }
                });
            }

            ui.add_space(6.0);
            if ui
                .add(egui::Button::new("Copy Comparison Report").min_size(Vec2::new(0.0, 26.0)))
                .clicked()
            {
                let txt = format_comparison_text(
                    &a.title(),
                    &tracks_a,
                    &b.title(),
                    &tracks_b,
                    &config.log_filenames,
                    config.use_embedded_tags,
                );
                ctx.copy_text(txt);
            }

            ui.add_space(8.0);
            let card_pad = 10.0_f32;
            Frame::none()
                .fill(pal.surface)
                .rounding(Rounding::same(8.0))
                .stroke(Stroke::new(1.0_f32, pal.border))
                .inner_margin(Margin::same(card_pad))
                .show(ui, |ui| {
                    let inner_avail = (v_avail - card_pad * 2.0 - 2.0).max(200.0);
                    ui.set_width(inner_avail);

                    let scale = (inner_avail / 880.0).clamp(1.0, 1.25);
                    let c_gap = (8.0 * scale).round();
                    let pad_x = 10.0_f32;
                    let w_num = (30.0 * scale).round();
                    let w_dra = (54.0 * scale).round();
                    let w_drb = (54.0 * scale).round();
                    let w_ddr = (44.0 * scale).round();
                    let w_la = (66.0 * scale).round();
                    let w_lb = (66.0 * scale).round();
                    let w_dl = (64.0 * scale).round();
                    let w_ta = (66.0 * scale).round();
                    let w_tb = (66.0 * scale).round();
                    let fixed_sum = w_num + w_dra + w_drb + w_ddr + w_la + w_lb + w_dl + w_ta + w_tb;
                    let total_gaps = c_gap * 9.0;
                    let w_track = (inner_avail - pad_x * 2.0 - total_gaps - fixed_sum).max(160.0);
                    let table_w = pad_x * 2.0 + fixed_sum + w_track + total_gaps;

                    let cols = [
                        ("#", w_num, true),
                        ("Track", w_track, false),
                        ("DR A", w_dra, true),
                        ("DR B", w_drb, true),
                        ("dDR", w_ddr, true),
                        ("LUFS A", w_la, true),
                        ("LUFS B", w_lb, true),
                        ("dLUFS", w_dl, true),
                        ("TPL A", w_ta, true),
                        ("TPL B", w_tb, true),
                    ];
                    let mut col_x = Vec::with_capacity(cols.len());
                    let mut cur_x = pad_x;
                    for &(_, cw, r_al) in &cols {
                        col_x.push((cur_x, cw, r_al));
                        cur_x += cw + c_gap;
                    }

                    egui::ScrollArea::horizontal()
                        .id_salt("compare_ab_hscroll")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let hdr_h = 28.0_f32;
                            let (hdr_rect, _) = ui.allocate_exact_size(
                                Vec2::new(table_w, hdr_h),
                                Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                hdr_rect,
                                Rounding {
                                    nw: 5.0,
                                    ne: 5.0,
                                    sw: 0.0,
                                    se: 0.0,
                                },
                                pal.well,
                            );
                            ui.painter().line_segment(
                                [
                                    Pos2::new(hdr_rect.left(), hdr_rect.bottom()),
                                    Pos2::new(hdr_rect.right(), hdr_rect.bottom()),
                                ],
                                Stroke::new(1.0_f32, pal.border),
                            );
                            for (idx, (lbl, _, _)) in cols.iter().enumerate() {
                                let (rx, rw, r_al) = col_x[idx];
                                let (pos, align) = if r_al {
                                    (Pos2::new(hdr_rect.left() + rx + rw, hdr_rect.center().y), Align2::RIGHT_CENTER)
                                } else {
                                    (Pos2::new(hdr_rect.left() + rx, hdr_rect.center().y), Align2::LEFT_CENTER)
                                };
                                ui.painter().text(
                                    pos,
                                    align,
                                    *lbl,
                                    FontId::proportional(11.5),
                                    pal.text_muted,
                                );
                            }

                            let row_h = 26.0_f32;
                            let zebra = if pal.dark {
                                Color32::from_rgba_unmultiplied(0, 0, 0, 72)
                            } else {
                                Color32::from_rgba_unmultiplied(15, 23, 42, 22)
                            };

                            let n = tracks_a.len().max(tracks_b.len());
                            for i in 0..n {
                                let ta = tracks_a.get(i);
                                let tb = tracks_b.get(i);
                                let (row_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(table_w, row_h),
                                    Sense::hover(),
                                );
                                if i % 2 == 1 {
                                    ui.painter().rect_filled(row_rect, Rounding::ZERO, zebra);
                                }
                                let cy = row_rect.center().y;
                                let mono = FontId::monospace(12.0);
                                let painter = ui.painter();

                                let (x0, w0, _) = col_x[0];
                                painter.text(
                                    Pos2::new(row_rect.left() + x0 + w0, cy),
                                    Align2::RIGHT_CENTER,
                                    format!("{}", i + 1),
                                    FontId::monospace(11.5),
                                    pal.text_muted,
                                );

                                let name = ta
                                    .or(tb)
                                    .map(|t| {
                                        format_log_track_name(
                                            &t.display_name(config.use_embedded_tags),
                                            &config.log_filenames,
                                        )
                                    })
                                    .unwrap_or_default();
                                let (x1, w1, _) = col_x[1];
                                paint_truncated_text(
                                    ui,
                                    painter,
                                    row_rect.left() + x1,
                                    cy,
                                    w1,
                                    &name,
                                    FontId::proportional(12.5),
                                    pal.text_primary,
                                );

                                let a_dr = ta.map(|t| format!("DR{}", t.dr_track_rounded)).unwrap_or_else(|| "-".into());
                                let b_dr = tb.map(|t| format!("DR{}", t.dr_track_rounded)).unwrap_or_else(|| "-".into());
                                let d_dr = match (ta, tb) {
                                    (Some(x), Some(y)) => y.dr_track_rounded - x.dr_track_rounded,
                                    _ => 0,
                                };
                                let d_color = if d_dr > 0 {
                                    pal.success
                                } else if d_dr < 0 {
                                    pal.error
                                } else {
                                    pal.text_muted
                                };

                                let (x2, w2, _) = col_x[2];
                                painter.text(Pos2::new(row_rect.left() + x2 + w2, cy), Align2::RIGHT_CENTER, a_dr, mono.clone(), pal.text_primary);
                                let (x3, w3, _) = col_x[3];
                                painter.text(Pos2::new(row_rect.left() + x3 + w3, cy), Align2::RIGHT_CENTER, b_dr, mono.clone(), pal.text_primary);
                                let (x4, w4, _) = col_x[4];
                                painter.text(Pos2::new(row_rect.left() + x4 + w4, cy), Align2::RIGHT_CENTER, format!("{:+}", d_dr), mono.clone(), d_color);

                                let lufs_a = ta.map(|t| format!("{:.2}", t.integrated_lufs)).unwrap_or_else(|| "-".into());
                                let lufs_b = tb.map(|t| format!("{:.2}", t.integrated_lufs)).unwrap_or_else(|| "-".into());
                                let d_lufs = match (ta, tb) {
                                    (Some(x), Some(y)) if x.integrated_lufs.is_finite() && y.integrated_lufs.is_finite() => {
                                        format!("{:+.2}", y.integrated_lufs - x.integrated_lufs)
                                    }
                                    _ => "-".into(),
                                };
                                let (x5, w5, _) = col_x[5];
                                painter.text(Pos2::new(row_rect.left() + x5 + w5, cy), Align2::RIGHT_CENTER, lufs_a, mono.clone(), pal.text_primary);
                                let (x6, w6, _) = col_x[6];
                                painter.text(Pos2::new(row_rect.left() + x6 + w6, cy), Align2::RIGHT_CENTER, lufs_b, mono.clone(), pal.text_primary);
                                let (x7, w7, _) = col_x[7];
                                painter.text(Pos2::new(row_rect.left() + x7 + w7, cy), Align2::RIGHT_CENTER, d_lufs, mono.clone(), pal.text_primary);

                                let tpl_a = ta.map(|t| format!("{:+.2}", t.max_tpl_joint)).unwrap_or_else(|| "-".into());
                                let tpl_b = tb.map(|t| format!("{:+.2}", t.max_tpl_joint)).unwrap_or_else(|| "-".into());
                                let (x8, w8, _) = col_x[8];
                                painter.text(Pos2::new(row_rect.left() + x8 + w8, cy), Align2::RIGHT_CENTER, tpl_a, mono.clone(), pal.text_primary);
                                let (x9, w9, _) = col_x[9];
                                painter.text(Pos2::new(row_rect.left() + x9 + w9, cy), Align2::RIGHT_CENTER, tpl_b, mono, pal.text_primary);
                            }
                        });
                });
        });
}
