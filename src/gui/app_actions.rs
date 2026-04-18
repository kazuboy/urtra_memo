use chrono::Local;
use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;

use crate::model::safe_title;
use crate::Note;

use super::clipboard::{is_clipboard_note_id, is_image_clipboard_note_id};
use super::{MemoGuiApp, WINDOW_MEDIUM_SIZE, WINDOW_SMALL_SIZE};

impl MemoGuiApp {
    pub(super) fn create_note(&mut self, body: String) {
        self.flush_pending_now();
        if let Some(store) = &mut self.store {
            match store.create_note(&body) {
                Ok(note) => {
                    self.clear_search_cache();
                    self.select_note(note);
                    self.list_dirty = true;
                }
                Err(err) => self.status_line = format!("菴懈・縺ｫ螟ｱ謨・ {err}"),
            }
        }
    }

    pub(super) fn create_today_note(&mut self) {
        self.flush_pending_now();
        if let Some(store) = &mut self.store {
            let today = Local::now().date_naive();
            match store.create_or_open_daily_note(today) {
                Ok(note) => {
                    self.clear_search_cache();
                    self.select_note(note);
                    self.list_dirty = true;
                }
                Err(err) => self.status_line = format!("莉頑律繝｡繝｢縺ｮ菴懈・縺ｫ螟ｱ謨・ {err}"),
            }
        }
    }

    pub(super) fn select_note_by_id(&mut self, note_id: &str) {
        if is_clipboard_note_id(note_id) {
            self.select_clipboard_note();
            return;
        }
        if is_image_clipboard_note_id(note_id) {
            self.select_image_clipboard_note();
            return;
        }
        self.flush_pending_now();
        if let Some(store) = &mut self.store {
            match store.load_note(note_id) {
                Ok(note) => self.select_note(note),
                Err(err) => self.status_line = format!("繝｡繝｢繧帝幕縺代∪縺帙ｓ: {err}"),
            }
        }
    }

    pub(super) fn select_note(&mut self, note: Note) {
        self.selected_note_id = Some(note.id.clone());
        self.selected_note_deleted = note.deleted;
        self.editor_body = note.body.clone();
        self.editor_tags = note.tags.join(" ");
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        if !note.deleted {
            if let Some(draft_body) = self.read_recovery_draft(&note.id) {
                if draft_body != note.body {
                    self.editor_body = draft_body.clone();
                    self.autosave.schedule(note.id.clone(), draft_body);
                    self.status_line =
                        "繧ｯ繝ｩ繝・す繝･蠕ｩ譌ｧ縺ｮ荳区嶌縺阪ｒ隱ｭ縺ｿ霎ｼ縺ｿ縺ｾ縺励◆".to_string();
                    return;
                }
                self.clear_recovery_draft(&note.id);
            }
        }
        self.status_line = format!("髢九＞縺溘Γ繝｢: {}", safe_title(&note.title));
    }

    pub(super) fn delete_selected_note(&mut self) {
        let Some(id) = self.selected_note_id.clone() else {
            return;
        };
        self.delete_note_by_id(&id);
    }

    pub(super) fn restore_selected_note(&mut self) {
        let Some(id) = self.selected_note_id.clone() else {
            return;
        };
        self.restore_note_by_id(&id);
    }

    pub(super) fn delete_note_by_id(&mut self, id: &str) {
        if is_clipboard_note_id(id) {
            self.clear_clipboard_history();
            return;
        }
        if is_image_clipboard_note_id(id) {
            self.clear_image_clipboard_history();
            return;
        }
        if let Some(store) = &mut self.store {
            match store.soft_delete_note(id) {
                Ok(_) => {
                    self.clear_recovery_draft(id);
                    self.clear_search_cache();
                    if self.selected_note_id.as_deref() == Some(id) {
                        self.selected_note_id = None;
                        self.selected_note_deleted = false;
                        self.editor_body.clear();
                        self.editor_tags.clear();
                    }
                    self.list_dirty = true;
                    self.delete_armed_for = None;
                    self.purge_armed_for = None;
                    self.status_line = "繝｡繝｢繧貞炎髯､縺励∪縺励◆".to_string();
                }
                Err(err) => self.status_line = format!("蜑企勁縺ｫ螟ｱ謨・ {err}"),
            }
        }
    }

    pub(super) fn restore_note_by_id(&mut self, id: &str) {
        let should_reload_selected = self.selected_note_id.as_deref() == Some(id);
        let result = if let Some(store) = &mut self.store {
            match store.restore_note(id) {
                Ok(()) => {
                    if should_reload_selected {
                        store.load_note(id).map(Some)
                    } else {
                        Ok(None)
                    }
                }
                Err(err) => Err(err),
            }
        } else {
            return;
        };

        match result {
            Ok(loaded_note) => {
                self.clear_search_cache();
                if let Some(note) = loaded_note {
                    self.select_note(note);
                }
                self.list_dirty = true;
                self.delete_armed_for = None;
                self.purge_armed_for = None;
                self.status_line = "繝｡繝｢繧貞ｾｩ蜈・＠縺ｾ縺励◆".to_string();
            }
            Err(err) => self.status_line = format!("蠕ｩ蜈・↓螟ｱ謨・ {err}"),
        }
    }

