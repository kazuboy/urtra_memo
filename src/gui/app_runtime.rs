use eframe::egui;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use crate::WindowState;

use super::{MemoGuiApp, CLIPBOARD_POLL_MS};

impl MemoGuiApp {
    pub(super) fn rebuild_progress_text(&self) -> Option<String> {
        self.rebuild_index_started_at.map(|started| {
            let secs = started.elapsed().as_secs_f64();
            format!("検索インデックス再構築中... {:.1}s", secs)
        })
    }

    pub(super) fn poll_rebuild_search_index(&mut self) {
        let Some(rx) = &self.rebuild_index_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                let elapsed_ms = self
                    .rebuild_index_started_at
                    .map(|t| t.elapsed().as_secs_f64() * 1000.0)
                    .unwrap_or(0.0);
                self.rebuild_index_rx = None;
                self.rebuild_index_started_at = None;
                match result {
                    Ok(()) => {
                        self.clear_search_cache();
                        self.list_dirty = true;
                        self.status_line =
                            format!("検索インデックスを再構築しました ({elapsed_ms:.0}ms)");
                    }
                    Err(err) => {
                        self.status_line =
                            format!("検索インデックス再構築に失敗 ({elapsed_ms:.0}ms): {err}");
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.rebuild_index_rx = None;
                self.rebuild_index_started_at = None;
                self.status_line = "検索インデックス再構築ジョブが切断されました".to_string();
            }
        }
    }

    pub(super) fn schedule_repaint_if_needed(&self, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(CLIPBOARD_POLL_MS));
        if self.autosave.has_pending()
            || self.search_inflight.is_some()
            || self.rebuild_index_rx.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if let Some(due) = self.search_debounce_until {
            if Instant::now() < due {
                ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
        }
    }

    pub(super) fn process_shortcuts(&mut self, ctx: &egui::Context) {
        let keyboard_captured = ctx.wants_keyboard_input();
        let (
            new_note_shortcut,
            focus_search_shortcut,
            today_note_shortcut,
            close_shortcut,
            delete_shortcut,
            purge_shortcut,
            list_up_shortcut,
            list_down_shortcut,
            list_up_plain_shortcut,
            list_down_plain_shortcut,
            menu_shortcut,
        ) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let plain_arrow = i.modifiers.is_none() && !keyboard_captured;
            (
                cmd && i.key_pressed(egui::Key::N),
                cmd && i.key_pressed(egui::Key::F),
                cmd && i.key_pressed(egui::Key::D),
                cmd && i.key_pressed(egui::Key::W),
                (i.modifiers.is_none() && i.key_pressed(egui::Key::Delete))
                    || (cmd && i.key_pressed(egui::Key::Backspace)),
                i.modifiers.shift && i.key_pressed(egui::Key::Delete),
                cmd && i.key_pressed(egui::Key::ArrowUp),
                cmd && i.key_pressed(egui::Key::ArrowDown),
                plain_arrow && i.key_pressed(egui::Key::ArrowUp),
                plain_arrow && i.key_pressed(egui::Key::ArrowDown),
                cmd && i.key_pressed(egui::Key::Comma),
            )
        });

        if new_note_shortcut {
            self.create_note(String::new());
            self.status_line = "新規メモを作成".to_string();
        }
        if today_note_shortcut {
            self.create_today_note();
            self.status_line = "今日のメモを作成".to_string();
        }
        if focus_search_shortcut {
            self.focus_search_requested = true;
            self.status_line = "検索欄へフォーカス".to_string();
        }
        if delete_shortcut {
            self.handle_delete_action();
        }
        if purge_shortcut {
            self.handle_purge_action();
        }
        if list_up_shortcut || list_up_plain_shortcut {
            self.move_selection(-1);
        }
        if list_down_shortcut || list_down_plain_shortcut {
            self.move_selection(1);
        }
        if menu_shortcut {
            self.show_menu = true;
        }
        if ctx.input(|i| i.pointer.secondary_clicked()) {
            self.show_menu = true;
        }
        if close_shortcut {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn sync_window_state(&mut self, ctx: &egui::Context) {
        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
            self.app_state.window = WindowState {
                width: rect.width().max(1.0) as u32,
                height: rect.height().max(1.0) as u32,
                x: rect.min.x.round() as i32,
                y: rect.min.y.round() as i32,
            };
        } else if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            self.app_state.window = WindowState {
                width: rect.width().max(1.0) as u32,
                height: rect.height().max(1.0) as u32,
                x: self.app_state.window.x,
                y: self.app_state.window.y,
            };
        }
    }
}
