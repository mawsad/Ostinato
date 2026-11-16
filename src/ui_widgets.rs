use crate::theme::{active_palette, paint_neon_glow, SkinGraphic};
use crate::models::*;
use eframe::egui::{self, Color32, FontId, RichText, Rounding, Sense, Stroke, Vec2};


pub fn setup_ostinato_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(c_text_pri());
    visuals.panel_fill = c_bg_main();
    visuals.window_fill = c_bg_panel();
    visuals.extreme_bg_color = c_bg_void();
    visuals.faint_bg_color = c_bg_widget();
    visuals.code_bg_color = c_bg_void();
    visuals.warn_fg_color = c_bright();
    visuals.error_fg_color = c_neon();
    visuals.hyperlink_color = c_bright();

    visuals.window_stroke = Stroke::new(1.0_f32, c_border_dark());
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.resize_corner_size = 0.0;

    visuals.selection.bg_fill = c_deep();
    visuals.selection.stroke = Stroke::new(1.0_f32, c_neon());
    visuals.text_cursor.stroke = Stroke::new(1.5_f32, c_bright());

    visuals.widgets.noninteractive.bg_fill = c_bg_panel();
    visuals.widgets.noninteractive.weak_bg_fill = c_bg_void();
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c_border_dark());
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, c_text_pri());
    visuals.widgets.noninteractive.rounding = Rounding::same(6.0_f32);

    visuals.widgets.inactive.bg_fill = c_deep();
    visuals.widgets.inactive.weak_bg_fill = c_bg_widget();
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, c_border_dark());
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, c_text_pri());
    visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

    visuals.widgets.hovered.bg_fill = c_neon();
    visuals.widgets.hovered.weak_bg_fill = c_bg_hover();
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, c_neon());
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, c_bright());
    visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

    visuals.widgets.active.bg_fill = c_deep();
    visuals.widgets.active.weak_bg_fill = c_deep();
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, c_bright());
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, c_bright());
    visuals.widgets.active.rounding = Rounding::same(6.0_f32);

    visuals.widgets.open.bg_fill = c_bg_void();
    visuals.widgets.open.weak_bg_fill = c_bg_void();
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, c_neon());
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, c_bright());
    visuals.widgets.open.rounding = Rounding::same(6.0_f32);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.window_margin = egui::Margin::ZERO;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_width = 4.0;
    style.spacing.scroll.bar_inner_margin = 2.0;
    style.spacing.scroll.bar_outer_margin = 2.0;
    style.spacing.scroll.foreground_color = false;
    ctx.set_style(style);
        ctx.options_mut(|o| {
        o.warn_on_id_clash = false;
    });

}

pub fn show_nl_tooltip(resp: &egui::Response, text: &str) {
    let hover_id = resp.id.with("nl_hov_state");
    let now = resp.ctx.input(|i| i.time);
    let ptr_opt = resp.ctx.input(|i| i.pointer.hover_pos());

    if resp.hovered() && !resp.is_pointer_button_down_on() {
        let Some(ptr) = ptr_opt else { return };
        let (start_time, last_pos) = resp
            .ctx
            .data_mut(|d| *d.get_temp_mut_or_insert_with(hover_id, || (now, ptr)));

        if ptr.distance(last_pos) > 1.5 {
            resp.ctx.data_mut(|d| d.insert_temp(hover_id, (now, ptr)));
            resp.ctx.request_repaint_after(std::time::Duration::from_millis(80));
            return;
        }
        let elapsed = now - start_time;
        if elapsed < 0.55 {
            resp.ctx.request_repaint_after(std::time::Duration::from_millis(80));
            return;
        }
        let alpha = ((elapsed - 0.55) / 0.12).clamp(0.0, 1.0) as f32;
        if alpha < 1.0 { resp.ctx.request_repaint(); }

        let screen_w = resp.ctx.screen_rect().width();
        let approx_w = text.chars().count() as f32 * 7.2 + 24.0;
        let pos_x = (ptr.x + 12.0).min((screen_w - approx_w - 8.0).max(8.0));
        let pos_y = (ptr.y - 34.0).max(8.0);

        let a_u8 = (alpha * 255.0) as u8;
        let pal = active_palette();
        let bg = Color32::from_rgba_unmultiplied(pal.bg_bar.0, pal.bg_bar.1, pal.bg_bar.2, a_u8);
        let brd = Color32::from_rgba_unmultiplied(pal.border_active.0, pal.border_active.1, pal.border_active.2, a_u8);
        let txt_c = Color32::from_rgba_unmultiplied(pal.accent_bright.0, pal.accent_bright.1, pal.accent_bright.2, a_u8);

        egui::Area::new(resp.id.with("nl_clean_tip"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .fixed_pos(egui::pos2(pos_x, pos_y))
            .show(&resp.ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                egui::Frame::none()
                    .fill(bg)
                    .stroke(Stroke::new(1.0_f32, brd))
                    .rounding(Rounding::same(6.0_f32))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| { ui.label(RichText::new(text).size(12.0).color(txt_c)); });
            });
    } else {
        resp.ctx.data_mut(|d| d.remove::<(f64, egui::Pos2)>(hover_id));
    }
}
pub enum MediaIcon {
    Shuffle(bool),
    Heart(bool),
    Prev,
    Play,
    Pause,
    Next,
    Repeat(RepeatMode),
    Volume(f32),
    Lyrics(bool),
    Download,
}

