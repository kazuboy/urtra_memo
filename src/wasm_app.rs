use crate::model::{derive_title, safe_title, truncate_chars};
use chrono::{TimeZone, Utc};
use eframe::egui;
use eframe::egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

const LEGACY_STORAGE_KEY: &str = "ultra_memo.web.state.v1";
const MAIN_FOLDER_ID: &str = "main";
const MAIN_FOLDER_NAME: &str = "Main";
const TITLE_MAX_PREVIEW_CHARS: usize = 28;
const SNIPPET_MAX_PREVIEW_CHARS: usize = 80;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(700);
const LIST_PANEL_MIN_WIDTH: f32 = 220.0;
const LIST_PANEL_DEFAULT_WIDTH: f32 = 300.0;
const LIST_PANEL_MAX_WIDTH: f32 = 520.0;
const UI_ZOOM_MIN: f32 = 0.85;
const UI_ZOOM_MAX: f32 = 1.35;
const DEFAULT_TEXT_COLOR_RGB: [u8; 3] = [28, 28, 30];
const DEFAULT_BG_COLOR_RGB: [u8; 3] = [245, 245, 247];
const DEFAULT_ACCENT_COLOR_RGB: [u8; 3] = [88, 86, 214];
const FONT_PRESET_DEFAULT: &str = "default";
const FONT_PRESET_SERIF: &str = "serif";
const FONT_PRESET_MONO: &str = "mono";
const TEXT_COLOR_PRESETS: [(&str, [u8; 3]); 5] = [
    ("Ink", [28, 28, 30]),
    ("Slate", [55, 64, 81]),
    ("Wine", [121, 34, 57]),
    ("Brown", [109, 70, 41]),
    ("Blue", [17, 62, 128]),
];
const BG_COLOR_PRESETS: [(&str, [u8; 3]); 5] = [
    ("Paper", [252, 252, 252]),
    ("Mist", [245, 245, 247]),
    ("Warm", [247, 243, 234]),
    ("Sky", [237, 244, 250]),
    ("Graphite", [234, 236, 240]),
];
const ACCENT_COLOR_PRESETS: [(&str, [u8; 3]); 5] = [
    ("Orange", [255, 149, 0]),
    ("Indigo", [88, 86, 214]),
    ("Teal", [0, 148, 136]),
    ("Green", [52, 199, 89]),
    ("Pink", [255, 45, 85]),
];
const PAPER_TEXT_RGB: [u8; 3] = [244, 244, 246];
const NIGHT_MINT_ACCENT_RGB: [u8; 3] = [82, 196, 170];
const SUPPORTED_IMPORT_EXTENSIONS: &[&str] = &[
    "json", "md", "markdown", "txt", "text", "log", "rst", "adoc", "org",
];

