use crate::model::truncate_chars;
use anyhow::Result;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use png::{BitDepth, ColorType, Encoder};
use std::fs;
use std::io::BufWriter;
use std::path::Path;

use super::types::{
    ClipboardHistoryEntry, ClipboardHistoryFile, ImageClipboardEntry, ImageClipboardHistoryFile,
};
use super::{
    CLIPBOARD_HISTORY_MAX_ITEMS, CLIPBOARD_ITEM_MAX_CHARS, IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS,
    SPECIAL_CLIPBOARD_NOTE_ID, SPECIAL_IMAGE_CLIPBOARD_NOTE_ID,
};

pub(crate) fn is_clipboard_note_id(id: &str) -> bool {
    id == SPECIAL_CLIPBOARD_NOTE_ID
}

pub(crate) fn is_image_clipboard_note_id(id: &str) -> bool {
    id == SPECIAL_IMAGE_CLIPBOARD_NOTE_ID
}

pub(crate) fn is_special_note_id(id: &str) -> bool {
    is_clipboard_note_id(id) || is_image_clipboard_note_id(id)
}

pub(crate) fn normalize_clipboard_text(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, CLIPBOARD_ITEM_MAX_CHARS))
}

pub(crate) fn render_clipboard_history_body(entries: &[ClipboardHistoryEntry]) -> String {
    let mut out = String::new();
    out.push_str("# クリップボード履歴\n\n");
    if entries.is_empty() {
        out.push_str("（履歴はまだありません）\n");
        return out;
    }

    for (idx, item) in entries.iter().enumerate() {
        let at = item
            .copied_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S");
        out.push_str(&format!("## {}. {at}\n", idx + 1));
        out.push_str(&item.text);
        out.push_str("\n\n");
    }
    out
}

pub(crate) fn parse_clipboard_history_body(body: &str) -> Vec<ClipboardHistoryEntry> {
    let normalized = body.replace("\r\n", "\n");
    let now = Utc::now();
    let mut entries: Vec<ClipboardHistoryEntry> = Vec::new();
    let mut heading_mode = false;
    let mut current_time = now;
    let mut current_lines: Vec<String> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();

    for raw_line in normalized.lines() {
        let line = raw_line.trim_end();
        if line.starts_with("# クリップボード履歴") {
            continue;
        }
        if let Some(parsed_at) = parse_clipboard_section_time(line) {
            heading_mode = true;
            if !current_lines.is_empty() {
                let text = current_lines.join("\n");
                if let Some(normalized_text) = normalize_clipboard_text(&text) {
                    entries.push(ClipboardHistoryEntry {
                        copied_at: current_time,
                        text: normalized_text,
                    });
                }
                current_lines.clear();
            }
            current_time = parsed_at;
            continue;
        }

        if heading_mode {
            current_lines.push(line.to_string());
        } else {
            plain_lines.push(line.to_string());
        }
    }

    if heading_mode {
        if !current_lines.is_empty() {
            let text = current_lines.join("\n");
            if let Some(normalized_text) = normalize_clipboard_text(&text) {
                entries.push(ClipboardHistoryEntry {
                    copied_at: current_time,
                    text: normalized_text,
                });
            }
        }
    } else {
        let text = plain_lines.join("\n");
        if let Some(normalized_text) = normalize_clipboard_text(&text) {
            entries.push(ClipboardHistoryEntry {
                copied_at: now,
                text: normalized_text,
            });
        }
    }

    if entries.len() > CLIPBOARD_HISTORY_MAX_ITEMS {
        entries.truncate(CLIPBOARD_HISTORY_MAX_ITEMS);
    }
    entries
}

fn parse_clipboard_section_time(line: &str) -> Option<DateTime<Utc>> {
    let heading = line.strip_prefix("## ")?;
    let ts_text = heading
        .split_once(". ")
        .map_or(heading, |(_, ts)| ts)
        .trim();
    let naive = NaiveDateTime::parse_from_str(ts_text, "%Y-%m-%d %H:%M:%S").ok()?;
    let local_dt = Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())?;
    Some(local_dt.with_timezone(&Utc))
}

pub(crate) fn render_image_clipboard_history_body(
    entries: &[ImageClipboardEntry],
    image_dir: &Path,
) -> String {
    let mut out = String::new();
    out.push_str("# 画像クリップ履歴\n\n");
    out.push_str("- 一括削除: この履歴内の画像ファイルを削除\n");
    out.push_str("- 保持して履歴から外す: ファイルは残して管理対象から除外\n\n");
    if entries.is_empty() {
        out.push_str("（画像履歴はまだありません）\n");
        return out;
    }
    for (idx, item) in entries.iter().enumerate() {
        let at = item
            .copied_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S");
        let path = image_dir.join(&item.file_name);
        out.push_str(&format!(
            "{}. {at} / {}x{} / {} KB\n{}\n\n",
            idx + 1,
            item.width,
            item.height,
            (item.byte_size / 1024).max(1),
            path.display()
        ));
    }
    out
}

pub(crate) fn load_clipboard_history(path: &Path) -> Vec<ClipboardHistoryEntry> {
    if !path.exists() {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<ClipboardHistoryFile>(&text) else {
        return Vec::new();
    };
    let mut entries = file.entries;
    for item in &mut entries {
        if let Some(normalized) = normalize_clipboard_text(&item.text) {
            item.text = normalized;
        } else {
            item.text.clear();
        }
    }
    entries.retain(|item| !item.text.is_empty());
    if entries.len() > CLIPBOARD_HISTORY_MAX_ITEMS {
        entries.truncate(CLIPBOARD_HISTORY_MAX_ITEMS);
    }
    entries
}

pub(crate) fn save_clipboard_history(path: &Path, entries: &[ClipboardHistoryEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = ClipboardHistoryFile {
        version: 1,
        entries: entries.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub(crate) fn load_image_clipboard_history(path: &Path) -> Vec<ImageClipboardEntry> {
    if !path.exists() {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<ImageClipboardHistoryFile>(&text) else {
        return Vec::new();
    };
    let mut entries = file.entries;
    entries.retain(|entry| !entry.id.is_empty() && !entry.file_name.is_empty());
    if entries.len() > IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS {
        entries.truncate(IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS);
    }
    entries
}

pub(crate) fn save_image_clipboard_history(
    path: &Path,
    entries: &[ImageClipboardEntry],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = ImageClipboardHistoryFile {
        version: 1,
        entries: entries.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub(crate) fn hash_clipboard_image(width: usize, height: usize, bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET_BASIS;

    for b in (width as u64).to_le_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for b in (height as u64).to_le_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    let mut encoder = Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_clipboard_history_body;

    #[test]
    fn parse_clipboard_history_body_with_sections() {
        let body = "# クリップボード履歴\n\n## 1. 2026-03-26 09:10:11\nfoo\n\n## 2. 2026-03-26 09:11:12\nbar\n";
        let entries = parse_clipboard_history_body(body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "foo");
        assert_eq!(entries[1].text, "bar");
    }

    #[test]
    fn parse_clipboard_history_body_as_plain_text() {
        let entries = parse_clipboard_history_body("just one block");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "just one block");
    }
}