impl MediaIcon {
    pub fn skin_key(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Prev => "prev",
            Self::Next => "next",
            Self::Heart(_) => "like",
            Self::Shuffle(_) => "shuffle",
            Self::Repeat(_) => "repeat",
            Self::Volume(_) => "volume",
            Self::Lyrics(_) => "lyrics",
            Self::Download => "download",
        }
    }
}

pub fn draw_media_icon_btn(
    ui: &mut egui::Ui,
    id_str: &str,
    rect: egui::Rect,
    icon: MediaIcon,
    primary: bool,
    disabled: bool,
    custom_tex: Option<&SkinGraphic>,
) -> egui::Response {
    let sense = if disabled { Sense::hover() } else { Sense::click() };
    let resp = ui.interact(rect, ui.id().with(id_str), sense);
    let hov = resp.hovered() && !disabled;
    if hov { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }

    let press_t = ui.ctx().animate_bool_with_time(
        resp.id.with("p_anim"),
        !disabled && resp.is_pointer_button_down_on(),
        0.09,
    );

    let is_toggled = matches!(
        icon,
        MediaIcon::Shuffle(true) | MediaIcon::Lyrics(true) | MediaIcon::Repeat(RepeatMode::All) | MediaIcon::Repeat(RepeatMode::One)
    );

    let bg = if disabled {
        c_bg_void()
    } else if primary {
        if hov { c_neon() } else { c_deep() }
    } else if is_toggled {
        if hov { c_deep() } else { c_subtle() }
    } else if hov {
        c_bg_hover()
    } else {
        c_bg_widget()
    };

    let border = if disabled { c_border_dark() } else if primary || hov || is_toggled { c_neon() } else { c_border_dark() };
    let ease = press_t * press_t * (3.0 - 2.0 * press_t);
    let draw_rect = rect.shrink(ease * 2.2);
    let rnd = Rounding::same(draw_rect.width() * 0.5);

    if primary && !disabled {
        let pal = active_palette();
        paint_neon_glow(ui.painter(), draw_rect, rnd, pal.accent_neon, if hov { 0.85 } else { 0.45 } * pal.glow_strength);
    }

    ui.painter().rect(draw_rect, rnd, bg, Stroke::new(1.0_f32, border));

    let fg = if disabled { c_text_dark() } else if primary || hov || is_toggled { c_bright() } else { c_text_pri() };

    if let Some(gfx) = custom_tex {
        let tex = gfx.current_texture(ui.ctx());
        let img_rect = draw_rect.shrink(draw_rect.width() * 0.20);
        let tint = if disabled {
            Color32::from_white_alpha(85)
        } else if matches!(icon, MediaIcon::Heart(true)) || is_toggled {
            c_neon()
        } else if hov {
            Color32::WHITE
        } else {
            Color32::from_white_alpha(230)
        };
            egui::Image::from_texture(tex).tint(tint).paint_at(ui, img_rect);
        if matches!(icon, MediaIcon::Repeat(RepeatMode::One)) {
            ui.painter().text(draw_rect.center(), egui::Align2::CENTER_CENTER, "1", FontId::proportional(9.5), c_bright());
        }
        return resp;
    }


    let c = draw_rect.center();
    let s = (rect.width() / 38.0).clamp(0.85, 1.5);
    let p = ui.painter();

    match icon {
        MediaIcon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - 3.5 * s, c.y - 6.8 * s), egui::pos2(c.x - 3.5 * s, c.y + 6.8 * s), egui::pos2(c.x + 7.2 * s, c.y)],
                fg, Stroke::NONE,
            ));
        }
        MediaIcon::Pause => {
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x - 4.0 * s, c.y), Vec2::new(3.2 * s, 13.0 * s)), Rounding::same(1.0), fg);
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x + 4.0 * s, c.y), Vec2::new(3.2 * s, 13.0 * s)), Rounding::same(1.0), fg);
        }
        MediaIcon::Prev => {
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x - 5.5 * s, c.y), Vec2::new(2.2 * s, 11.0 * s)), Rounding::same(1.0), fg);
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x + 5.5 * s, c.y - 5.5 * s), egui::pos2(c.x + 5.5 * s, c.y + 5.5 * s), egui::pos2(c.x - 3.0 * s, c.y)],
                fg, Stroke::NONE,
            ));
        }
        MediaIcon::Next => {
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x + 5.5 * s, c.y), Vec2::new(2.2 * s, 11.0 * s)), Rounding::same(1.0), fg);
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - 5.5 * s, c.y - 5.5 * s), egui::pos2(c.x - 5.5 * s, c.y + 5.5 * s), egui::pos2(c.x + 3.0 * s, c.y)],
                fg, Stroke::NONE,
            ));
        }
        MediaIcon::Heart(active) => {
            let h_col = if active { c_neon() } else if hov { c_bright() } else { c_text_gray() };
            p.circle_filled(egui::pos2(c.x - 3.1 * s, c.y - 2.0 * s), 3.5 * s, h_col);
            p.circle_filled(egui::pos2(c.x + 3.1 * s, c.y - 2.0 * s), 3.5 * s, h_col);
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - 6.4 * s, c.y - 0.5 * s), egui::pos2(c.x + 6.4 * s, c.y - 0.5 * s), egui::pos2(c.x, c.y + 6.5 * s)],
                h_col, Stroke::NONE,
            ));
        }
        MediaIcon::Shuffle(active) => {
            let col = if disabled { c_text_dark() } else if active { c_bright() } else if hov { c_text_pri() } else { c_text_gray() };
            let st = Stroke::new(1.6 * s, col);
            p.line_segment([egui::pos2(c.x - 6.0 * s, c.y - 4.0 * s), egui::pos2(c.x + 5.0 * s, c.y + 4.0 * s)], st);
            p.line_segment([egui::pos2(c.x - 6.0 * s, c.y + 4.0 * s), egui::pos2(c.x + 5.0 * s, c.y - 4.0 * s)], st);
            p.circle_filled(egui::pos2(c.x + 5.5 * s, c.y - 4.0 * s), 1.8 * s, col);
            p.circle_filled(egui::pos2(c.x + 5.5 * s, c.y + 4.0 * s), 1.8 * s, col);
        }
        MediaIcon::Repeat(mode) => {
            let col = if disabled { c_text_dark() } else if mode != RepeatMode::Off { c_bright() } else if hov { c_text_pri() } else { c_text_gray() };
            p.rect_stroke(egui::Rect::from_center_size(c, Vec2::new(12.0 * s, 8.5 * s)), Rounding::same(3.0 * s), Stroke::new(1.5 * s, col));
            if mode == RepeatMode::One {
                p.text(c, egui::Align2::CENTER_CENTER, "1", FontId::proportional(9.5 * s), c_bright());
            }
        }
        MediaIcon::Volume(v) => {
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x - 4.5 * s, c.y), Vec2::new(3.2 * s, 5.6 * s)), Rounding::same(1.0), fg);
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - 3.5 * s, c.y - 2.8 * s), egui::pos2(c.x + 1.5 * s, c.y - 6.2 * s), egui::pos2(c.x + 1.5 * s, c.y + 6.2 * s), egui::pos2(c.x - 3.5 * s, c.y + 2.8 * s)],
                fg, Stroke::NONE,
            ));
            if v > 0.02 {
                p.line_segment([egui::pos2(c.x + 4.5 * s, c.y - 3.5 * s), egui::pos2(c.x + 4.5 * s, c.y + 3.5 * s)], Stroke::new(1.6 * s, fg));
            } else {
                p.line_segment([egui::pos2(c.x + 3.5 * s, c.y - 2.5 * s), egui::pos2(c.x + 7.0 * s, c.y + 2.5 * s)], Stroke::new(1.5 * s, c_text_gray()));
            }
        }
        MediaIcon::Lyrics(active) => {
            let st = Stroke::new(1.6 * s, if active { c_bright() } else { fg });
            p.line_segment([egui::pos2(c.x - 5.5 * s, c.y - 4.2 * s), egui::pos2(c.x + 5.5 * s, c.y - 4.2 * s)], st);
            p.line_segment([egui::pos2(c.x - 5.5 * s, c.y), egui::pos2(c.x + 3.5 * s, c.y)], st);
            p.line_segment([egui::pos2(c.x - 5.5 * s, c.y + 4.2 * s), egui::pos2(c.x + 1.5 * s, c.y + 4.2 * s)], st);
        }
        MediaIcon::Download => {
            let st = Stroke::new(1.6 * s, fg);
            p.line_segment([egui::pos2(c.x, c.y - 5.8 * s), egui::pos2(c.x, c.y + 2.5 * s)], st);
            p.line_segment([egui::pos2(c.x - 3.8 * s, c.y - 1.0 * s), egui::pos2(c.x, c.y + 2.8 * s)], st);
            p.line_segment([egui::pos2(c.x + 3.8 * s, c.y - 1.0 * s), egui::pos2(c.x, c.y + 2.8 * s)], st);
            p.line_segment([egui::pos2(c.x - 5.5 * s, c.y + 5.5 * s), egui::pos2(c.x + 5.5 * s, c.y + 5.5 * s)], st);
        }
    }
    resp
}
pub fn draw_side_nav_btn(ui: &mut egui::Ui, tab: LeftTab, active: bool, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    show_nl_tooltip(&resp, tip);

    let bg = if active { c_deep() } else if resp.hovered() { c_bg_hover() } else { c_bg_widget() };
    let border = if active || resp.hovered() { c_neon() } else { c_border_dark() };
    ui.painter().rect(rect, Rounding::same(8.0_f32), bg, Stroke::new(1.0_f32, border));

    if active {
        let ind = egui::Rect::from_center_size(egui::pos2(rect.left() - 3.0, rect.center().y), Vec2::new(3.0, 18.0));
        ui.painter().rect_filled(ind, Rounding::same(1.5), c_bright());
    }

    let col = if active || resp.hovered() { c_bright() } else { c_text_pri() };
    let c = rect.center();
    let p = ui.painter();
    let st = Stroke::new(1.6_f32, col);

    match tab {
        LeftTab::Wave => {
            for (dx, h) in [(-6.0_f32, 6.0_f32), (-2.0, 13.0), (2.0, 9.0), (6.0, 14.0)] {
                p.line_segment([egui::pos2(c.x + dx, c.y - h * 0.5), egui::pos2(c.x + dx, c.y + h * 0.5)], Stroke::new(2.0, col));
            }
        }
        LeftTab::Playlists => {
            for y in [-5.0_f32, 0.0, 5.0] {
                p.line_segment([egui::pos2(c.x - 6.5, c.y + y), egui::pos2(c.x + 6.5, c.y + y)], st);
            }
        }
        LeftTab::Search => {
            let lc = egui::pos2(c.x - 1.5, c.y - 1.5);
            p.circle_stroke(lc, 5.0, st);
            p.line_segment([egui::pos2(lc.x + 3.6, lc.y + 3.6), egui::pos2(c.x + 6.5, c.y + 6.5)], Stroke::new(2.0, col));
        }
        LeftTab::QueueHistory => {
            p.line_segment([egui::pos2(c.x - 6.5, c.y - 4.5), egui::pos2(c.x + 6.5, c.y - 4.5)], st);
            p.line_segment([egui::pos2(c.x - 6.5, c.y + 0.5), egui::pos2(c.x + 3.5, c.y + 0.5)], st);
            p.line_segment([egui::pos2(c.x - 6.5, c.y + 5.5), egui::pos2(c.x + 1.0, c.y + 5.5)], st);
            p.circle_filled(egui::pos2(c.x + 5.5, c.y + 4.5), 2.2, col);
        }
        LeftTab::LocalFiles => {
            let r = egui::Rect::from_center_size(egui::pos2(c.x, c.y + 1.0), Vec2::new(14.0, 10.0));
            p.rect_stroke(r, Rounding::same(2.0), st);
            p.line_segment([egui::pos2(r.left() + 1.0, r.top() - 2.0), egui::pos2(r.left() + 6.0, r.top() - 2.0)], st);
        }
        LeftTab::Accounts => {
            p.circle_stroke(egui::pos2(c.x, c.y - 3.2), 3.4, st);
            p.rect_stroke(egui::Rect::from_center_size(egui::pos2(c.x, c.y + 4.8), Vec2::new(12.0, 5.5)), Rounding::same(2.5), st);
        }
        LeftTab::Themes => {
            p.circle_stroke(c, 6.5, st);
            p.circle_filled(egui::pos2(c.x - 2.2, c.y - 2.0), 1.4, col);
            p.circle_filled(egui::pos2(c.x + 2.4, c.y - 1.5), 1.4, col);
            p.circle_filled(egui::pos2(c.x, c.y + 2.5), 1.6, col);
        }
        LeftTab::Settings => {
            p.circle_stroke(c, 5.8, st);
            p.circle_filled(c, 2.0, col);
        }
    }
    resp.clicked()
}

