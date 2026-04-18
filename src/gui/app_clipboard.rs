use arboard::Clipboard;
use chrono::{Local, Utc};
use eframe::egui;
use png::{BitDepth, ColorType, Decoder};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::clipboard::{
    hash_clipboard_image, is_clipboard_note_id, is_image_clipboard_note_id, is_special_note_id,
    normalize_clipboard_text, parse_clipboard_history_body, render_clipboard_history_body,
    render_image_clipboard_history_body, save_clipboard_history, save_image_clipboard_history,
    write_rgba_png,
};
use super::types::{ClipboardHistoryEntry, ImageClipboardEntry, ListItem, ListItemKind};
use super::{
    MemoGuiApp, CLIPBOARD_HISTORY_MAX_ITEMS, CLIPBOARD_IMAGE_POLL_MS, CLIPBOARD_POLL_MS,
    IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS, IMAGE_THUMB_CACHE_CAP, IMAGE_THUMB_MAX_EDGE,
    SPECIAL_CLIPBOARD_NOTE_ID, SPECIAL_IMAGE_CLIPBOARD_NOTE_ID,
};

impl MemoGuiApp {
    pub(super) fn is_clipboard_note_selected(&self) -> bool {
        self.selected_note_id
            .as_deref()
            .is_some_and(is_clipboard_note_id)
    }

    pub(super) fn is_image_clipboard_note_selected(&self) -> bool {
        self.selected_note_id
            .as_deref()
            .is_some_and(is_image_clipboard_note_id)
    }

    pub(super) fn selected_regular_note_id(&self) -> Option<String> {
        self.selected_note_id
            .as_deref()
            .filter(|id| !is_special_note_id(id))
            .map(|id| id.to_string())
    }