static NOTE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static FOLDER_SEQUENCE: AtomicU64 = AtomicU64::new(1);
thread_local! {
    static IMPORT_RESULT: RefCell<Option<Result<ImportPayload, String>>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct WebState {
    notes: Vec<WebNote>,
    folders: Vec<WebFolder>,
    recent_note_ids: Vec<String>,
    selected_note_id: Option<String>,
    selected_folder_id: String,
    search_query: String,
    show_recent: bool,
    show_trash: bool,
    list_sort: WebListSort,
    markdown_render_mode: bool,
    focus_mode: bool,
    list_panel_width: f32,
    ui_zoom: f32,
    ui_font_preset: String,
    ui_text_color_rgb: [u8; 3],
    ui_background_color_rgb: [u8; 3],
    ui_accent_color_rgb: [u8; 3],
}

impl Default for WebState {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            folders: vec![main_folder()],
            recent_note_ids: Vec::new(),
            selected_note_id: None,
            selected_folder_id: MAIN_FOLDER_ID.to_string(),
            search_query: String::new(),
            show_recent: false,
            show_trash: false,
            list_sort: WebListSort::UpdatedDesc,
            markdown_render_mode: false,
            focus_mode: false,
            list_panel_width: LIST_PANEL_DEFAULT_WIDTH,
            ui_zoom: 1.0,
            ui_font_preset: FONT_PRESET_DEFAULT.to_string(),
            ui_text_color_rgb: DEFAULT_TEXT_COLOR_RGB,
            ui_background_color_rgb: DEFAULT_BG_COLOR_RGB,
            ui_accent_color_rgb: DEFAULT_ACCENT_COLOR_RGB,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebNote {
    id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    #[serde(default)]
    deleted: bool,
    #[serde(default = "default_main_folder_id")]
    folder_id: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebFolder {
    id: String,
    name: String,
    created_at_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
enum WebListSort {
    #[default]
    UpdatedDesc,
    CreatedDesc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppearancePreset {
    Classic,
    WarmJournal,
    QuietModern,
    NightNotes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebExportBundle {
    version: u32,
    exported_at_ms: i64,
    folders: Vec<WebFolder>,
    notes: Vec<WebNote>,
}

#[derive(Debug, Clone)]
struct ImportPayload {
    file_name: String,
    content: String,
}

pub fn run_web() {
    wasm_bindgen_futures::spawn_local(async move {
        set_boot_status("Ultra Memo Web: initializing...");
        let Some(window) = web_sys::window() else {
            web_sys::console::error_1(&"window is not available".into());
            set_boot_status("Failed: window is not available");
            return;
        };
        let Some(document) = window.document() else {
            web_sys::console::error_1(&"document is not available".into());
            set_boot_status("Failed: document is not available");
            return;
        };
        let Some(canvas_element) = document.get_element_by_id("the_canvas_id") else {
            web_sys::console::error_1(&"missing canvas element: #the_canvas_id".into());
            set_boot_status("Failed: missing #the_canvas_id");
            return;
        };
        let Ok(canvas) = canvas_element.dyn_into::<web_sys::HtmlCanvasElement>() else {
            web_sys::console::error_1(&"canvas element type mismatch".into());
            set_boot_status("Failed: canvas type mismatch");
            return;
        };

        let web_options = eframe::WebOptions::default();
        set_boot_status("Ultra Memo Web: starting engine...");
        let start = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|_cc| Ok(Box::new(WebMemoApp::new()))),
            )
            .await;

        match start {
            Ok(()) => set_boot_status(""),
            Err(err) => {
                web_sys::console::error_1(&format!("failed to start web app: {err:?}").into());
                set_boot_status(&format!("Failed to start app: {err:?}"));
            }
        }
    });
}

struct WebMemoApp {
    state: WebState,
    editor_body: String,
    editor_tags: String,
    new_folder_name: String,
    status_line: String,
    dirty_since: Option<Instant>,
    boot_status_cleared: bool,
    show_folder_manager: bool,
    show_menu: bool,
}

impl WebMemoApp {
    fn new() -> Self {
        let mut state = load_state();
        ensure_state_integrity(&mut state);
        if state.list_panel_width < LIST_PANEL_MIN_WIDTH || state.list_panel_width.is_nan() {
            state.list_panel_width = LIST_PANEL_DEFAULT_WIDTH;
        }
        state.list_panel_width = state
            .list_panel_width
            .clamp(LIST_PANEL_MIN_WIDTH, LIST_PANEL_MAX_WIDTH);
        sort_notes_by_updated_desc(&mut state.notes);

        if state.selected_note_id.is_none() && !state.notes.is_empty() {
            state.selected_note_id = state.notes.first().map(|note| note.id.clone());
        } else if let Some(selected_id) = state.selected_note_id.clone() {
            if !state.notes.iter().any(|note| note.id == selected_id) {
                state.selected_note_id = state.notes.first().map(|note| note.id.clone());
            }
        }
        if state.notes.is_empty() {
            let note = make_empty_note_in_folder(&state.selected_folder_id);
            state.selected_note_id = Some(note.id.clone());
            state.notes.push(note);
            save_state(&state);
        }

        let (editor_body, editor_tags) = if let Some(note) = selected_note(&state) {
            (note.body.clone(), note.tags.join(" "))
        } else {
            (String::new(), String::new())
        };

        let mut app = Self {
            state,
            editor_body,
            editor_tags,
            new_folder_name: String::new(),
            status_line: "ready".to_string(),
            dirty_since: None,
            boot_status_cleared: false,
            show_folder_manager: false,
            show_menu: false,
        };
        app.sync_selection_for_current_scope();
        app
    }

    fn create_note(&mut self) {
        self.flush_editor_now();
        self.state.show_recent = false;
        self.state.show_trash = false;
        let note = make_empty_note_in_folder(&self.state.selected_folder_id);
        let id = note.id.clone();
        self.state.notes.insert(0, note);
        self.state.selected_note_id = Some(id);
        self.editor_body.clear();
        self.editor_tags.clear();
        self.status_line = "new note".to_string();
        save_state(&self.state);
    }

    fn delete_selected_note(&mut self) {
        let Some(id) = self.state.selected_note_id.clone() else {
            return;
        };
        let Some(index) = self.state.notes.iter().position(|note| note.id == id) else {
            return;
        };
        if self.state.notes[index].deleted {
            return;
        }
        self.state.notes[index].deleted = true;
        self.state.notes[index].updated_at_ms = now_millis();
        sort_notes_by_updated_desc(&mut self.state.notes);
        self.sync_selection_for_current_scope();
        self.dirty_since = None;
        self.status_line = "moved to trash".to_string();
        save_state(&self.state);
    }

    fn restore_selected_note(&mut self) {
        let Some(id) = self.state.selected_note_id.clone() else {
            return;
        };
        let Some(index) = self.state.notes.iter().position(|note| note.id == id) else {
            return;
        };
        if !self.state.notes[index].deleted {
            return;
        }
        self.state.notes[index].deleted = false;
        self.state.notes[index].updated_at_ms = now_millis();
        sort_notes_by_updated_desc(&mut self.state.notes);
        self.sync_selection_for_current_scope();
        self.dirty_since = None;
        self.status_line = "restored from trash".to_string();
        save_state(&self.state);
    }

    fn purge_selected_note(&mut self) {
        let Some(id) = self.state.selected_note_id.clone() else {
            return;
        };
        if !self.selected_note_is_deleted() {
            return;
        }
        let before = self.state.notes.len();
        self.state.notes.retain(|note| note.id != id);
        self.state.recent_note_ids.retain(|note_id| note_id != &id);
        if self.state.notes.len() == before {
            return;
        }
        self.sync_selection_for_current_scope();
        self.dirty_since = None;
        self.status_line = "deleted permanently".to_string();
        save_state(&self.state);
    }

    fn purge_all_deleted(&mut self) {
        let before = self.state.notes.len();
        let deleted_ids: Vec<String> = self
            .state
            .notes
            .iter()
            .filter(|note| note.deleted)
            .map(|note| note.id.clone())
            .collect();
        self.state.notes.retain(|note| !note.deleted);
        if !deleted_ids.is_empty() {
            self.state
                .recent_note_ids
                .retain(|id| !deleted_ids.iter().any(|deleted| deleted == id));
        }
        let removed = before.saturating_sub(self.state.notes.len());
        self.sync_selection_for_current_scope();
        if let Some(note) = selected_note(&self.state) {
            self.editor_body = note.body.clone();
            self.editor_tags = note.tags.join(" ");
        } else {
            self.editor_body.clear();
            self.editor_tags.clear();
        }
        self.dirty_since = None;
        self.status_line = format!("purged {removed} notes");
        save_state(&self.state);
    }

    fn set_active_folder(&mut self, folder_id: String) {
        if self.state.selected_folder_id == folder_id {
            return;
        }
        self.flush_editor_now();
        self.state.selected_folder_id = folder_id;
        if self.state.show_trash {
            self.state.show_trash = false;
        }
        self.sync_selection_for_current_scope();
        save_state(&self.state);
    }

    fn select_note(&mut self, note_id: String) {
        if self.state.selected_note_id.as_deref() == Some(note_id.as_str()) {
            return;
        }
        self.flush_editor_now();
        self.state.selected_note_id = Some(note_id);
        if let Some(note) = selected_note(&self.state).cloned() {
            if !note.deleted {
                self.touch_recent_note(&note.id);
            }
            self.editor_body = note.body.clone();
            self.editor_tags = note.tags.join(" ");
        }
    }

    fn touch_recent_note(&mut self, note_id: &str) {
        self.state.recent_note_ids.retain(|id| id != note_id);
        self.state.recent_note_ids.insert(0, note_id.to_string());
        const RECENT_LIMIT: usize = 300;
        if self.state.recent_note_ids.len() > RECENT_LIMIT {
            self.state.recent_note_ids.truncate(RECENT_LIMIT);
        }
    }

    fn mark_dirty(&mut self) {
        let changed = self.commit_editor_into_selected();
        if changed {
            self.dirty_since = Some(Instant::now());
            self.status_line = "editing...".to_string();
        }
    }

    fn commit_editor_into_selected(&mut self) -> bool {
        let Some(selected_id) = self.state.selected_note_id.clone() else {
            return false;
        };
        let Some(index) = self
            .state
            .notes
            .iter()
            .position(|note| note.id == selected_id)
        else {
            return false;
        };
        if self.state.notes[index].deleted {
            return false;
        }
        let normalized_tags = normalize_tags(&self.editor_tags);
        let note = &mut self.state.notes[index];
        if note.body == self.editor_body && note.tags == normalized_tags {
            return false;
        }
        note.body = self.editor_body.clone();
        note.tags = normalized_tags;
        note.title = derive_title(&note.body);
        note.updated_at_ms = now_millis();
        sort_notes_by_updated_desc(&mut self.state.notes);
        true
    }

    fn flush_editor_now(&mut self) {
        if self.commit_editor_into_selected() {
            save_state(&self.state);
        }
        self.dirty_since = None;
        self.status_line = "saved".to_string();
    }

    fn autosave_if_needed(&mut self) {
        let Some(dirty_since) = self.dirty_since else {
            return;
        };
        if dirty_since.elapsed() < AUTOSAVE_DELAY {
            return;
        }
        if self.commit_editor_into_selected() {
            save_state(&self.state);
        }
        self.dirty_since = None;
        self.status_line = "auto-saved".to_string();
    }

    fn filtered_note_ids(&self) -> Vec<String> {
        let query = self.state.search_query.trim().to_lowercase();
        let terms: Vec<&str> = if query.is_empty() {
            Vec::new()
        } else {
            query.split_whitespace().collect()
        };
        let mut items: Vec<&WebNote> = self
            .state
            .notes
            .iter()
            .filter(|note| self.note_matches_current_scope(note))
            .filter(|note| terms.is_empty() || note_matches_query(note, &terms))
            .collect();

        if self.state.show_recent {
            let rank = self.recent_rank_map();
            items.sort_by(|a, b| {
                let a_rank = rank.get(&a.id).copied().unwrap_or(usize::MAX);
                let b_rank = rank.get(&b.id).copied().unwrap_or(usize::MAX);
                a_rank.cmp(&b_rank)
            });
        } else {
            match self.state.list_sort {
                WebListSort::UpdatedDesc => {
                    items.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
                }
                WebListSort::CreatedDesc => {
                    items.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
                }
            }
        }

        items.into_iter().map(|note| note.id.clone()).collect()
    }

    fn recent_rank_map(&self) -> std::collections::HashMap<String, usize> {
        let mut map = std::collections::HashMap::new();
        for (idx, id) in self.state.recent_note_ids.iter().enumerate() {
            map.insert(id.clone(), idx);
        }
        map
    }

    fn note_matches_current_scope(&self, note: &WebNote) -> bool {
        if self.state.show_trash {
            return note.deleted;
        }
        if note.deleted {
            return false;
        }
        if self.state.show_recent {
            return self.state.recent_note_ids.iter().any(|id| id == &note.id);
        }
        note.folder_id == self.state.selected_folder_id
    }

    fn set_list_mode(&mut self, show_recent: bool, show_trash: bool) {
        if self.state.show_recent == show_recent && self.state.show_trash == show_trash {
            return;
        }
        self.flush_editor_now();
        self.state.show_recent = show_recent;
        self.state.show_trash = show_trash;
        self.sync_selection_for_current_scope();
        save_state(&self.state);
    }

    fn selected_note_is_deleted(&self) -> bool {
        selected_note(&self.state)
            .map(|note| note.deleted)
            .unwrap_or(false)
    }

    fn active_folder_name(&self) -> String {
        self.state
            .folders
            .iter()
            .find(|folder| folder.id == self.state.selected_folder_id)
            .map(|folder| folder.name.clone())
            .unwrap_or_else(|| MAIN_FOLDER_NAME.to_string())
    }

    fn folder_name_by_id(&self, folder_id: &str) -> String {
        self.state
            .folders
            .iter()
            .find(|folder| folder.id == folder_id)
            .map(|folder| folder.name.clone())
            .unwrap_or_else(|| MAIN_FOLDER_NAME.to_string())
    }

    fn folder_options(&self) -> Vec<(String, String)> {
        self.state
            .folders
            .iter()
            .map(|folder| (folder.id.clone(), folder.name.clone()))
            .collect()
    }

    fn add_folder(&mut self) {
        let name = self.new_folder_name.trim();
        if name.is_empty() {
            self.status_line = "folder name is empty".to_string();
            return;
        }

        if let Some(existing) = self
            .state
            .folders
            .iter()
            .find(|folder| folder.name.eq_ignore_ascii_case(name))
        {
            self.set_active_folder(existing.id.clone());
            self.status_line = "folder already exists".to_string();
            self.new_folder_name.clear();
            return;
        }

        let folder = WebFolder {
            id: make_folder_id(name),
            name: name.to_string(),
            created_at_ms: now_millis(),
        };
        self.state.folders.push(folder.clone());
        self.new_folder_name.clear();
        self.set_active_folder(folder.id);
        self.status_line = "folder created".to_string();
    }

    fn sync_selection_for_current_scope(&mut self) {
        let selected_in_scope = self.state.selected_note_id.as_ref().is_some_and(|id| {
            self.state
                .notes
                .iter()
                .any(|note| note.id == *id && self.note_matches_current_scope(note))
        });
        if selected_in_scope {
            return;
        }

        self.state.selected_note_id = self
            .state
            .notes
            .iter()
            .find(|note| self.note_matches_current_scope(note))
            .map(|note| note.id.clone());

        if let Some(note) = selected_note(&self.state) {
            self.editor_body = note.body.clone();
            self.editor_tags = note.tags.join(" ");
        } else {
            self.editor_body.clear();
            self.editor_tags.clear();
        }
    }

    fn process_import_result(&mut self) {
        let Some(result) = take_import_result() else {
            return;
        };
        match result {
            Ok(payload) => self.import_payload(payload),
            Err(err) => self.status_line = format!("Import failed: {err}"),
        }
    }

    fn import_payload(&mut self, payload: ImportPayload) {
        self.flush_editor_now();
        let ext = extension_from_name(&payload.file_name);
        if ext.as_deref() == Some("json") {
            if self.try_import_json_bundle(&payload.content) {
                return;
            }
            self.status_line =
                "JSON format is not supported. Use Ultra Memo Web export JSON.".to_string();
            return;
        }
        self.import_as_single_note(&payload.file_name, &payload.content);
    }

    fn try_import_json_bundle(&mut self, raw: &str) -> bool {
        let bundle = if let Ok(bundle) = serde_json::from_str::<WebExportBundle>(raw) {
            bundle
        } else if let Ok(state) = serde_json::from_str::<WebState>(raw) {
            WebExportBundle {
                version: 0,
                exported_at_ms: now_millis(),
                folders: state.folders,
                notes: state.notes,
            }
        } else {
            return false;
        };
        let mut created = 0usize;
        let mut updated = 0usize;
        for folder in bundle.folders {
            if folder.id.trim().is_empty() || folder.name.trim().is_empty() {
                continue;
            }
            if self.state.folders.iter().any(|f| f.id == folder.id) {
                continue;
            }
            self.state.folders.push(folder);
        }
        for mut note in bundle.notes {
            if note.id.trim().is_empty() {
                continue;
            }
            if note.folder_id.trim().is_empty() {
                note.folder_id = MAIN_FOLDER_ID.to_string();
            }
            if !self.state.folders.iter().any(|f| f.id == note.folder_id) {
                note.folder_id = MAIN_FOLDER_ID.to_string();
            }
            if let Some(idx) = self.state.notes.iter().position(|n| n.id == note.id) {
                self.state.notes[idx] = note;
                updated += 1;
            } else {
                self.state.notes.push(note);
                created += 1;
            }
        }
        ensure_state_integrity(&mut self.state);
        sort_notes_by_updated_desc(&mut self.state.notes);
        self.sync_selection_for_current_scope();
        save_state(&self.state);
        self.status_line = format!("imported {created} new, {updated} updated notes");
        true
    }

    fn import_as_single_note(&mut self, file_name: &str, content: &str) {
        let now = now_millis();
        let id = format!("n{now}-{}", NOTE_SEQUENCE.fetch_add(1, Ordering::Relaxed));
        let body = content.replace("\r\n", "\n");
        let mut title = derive_title(&body);
        if title.trim().is_empty() {
            title = trim_file_name(file_name);
        }
        let note = WebNote {
            id: id.clone(),
            title,
            body,
            tags: Vec::new(),
            deleted: false,
            folder_id: self.state.selected_folder_id.clone(),
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.state.show_recent = false;
        self.state.show_trash = false;
        self.state.notes.insert(0, note);
        self.state.selected_note_id = Some(id);
        self.sync_selection_for_current_scope();
        save_state(&self.state);
        self.status_line = format!("imported '{file_name}'");
    }

    fn export_json_all(&mut self) {
        self.flush_editor_now();
        let payload = WebExportBundle {
            version: 1,
            exported_at_ms: now_millis(),
            folders: self.state.folders.clone(),
            notes: self.state.notes.clone(),
        };
        match serde_json::to_string_pretty(&payload) {
            Ok(json) => {
                match download_text_file("ultra-memo-export.json", "application/json", &json) {
                    Ok(()) => self.status_line = "exported JSON".to_string(),
                    Err(err) => self.status_line = format!("export failed: {err}"),
                }
            }
            Err(err) => self.status_line = format!("export failed: {err}"),
        }
    }

    fn export_selected_markdown(&mut self) {
        self.flush_editor_now();
        let Some(note) = selected_note(&self.state) else {
            self.status_line = "no selected note".to_string();
            return;
        };
        let base_name = sanitize_file_name(&safe_title(&note.title));
        let file_name = format!("{base_name}.md");
        match download_text_file(&file_name, "text/markdown", &note.body) {
            Ok(()) => self.status_line = format!("exported {file_name}"),
            Err(err) => self.status_line = format!("export failed: {err}"),
        }
    }

    fn open_import_picker(&mut self) {
        match start_import_picker() {
            Ok(()) => self.status_line = "choose a file to import".to_string(),
            Err(err) => self.status_line = format!("import dialog failed: {err}"),
        }
    }

    fn reset_appearance(&mut self) {
        self.state.ui_font_preset = FONT_PRESET_DEFAULT.to_string();
        self.state.ui_text_color_rgb = DEFAULT_TEXT_COLOR_RGB;
        self.state.ui_background_color_rgb = DEFAULT_BG_COLOR_RGB;
        self.state.ui_accent_color_rgb = DEFAULT_ACCENT_COLOR_RGB;
        self.state.ui_zoom = 1.0;
        self.status_line = "appearance reset".to_string();
    }

    fn apply_appearance_preset(&mut self, preset: AppearancePreset) {
        match preset {
            AppearancePreset::Classic => {
                self.state.ui_text_color_rgb = [109, 70, 41];
                self.state.ui_background_color_rgb = [252, 252, 252];
                self.state.ui_accent_color_rgb = [0, 148, 136];
            }
            AppearancePreset::WarmJournal => {
                self.state.ui_text_color_rgb = [121, 34, 57];
                self.state.ui_background_color_rgb = [252, 252, 252];
                self.state.ui_accent_color_rgb = [255, 149, 0];
            }
            AppearancePreset::QuietModern => {
                self.state.ui_text_color_rgb = [55, 64, 81];
                self.state.ui_background_color_rgb = [245, 245, 247];
                self.state.ui_accent_color_rgb = [88, 86, 214];
            }
            AppearancePreset::NightNotes => {
                self.state.ui_text_color_rgb = PAPER_TEXT_RGB;
                self.state.ui_background_color_rgb = [234, 236, 240];
                self.state.ui_accent_color_rgb = NIGHT_MINT_ACCENT_RGB;
            }
        }
        self.status_line = "appearance preset applied".to_string();
    }
}

impl eframe::App for WebMemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.boot_status_cleared {
            set_boot_status("");
            self.boot_status_cleared = true;
        }
        self.process_import_result();
        self.state.ui_zoom = self.state.ui_zoom.clamp(UI_ZOOM_MIN, UI_ZOOM_MAX);
        ctx.set_zoom_factor(self.state.ui_zoom);
        apply_apple_like_style(
            ctx,
            &self.state.ui_font_preset,
            self.state.ui_text_color_rgb,
            self.state.ui_background_color_rgb,
            self.state.ui_accent_color_rgb,
        );
        self.autosave_if_needed();

        egui::TopBottomPanel::top("top_toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let search_width = (ui.available_width() - 340.0).max(140.0);
                let search = ui.add_sized(
                    [search_width, 30.0],
                    egui::TextEdit::singleline(&mut self.state.search_query)
                        .hint_text("Search or #tag"),
                );
                if search.changed() {
                    self.status_line = "search updated".to_string();
                }
                if ui.button("+ New").clicked() {
                    self.create_note();
                }
                ui.toggle_value(&mut self.state.markdown_render_mode, "M");
                ui.toggle_value(&mut self.state.focus_mode, "Focus");
                if ui.button("Menu").clicked() {
                    self.show_menu = true;
                }
            });
        });

        if !self.state.focus_mode {
            egui::SidePanel::left("note_list")
                .resizable(true)
                .min_width(LIST_PANEL_MIN_WIDTH)
                .max_width(LIST_PANEL_MAX_WIDTH)
                .default_width(self.state.list_panel_width)
                .show(ctx, |ui| {
                    self.state.list_panel_width = ui.max_rect().width();
                    let all_count = self
                        .state
                        .notes
                        .iter()
                        .filter(|note| {
                            !note.deleted && note.folder_id == self.state.selected_folder_id
                        })
                        .count();
                    let recent_count = self
                        .state
                        .recent_note_ids
                        .iter()
                        .filter(|id| {
                            self.state
                                .notes
                                .iter()
                                .any(|note| &note.id == *id && !note.deleted)
                        })
                        .count();
                    let trash_count = self.state.notes.iter().filter(|note| note.deleted).count();

                    ui.horizontal(|ui| {
                        let all_selected = !self.state.show_recent && !self.state.show_trash;
                        if ui
                            .selectable_label(all_selected, format!("All ({all_count})"))
                            .clicked()
                        {
                            self.set_list_mode(false, false);
                        }
                        if ui
                            .selectable_label(
                                self.state.show_recent,
                                format!("Recent ({recent_count})"),
                            )
                            .clicked()
                        {
                            self.set_list_mode(true, false);
                        }
                        if ui
                            .selectable_label(
                                self.state.show_trash,
                                format!("Trash ({trash_count})"),
                            )
                            .clicked()
                        {
                            self.set_list_mode(false, true);
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Sort:");
                        ui.add_enabled_ui(!self.state.show_recent, |ui| {
                            if ui
                                .selectable_label(
                                    matches!(self.state.list_sort, WebListSort::UpdatedDesc),
                                    "Updated",
                                )
                                .clicked()
                            {
                                self.state.list_sort = WebListSort::UpdatedDesc;
                                save_state(&self.state);
                            }
                            if ui
                                .selectable_label(
                                    matches!(self.state.list_sort, WebListSort::CreatedDesc),
                                    "Created",
                                )
                                .clicked()
                            {
                                self.state.list_sort = WebListSort::CreatedDesc;
                                save_state(&self.state);
                            }
                        });
                        if self.state.show_recent {
                            ui.small("(Recent keeps open order)");
                        }
                    });

                    ui.horizontal(|ui| {
                        let can_use_folder = !self.state.show_trash && !self.state.show_recent;
                        if can_use_folder {
                            ui.label(format!("Folder: {}", self.active_folder_name()));
                        } else {
                            ui.label("Folder: all");
                        }
                        ui.add_enabled_ui(can_use_folder, |ui| {
                            if ui.small_button("Folders").clicked() {
                                self.show_folder_manager = true;
                            }
                            if self.state.selected_folder_id != MAIN_FOLDER_ID
                                && ui.small_button("Main").clicked()
                            {
                                self.set_active_folder(MAIN_FOLDER_ID.to_string());
                            }
                        });
                        if self.state.show_trash && ui.small_button("Empty Trash").clicked() {
                            self.purge_all_deleted();
                        }
                    });

                    let ids = self.filtered_note_ids();
                    let list_name = if self.state.show_trash {
                        "Trash"
                    } else if self.state.show_recent {
                        "Recent"
                    } else {
                        "All"
                    };
                    let scope_total = if self.state.show_trash {
                        trash_count
                    } else if self.state.show_recent {
                        recent_count
                    } else {
                        all_count
                    };
                    ui.label(format!("{list_name}: {} / {scope_total}", ids.len()));
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for note_id in ids {
                                let Some(note) = self.state.notes.iter().find(|n| n.id == note_id)
                                else {
                                    continue;
                                };
                                let selected = self.state.selected_note_id.as_deref()
                                    == Some(note.id.as_str());
                                let mut title = truncate_chars(
                                    safe_title(&note.title),
                                    TITLE_MAX_PREVIEW_CHARS,
                                );
                                if note.deleted {
                                    title = format!("[Trash] {title}");
                                }
                                let snippet = truncate_chars(
                                    &note
                                        .body
                                        .lines()
                                        .next()
                                        .unwrap_or_default()
                                        .replace('\t', " "),
                                    SNIPPET_MAX_PREVIEW_CHARS,
                                );
                                let updated = format_time(note.updated_at_ms);
                                let tags = if note.tags.is_empty() {
                                    String::new()
                                } else {
                                    format!("#{}", note.tags.join(" #"))
                                };

                                let text = if tags.is_empty() {
                                    format!("{title}\n{updated}\n{snippet}")
                                } else {
                                    format!("{title}\n{updated}\n{snippet}\n{tags}")
                                };
                                let clicked = ui
                                    .add_sized(
                                        [ui.available_width(), 74.0],
                                        egui::Button::new(text).selected(selected),
                                    )
                                    .clicked();
                                if clicked {
                                    self.select_note(note.id.clone());
                                }
                                ui.add_space(6.0);
                            }
                        });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(selected_id) = self.state.selected_note_id.clone() else {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    if self.state.show_trash {
                        ui.label("Trash is empty");
                        if ui.button("Back to notes").clicked() {
                            self.state.show_trash = false;
                            self.sync_selection_for_current_scope();
                            save_state(&self.state);
                        }
                    } else {
                        ui.label("Create a note with + New");
                        if ui.button("New note in this folder").clicked() {
                            self.create_note();
                        }
                    }
                });
                return;
            };

            let Some(selected_note) = self
                .state
                .notes
                .iter()
                .find(|n| n.id == selected_id)
                .cloned()
            else {
                self.state.selected_note_id = None;
                self.editor_body.clear();
                self.editor_tags.clear();
                return;
            };
            let selected_is_deleted = selected_note.deleted;
            let folder_options = self.folder_options();
            let selected_title = truncate_chars(safe_title(&selected_note.title), 64);
            let current_note_folder_id = selected_note.folder_id.clone();
            let mut move_target_folder = current_note_folder_id.clone();

            ui.horizontal(|ui| {
                ui.heading(selected_title);
                if selected_is_deleted {
                    ui.label("(in trash)");
                }
                ui.separator();
                ui.add_enabled_ui(!selected_is_deleted, |ui| {
                    egui::ComboBox::from_id_salt("selected_note_folder_combo")
                        .selected_text(self.folder_name_by_id(&move_target_folder))
                        .show_ui(ui, |ui| {
                            for (folder_id, folder_name) in &folder_options {
                                ui.selectable_value(
                                    &mut move_target_folder,
                                    folder_id.clone(),
                                    folder_name,
                                );
                            }
                        });
                });
                if selected_is_deleted {
                    if ui.button("Restore").clicked() {
                        self.restore_selected_note();
                    }
                    if ui.button("Delete Forever").clicked() {
                        self.purge_selected_note();
                    }
                } else if ui.button("Delete").clicked() {
                    self.delete_selected_note();
                }
            });
            if !selected_is_deleted && move_target_folder != current_note_folder_id {
                let moved_folder_name = self.folder_name_by_id(&move_target_folder);
                if let Some(note) = self.state.notes.iter_mut().find(|n| n.id == selected_id) {
                    note.folder_id = move_target_folder.clone();
                    note.updated_at_ms = now_millis();
                }
                sort_notes_by_updated_desc(&mut self.state.notes);
                self.sync_selection_for_current_scope();
                save_state(&self.state);
                self.status_line = format!("moved to {moved_folder_name}");
            }

            ui.horizontal(|ui| {
                ui.label("Tags:");
                let tags_resp = ui.add_enabled_ui(!selected_is_deleted, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::TextEdit::singleline(&mut self.editor_tags)
                            .hint_text("work idea rust"),
                    )
                });
                if tags_resp.inner.changed() {
                    self.mark_dirty();
                }
            });

            ui.add_space(6.0);
            let available = ui.available_size();
            let editor_height = if self.state.markdown_render_mode {
                (available.y * 0.55).max(180.0)
            } else {
                available.y.max(220.0)
            };
            let edit_resp = ui.add_enabled_ui(!selected_is_deleted, |ui| {
                ui.add_sized(
                    [available.x, editor_height],
                    egui::TextEdit::multiline(&mut self.editor_body)
                        .hint_text("Write your memo...")
                        .desired_width(f32::INFINITY),
                )
            });
            if edit_resp.inner.changed() {
                self.mark_dirty();
            }
            if selected_is_deleted {
                ui.small("This note is in Trash. Restore it to edit.");
            }

            if self.state.markdown_render_mode {
                ui.separator();
                ui.label("Markdown Preview");
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| render_markdown_preview(ui, &self.editor_body));
            }
        });

        egui::TopBottomPanel::bottom("status_bar")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&self.status_line);
                    ui.separator();
                    ui.label("Storage: browser localStorage");
                });
            });

        let menu_was_open = self.show_menu;
        if self.show_menu {
            let mut open = self.show_menu;
            egui::Window::new("Menu")
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Appearance");
                        if ui.small_button("Reset").clicked() {
                            self.reset_appearance();
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Preset:");
                        if ui.button("Classic").clicked() {
                            self.apply_appearance_preset(AppearancePreset::Classic);
                        }
                        if ui.button("Warm Journal").clicked() {
                            self.apply_appearance_preset(AppearancePreset::WarmJournal);
                        }
                        if ui.button("Quiet Modern").clicked() {
                            self.apply_appearance_preset(AppearancePreset::QuietModern);
                        }
                        if ui.button("Night Notes").clicked() {
                            self.apply_appearance_preset(AppearancePreset::NightNotes);
                        }
                    });
                    egui::ComboBox::from_id_salt("web_font_preset")
                        .selected_text(font_preset_label(&self.state.ui_font_preset))
                        .show_ui(ui, |ui| {
                            for (id, label) in font_preset_options() {
                                ui.selectable_value(
                                    &mut self.state.ui_font_preset,
                                    id.to_string(),
                                    label,
                                );
                            }
                        });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Text:");
                        for (label, rgb) in TEXT_COLOR_PRESETS {
                            let selected = self.state.ui_text_color_rgb == rgb;
                            if ui.selectable_label(selected, label).clicked() {
                                self.state.ui_text_color_rgb = rgb;
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Background:");
                        for (label, rgb) in BG_COLOR_PRESETS {
                            let selected = self.state.ui_background_color_rgb == rgb;
                            if ui.selectable_label(selected, label).clicked() {
                                self.state.ui_background_color_rgb = rgb;
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Accent:");
                        for (label, rgb) in ACCENT_COLOR_PRESETS {
                            let selected = self.state.ui_accent_color_rgb == rgb;
                            if ui.selectable_label(selected, label).clicked() {
                                self.state.ui_accent_color_rgb = rgb;
                            }
                        }
                    });
                    ui.add(
                        egui::Slider::new(&mut self.state.ui_zoom, UI_ZOOM_MIN..=UI_ZOOM_MAX)
                            .text("UI scale")
                            .step_by(0.01),
                    );

                    ui.separator();
                    ui.label("Data I/O");
                    if ui.button("Export JSON (All notes)").clicked() {
                        self.export_json_all();
                    }
                    if ui.button("Export Markdown (Selected note)").clicked() {
                        self.export_selected_markdown();
                    }
                    if ui.button("Import file...").clicked() {
                        self.open_import_picker();
                    }
                    ui.small("Import: json/md/markdown/txt/text/log/rst/adoc/org");
                });
            self.show_menu = open;
        }
        if menu_was_open && !self.show_menu {
            save_state(&self.state);
        }

        if self.show_folder_manager {
            let mut open = self.show_folder_manager;
            egui::Window::new("Folders")
                .collapsible(false)
                .resizable(false)
                .default_width(280.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Select folder");
                    let folder_items: Vec<(String, String, usize)> = self
                        .state
                        .folders
                        .iter()
                        .map(|folder| {
                            let count = self
                                .state
                                .notes
                                .iter()
                                .filter(|note| note.folder_id == folder.id)
                                .count();
                            (folder.id.clone(), folder.name.clone(), count)
                        })
                        .collect();
                    let mut activate_folder: Option<String> = None;
                    for (folder_id, folder_name, count) in folder_items {
                        let label = format!("{folder_name} ({count})");
                        if ui
                            .selectable_label(self.state.selected_folder_id == folder_id, label)
                            .clicked()
                        {
                            activate_folder = Some(folder_id);
                        }
                    }
                    if let Some(folder_id) = activate_folder {
                        self.set_active_folder(folder_id);
                    }

                    ui.separator();
                    ui.label("Create folder");
                    ui.horizontal(|ui| {
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut self.new_folder_name)
                                .hint_text("Folder name"),
                        );
                        let submitted = input.lost_focus()
                            && ui.input(|input_state| input_state.key_pressed(egui::Key::Enter));
                        if ui.button("Add").clicked() || submitted {
                            self.add_folder();
                        }
                    });
                    ui.small(format!("Default folder is '{MAIN_FOLDER_NAME}'"));
                });
            self.show_folder_manager = open;
        }
    }
}