pub fn eye_toggle_btn(ui: &mut egui::Ui, open: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 26.0), Sense::click());
    let bg = if resp.hovered() { c_bg_hover() } else { c_bg_widget() };
    ui.painter().rect(rect, Rounding::same(5.0), bg, Stroke::new(1.0, c_border_dark()));
    let c = rect.center();
    let col = if open { c_bright() } else { c_text_gray() };
    ui.painter().circle_stroke(c, 4.5, Stroke::new(1.4, col));
    if open {
        ui.painter().circle_filled(c, 1.8, col);
    } else {
        ui.painter().line_segment([egui::pos2(c.x - 5.0, c.y + 5.0), egui::pos2(c.x + 5.0, c.y - 5.0)], Stroke::new(1.4, col));
    }
    resp.clicked()
}

pub fn win_bar_btn(ui: &mut egui::Ui, kind: u8) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), Sense::click());
    let bg = if resp.hovered() { c_bg_hover() } else { c_bg_widget() };
    ui.painter().rect_filled(rect, Rounding::same(4.0), bg);
    let col = if resp.hovered() { c_bright() } else { c_text_gray() };
    let c = rect.center();
    let s = Stroke::new(1.4, col);
    match kind {
        0 => { ui.painter().line_segment([egui::pos2(c.x - 4.5, c.y + 2.0), egui::pos2(c.x + 4.5, c.y + 2.0)], s); }
        1 => { ui.painter().rect_stroke(egui::Rect::from_center_size(c, Vec2::splat(8.5)), Rounding::same(1.0), s); }
        _ => {
            ui.painter().line_segment([egui::pos2(c.x - 4.0, c.y - 4.0), egui::pos2(c.x + 4.0, c.y + 4.0)], s);
            ui.painter().line_segment([egui::pos2(c.x + 4.0, c.y - 4.0), egui::pos2(c.x - 4.0, c.y + 4.0)], s);
        }
    }
    resp
}

