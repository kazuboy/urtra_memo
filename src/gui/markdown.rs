use eframe::egui;
use eframe::egui::{Color32, RichText};

pub(crate) fn render_markdown_preview(ui: &mut egui::Ui, body: &str) {
    let mut in_code = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            ui.label(
                RichText::new(trimmed)
                    .monospace()
                    .color(Color32::from_gray(130)),
            );
            continue;
        }
        if in_code {
            ui.label(RichText::new(trimmed).monospace());
            continue;
        }
        if let Some((level, text)) = markdown_heading(trimmed) {
            let size = match level {
                1 => 26.0,
                2 => 22.0,
                3 => 19.0,
                _ => 17.0,
            };
            ui.label(RichText::new(text).size(size).strong());
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("- [ ] ") {
            ui.horizontal(|ui| {
                ui.checkbox(&mut false, "");
                ui.label(text);
            });
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("- [x] ") {
            let mut checked = true;
            ui.horizontal(|ui| {
                ui.checkbox(&mut checked, "");
                ui.label(RichText::new(text).strikethrough());
            });
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("- ") {
            ui.label(format!("• {text}"));
            continue;
        }
        if let Some((num, text)) = markdown_ordered(trimmed) {
            ui.label(format!("{num}. {text}"));
            continue;
        }
        if trimmed.starts_with('>') {
            ui.label(
                RichText::new(trimmed)
                    .italics()
                    .color(Color32::from_gray(95)),
            );
            continue;
        }
        ui.label(trimmed);
    }
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let mut level = 0usize;
    for ch in line.chars() {
        if ch == '#' {
            level += 1;
        } else {
            break;
        }
    }
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = line[level..].trim_start();
    if rest.is_empty() {
        return None;
    }
    Some((level, rest))
}

fn markdown_ordered(line: &str) -> Option<(usize, &str)> {
    let mut digits = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() || chars.next()? != '.' || chars.next()? != ' ' {
        return None;
    }
    let rest_start = digits.len() + 2;
    Some((digits.parse().ok()?, &line[rest_start..]))
}
