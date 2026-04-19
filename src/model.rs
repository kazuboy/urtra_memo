use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const TITLE_MAX_CHARS: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Full note entity persisted by the store.
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted: bool,
}

impl Note {
    /// Returns `title` or a fallback when title is empty.
    pub fn effective_title(&self) -> String {
        safe_title(&self.title).to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Lightweight note representation for lists/search.
pub struct NoteDigest {
    pub id: String,
    pub title: String,
    pub updated_at: DateTime<Utc>,
    pub snippet: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Summary item used in note lists.
pub type NoteSummary = NoteDigest;
/// Hit item used in search results.
pub type SearchResult = NoteDigest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
/// Sort direction for note list queries.
pub enum SortOrder {
    #[default]
    UpdatedDesc,
    CreatedDesc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Saved window geometry.
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 800,
            x: 100,
            y: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
/// Persisted GUI state restored on launch.
pub struct AppState {
    pub last_open_note_id: Option<String>,
    pub window: WindowState,
    pub last_query: Option<String>,
    pub ui_zoom_pct: u16,
    pub show_perf_line: bool,
    pub show_recent: bool,
    pub show_trash: bool,
    pub list_sort: SortOrder,
    pub markdown_render_mode: bool,
    pub ui_font_family: String,
    pub ui_text_color_rgb: [u8; 3],
    pub ui_background_color_rgb: [u8; 3],
    pub focus_mode: bool,
    pub always_on_top: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            last_open_note_id: None,
            window: WindowState::default(),
            last_query: None,
            ui_zoom_pct: 100,
            show_perf_line: true,
            show_recent: true,
            show_trash: false,
            list_sort: SortOrder::UpdatedDesc,
            markdown_render_mode: false,
            ui_font_family: "yu-gothic-ui".to_string(),
            ui_text_color_rgb: [48, 48, 48],
            ui_background_color_rgb: [245, 245, 246],
            focus_mode: false,
            always_on_top: false,
        }
    }
}

/// Derives a title from the first non-empty line of note body.
pub fn derive_title(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let without_heading = trimmed.trim_start_matches('#').trim();
        if !without_heading.is_empty() {
            return truncate_chars(without_heading, TITLE_MAX_CHARS);
        }
    }
    String::new()
}

/// Returns fallback title when `title` is empty.
pub fn safe_title(title: &str) -> &str {
    if title.is_empty() {
        "(untitled)"
    } else {
        title
    }
}

/// Truncates by character count and appends `...` when needed.
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    if max_chars <= 3 {
        return text.chars().take(max_chars).collect();
    }
    format!(
        "{}...",
        text.chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>()
    )
}

/// Normalizes one tag token for stable matching and persistence.
pub fn normalize_tag_token(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| c == '#' || c == '|' || c == ',' || c == ';')
        .to_lowercase()
}

/// Normalizes raw tag input into unique, lowercase tags.
pub fn normalize_tags(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in raw
        .replace(',', " ")
        .split_whitespace()
        .map(|t| t.trim_start_matches('#'))
    {
        let normalized = normalize_tag_token(token);
        if !normalized.is_empty() && !out.iter().any(|v| v == &normalized) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{derive_title, normalize_tag_token, normalize_tags, truncate_chars};

    #[test]
    fn derive_title_skips_empty_and_markdown_heading() {
        let body = "\n\n#  見出しタイトル  \n本文";
        assert_eq!(derive_title(body), "見出しタイトル");
    }

    #[test]
    fn derive_title_truncates_to_title_limit() {
        let long = "# 1234567890123456789012345678901234567890XYZ";
        assert_eq!(
            derive_title(long),
            "1234567890123456789012345678901234567..."
        );
    }

    #[test]
    fn truncate_chars_handles_multibyte_text() {
        assert_eq!(truncate_chars("あいうえお", 4), "あ...");
        assert_eq!(truncate_chars("abc", 10), "abc");
        assert_eq!(truncate_chars("abcdef", 0), "");
    }

    #[test]
    fn normalize_tags_removes_duplicates_and_delimiters() {
        let tags = normalize_tags(" #Work, rust ;work |idea|  ");
        assert_eq!(
            tags,
            vec!["work".to_string(), "rust".to_string(), "idea".to_string()]
        );
    }

    #[test]
    fn normalize_tag_token_trims_special_chars() {
        assert_eq!(normalize_tag_token(" #Rust; "), "rust");
    }
}