pub fn pill_action_btn(ui: &mut egui::Ui, label: &str, accent: bool) -> bool {
    let w = (label.chars().count() as f32 * 7.2 + 24.0).max(84.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let press_t = ui.ctx().animate_bool_with_time(resp.id.with("pill_p"), resp.is_pointer_button_down_on(), 0.09);
    let draw_r = rect.shrink(press_t * press_t * (3.0 - 2.0 * press_t) * 1.4);
    let bg = if accent { if resp.hovered() { c_neon() } else { c_deep() } } else if resp.hovered() { c_bg_hover() } else { c_bg_widget() };
    let border = if accent || resp.hovered() { c_neon() } else { c_border_dark() };
    ui.painter().rect(draw_r, Rounding::same(14.0), bg, Stroke::new(1.0, border));
    let col = if accent || resp.hovered() { c_bright() } else { c_text_pri() };
    ui.painter().text(draw_r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.5), col);
    resp.clicked()
}

pub fn full_width_btn(ui: &mut egui::Ui, label: &str, height: f32, accent: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let press_t = ui.ctx().animate_bool_with_time(resp.id.with("fw_p"), resp.is_pointer_button_down_on(), 0.09);
    let draw_r = rect.shrink(press_t * press_t * (3.0 - 2.0 * press_t) * 1.4);
    let bg = if accent { if resp.hovered() { c_neon() } else { c_deep() } } else if resp.hovered() { c_bg_hover() } else { c_bg_widget() };
    let border = if accent || resp.hovered() { c_neon() } else { c_border_dark() };
    ui.painter().rect(draw_r, Rounding::same(6.0), bg, Stroke::new(1.0, border));
    let col = if accent || resp.hovered() { c_bright() } else { c_text_pri() };
    ui.painter().text(draw_r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.0), col);
    resp.clicked()
}
pub struct RowAction {
    pub play: bool,
    pub play_next: bool,
    pub add_queue: bool,
    pub add_mix: bool,
}