fn normalize_tags(raw: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for token in raw.split_whitespace() {
        let normalized = token.trim().trim_start_matches('#').to_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if !tags.iter().any(|item| item == &normalized) {
            tags.push(normalized);
        }
    }
    tags
}

fn note_matches_query(note: &WebNote, terms: &[&str]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let title = note.title.to_lowercase();
    let body = note.body.to_lowercase();
    for term in terms {
        if term.is_empty() {
            continue;
        }
        if let Some(tag_term) = term.strip_prefix('#') {
            if tag_term.is_empty() {
                continue;
            }
            if !note.tags.iter().any(|tag| tag.contains(tag_term)) {
                return false;
            }
            continue;
        }
        if !title.contains(term) && !body.contains(term) {
            return false;
        }
    }
    true
}

fn default_main_folder_id() -> String {
    MAIN_FOLDER_ID.to_string()
}

fn main_folder() -> WebFolder {
    WebFolder {
        id: MAIN_FOLDER_ID.to_string(),
        name: MAIN_FOLDER_NAME.to_string(),
        created_at_ms: 0,
    }
}

fn ensure_state_integrity(state: &mut WebState) {
    if state.folders.is_empty() {
        state.folders.push(main_folder());
    }
    if !state
        .folders
        .iter()
        .any(|folder| folder.id == MAIN_FOLDER_ID)
    {
        state.folders.insert(0, main_folder());
    }

    // Keep folder ids unique and names non-empty for stable serialization/migration.
    let mut unique_folders = Vec::new();
    for folder in state.folders.drain(..) {
        if folder.id.trim().is_empty()
            || unique_folders
                .iter()
                .any(|existing: &WebFolder| existing.id == folder.id)
        {
            continue;
        }
        let normalized_name = folder.name.trim();
        unique_folders.push(WebFolder {
            id: folder.id,
            name: if normalized_name.is_empty() {
                "Folder".to_string()
            } else {
                normalized_name.to_string()
            },
            created_at_ms: folder.created_at_ms,
        });
    }
    if !unique_folders
        .iter()
        .any(|folder| folder.id == MAIN_FOLDER_ID)
    {
        unique_folders.insert(0, main_folder());
    }
    state.folders = unique_folders;

    if state.selected_folder_id.trim().is_empty()
        || !state
            .folders
            .iter()
            .any(|folder| folder.id == state.selected_folder_id)
    {
        state.selected_folder_id = MAIN_FOLDER_ID.to_string();
    }

    if state.show_trash {
        state.show_recent = false;
    }

    for note in &mut state.notes {
        if note.folder_id.trim().is_empty()
            || !state
                .folders
                .iter()
                .any(|folder| folder.id == note.folder_id)
        {
            note.folder_id = MAIN_FOLDER_ID.to_string();
        }
    }

    let existing_ids: Vec<String> = state.notes.iter().map(|note| note.id.clone()).collect();
    let mut normalized_recent = Vec::new();
    for id in state.recent_note_ids.drain(..) {
        if !existing_ids.iter().any(|existing| existing == &id) {
            continue;
        }
        if normalized_recent
            .iter()
            .any(|existing: &String| existing == &id)
        {
            continue;
        }
        normalized_recent.push(id);
    }
    state.recent_note_ids = normalized_recent;

    if !is_supported_font_preset(&state.ui_font_preset) {
        state.ui_font_preset = FONT_PRESET_DEFAULT.to_string();
    }
    state.ui_zoom = state.ui_zoom.clamp(UI_ZOOM_MIN, UI_ZOOM_MAX);
    if state.ui_text_color_rgb == [0, 0, 0] {
        state.ui_text_color_rgb = DEFAULT_TEXT_COLOR_RGB;
    }
    if state.ui_background_color_rgb == [0, 0, 0] {
        state.ui_background_color_rgb = DEFAULT_BG_COLOR_RGB;
    }
    if state.ui_accent_color_rgb == [0, 0, 0] {
        state.ui_accent_color_rgb = DEFAULT_ACCENT_COLOR_RGB;
    }
}

