use eframe::egui;
use eframe::egui::{Color32, RichText};
use std::fs;

use super::{FONT_CANDIDATES, ICON_FAMILY_NAME, UI_ZOOM_MAX, UI_ZOOM_MIN};

pub(crate) fn clamp_zoom_pct(pct: u16) -> u16 {
    pct.clamp((UI_ZOOM_MIN * 100.0) as u16, (UI_ZOOM_MAX * 100.0) as u16)
}

pub(crate) fn zoom_from_pct(pct: u16) -> f32 {
    clamp_zoom_pct(pct) as f32 / 100.0
}

pub(crate) fn zoom_to_pct(zoom: f32) -> u16 {
    let bounded = zoom.clamp(UI_ZOOM_MIN, UI_ZOOM_MAX);
    (bounded * 100.0).round() as u16
}

pub(crate) fn setup_visuals_and_fonts(
    ctx: &egui::Context,
    preferred_font: &str,
    text_rgb: [u8; 3],
    bg_rgb: [u8; 3],
) {
    let text_color = Color32::from_rgb(text_rgb[0], text_rgb[1], text_rgb[2]);
    let bg_color = Color32::from_rgb(bg_rgb[0], bg_rgb[1], bg_rgb[2]);
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(text_color);
    visuals.panel_fill = bg_color;
    visuals.window_fill = bg_color;
    visuals.faint_bg_color = blend(bg_color, Color32::WHITE, 0.92);
    visuals.extreme_bg_color = blend(bg_color, Color32::BLACK, 0.92);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.indent = 10.0;
    style.spacing.slider_width = 160.0;
    style.spacing.interact_size = egui::vec2(28.0, 22.0);
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(15.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);

    let mut fonts = egui::FontDefinitions::default();
    let mut jp_fonts = load_japanese_fonts(&mut fonts);
    if let Some(pos) = jp_fonts.iter().position(|name| name == preferred_font) {
        jp_fonts.rotate_left(pos);
    }
    if !jp_fonts.is_empty() {
        for name in jp_fonts.iter().rev() {
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, name.clone());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, name.clone());
        }
    }
    load_windows_icon_font(&mut fonts);
    ctx.set_fonts(fonts);
}

pub(crate) fn icon_text(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph)
        .size(size)
        .family(egui::FontFamily::Name(ICON_FAMILY_NAME.into()))
}

pub(crate) fn icon_button<'a>(
    size: egui::Vec2,
    glyph: &'a str,
    icon_size: f32,
) -> impl egui::Widget + 'a {
    move |ui: &mut egui::Ui| {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            ui.painter().rect(
                rect,
                egui::CornerRadius::same(12),
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::new(icon_size, egui::FontFamily::Name(ICON_FAMILY_NAME.into())),
                visuals.text_color(),
            );
        }
        response
    }
}

pub(crate) fn ui_font_label(font_id: &str) -> &str {
    for (id, label, _) in FONT_CANDIDATES {
        if id == font_id {
            return label;
        }
    }
    "Yu Gothic UI"
}

pub(crate) fn load_app_icon_data() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../../app_icon_square.png")).ok()
}

fn load_japanese_fonts(fonts: &mut egui::FontDefinitions) -> Vec<String> {
    let mut loaded = Vec::new();
    for (name, _label, path) in FONT_CANDIDATES {
        if let Ok(bytes) = fs::read(path) {
            fonts
                .font_data
                .insert(name.to_string(), egui::FontData::from_owned(bytes).into());
            loaded.push(name.to_string());
        }
    }
    loaded
}

fn load_windows_icon_font(fonts: &mut egui::FontDefinitions) {
    let path = r"C:\Windows\Fonts\segmdl2.ttf";
    if let Ok(bytes) = fs::read(path) {
        fonts.font_data.insert(
            ICON_FAMILY_NAME.to_string(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Name(ICON_FAMILY_NAME.into()))
            .or_default()
            .insert(0, ICON_FAMILY_NAME.to_string());
    }
}

fn blend(a: Color32, b: Color32, ratio_a: f32) -> Color32 {
    let ra = ratio_a.clamp(0.0, 1.0);
    let rb = 1.0 - ra;
    let r = (a.r() as f32 * ra + b.r() as f32 * rb).round() as u8;
    let g = (a.g() as f32 * ra + b.g() as f32 * rb).round() as u8;
    let bch = (a.b() as f32 * ra + b.b() as f32 * rb).round() as u8;
    Color32::from_rgb(r, g, bch)
}