pub fn list_item_row(ui: &mut egui::Ui, text: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let bg = if selected {
        c_bg_hover()
    } else if resp.hovered() {
        c_bg_widget()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, Rounding::same(5.0), bg);
    if selected {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 5.0),
            Vec2::new(3.0, rect.height() - 10.0),
        );
        ui.painter().rect_filled(bar, Rounding::same(1.5), c_neon());
    }
    let col = if selected {
        c_bright()
    } else if resp.hovered() {
        c_text_pri()
    } else {
        c_text_gray()
    };
    let max_chars = ((rect.width() - 20.0) / 7.0).max(12.0) as usize;
    let clipped: String = text.chars().take(max_chars).collect();
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clipped,
        FontId::proportional(12.2),
        col,
    );
    resp.clicked()
}

pub fn draw_provider_badge(painter: &egui::Painter, center: egui::Pos2, prov: TrackProvider) {
    let (r, g, b) = prov.badge_rgb();
    let rc = egui::Rect::from_center_size(center, Vec2::new(26.0, 15.0));
    painter.rect(
        rc,
        Rounding::same(3.5),
        Color32::from_rgba_unmultiplied(r, g, b, 26),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(r, g, b, 145)),
    );
    painter.text(
        rc.center(),
        egui::Align2::CENTER_CENTER,
        prov.short_tag(),
        FontId::proportional(9.2),
        Color32::from_rgb(r, g, b),
    );
}