fn selected_note(state: &WebState) -> Option<&WebNote> {
    let selected = state.selected_note_id.as_deref()?;
    state.notes.iter().find(|note| note.id == selected)
}

fn sort_notes_by_updated_desc(notes: &mut [WebNote]) {
    notes.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
}

fn now_millis() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_millis() as i64)
            .unwrap_or_default()
    }
}

fn make_empty_note_in_folder(folder_id: &str) -> WebNote {
    let now = now_millis();
    let id = format!("n{now}-{}", NOTE_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    WebNote {
        id,
        title: String::new(),
        body: String::new(),
        tags: Vec::new(),
        deleted: false,
        folder_id: folder_id.to_string(),
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn make_folder_id(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    let base = if slug.is_empty() { "folder" } else { slug };
    let seq = FOLDER_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("f-{base}-{seq}")
}

fn format_time(ms: i64) -> String {
    match Utc.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%m/%d %H:%M").to_string(),
        None => "-".to_string(),
    }
}

fn load_state() -> WebState {
    let Some(storage) = browser_storage() else {
        return WebState::default();
    };
    let scoped_key = storage_key();
    if let Ok(Some(raw)) = storage.get_item(&scoped_key) {
        return serde_json::from_str(&raw).unwrap_or_default();
    }

    // Backward compatibility: migrate old single-key data to scoped key.
    if let Ok(Some(raw)) = storage.get_item(LEGACY_STORAGE_KEY) {
        let parsed = serde_json::from_str::<WebState>(&raw).unwrap_or_default();
        let _ = storage.set_item(&scoped_key, &raw);
        let _ = storage.remove_item(LEGACY_STORAGE_KEY);
        return parsed;
    }

    WebState::default()
}

fn save_state(state: &WebState) {
    let Some(storage) = browser_storage() else {
        return;
    };
    let Ok(serialized) = serde_json::to_string(state) else {
        return;
    };
    let key = storage_key();
    let _ = storage.set_item(&key, &serialized);
}

fn browser_storage() -> Option<web_sys::Storage> {
    let window = web_sys::window()?;
    window.local_storage().ok()?
}

fn storage_key() -> String {
    let scope = web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .map(|path| {
            let trimmed = path.trim_matches('/');
            if trimmed.is_empty() {
                "root".to_string()
            } else {
                trimmed
                    .split('/')
                    .next()
                    .unwrap_or("root")
                    .to_ascii_lowercase()
            }
        })
        .unwrap_or_else(|| "root".to_string());
    format!("{LEGACY_STORAGE_KEY}:{scope}")
}

fn set_boot_status(message: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(element) = document.get_element_by_id("boot_status") else {
        return;
    };
    element.set_text_content(Some(message));
}

fn apply_apple_like_style(
    ctx: &egui::Context,
    font_preset: &str,
    text_color_rgb: [u8; 3],
    background_color_rgb: [u8; 3],
    accent_color_rgb: [u8; 3],
) {
    let text_color = Color32::from_rgb(text_color_rgb[0], text_color_rgb[1], text_color_rgb[2]);
    let bg_color = Color32::from_rgb(
        background_color_rgb[0],
        background_color_rgb[1],
        background_color_rgb[2],
    );
    let accent_color = Color32::from_rgb(
        accent_color_rgb[0],
        accent_color_rgb[1],
        accent_color_rgb[2],
    );
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(text_color);
    visuals.panel_fill = bg_color;
    visuals.window_fill = Color32::from_rgb(250, 250, 252);
    visuals.extreme_bg_color = Color32::from_rgb(238, 239, 243);
    visuals.faint_bg_color = Color32::from_rgb(243, 244, 247);
    visuals.code_bg_color = Color32::from_rgb(240, 241, 245);
    visuals.selection.bg_fill = accent_color;
    visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(255, 255, 255));
    visuals.hyperlink_color = accent_color;
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgba_premultiplied(60, 60, 67, 40));

    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(248, 248, 250);
    visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgba_premultiplied(60, 60, 67, 30));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(255, 255, 255);
    visuals.widgets.inactive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgba_premultiplied(60, 60, 67, 35));
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(242, 246, 255);
    visuals.widgets.hovered.bg_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_premultiplied(
            accent_color_rgb[0],
            accent_color_rgb[1],
            accent_color_rgb[2],
            120,
        ),
    );
    visuals.widgets.active.bg_fill = Color32::from_rgb(232, 240, 255);
    visuals.widgets.active.bg_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_premultiplied(
            accent_color_rgb[0],
            accent_color_rgb[1],
            accent_color_rgb[2],
            180,
        ),
    );

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.window_margin = egui::Margin::same(12);
    style.spacing.indent = 18.0;
    style.visuals.window_corner_radius = egui::CornerRadius::same(14);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(12);

    let body_family = font_family_for_preset(font_preset);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, body_family.clone()));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::new(14.0, body_family.clone()));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::new(12.0, body_family));
    ctx.set_style(style);
}

