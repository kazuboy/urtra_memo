use std::time::Instant;

use super::clipboard::{is_clipboard_note_id, is_image_clipboard_note_id};
use super::search_worker::has_tag_query;
use super::types::{ListItem, ListItemKind};
use super::{MemoGuiApp, LIST_FETCH_LIMIT, SEARCH_FETCH_LIMIT, SEARCH_FETCH_LIMIT_SHORT_QUERY};

impl MemoGuiApp {
    pub(super) fn refresh_list_if_needed(&mut self) {
        let query = self.search_query.clone();
        let debounce_active = self
            .search_debounce_until
            .is_some_and(|due| Instant::now() < due);
        let is_search_mode = !self.show_trash && !query.trim().is_empty();
        let needs_refresh = self.list_dirty
            || (!debounce_active && self.list_last_query != query)
            || self.list_last_trash != self.show_trash
            || self.list_last_recent != self.show_recent
            || self.list_last_sort != self.list_sort;
        if !needs_refresh {
            return;
        }

        if is_search_mode {
            if has_tag_query(&query) {
                let start = Instant::now();
                let result = if let Some(store) = &self.store {
                    store.search_notes(&query, SEARCH_FETCH_LIMIT).map(|hits| {
                        hits.into_iter()
                            .map(|n| ListItem {
                                id: n.id,
                                title: n.title,
                                tags: n.tags,
                                updated_text: n.updated_at.format("%-m/%-d %H:%M").to_string(),
                                deleted: false,
                                kind: ListItemKind::Note,
                            })
                            .collect::<Vec<_>>()
                    })
                } else {
                    Ok(Vec::new())
                };
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                match result {
                    Ok(items) => {
                        self.remember_search_results(&query, &items);
                        self.list_items = self.with_pinned_special_items(items);
                        self.search_perf.push(elapsed_ms);
                    }
                    Err(err) => {
                        self.status_line = format!("タグ検索エラー: {err}");
                        self.list_items = self.with_pinned_special_items(Vec::new());
                    }
                }
                self.list_last_query = query;
                self.list_last_trash = self.show_trash;
                self.list_last_recent = self.show_recent;
                self.list_last_sort = self.list_sort;
                self.list_dirty = false;
                return;
            }

            if let Some(cached) = self.cached_search_results(&query) {
                self.list_items = self.with_pinned_special_items(cached);
                self.list_last_query = query;
                self.list_last_trash = self.show_trash;
                self.list_last_recent = self.show_recent;
                self.list_last_sort = self.list_sort;
                self.list_dirty = false;
                return;
            }

            if let Some(resp) = self.search_ready.take() {
                if resp.generation == self.search_generation {
                    if let Some(err) = resp.error {
                        self.status_line = format!("検索エラー: {err}");
                    } else if resp.query == query {
                        self.remember_search_results(&resp.query, &resp.items);
                        self.list_items = self.with_pinned_special_items(resp.items);
                        self.search_perf.push(resp.elapsed_ms);
                        self.list_last_query = query;
                        self.list_last_trash = self.show_trash;
                        self.list_last_recent = self.show_recent;
                        self.list_last_sort = self.list_sort;
                        self.list_dirty = false;
                        return;
                    } else {
                        self.remember_search_results(&resp.query, &resp.items);
                    }
                }
            }

            if !debounce_active {
                let fetch_limit = if query.chars().count() <= 1 {
                    SEARCH_FETCH_LIMIT_SHORT_QUERY
                } else {
                    SEARCH_FETCH_LIMIT
                };
                self.submit_async_search(&query, fetch_limit);
                self.list_last_query = query;
            }
            self.list_last_trash = self.show_trash;
            self.list_last_recent = self.show_recent;
            self.list_last_sort = self.list_sort;
            self.list_dirty = false;
            return;
        }

        let start = Instant::now();
        let result = if let Some(store) = &self.store {
            if self.show_trash {
                store.list_deleted_notes(LIST_FETCH_LIMIT).map(|notes| {
                    notes
                        .into_iter()
                        .map(|n| ListItem {
                            id: n.id,
                            title: n.title,
                            tags: n.tags,
                            updated_text: n.updated_at.format("%-m/%-d %H:%M").to_string(),
                            deleted: true,
                            kind: ListItemKind::Note,
                        })
                        .collect::<Vec<_>>()
                })
            } else if self.show_recent {
                store.list_recent(LIST_FETCH_LIMIT).map(|notes| {
                    notes
                        .into_iter()
                        .map(|n| ListItem {
                            id: n.id,
                            title: n.title,
                            tags: n.tags,
                            updated_text: n.updated_at.format("%-m/%-d %H:%M").to_string(),
                            deleted: false,
                            kind: ListItemKind::Note,
                        })
                        .collect::<Vec<_>>()
                })
            } else {
                store
                    .list_notes(self.list_sort, LIST_FETCH_LIMIT, false)
                    .map(|notes| {
                        notes
                            .into_iter()
                            .map(|n| ListItem {
                                id: n.id,
                                title: n.title,
                                tags: n.tags,
                                updated_text: n.updated_at.format("%-m/%-d %H:%M").to_string(),
                                deleted: false,
                                kind: ListItemKind::Note,
                            })
                            .collect::<Vec<_>>()
                    })
            }
        } else {
            Ok(Vec::new())
        };
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(items) => {
                self.list_items = self.with_pinned_special_items(items);
                self.list_perf.push(elapsed_ms);
                self.list_last_query = query;
                self.list_last_trash = self.show_trash;
                self.list_last_recent = self.show_recent;
                self.list_last_sort = self.list_sort;
                self.list_dirty = false;
            }
            Err(err) => {
                self.status_line = format!("一覧取得エラー: {err}");
                self.list_items = self.with_pinned_special_items(Vec::new());
                self.list_dirty = false;
            }
        }
    }

    pub(super) fn perf_summary_line(&self) -> String {
        format!(
            "起動 {:.2}ms | 一覧 平均/p95 {:.2}/{:.2}ms (n={}) | 検索 平均/p95 {:.2}/{:.2}ms (n={})",
            self.open_ms,
            self.list_perf.avg(),
            self.list_perf.p95(),
            self.list_perf.len(),
            self.search_perf.avg(),
            self.search_perf.p95(),
            self.search_perf.len()
        )
    }

    pub(super) fn handle_delete_action(&mut self) {
        let Some(selected_id) = self.selected_note_id.clone() else {
            self.status_line = "メモが選択されていません".to_string();
            return;
        };
        if is_clipboard_note_id(&selected_id) {
            self.clear_clipboard_history();
            return;
        }
        if is_image_clipboard_note_id(&selected_id) {
            self.clear_image_clipboard_history();
            return;
        }
        self.purge_armed_for = None;
        self.purge_all_trash_armed = false;
        if self.selected_note_deleted {
            self.restore_selected_note();
            self.list_dirty = true;
            return;
        }
        let armed = self.delete_armed_for.as_deref() == Some(selected_id.as_str());
        if armed {
            self.delete_selected_note();
            self.list_dirty = true;
        } else {
            self.delete_armed_for = Some(selected_id);
            self.status_line = "もう一度押すと削除します".to_string();
        }
    }

    pub(super) fn set_list_mode(&mut self, show_recent: bool, show_trash: bool) {
        if self.show_recent == show_recent && self.show_trash == show_trash {
            return;
        }
        self.show_recent = show_recent;
        self.show_trash = show_trash;
        self.search_debounce_until = None;
        self.selected_note_id = None;
        self.selected_note_deleted = false;
        self.editor_body.clear();
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        self.purge_all_trash_armed = false;
        self.list_dirty = true;
    }

    pub(super) fn handle_purge_action(&mut self) {
        let Some(selected_id) = self.selected_note_id.clone() else {
            self.status_line = "メモが選択されていません".to_string();
            return;
        };
        if !self.selected_note_deleted {
            self.status_line = "完全削除はゴミ箱内メモのみ実行できます".to_string();
            return;
        }
        self.purge_all_trash_armed = false;

        let armed = self.purge_armed_for.as_deref() == Some(selected_id.as_str());
        if !armed {
            self.purge_armed_for = Some(selected_id);
            self.status_line = "もう一度押すと完全削除します".to_string();
            return;
        }

        let id = selected_id;
        if let Some(store) = &mut self.store {
            match store.purge_note(&id) {
                Ok(_) => {
                    self.clear_recovery_draft(&id);
                    self.clear_search_cache();
                    self.selected_note_id = None;
                    self.selected_note_deleted = false;
                    self.editor_body.clear();
                    self.delete_armed_for = None;
                    self.purge_armed_for = None;
                    self.purge_all_trash_armed = false;
                    self.list_dirty = true;
                    self.status_line = "メモを完全削除しました".to_string();
                }
                Err(err) => {
                    self.status_line = format!("完全削除に失敗: {err}");
                }
            }
        }
    }

    pub(super) fn handle_purge_all_trash_action(&mut self) {
        if !self.show_trash {
            self.status_line = "ゴミ箱表示中のみ実行できます".to_string();
            return;
        }
        self.purge_armed_for = None;
        if !self.purge_all_trash_armed {
            self.purge_all_trash_armed = true;
            self.status_line = "もう一度押すとゴミ箱を空にします".to_string();
            return;
        }

        let result = if let Some(store) = &mut self.store {
            store.purge_deleted_notes()
        } else {
            return;
        };

        match result {
            Ok(count) => {
                self.clear_search_cache();
                self.selected_note_id = None;
                self.selected_note_deleted = false;
                self.editor_body.clear();
                self.editor_tags.clear();
                self.delete_armed_for = None;
                self.purge_armed_for = None;
                self.purge_all_trash_armed = false;
                self.list_dirty = true;
                self.status_line = format!("ゴミ箱を空にしました ({count}件)");
            }
            Err(err) => {
                self.status_line = format!("ゴミ箱の掃除に失敗: {err}");
            }
        }
    }
}