pub fn track_row_with_queue(
    ui: &mut egui::Ui,
    id_salt: usize,
    prov: TrackProvider,
    title: &str,
    artist: &str,
    dur_sec: f32,
    is_current: bool,
    wide_mode: bool,
) -> RowAction {
    let row_h = if wide_mode { 36.0 } else { 30.0 };
    let avail_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::hover());

    let ptr = ui.input(|i| i.pointer.hover_pos());
    let row_hovered = ptr.map(|p| rect.contains(p)).unwrap_or(false);

    let btn_area_w = if row_hovered { 90.0 } else { 72.0 };
    let main_rect = egui::Rect::from_min_size(rect.min, Vec2::new((avail_w - btn_area_w).max(60.0), row_h));
    let main_resp = ui.interact(main_rect, ui.id().with(("tr_m", id_salt)), Sense::click());
    if main_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let bg = if is_current {
        c_bg_hover()
    } else if row_hovered {
        c_bg_widget()
    } else if wide_mode {
        c_bg_card()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, Rounding::same(6.0), bg);
    if is_current {
        let bar = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.top() + 5.0), Vec2::new(3.0, row_h - 10.0));
        ui.painter().rect_filled(bar, Rounding::same(1.5), c_neon());
    }

    let text_col = if is_current { c_bright() } else if row_hovered { c_text_pri() } else { c_text_muted() };
    let full_label = format!("{} — {}", artist, title);
    let max_ch = ((main_rect.width() - 16.0) / 7.1).max(12.0) as usize;
    let clipped: String = full_label.chars().take(max_ch).collect();

    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clipped,
        FontId::proportional(if wide_mode { 13.0 } else { 12.0 }),
        text_col,
    );

    let mut play_next = false;
    let mut add_queue = false;
    let mut add_mix = false;

    if row_hovered {
        let b1 = egui::Rect::from_center_size(egui::pos2(rect.right() - 73.0, rect.center().y), Vec2::new(24.0, 20.0));
        let b2 = egui::Rect::from_center_size(egui::pos2(rect.right() - 45.0, rect.center().y), Vec2::new(24.0, 20.0));
        let b3 = egui::Rect::from_center_size(egui::pos2(rect.right() - 17.0, rect.center().y), Vec2::new(24.0, 20.0));

        let r1 = ui.interact(b1, ui.id().with(("tr_n", id_salt)), Sense::click());
        let r2 = ui.interact(b2, ui.id().with(("tr_q", id_salt)), Sense::click());
        let r3 = ui.interact(b3, ui.id().with(("tr_mx", id_salt)), Sense::click());
        show_nl_tooltip(&r1, "Включить следующим");
        show_nl_tooltip(&r2, "Добавить в очередь");
        show_nl_tooltip(&r3, "Добавить в свой микс-плейлист");

        for (r_b, b_rc, lbl) in [(&r1, b1, ">1"), (&r2, b2, "+Q"), (&r3, b3, "+M")] {
            let bh = r_b.hovered();
            if bh { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
            ui.painter().rect(b_rc, Rounding::same(4.0), if bh { c_deep() } else { c_bg_void() }, Stroke::new(1.0, if bh { c_neon() } else { c_border_dark() }));
            ui.painter().text(b_rc.center(), egui::Align2::CENTER_CENTER, lbl, FontId::proportional(9.8), if bh { c_bright() } else { c_text_pri() });
        }
        play_next = r1.clicked();
        add_queue = r2.clicked();
        add_mix = r3.clicked();
    } else {
        draw_provider_badge(ui.painter(), egui::pos2(rect.right() - 56.0, rect.center().y), prov);
        ui.painter().text(egui::pos2(rect.right() - 6.0, rect.center().y), egui::Align2::RIGHT_CENTER, format_time(dur_sec), FontId::proportional(11.0), c_text_dark());
    }

    if main_resp.secondary_clicked() { play_next = true; }
    RowAction { play: main_resp.clicked(), play_next, add_queue, add_mix }
}