    pub(super) fn export_notes_from_gui(&mut self) {
        self.flush_pending_now();
        let Some(path) = self.pick_export_path() else {
            self.status_line = "繧ｨ繧ｯ繧ｹ繝昴・繝医ｒ繧ｭ繝｣繝ｳ繧ｻ繝ｫ縺励∪縺励◆".to_string();
            return;
        };
        self.export_path = path.display().to_string();
        let selected_id = self.selected_regular_note_id();
        if let Some(store) = &mut self.store {
            match store.export_to_path(&path, selected_id.as_deref()) {
                Ok(count) => {
                    self.status_line = format!(
                        "{count}莉ｶ繧偵お繧ｯ繧ｹ繝昴・繝医＠縺ｾ縺励◆: {}",
                        path.display()
                    );
                }
                Err(err) => {
                    self.status_line = format!("繧ｨ繧ｯ繧ｹ繝昴・繝亥､ｱ謨・ {err}");
                }
            }
        }
    }

    pub(super) fn import_notes_from_gui(&mut self) {
        self.flush_pending_now();
        let Some(path) = self.pick_import_path() else {
            self.status_line = "繧､繝ｳ繝昴・繝医ｒ繧ｭ繝｣繝ｳ繧ｻ繝ｫ縺励∪縺励◆".to_string();
            return;
        };
        self.import_path = path.display().to_string();
        let selected_id = self.selected_regular_note_id();
        let result = if let Some(store) = &mut self.store {
            match store.import_from_path(&path) {
                Ok(summary) => {
                    let loaded_note = if let Some(id) = &selected_id {
                        store.load_note(id).ok()
                    } else {
                        None
                    };
                    Ok((summary, loaded_note))
                }
                Err(err) => Err(err),
            }
        } else {
            return;
        };

        match result {
            Ok((summary, loaded_note)) => {
                self.clear_search_cache();
                self.list_dirty = true;
                self.delete_armed_for = None;
                self.purge_armed_for = None;

                if let Some(note) = loaded_note {
                    self.select_note(note);
                } else if selected_id.is_some() {
                    self.selected_note_id = None;
                    self.selected_note_deleted = false;
                    self.editor_body.clear();
                    self.editor_tags.clear();
                }
                self.status_line = format!(
                    "繧､繝ｳ繝昴・繝育ｵ先棡 菴懈・/譖ｴ譁ｰ/繧ｹ繧ｭ繝・・ = {}/{}/{}: {}",
                    summary.created,
                    summary.updated,
                    summary.skipped,
                    path.display()
                );
            }
            Err(err) => {
                self.status_line = format!("繧､繝ｳ繝昴・繝亥､ｱ謨・ {err}");
            }
        }
    }

    fn pick_export_path(&self) -> Option<PathBuf> {
        let mut dialog = FileDialog::new()
            .add_filter("JSON", &["json"])
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt", "text", "log", "rst", "adoc", "org"]);
        if let Some(parent) = PathBuf::from(&self.export_path).parent() {
            dialog = dialog.set_directory(parent);
        }
        if let Some(name) = PathBuf::from(&self.export_path)
            .file_name()
            .and_then(|s| s.to_str())
        {
            dialog = dialog.set_file_name(name);
        } else {
            dialog = dialog.set_file_name("ultra-memo-export.json");
        }
        dialog.save_file()
    }

    fn pick_import_path(&self) -> Option<PathBuf> {
        let mut dialog = FileDialog::new()
            .add_filter("JSON", &["json"])
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt", "text", "log", "rst", "adoc", "org"]);
        if let Some(parent) = PathBuf::from(&self.import_path).parent() {
            dialog = dialog.set_directory(parent);
        }
        dialog.pick_file()
    }

