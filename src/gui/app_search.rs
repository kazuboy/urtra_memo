use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Instant;

use super::search_worker::SearchWorkerRequest;
use super::types::{ListItem, ListItemKind};
use super::{MemoGuiApp, SEARCH_CACHE_CAP};
use crate::NoteStore;

impl MemoGuiApp {
    pub(super) fn clear_search_cache(&mut self) {
        self.search_cache.clear();
        self.search_cache_order.clear();
        self.search_generation = self.search_generation.saturating_add(1);
        self.search_inflight = None;
        self.search_ready = None;
    }

    pub(super) fn cached_search_results(&mut self, query: &str) -> Option<Vec<ListItem>> {
        let items = self.search_cache.get(query)?.clone();
        if let Some(pos) = self.search_cache_order.iter().position(|q| q == query) {
            self.search_cache_order.remove(pos);
        }
        self.search_cache_order.push_back(query.to_string());
        Some(items)
    }

    pub(super) fn remember_search_results(&mut self, query: &str, items: &[ListItem]) {
        if query.trim().is_empty() {
            return;
        }
        if !self.search_cache.contains_key(query) && self.search_cache.len() >= SEARCH_CACHE_CAP {
            if let Some(oldest) = self.search_cache_order.pop_front() {
                self.search_cache.remove(&oldest);
            }
        }
        if let Some(pos) = self.search_cache_order.iter().position(|q| q == query) {
            self.search_cache_order.remove(pos);
        }
        self.search_cache_order.push_back(query.to_string());
        self.search_cache.insert(query.to_string(), items.to_vec());
    }

    pub(super) fn poll_search_worker(&mut self) {
        let Some(rx) = &self.search_rx else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(resp) => {
                    if resp.generation != self.search_generation {
                        continue;
                    }
                    if self
                        .search_inflight
                        .as_ref()
                        .is_some_and(|(seq, _, generation)| {
                            *seq == resp.seq && *generation == resp.generation
                        })
                    {
                        self.search_inflight = None;
                    }
                    self.search_ready = Some(resp);
                    self.list_dirty = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.search_rx = None;
                    self.search_tx = None;
                    self.search_inflight = None;
                    break;
                }
            }
        }
    }

    pub(super) fn submit_async_search(&mut self, query: &str, limit: usize) {
        self.refresh_deleted_ids_for_search();
        let Some(tx) = self.search_tx.clone() else {
            return;
        };
        if self
            .search_inflight
            .as_ref()
            .is_some_and(|(_, q, generation)| q == query && *generation == self.search_generation)
        {
            return;
        }
        let seq = self.search_next_seq;
        self.search_next_seq = self.search_next_seq.saturating_add(1);
        let req = SearchWorkerRequest {
            seq,
            generation: self.search_generation,
            query: query.to_string(),
            limit,
            deleted_ids: self.search_deleted_ids.clone(),
        };
        if tx.send(req).is_ok() {
            self.search_inflight = Some((seq, query.to_string(), self.search_generation));
        } else {
            self.status_line = "検索ワーカーへの送信に失敗しました".to_string();
            self.search_tx = None;
            self.search_rx = None;
            self.search_inflight = None;
        }
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.list_items.is_empty() {
            return;
        }
        let current_index = self
            .selected_note_id
            .as_deref()
            .and_then(|id| self.list_items.iter().position(|item| item.id == id));
        let len = self.list_items.len() as isize;
        let base = current_index
            .map(|idx| idx as isize)
            .unwrap_or(if delta >= 0 { -1 } else { len });
        let target = (base + delta).clamp(0, len - 1) as usize;
        if let Some(item) = self.list_items.get(target).cloned() {
            match item.kind {
                ListItemKind::ClipboardTextHistory => self.select_clipboard_note(),
                ListItemKind::ClipboardImageHistory => self.select_image_clipboard_note(),
                ListItemKind::Note => {
                    self.selected_note_deleted = item.deleted;
                    self.select_note_by_id(&item.id);
                }
            }
        }
    }

    pub(super) fn refresh_deleted_ids_for_search(&mut self) {
        if self.search_deleted_ids_generation == self.search_generation {
            return;
        }
        self.search_deleted_ids = if let Some(store) = &self.store {
            match store.list_deleted_note_ids() {
                Ok(ids) => ids,
                Err(err) => {
                    self.status_line = format!("削除済みIDの取得に失敗: {err}");
                    self.search_deleted_ids.clone()
                }
            }
        } else {
            Vec::new()
        };
        self.search_deleted_ids_generation = self.search_generation;
    }

    pub(super) fn rebuild_search_index(&mut self) {
        if self.rebuild_index_rx.is_some() {
            self.status_line = "検索インデックス再構築を実行中です".to_string();
            return;
        }
        let Some(paths) = self.paths.clone() else {
            self.status_line = "再構築の開始に必要なパス情報がありません".to_string();
            return;
        };

        let (tx, rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
        self.rebuild_index_rx = Some(rx);
        self.rebuild_index_started_at = Some(Instant::now());
        self.status_line = "検索インデックス再構築を開始しました...".to_string();

        thread::spawn(move || {
            let result = NoteStore::open(paths)
                .and_then(|mut store| store.rebuild_index())
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }
}