pub fn small_text_btn(ui: &mut egui::Ui, label: &str) -> bool {
    let w = (label.chars().count() as f32 * 6.5 + 10.0).max(52.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::click());
    if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let col = if resp.hovered() { c_bright() } else { c_text_gray() };
    ui.painter().text(rect.right_center(), egui::Align2::RIGHT_CENTER, label, FontId::proportional(10.8), col);
    resp.clicked()
}

pub fn dark_text_input(ui: &mut egui::Ui, width: f32, height: f32, text: &mut String, hint: &str, password: bool, use_token_white: bool) -> egui::Response {
    let mut resp_opt = None;
    let txt_col = if use_token_white { TOKEN_WHITE } else { c_bright() };
    egui::Frame::none().fill(c_bg_void()).stroke(Stroke::new(1.0, c_border_dark())).rounding(Rounding::same(6.0)).inner_margin(egui::Margin::symmetric(10.0, 5.0)).show(ui, |ui| {
        let inner_w = (width - 22.0).max(40.0);
        let r = ui.add_sized([inner_w, height], egui::TextEdit::singleline(text).frame(false).password(password).text_color(txt_col).hint_text(RichText::new(hint).color(c_text_gray())));
        if r.has_focus() || r.hovered() {
            ui.painter().rect_stroke(r.rect.expand2(Vec2::new(10.0, 5.0)), Rounding::same(6.0), Stroke::new(1.0, c_neon()));
        }
        resp_opt = Some(r);
    });
    resp_opt.unwrap()
}