fn font_preset_options() -> [(&'static str, &'static str); 3] {
    [
        (FONT_PRESET_DEFAULT, "Standard"),
        (FONT_PRESET_SERIF, "Serif"),
        (FONT_PRESET_MONO, "Monospace"),
    ]
}

fn is_supported_font_preset(font_preset: &str) -> bool {
    font_preset_options()
        .iter()
        .any(|(id, _)| *id == font_preset)
}

fn font_preset_label(font_preset: &str) -> &'static str {
    for (id, label) in font_preset_options() {
        if id == font_preset {
            return label;
        }
    }
    "Standard"
}

fn font_family_for_preset(font_preset: &str) -> FontFamily {
    match font_preset {
        // Web default fonts always provide Proportional/Monospace.
        // Avoid custom named families here because missing family resolution can break rendering.
        FONT_PRESET_SERIF => FontFamily::Proportional,
        FONT_PRESET_MONO => FontFamily::Monospace,
        _ => FontFamily::Proportional,
    }
}

fn extension_from_name(file_name: &str) -> Option<String> {
    let ext = file_name.rsplit('.').next()?.trim().to_ascii_lowercase();
    if ext.is_empty() || ext == file_name {
        return None;
    }
    Some(ext)
}

fn trim_file_name(file_name: &str) -> String {
    let trimmed = file_name.trim();
    let Some((stem, _)) = trimmed.rsplit_once('.') else {
        return trimmed.to_string();
    };
    if stem.trim().is_empty() {
        trimmed.to_string()
    } else {
        stem.trim().to_string()
    }
}