    pub(super) fn toggle_markdown_mode_for_note(&mut self, note_id: &str) {
        if is_clipboard_note_id(note_id) || is_image_clipboard_note_id(note_id) {
            self.status_line =
                "迚ｹ谿翫Γ繝｢縺ｧ縺ｯMarkdown謨ｴ蠖｢陦ｨ遉ｺ繧貞・繧頑崛縺医〒縺阪∪縺帙ｓ".to_string();
            return;
        }
        let same_selected = self.selected_note_id.as_deref() == Some(note_id);
        if !same_selected {
            self.select_note_by_id(note_id);
        }
        if self.selected_note_id.as_deref() != Some(note_id) {
            return;
        }
        if self.selected_note_deleted {
            self.status_line =
                "繧ｴ繝溽ｮｱ蜀・Γ繝｢縺ｯMarkdown謨ｴ蠖｢陦ｨ遉ｺ縺ｫ縺ｧ縺阪∪縺帙ｓ".to_string();
            return;
        }
        self.markdown_render_mode = if same_selected {
            !self.markdown_render_mode
        } else {
            true
        };
        self.status_line = if self.markdown_render_mode {
            "Markdown謨ｴ蠖｢陦ｨ遉ｺ繝｢繝ｼ繝峨↓縺励∪縺励◆".to_string()
        } else {
            "Markdown謨ｴ蠖｢陦ｨ遉ｺ繝｢繝ｼ繝峨ｒ隗｣髯､縺励∪縺励◆".to_string()
        };
    }

    pub(super) fn open_tag_editor_for_note(&mut self, note_id: &str) {
        if is_clipboard_note_id(note_id) || is_image_clipboard_note_id(note_id) {
            self.status_line = "迚ｹ谿翫Γ繝｢縺ｫ縺ｯ繧ｿ繧ｰ繧定ｨｭ螳壹〒縺阪∪縺帙ｓ".to_string();
            return;
        }
        if self.selected_note_id.as_deref() != Some(note_id) {
            self.select_note_by_id(note_id);
        }
        if self.selected_note_id.as_deref() != Some(note_id) {
            return;
        }
        if self.selected_note_deleted {
            self.status_line = "繧ｴ繝溽ｮｱ蜀・Γ繝｢縺ｫ縺ｯ繧ｿ繧ｰ繧定ｨｭ螳壹〒縺阪∪縺帙ｓ".to_string();
            return;
        }
        self.tag_editor_note_id = Some(note_id.to_string());
        self.tag_editor_input = self.editor_tags.clone();
        self.show_tag_editor = true;
    }

    pub(super) fn commit_tag_editor(&mut self) {
        let Some(id) = self.tag_editor_note_id.clone() else {
            self.close_tag_editor();
            return;
        };
        let tags = self.tag_editor_input.clone();
        self.apply_tags_to_note(&id, &tags);
        self.close_tag_editor();
    }

    pub(super) fn close_tag_editor(&mut self) {
        self.show_tag_editor = false;
        self.tag_editor_note_id = None;
        self.tag_editor_input.clear();
    }

    fn apply_tags_to_note(&mut self, id: &str, tags: &str) {
        let result = if let Some(store) = &mut self.store {
            match store.update_note_tags(id, tags) {
                Ok(()) => store.load_note(id),
                Err(err) => {
                    self.status_line = format!("タグ更新に失敗: {err}");
                    return;
                }
            }
        } else {
            return;
        };
        match result {
            Ok(note) => {
                self.clear_search_cache();
                self.select_note(note);
                self.editor_tags = tags.to_string();
                self.list_dirty = true;
                self.status_line = "タグを保存しました".to_string();
            }
            Err(err) => {
                self.status_line = format!("タグ更新後の再読込に失敗: {err}");
            }
        }
    }
    pub(super) fn apply_window_level(&self, ctx: &egui::Context) {
        let level = if self.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    pub(super) fn toggle_window_size_preset(&mut self, ctx: &egui::Context) {
        let current_width = ctx
            .input(|i| i.viewport().inner_rect.or(i.viewport().outer_rect))
            .map(|r| r.width())
            .unwrap_or(self.app_state.window.width as f32);
        let threshold = (WINDOW_SMALL_SIZE[0] + WINDOW_MEDIUM_SIZE[0]) * 0.5;
        let (target, label) = if current_width <= threshold {
            (WINDOW_MEDIUM_SIZE, "中")
        } else {
            (WINDOW_SMALL_SIZE, "小")
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            target[0], target[1],
        )));
        self.status_line = format!(
            "ウィンドウサイズを{}に切替 ({:.0}x{:.0})",
            label, target[0], target[1]
        );
    }

    pub(super) fn restore_last_note_if_needed(&mut self) {
        let Some(id) = self.pending_restore_note_id.take() else {
            return;
        };
        if is_clipboard_note_id(&id) {
            self.select_clipboard_note();
            self.status_line = "前回のクリップボード履歴メモを復元しました".to_string();
            return;
        }
        if is_image_clipboard_note_id(&id) {
            self.select_image_clipboard_note();
            self.status_line = "前回の画像クリップ履歴メモを復元しました".to_string();
            return;
        }
        if let Some(store) = &mut self.store {
            match store.load_note(&id) {
                Ok(note) => {
                    self.select_note(note);
                    self.status_line = "前回のメモを復元しました".to_string();
                }
                Err(err) => {
                    self.status_line = format!("前回メモの復元に失敗: {err}");
                    self.app_state.last_open_note_id = None;
                }
            }
        }
    }
}