pub fn artist_chip_card(ui: &mut egui::Ui, name: &str) -> bool {
    let w = (name.chars().count() as f32 * 7.8 + 46.0).clamp(130.0, 230.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::click());
    let hov = resp.hovered();
    if hov { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let press_t = ui.ctx().animate_bool_with_time(resp.id.with("art_p"), resp.is_pointer_button_down_on(), 0.09);
    let draw_r = rect.shrink(press_t * press_t * (3.0 - 2.0 * press_t) * 1.5);
    ui.painter().rect(draw_r, Rounding::same(8.0), if hov { c_deep() } else { c_bg_widget() }, Stroke::new(1.0, if hov { c_neon() } else { c_border_dark() }));
    let badge_c = egui::pos2(draw_r.left() + 16.0, draw_r.center().y);
    ui.painter().circle_filled(badge_c, 8.5, if hov { c_neon() } else { c_bg_void() });
    ui.painter().circle_stroke(badge_c, 8.5, Stroke::new(1.0, c_neon()));
    ui.painter().circle_filled(badge_c, 2.5, c_bright());
    let clipped: String = name.chars().take(22).collect();
    ui.painter().text(egui::pos2(draw_r.left() + 31.0, draw_r.center().y), egui::Align2::LEFT_CENTER, clipped, FontId::proportional(12.5), if hov { c_bright() } else { c_text_pri() });
    resp.clicked()
}

pub fn album_chip_card(ui: &mut egui::Ui, title: &str, artist: &str, year: Option<u32>) -> bool {
    let w = (title.chars().count() as f32 * 7.5 + 64.0).clamp(170.0, 270.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::click());
    let hov = resp.hovered();
    if hov { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let press_t = ui.ctx().animate_bool_with_time(resp.id.with("alb_p"), resp.is_pointer_button_down_on(), 0.09);
    let draw_r = rect.shrink(press_t * press_t * (3.0 - 2.0 * press_t) * 1.5);
    ui.painter().rect(draw_r, Rounding::same(8.0), if hov { c_deep() } else { c_bg_card() }, Stroke::new(1.0, if hov { c_neon() } else { c_border_dark() }));
    let sq = egui::Rect::from_center_size(egui::pos2(draw_r.left() + 20.0, draw_r.center().y), Vec2::splat(22.0));
    ui.painter().rect(sq, Rounding::same(4.0), if hov { c_neon() } else { c_bg_void() }, Stroke::new(1.0, c_neon()));
    ui.painter().circle_stroke(sq.center(), 5.0, Stroke::new(1.2, c_bright()));
    let clipped_t: String = title.chars().take(24).collect();
    ui.painter().text(egui::pos2(draw_r.left() + 38.0, draw_r.center().y - 7.0), egui::Align2::LEFT_CENTER, clipped_t, FontId::proportional(12.2), if hov { c_bright() } else { c_text_pri() });
    let sub = match year { Some(y) => format!("{} · {}", artist, y), None => artist.to_string() };
    let clipped_s: String = sub.chars().take(28).collect();
    ui.painter().text(egui::pos2(draw_r.left() + 38.0, draw_r.center().y + 8.0), egui::Align2::LEFT_CENTER, clipped_s, FontId::proportional(10.5), c_text_gray());
    resp.clicked()
}

pub fn format_time(sec: f32) -> String {
    let total = sec.max(0.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub fn wave_filter_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let w = (label.chars().count() as f32 * 6.8 + 20.0).max(64.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 25.0), Sense::click());
    let hov = resp.hovered();
    if hov { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
    let press_t = ui.ctx().animate_bool_with_time(resp.id.with("wf_p"), resp.is_pointer_button_down_on(), 0.09);
    let draw_r = rect.shrink(press_t * press_t * (3.0 - 2.0 * press_t) * 1.2);
    let bg = if selected { c_deep() } else if hov { c_bg_hover() } else { c_bg_widget() };
    let border = if selected || hov { c_neon() } else { c_border_dark() };
    ui.painter().rect(draw_r, Rounding::same(12.0), bg, Stroke::new(1.0, border));
    let col = if selected || hov { c_bright() } else { c_text_pri() };
    ui.painter().text(draw_r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.2), col);
    resp.clicked()
}
