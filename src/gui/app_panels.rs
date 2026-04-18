use eframe::egui;
use eframe::egui::{Color32, RichText};
use std::time::{Duration, Instant};

use crate::SortOrder;

use super::types::{ListItem, ListItemKind};
use super::ui_style::{icon_button, icon_text, ui_font_label};
use super::{
    clamp_list_title, MemoGuiApp, BG_COLOR_PRESETS, FONT_CANDIDATES, ICON_ADD, ICON_DOCS,
    ICON_MENU, ICON_RESTORE, ICON_TRASH, SEARCH_DEBOUNCE_MS, TEXT_COLOR_PRESETS, UI_ZOOM_MAX,
    UI_ZOOM_MIN,
};

impl MemoGuiApp {
    pub(super) fn draw_left_panel(&mut self, ctx: &egui::Context) {
        if self.focus_mode {
            return;
        }

        let viewport_width = ctx
            .input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect))
            .map(|r| r.width())
            .unwrap_or(1280.0);
        let left_min_width = 220.0_f32;
        let left_max_width = (viewport_width * 0.70).clamp(360.0, 960.0);
        let left_default_width = (viewport_width * 0.32).clamp(320.0, 460.0);

        egui::SidePanel::left("memo_left_panel_v3")
            .resizable(true)
            .default_width(left_default_width)
            .min_width(left_min_width)
            .max_width(left_max_width)
            .show(ctx, |ui| {
                let visuals = ui.visuals().clone();
                let subtle_text = visuals.widgets.noninteractive.fg_stroke.color;
                let accent = visuals.hyperlink_color;
                let row_fill = visuals.faint_bg_color;
                let row_selected_fill = visuals.selection.bg_fill;
                let row_stroke = visuals.widgets.noninteractive.bg_stroke.color;

                ui.add_space(6.0);
                let search_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("検索 or #tag")
                        .desired_width(f32::INFINITY),
                );
                if self.focus_search_requested {
                    search_resp.request_focus();
                    self.focus_search_requested = false;
                }
                if search_resp.changed() {
                    self.app_state.last_query = if self.search_query.trim().is_empty() {
                        None
                    } else {
                        Some(self.search_query.clone())
                    };
                    self.search_debounce_until =
                        Some(Instant::now() + Duration::from_millis(SEARCH_DEBOUNCE_MS));
                }

                ui.separator();
                ui.horizontal(|ui| {
                    let all_selected = !self.show_recent && !self.show_trash;
                    if ui.selectable_label(all_selected, "すべて").clicked() {
                        self.set_list_mode(false, false);
                    }
                    if ui.selectable_label(self.show_recent, "最近").clicked() {
                        self.set_list_mode(true, false);
                    }
                    if ui.selectable_label(self.show_trash, "ゴミ箱").clicked() {
                        self.set_list_mode(false, true);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("並び").small().color(subtle_text));
                    if ui
                        .selectable_label(
                            matches!(self.list_sort, SortOrder::UpdatedDesc),
                            "更新日",
                        )
                        .clicked()
                    {
                        self.list_sort = SortOrder::UpdatedDesc;
                        self.list_dirty = true;
                    }
                    if ui
                        .selectable_label(
                            matches!(self.list_sort, SortOrder::CreatedDesc),
                            "作成日",
                        )
                        .clicked()
                    {
                        self.list_sort = SortOrder::CreatedDesc;
                        self.list_dirty = true;
                    }
                });

                ui.separator();

                let plus_button_size = egui::vec2(44.0, 44.0);
                let info_rows = if self.show_perf_line { 2.0 } else { 1.0 };
                let controls_height = plus_button_size.y + 24.0 + info_rows * 22.0;
                let list_height = (ui.available_height() - controls_height).max(120.0);
                let list_width = ui.available_width();
                let compact = list_width < 320.0;
                let ultra_compact = list_width < 260.0;

                let row_total_height = if ultra_compact {
                    56.0
                } else if compact {
                    64.0
                } else {
                    74.0
                };
                let row_inner_height = if ultra_compact {
                    44.0
                } else if compact {
                    52.0
                } else {
                    62.0
                };
                let row_padding = if ultra_compact {
                    5.0
                } else if compact {
                    6.0
                } else {
                    8.0
                };
                let button_side: f32 = if ultra_compact {
                    16.0
                } else if compact {
                    17.0
                } else {
                    18.0
                };
                let icon_size: f32 = if ultra_compact {
                    10.0
                } else if compact {
                    10.5
                } else {
                    11.0
                };
                let show_tags_in_list = !ultra_compact;
                let show_updated_text = list_width >= 280.0;

                let mut clicked_note: Option<ListItem> = None;
                let mut quick_action: Option<ListItem> = None;
                let mut markdown_toggle_for: Option<String> = None;
                let mut tag_edit_for: Option<String> = None;

                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .show_rows(
                        ui,
                        row_total_height,
                        self.list_items.len(),
                        |ui, row_range| {
                            for row_idx in row_range {
                                let item = &self.list_items[row_idx];
                                let selected =
                                    self.selected_note_id.as_deref() == Some(item.id.as_str());

                                let card_stroke = if selected {
                                    egui::Stroke::new(1.2, accent.gamma_multiply(0.55))
                                } else {
                                    egui::Stroke::new(1.0, row_stroke)
                                };

                                egui::Frame::new()
                                    .fill(if selected {
                                        row_selected_fill
                                    } else {
                                        row_fill
                                    })
                                    .stroke(card_stroke)
                                    .corner_radius(egui::CornerRadius::same(10))
                                    .show(ui, |ui| {
                                        let response = ui.allocate_response(
                                            egui::vec2(ui.available_width(), row_inner_height),
                                            egui::Sense::click(),
                                        );

                                        let supports_note_tools =
                                            matches!(item.kind, ListItemKind::Note)
                                                && !item.deleted;
                                        let button_size = egui::vec2(button_side, button_side);
                                        let action_w = button_size.x;

                                        let row_inner = response.rect.shrink(row_padding);
                                        let actions_rect = egui::Rect::from_min_max(
                                            egui::pos2(
                                                (row_inner.max.x - action_w)
                                                    .max(row_inner.min.x + 32.0),
                                                row_inner.min.y,
                                            ),
                                            row_inner.max,
                                        );
                                        let content_rect = egui::Rect::from_min_max(
                                            row_inner.min,
                                            egui::pos2(
                                                (actions_rect.min.x - 8.0)
                                                    .max(row_inner.min.x + 48.0),
                                                row_inner.max.y,
                                            ),
                                        );

                                        let mut row_action_clicked = false;

                                        let mut row_ui = ui.new_child(
                                            egui::UiBuilder::new()
                                                .max_rect(content_rect)
                                                .layout(egui::Layout::top_down(egui::Align::Min)),
                                        );
                                        row_ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                                        row_ui.spacing_mut().button_padding = egui::vec2(1.5, 0.0);

                                        row_ui.horizontal(|ui| {
                                            if supports_note_tools {
                                                let md_on = selected && self.markdown_render_mode;
                                                let md_btn = egui::Button::new(
                                                    RichText::new("M")
                                                        .size((icon_size - 0.5).max(8.5))
                                                        .strong(),
                                                )
                                                .min_size(button_size)
                                                .fill(if md_on {
                                                    accent.gamma_multiply(0.22)
                                                } else {
                                                    visuals.widgets.inactive.bg_fill
                                                });
                                                if ui
                                                    .add_sized(button_size, md_btn)
                                                    .on_hover_text("Markdown整形表示を切替")
                                                    .clicked()
                                                {
                                                    row_action_clicked = true;
                                                    markdown_toggle_for = Some(item.id.clone());
                                                }
                                            }

                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(clamp_list_title(&item.title))
                                                        .strong(),
                                                )
                                                .truncate(),
                                            );

                                            if show_updated_text {
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Min),
                                                    |ui| {
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(&item.updated_text)
                                                                    .small()
                                                                    .color(subtle_text),
                                                            )
                                                            .truncate(),
                                                        );
                                                    },
                                                );
                                            }
                                        });

                                        if show_tags_in_list
                                            && (supports_note_tools || !item.tags.is_empty())
                                        {
                                            let tag_text = item
                                                .tags
                                                .iter()
                                                .map(|t| format!("#{t}"))
                                                .collect::<Vec<_>>()
                                                .join(" ");
                                            row_ui.horizontal(|ui| {
                                                if supports_note_tools {
                                                    let tag_btn = egui::Button::new(
                                                        RichText::new("#")
                                                            .size((icon_size + 0.8).max(9.5))
                                                            .strong(),
                                                    )
                                                    .min_size(button_size);
                                                    if ui
                                                        .add_sized(button_size, tag_btn)
                                                        .on_hover_text("タグ編集")
                                                        .clicked()
                                                    {
                                                        row_action_clicked = true;
                                                        tag_edit_for = Some(item.id.clone());
                                                    }
                                                }

                                                if tag_text.is_empty() {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new("タグなし")
                                                                .small()
                                                                .color(subtle_text),
                                                        )
                                                        .truncate(),
                                                    );
                                                } else {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(tag_text)
                                                                .small()
                                                                .color(accent),
                                                        )
                                                        .truncate(),
                                                    );
                                                }
                                            });
                                        }

                                        let (quick_icon, quick_hint) = match item.kind {
                                            ListItemKind::ClipboardTextHistory => {
                                                (ICON_TRASH, "クリップボード履歴を削除")
                                            }
                                            ListItemKind::ClipboardImageHistory => {
                                                (ICON_TRASH, "画像クリップ履歴を削除")
                                            }
                                            ListItemKind::Note if item.deleted => {
                                                (ICON_RESTORE, "復元")
                                            }
                                            ListItemKind::Note => (ICON_TRASH, "削除"),
                                        };

                                        let mut actions_ui = ui.new_child(
                                            egui::UiBuilder::new().max_rect(actions_rect).layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                            ),
                                        );
                                        actions_ui.spacing_mut().button_padding =
                                            egui::vec2(1.5, 0.0);
                                        let del_btn =
                                            egui::Button::new(icon_text(quick_icon, icon_size))
                                                .min_size(button_size);
                                        if actions_ui
                                            .add_sized(button_size, del_btn)
                                            .on_hover_text(quick_hint)
                                            .clicked()
                                        {
                                            row_action_clicked = true;
                                            quick_action = Some(item.clone());
                                        }

                                        if response.clicked() && !row_action_clicked {
                                            clicked_note = Some(item.clone());
                                        }
                                    });
                            }
                        },
                    );

                if let Some(item) = quick_action {
                    match item.kind {
                        ListItemKind::ClipboardTextHistory => self.clear_clipboard_history(),
                        ListItemKind::ClipboardImageHistory => self.clear_image_clipboard_history(),
                        ListItemKind::Note if item.deleted => self.restore_note_by_id(&item.id),
                        ListItemKind::Note => self.delete_note_by_id(&item.id),
                    }
                }
                if let Some(note_id) = markdown_toggle_for {
                    self.toggle_markdown_mode_for_note(&note_id);
                }
                if let Some(note_id) = tag_edit_for {
                    self.open_tag_editor_for_note(&note_id);
                }
                if let Some(item) = clicked_note {
                    match item.kind {
                        ListItemKind::ClipboardTextHistory => self.select_clipboard_note(),
                        ListItemKind::ClipboardImageHistory => self.select_image_clipboard_note(),
                        ListItemKind::Note => {
                            self.selected_note_deleted = item.deleted;
                            self.select_note_by_id(&item.id);
                        }
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(icon_button(plus_button_size, ICON_ADD, 20.0))
                            .on_hover_text("新規メモ (Ctrl+N)")
                            .clicked()
                        {
                            self.create_note(String::new());
                            self.set_list_mode(false, false);
                            self.list_dirty = true;
                        }

                        let trash_icon = if self.show_trash {
                            ICON_DOCS
                        } else {
                            ICON_TRASH
                        };
                        let trash_hint = if self.show_trash {
                            "通常リストへ戻る"
                        } else {
                            "ゴミ箱を開く"
                        };
                        if ui
                            .add(icon_button(egui::vec2(38.0, 38.0), trash_icon, 16.0))
                            .on_hover_text(trash_hint)
                            .clicked()
                        {
                            self.set_list_mode(false, !self.show_trash);
                        }

                        if ui
                            .add(icon_button(egui::vec2(38.0, 38.0), ICON_MENU, 16.0))
                            .on_hover_text("メニュー (Ctrl+,)")
                            .clicked()
                        {
                            self.show_menu = true;
                        }
                    });

                    if self.show_perf_line {
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.perf_summary_line())
                                    .small()
                                    .color(subtle_text),
                            )
                            .wrap(),
                        );
                    }

                    if self.search_inflight.is_some() && !self.search_query.trim().is_empty() {
                        ui.add(
                            egui::Label::new(RichText::new("検索中...").small().color(subtle_text))
                                .truncate(),
                        );
                    }

                    if let Some(progress) = self.rebuild_progress_text() {
                        ui.add(
                            egui::Label::new(RichText::new(progress).small().color(accent)).wrap(),
                        );
                    }

                    ui.add(
                        egui::Label::new(
                            RichText::new(&self.status_line).small().color(subtle_text),
                        )
                        .wrap(),
                    );
                });
            });
    }

    pub(super) fn draw_menu_window(&mut self, ctx: &egui::Context) {
        if !self.show_menu {
            return;
        }

        let mut open = self.show_menu;
        let viewport_width = ctx
            .input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect))
            .map(|r| r.width())
            .unwrap_or(960.0);
        let viewport_height = ctx
            .input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect))
            .map(|r| r.height())
            .unwrap_or(720.0);
        let width_cap = (viewport_width - 24.0).max(220.0);
        let height_cap = (viewport_height - 24.0).max(200.0);
        let menu_width = (viewport_width * 0.46).clamp(320.0, 520.0).min(width_cap);
        let menu_height = (viewport_height - 40.0).clamp(260.0, 820.0).min(height_cap);

        egui::Window::new("メニュー")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .fixed_size(egui::vec2(menu_width, menu_height))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(menu_height - 24.0)
                    .show(ui, |ui| {
                        let mut appearance_changed = false;

                        ui.label(RichText::new("設定").strong());
                        ui.horizontal(|ui| {
                            ui.label("フォント");
                            egui::ComboBox::from_id_salt("ui_font_family")
                                .selected_text(ui_font_label(&self.app_state.ui_font_family))
                                .show_ui(ui, |ui| {
                                    for (font_id, font_label, _) in FONT_CANDIDATES {
                                        if ui
                                            .selectable_value(
                                                &mut self.app_state.ui_font_family,
                                                font_id.to_string(),
                                                font_label,
                                            )
                                            .changed()
                                        {
                                            appearance_changed = true;
                                        }
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("文字色");
                        });
                        ui.horizontal_wrapped(|ui| {
                            for (label, rgb) in TEXT_COLOR_PRESETS {
                                let selected = self.app_state.ui_text_color_rgb == rgb;
                                if ui.selectable_label(selected, label).clicked() {
                                    self.app_state.ui_text_color_rgb = rgb;
                                    appearance_changed = true;
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("背景色");
                        });
                        ui.horizontal_wrapped(|ui| {
                            for (label, rgb) in BG_COLOR_PRESETS {
                                let selected = self.app_state.ui_background_color_rgb == rgb;
                                if ui.selectable_label(selected, label).clicked() {
                                    self.app_state.ui_background_color_rgb = rgb;
                                    appearance_changed = true;
                                }
                            }
                        });

                        ui.add(
                            egui::Slider::new(&mut self.ui_zoom, UI_ZOOM_MIN..=UI_ZOOM_MAX)
                                .text("UI拡大率")
                                .step_by(0.01),
                        );
                        if ui.button("中/小サイズ切替").clicked() {
                            self.toggle_window_size_preset(ctx);
                        }
                        ui.checkbox(&mut self.show_perf_line, "計測行を表示");
                        if ui
                            .checkbox(&mut self.always_on_top, "常に前面に表示")
                            .changed()
                        {
                            self.apply_window_level(ctx);
                        }

                        ui.separator();
                        ui.label(RichText::new("表示モード").strong());
                        ui.checkbox(&mut self.markdown_render_mode, "Markdown整形表示モード");
                        ui.checkbox(&mut self.focus_mode, "1メモ集中モード");

                        ui.separator();
                        ui.label(RichText::new("ツール").strong());
                        if ui.button("今日のメモを作成 (Ctrl+D)").clicked() {
                            self.create_today_note();
                            self.set_list_mode(false, false);
                            self.list_dirty = true;
                        }

                        let rebuilding = self.rebuild_index_rx.is_some();
                        let rebuild_label = if rebuilding {
                            "検索インデックス再構築中..."
                        } else {
                            "検索インデックス再構築"
                        };
                        if ui
                            .add_enabled(!rebuilding, egui::Button::new(rebuild_label))
                            .clicked()
                        {
                            self.rebuild_search_index();
                        }
                        if let Some(progress) = self.rebuild_progress_text() {
                            ui.label(
                                RichText::new(progress)
                                    .small()
                                    .color(Color32::from_gray(120)),
                            );
                        }

                        ui.separator();
                        ui.label(RichText::new("データ入出力").strong());
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("エクスポート先: {}", self.export_path))
                                    .small(),
                            )
                            .wrap(),
                        );
                        if ui.button("エクスポート").clicked() {
                            self.export_notes_from_gui();
                        }

                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("インポート元: {}", self.import_path))
                                    .small(),
                            )
                            .wrap(),
                        );
                        if ui.button("インポート").clicked() {
                            self.import_notes_from_gui();
                        }

                        if self.selected_note_deleted {
                            let purge_label = if self.purge_armed_for.as_deref()
                                == self.selected_note_id.as_deref()
                            {
                                "もう一度押すと完全削除 (Shift+Delete)"
                            } else {
                                "完全削除 (Shift+Delete)"
                            };
                            if ui
                                .add(egui::Button::new(
                                    RichText::new(purge_label)
                                        .color(Color32::from_rgb(170, 30, 30)),
                                ))
                                .on_hover_text("ゴミ箱から完全削除")
                                .clicked()
                            {
                                self.handle_purge_action();
                            }
                        }

                        if self.show_trash {
                            let empty_label = if self.purge_all_trash_armed {
                                "もう一度押すとゴミ箱を空にする"
                            } else {
                                "ゴミ箱を空にする"
                            };
                            if ui
                                .add(egui::Button::new(
                                    RichText::new(empty_label)
                                        .color(Color32::from_rgb(170, 30, 30)),
                                ))
                                .on_hover_text("ゴミ箱内のメモをすべて完全削除")
                                .clicked()
                            {
                                self.handle_purge_all_trash_action();
                            }
                        }

                        if appearance_changed {
                            self.appearance_dirty = true;
                        }
                    });
            });

        self.show_menu = open;
    }

    pub(super) fn draw_tag_editor_window(&mut self, ctx: &egui::Context) {
        if !self.show_tag_editor {
            return;
        }
        if self.tag_editor_note_id.is_none() {
            self.close_tag_editor();
            return;
        }

        let mut open = self.show_tag_editor;
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new("タグ編集")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label("スペース区切りで入力 (# は任意)");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.tag_editor_input)
                        .hint_text("work idea rust")
                        .desired_width(f32::INFINITY),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save_clicked = true;
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("保存").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("キャンセル").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if save_clicked {
            self.commit_tag_editor();
            return;
        }

        if cancel_clicked || !open {
            self.close_tag_editor();
        }
    }
}
