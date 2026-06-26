use chrono::Local;
use eframe::egui;
use eframe::egui::RichText;

use super::markdown::render_markdown_preview;
use super::ui_style::icon_button;
use super::{MemoGuiApp, ICON_MENU};

const EDITOR_BOTTOM_PADDING: f32 = 96.0;

impl MemoGuiApp {
    pub(super) fn draw_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_central_content(ui, ctx);
        });
    }

    pub(super) fn draw_central_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let visuals = ui.visuals().clone();
        let subtle_text = visuals.widgets.noninteractive.fg_stroke.color;
        let panel_fill = visuals.faint_bg_color;
        let panel_stroke = visuals.widgets.noninteractive.bg_stroke.color;
        if self.selected_note_id.is_none() {
            ui.add_space(16.0);
            ui.label(
                RichText::new("左の + からメモを作成")
                    .size(18.0)
                    .color(subtle_text),
            );
            return;
        }
        if !self.focus_mode {
            ui.add_space(4.0);
        }

        if self.is_clipboard_note_selected() {
            let available = ui.available_size_before_wrap();
            let editor_w = available.x.max(200.0);
            let editor_h = available.y.max(180.0);
            ui.allocate_ui(egui::vec2(editor_w, editor_h), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("clipboard_text_editor_scroll")
                    .show(ui, |ui| {
                        let desired_rows = self
                            .editor_body
                            .lines()
                            .count()
                            .saturating_add(2)
                            .clamp(12, 6000);
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut self.editor_body)
                                .desired_rows(desired_rows)
                                .lock_focus(true)
                                .desired_width(f32::INFINITY)
                                .frame(false)
                                .font(egui::TextStyle::Body),
                        );
                        if response.changed() {
                            self.apply_clipboard_note_changes();
                        }
                    });
            });
        } else if self.is_image_clipboard_note_selected() {
            ui.horizontal(|ui| {
                if ui.button("管理画像を一括削除").clicked() {
                    self.clear_image_clipboard_history();
                }
                ui.label(
                    RichText::new("保持したい画像は「保持して履歴から外す」を押す")
                        .small()
                        .color(subtle_text),
                );
            });
            ui.separator();

            let mut keep_id: Option<String> = None;
            let row_height = 168.0;
            egui::ScrollArea::vertical().show_rows(
                ui,
                row_height,
                self.clipboard_image_history.len(),
                |ui, row_range| {
                    for row_idx in row_range {
                        let entry = self.clipboard_image_history[row_idx].clone();
                        let image_path = self.clipboard_image_dir.join(&entry.file_name);
                        egui::Frame::new()
                            .fill(panel_fill)
                            .stroke(egui::Stroke::new(1.0, panel_stroke))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some((tex_id, tex_size)) =
                                        self.image_thumbnail_for_entry(ctx, &entry)
                                    {
                                        let max_w = 184.0;
                                        let max_h = 108.0;
                                        let scale =
                                            (max_w / tex_size.x).min(max_h / tex_size.y).min(1.0);
                                        let display_size = egui::vec2(
                                            (tex_size.x * scale).max(1.0),
                                            (tex_size.y * scale).max(1.0),
                                        );
                                        ui.image((tex_id, display_size));
                                    } else {
                                        ui.allocate_ui(egui::vec2(160.0, 96.0), |ui| {
                                            ui.vertical_centered(|ui| {
                                                ui.label(
                                                    RichText::new("サムネイルなし")
                                                        .small()
                                                        .color(subtle_text),
                                                );
                                            });
                                        });
                                    }

                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(
                                                    entry
                                                        .copied_at
                                                        .with_timezone(&Local)
                                                        .format("%Y-%m-%d %H:%M:%S")
                                                        .to_string(),
                                                )
                                                .strong(),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Min),
                                                |ui| {
                                                    if ui.button("保持して履歴から外す").clicked()
                                                    {
                                                        keep_id = Some(entry.id.clone());
                                                    }
                                                },
                                            );
                                        });
                                        ui.label(
                                            RichText::new(format!(
                                                "{}x{} / {} KB",
                                                entry.width,
                                                entry.height,
                                                (entry.byte_size / 1024).max(1)
                                            ))
                                            .small(),
                                        );
                                        ui.label(
                                            RichText::new(image_path.display().to_string())
                                                .small()
                                                .color(subtle_text),
                                        );
                                    });
                                });
                            });
                        ui.add_space(6.0);
                    }
                    if self.clipboard_image_history.is_empty() {
                        ui.label(
                            RichText::new("画像履歴はまだありません")
                                .small()
                                .color(subtle_text),
                        );
                    }
                },
            );
            if let Some(id) = keep_id {
                self.keep_image_clipboard_entry(&id);
            }
        } else if self.markdown_render_mode {
            let available = ui.available_size_before_wrap();
            let total_h = available.y.max(320.0);
            let total_w = available.x.max(200.0);
            let editor_h = (total_h * 0.52).clamp(170.0, total_h - 140.0);
            let preview_h = (total_h - editor_h - 30.0).max(120.0);
            ui.allocate_ui(egui::vec2(total_w, editor_h), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("markdown_editor_scroll")
                    .show(ui, |ui| {
                        let desired_rows = self
                            .editor_body
                            .lines()
                            .count()
                            .saturating_add(2)
                            .clamp(12, 6000);
                        let mut editor = egui::TextEdit::multiline(&mut self.editor_body)
                            .desired_rows(desired_rows)
                            .lock_focus(true)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .font(egui::TextStyle::Body);
                        if self.selected_note_deleted {
                            editor = editor.interactive(false);
                        }
                        let response = ui.add(editor);
                        ui.add_space(EDITOR_BOTTOM_PADDING);
                        if !self.selected_note_deleted && response.changed() {
                            if let Some(id) = self.selected_note_id.clone() {
                                let body = self.editor_body.clone();
                                self.autosave.schedule(id.clone(), body.clone());
                                self.write_recovery_draft(&id, &body);
                            }
                        }
                    });
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);
            ui.label(
                RichText::new("Markdownプレビュー")
                    .small()
                    .color(subtle_text),
            );
            ui.allocate_ui(egui::vec2(total_w, preview_h), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    render_markdown_preview(ui, &self.editor_body);
                });
            });
        } else {
            let available = ui.available_size_before_wrap();
            let editor_w = available.x.max(200.0);
            let editor_h = available.y.max(180.0);
            ui.allocate_ui(egui::vec2(editor_w, editor_h), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("note_editor_scroll")
                    .show(ui, |ui| {
                        let desired_rows = self
                            .editor_body
                            .lines()
                            .count()
                            .saturating_add(2)
                            .clamp(12, 6000);
                        let mut editor = egui::TextEdit::multiline(&mut self.editor_body)
                            .desired_rows(desired_rows)
                            .lock_focus(true)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .font(egui::TextStyle::Body);
                        if self.selected_note_deleted {
                            editor = editor.interactive(false);
                        }
                        let response = ui.add(editor);
                        ui.add_space(EDITOR_BOTTOM_PADDING);
                        if !self.selected_note_deleted && response.changed() {
                            if let Some(id) = self.selected_note_id.clone() {
                                let body = self.editor_body.clone();
                                self.autosave.schedule(id.clone(), body.clone());
                                self.write_recovery_draft(&id, &body);
                            }
                        }
                    });
            });
        }
    }

    pub(super) fn draw_focus_mode_button(&mut self, ctx: &egui::Context) {
        if !self.focus_mode {
            return;
        }
        egui::Area::new("focus_mode_menu_button".into())
            .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
            .show(ctx, |ui| {
                if ui
                    .add(icon_button(egui::vec2(32.0, 32.0), ICON_MENU, 15.0))
                    .on_hover_text("メニュー (Ctrl+,)")
                    .clicked()
                {
                    self.show_menu = true;
                }
            });
    }
}