    pub(super) fn clipboard_text_list_item(&self) -> ListItem {
        let updated_text = self
            .clipboard_history
            .first()
            .map(|item| {
                item.copied_at
                    .with_timezone(&Local)
                    .format("%-m/%-d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string());
        ListItem {
            id: SPECIAL_CLIPBOARD_NOTE_ID.to_string(),
            title: format!("クリップボード履歴 ({})", self.clipboard_history.len()),
            tags: Vec::new(),
            updated_text,
            deleted: false,
            kind: ListItemKind::ClipboardTextHistory,
        }
    }

    pub(super) fn clipboard_image_list_item(&self) -> ListItem {
        let updated_text = self
            .clipboard_image_history
            .first()
            .map(|item| {
                item.copied_at
                    .with_timezone(&Local)
                    .format("%-m/%-d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string());
        ListItem {
            id: SPECIAL_IMAGE_CLIPBOARD_NOTE_ID.to_string(),
            title: format!("画像クリップ履歴 ({})", self.clipboard_image_history.len()),
            tags: Vec::new(),
            updated_text,
            deleted: false,
            kind: ListItemKind::ClipboardImageHistory,
        }
    }

    pub(super) fn with_pinned_special_items(&self, mut items: Vec<ListItem>) -> Vec<ListItem> {
        if self.show_trash {
            return items;
        }
        items.retain(|item| !is_special_note_id(&item.id));
        items.insert(0, self.clipboard_image_list_item());
        items.insert(0, self.clipboard_text_list_item());
        items
    }

    pub(super) fn select_clipboard_note(&mut self) {
        self.flush_pending_now();
        self.selected_note_id = Some(SPECIAL_CLIPBOARD_NOTE_ID.to_string());
        self.selected_note_deleted = false;
        self.editor_tags.clear();
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        self.editor_body = render_clipboard_history_body(&self.clipboard_history);
        self.status_line = format!(
            "開いたメモ: クリップボード履歴 ({}件)",
            self.clipboard_history.len()
        );
    }

    pub(super) fn apply_clipboard_note_changes(&mut self) {
        let entries = parse_clipboard_history_body(&self.editor_body);
        self.clipboard_history = entries;
        self.clipboard_last_seen = self.clipboard_history.first().map(|item| item.text.clone());
        self.save_clipboard_history();
        self.list_dirty = true;
    }

    pub(super) fn select_image_clipboard_note(&mut self) {
        self.flush_pending_now();
        self.selected_note_id = Some(SPECIAL_IMAGE_CLIPBOARD_NOTE_ID.to_string());
        self.selected_note_deleted = false;
        self.editor_tags.clear();
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        self.editor_body = render_image_clipboard_history_body(
            &self.clipboard_image_history,
            &self.clipboard_image_dir,
        );
        self.status_line = format!(
            "開いたメモ: 画像クリップ履歴 ({}件)",
            self.clipboard_image_history.len()
        );
    }

    pub(super) fn image_thumbnail_for_entry(
        &mut self,
        ctx: &egui::Context,
        entry: &ImageClipboardEntry,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        if let Some(texture) = self.image_thumb_cache.get(&entry.id) {
            let [w, h] = texture.size();
            return Some((texture.id(), egui::vec2(w as f32, h as f32)));
        }

        let path = self.clipboard_image_dir.join(&entry.file_name);
        let (rgba, width, height) = load_png_rgba(&path)?;
        let (thumb_rgba, thumb_w, thumb_h) = downscale_rgba_nearest(
            &rgba,
            width,
            height,
            IMAGE_THUMB_MAX_EDGE,
            IMAGE_THUMB_MAX_EDGE,
        );
        let image =
            egui::ColorImage::from_rgba_unmultiplied([thumb_w, thumb_h], thumb_rgba.as_slice());
        let texture = ctx.load_texture(
            format!("clip-thumb-{}", entry.id),
            image,
            egui::TextureOptions::LINEAR,
        );

        self.image_thumb_cache.insert(entry.id.clone(), texture);
        self.image_thumb_order.push_back(entry.id.clone());
        while self.image_thumb_order.len() > IMAGE_THUMB_CACHE_CAP {
            if let Some(evicted) = self.image_thumb_order.pop_front() {
                self.image_thumb_cache.remove(&evicted);
            }
        }

        let texture = self.image_thumb_cache.get(&entry.id)?;
        let [w, h] = texture.size();
        Some((texture.id(), egui::vec2(w as f32, h as f32)))
    }

    pub(super) fn save_clipboard_history(&mut self) {
        if self.clipboard_history_path.as_os_str().is_empty() {
            return;
        }
        if let Err(err) =
            save_clipboard_history(&self.clipboard_history_path, &self.clipboard_history)
        {
            self.status_line = format!("クリップボード履歴の保存に失敗: {err}");
        }
    }

    pub(super) fn save_image_clipboard_history(&mut self) {
        if self.clipboard_image_history_path.as_os_str().is_empty() {
            return;
        }
        if let Err(err) = save_image_clipboard_history(
            &self.clipboard_image_history_path,
            &self.clipboard_image_history,
        ) {
            self.status_line = format!("画像クリップ履歴の保存に失敗: {err}");
        }
    }

    pub(super) fn clear_clipboard_history(&mut self) {
        self.clipboard_history.clear();
        self.save_clipboard_history();
        self.list_dirty = true;
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        if self.is_clipboard_note_selected() {
            self.editor_body = render_clipboard_history_body(&self.clipboard_history);
        }
        self.status_line = "クリップボード履歴を削除しました".to_string();
    }

    pub(super) fn clear_image_clipboard_history(&mut self) {
        let mut deleted = 0usize;
        for entry in &self.clipboard_image_history {
            let path = self.clipboard_image_dir.join(&entry.file_name);
            if path.exists() && fs::remove_file(&path).is_ok() {
                deleted += 1;
            }
        }
        self.clipboard_image_history.clear();
        self.clear_image_thumbnail_cache();
        self.save_image_clipboard_history();
        self.list_dirty = true;
        self.delete_armed_for = None;
        self.purge_armed_for = None;
        if self.is_image_clipboard_note_selected() {
            self.editor_body = render_image_clipboard_history_body(
                &self.clipboard_image_history,
                &self.clipboard_image_dir,
            );
        }
        self.status_line = format!("画像クリップ履歴を削除しました ({deleted}件)");
    }

    pub(super) fn keep_image_clipboard_entry(&mut self, id: &str) {
        let before = self.clipboard_image_history.len();
        self.clipboard_image_history.retain(|entry| entry.id != id);
        if self.clipboard_image_history.len() == before {
            return;
        }
        self.save_image_clipboard_history();
        self.remove_image_thumbnail(id);
        self.list_dirty = true;
        if self.is_image_clipboard_note_selected() {
            self.editor_body = render_image_clipboard_history_body(
                &self.clipboard_image_history,
                &self.clipboard_image_dir,
            );
        }
        self.status_line = "画像を履歴から外しました（ファイルは保持）".to_string();
    }

    pub(super) fn poll_clipboard_history(&mut self) {
        if self.clipboard.is_none() {
            self.clipboard = Clipboard::new().ok();
        }
        self.poll_text_clipboard_history();
        self.poll_image_clipboard_history();
    }

    fn poll_text_clipboard_history(&mut self) {
        if self.clipboard_last_poll_at.elapsed() < Duration::from_millis(CLIPBOARD_POLL_MS) {
            return;
        }
        self.clipboard_last_poll_at = Instant::now();
        let Some(clipboard) = &mut self.clipboard else {
            return;
        };
        let Ok(text) = clipboard.get_text() else {
            return;
        };
        let Some(normalized) = normalize_clipboard_text(&text) else {
            return;
        };
        if self
            .clipboard_last_seen
            .as_deref()
            .is_some_and(|prev| prev == normalized)
        {
            return;
        }
        self.clipboard_last_seen = Some(normalized.clone());
        self.clipboard_history.insert(
            0,
            ClipboardHistoryEntry {
                copied_at: Utc::now(),
                text: normalized,
            },
        );
        if self.clipboard_history.len() > CLIPBOARD_HISTORY_MAX_ITEMS {
            self.clipboard_history.truncate(CLIPBOARD_HISTORY_MAX_ITEMS);
        }
        self.save_clipboard_history();
        self.list_dirty = true;
        if self.is_clipboard_note_selected() {
            self.editor_body = render_clipboard_history_body(&self.clipboard_history);
        }
    }

    fn poll_image_clipboard_history(&mut self) {
        if self.clipboard_image_last_poll_at.elapsed()
            < Duration::from_millis(CLIPBOARD_IMAGE_POLL_MS)
        {
            return;
        }
        self.clipboard_image_last_poll_at = Instant::now();
        let Some(clipboard) = &mut self.clipboard else {
            return;
        };
        let Ok(image) = clipboard.get_image() else {
            return;
        };
        let hash = hash_clipboard_image(image.width, image.height, image.bytes.as_ref());
        if self
            .clipboard_image_last_seen_hash
            .is_some_and(|prev| prev == hash)
        {
            return;
        }

        let now = Utc::now();
        let id = format!("img-{}-{hash:016x}", now.timestamp_millis());
        let file_name = format!("{id}.png");
        let path = self.clipboard_image_dir.join(&file_name);
        if let Err(err) = write_rgba_png(
            &path,
            image.width as u32,
            image.height as u32,
            image.bytes.as_ref(),
        ) {
            self.status_line = format!("画像クリップ保存に失敗: {err}");
            return;
        }

        self.clipboard_image_last_seen_hash = Some(hash);
        self.clipboard_image_history.insert(
            0,
            ImageClipboardEntry {
                id,
                file_name,
                copied_at: now,
                width: image.width as u32,
                height: image.height as u32,
                byte_size: image.bytes.len() as u64,
                hash,
            },
        );
        if self.clipboard_image_history.len() > IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS {
            let removed: Vec<ImageClipboardEntry> = self
                .clipboard_image_history
                .drain(IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS..)
                .collect();
            for entry in removed {
                self.remove_image_thumbnail(&entry.id);
                let stale_path = self.clipboard_image_dir.join(entry.file_name);
                let _ = fs::remove_file(stale_path);
            }
        }

        self.save_image_clipboard_history();
        self.list_dirty = true;
        if self.is_image_clipboard_note_selected() {
            self.editor_body = render_image_clipboard_history_body(
                &self.clipboard_image_history,
                &self.clipboard_image_dir,
            );
        }
    }

    fn clear_image_thumbnail_cache(&mut self) {
        self.image_thumb_cache.clear();
        self.image_thumb_order.clear();
    }

    fn remove_image_thumbnail(&mut self, id: &str) {
        self.image_thumb_cache.remove(id);
        self.image_thumb_order.retain(|cached_id| cached_id != id);
    }
}

fn load_png_rgba(path: &Path) -> Option<(Vec<u8>, usize, usize)> {
    let file = fs::File::open(path).ok()?;
    let decoder = Decoder::new(file);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != BitDepth::Eight {
        return None;
    }
    let width = info.width as usize;
    let height = info.height as usize;
    let src = &buf[..info.buffer_size()];

    let rgba = match info.color_type {
        ColorType::Rgba => src.to_vec(),
        ColorType::Rgb => {
            let mut out = Vec::with_capacity(width * height * 4);
            for rgb in src.chunks_exact(3) {
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            out
        }
        ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(width * height * 4);
            for ga in src.chunks_exact(2) {
                out.extend_from_slice(&[ga[0], ga[0], ga[0], ga[1]]);
            }
            out
        }
        ColorType::Grayscale => {
            let mut out = Vec::with_capacity(width * height * 4);
            for g in src {
                out.extend_from_slice(&[*g, *g, *g, 255]);
            }
            out
        }
        ColorType::Indexed => return None,
    };
    Some((rgba, width, height))
}

fn downscale_rgba_nearest(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    max_w: usize,
    max_h: usize,
) -> (Vec<u8>, usize, usize) {
    if src_w == 0 || src_h == 0 {
        return (Vec::new(), 1, 1);
    }
    let width_ratio = max_w as f32 / src_w as f32;
    let height_ratio = max_h as f32 / src_h as f32;
    let scale = width_ratio.min(height_ratio).min(1.0);
    let dst_w = ((src_w as f32 * scale).round() as usize).max(1);
    let dst_h = ((src_h as f32 * scale).round() as usize).max(1);
    if dst_w == src_w && dst_h == src_h {
        return (src.to_vec(), src_w, src_h);
    }

    let mut out = vec![0u8; dst_w * dst_h * 4];
    for y in 0..dst_h {
        let sy = y * src_h / dst_h;
        for x in 0..dst_w {
            let sx = x * src_w / dst_w;
            let src_i = (sy * src_w + sx) * 4;
            let dst_i = (y * dst_w + x) * 4;
            out[dst_i..dst_i + 4].copy_from_slice(&src[src_i..src_i + 4]);
        }
    }
    (out, dst_w, dst_h)
}