fn sanitize_file_name(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        let safe = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_';
        if safe {
            out.push(ch);
        } else if ch.is_whitespace() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let normalized = out.trim_matches('-');
    if normalized.is_empty() {
        "note".to_string()
    } else {
        normalized.to_string()
    }
}

fn store_import_result(result: Result<ImportPayload, String>) {
    IMPORT_RESULT.with(|slot| *slot.borrow_mut() = Some(result));
}

fn take_import_result() -> Option<Result<ImportPayload, String>> {
    IMPORT_RESULT.with(|slot| slot.borrow_mut().take())
}

fn start_import_picker() -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "document is not available".to_string())?;
    let element = document
        .create_element("input")
        .map_err(|_| "failed to create input element".to_string())?;
    let input = element
        .dyn_into::<web_sys::HtmlInputElement>()
        .map_err(|_| "failed to cast input element".to_string())?;

    input.set_type("file");
    input.set_multiple(false);
    let accept = SUPPORTED_IMPORT_EXTENSIONS
        .iter()
        .map(|ext| format!(".{ext}"))
        .collect::<Vec<_>>()
        .join(",");
    input.set_accept(&accept);

    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }

    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event.target() else {
            store_import_result(Err("file input target is missing".to_string()));
            return;
        };
        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
            store_import_result(Err("file input target is invalid".to_string()));
            return;
        };

        let Some(files) = input.files() else {
            store_import_result(Err("no files selected".to_string()));
            input.remove();
            return;
        };
        let Some(file) = files.get(0) else {
            store_import_result(Err("no files selected".to_string()));
            input.remove();
            return;
        };
        let file_name = file.name();
        let Some(ext) = extension_from_name(&file_name) else {
            store_import_result(Err("unsupported file extension".to_string()));
            input.remove();
            return;
        };
        if !SUPPORTED_IMPORT_EXTENSIONS
            .iter()
            .any(|allowed| *allowed == ext)
        {
            store_import_result(Err("unsupported file extension".to_string()));
            input.remove();
            return;
        }

        let reader = match web_sys::FileReader::new() {
            Ok(reader) => reader,
            Err(_) => {
                store_import_result(Err("failed to create file reader".to_string()));
                input.remove();
                return;
            }
        };
        let reader_for_cb = reader.clone();
        let file_name_for_cb = file_name.clone();
        let on_load = Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(
            move |_event: web_sys::ProgressEvent| match reader_for_cb.result() {
                Ok(result) => match result.as_string() {
                    Some(content) => store_import_result(Ok(ImportPayload {
                        file_name: file_name_for_cb.clone(),
                        content,
                    })),
                    None => store_import_result(Err("failed to read file text".to_string())),
                },
                Err(_) => store_import_result(Err("failed to read selected file".to_string())),
            },
        );
        reader.set_onloadend(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();

        if reader.read_as_text(&file).is_err() {
            store_import_result(Err("failed to start file read".to_string()));
        }
        input.remove();
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
    Ok(())
}

fn download_text_file(file_name: &str, mime: &str, content: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "window is not available".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "document is not available".to_string())?;

    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(content));

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime);
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|_| "failed to create blob".to_string())?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "failed to create download url".to_string())?;

    let anchor = document
        .create_element("a")
        .map_err(|_| "failed to create anchor".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "failed to cast anchor".to_string())?;

    anchor.set_href(&url);
    anchor.set_download(file_name);

    if let Some(body) = document.body() {
        let _ = body.append_child(&anchor);
        anchor.click();
        anchor.remove();
    } else {
        anchor.click();
    }

    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

fn render_markdown_preview(ui: &mut egui::Ui, body: &str) {
    let mut in_code = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            ui.label(egui::RichText::new(trimmed).monospace().weak());
            continue;
        }
        if in_code {
            ui.label(egui::RichText::new(trimmed).monospace());
            continue;
        }
        if let Some((level, text)) = markdown_heading(trimmed) {
            let size = match level {
                1 => 26.0,
                2 => 22.0,
                3 => 19.0,
                _ => 17.0,
            };
            ui.label(egui::RichText::new(text).size(size).strong());
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
                ui.label(egui::RichText::new(text).strikethrough());
            });
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("- ") {
            ui.label(format!("* {text}"));
            continue;
        }
        if let Some((num, text)) = markdown_ordered(trimmed) {
            ui.label(format!("{num}. {text}"));
            continue;
        }
        if trimmed.starts_with('>') {
            ui.label(egui::RichText::new(trimmed).italics().weak());
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
