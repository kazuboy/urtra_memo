use crate::model::{derive_title, safe_title, truncate_chars};
use chrono::{TimeZone, Utc};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
use wasm_bindgen::JsCast;

const LEGACY_STORAGE_KEY: &str = "ultra_memo.web.state.v1";
const TITLE_MAX_PREVIEW_CHARS: usize = 28;
const SNIPPET_MAX_PREVIEW_CHARS: usize = 80;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(700);
const LIST_PANEL_MIN_WIDTH: f32 = 220.0;
const LIST_PANEL_DEFAULT_WIDTH: f32 = 300.0;
const LIST_PANEL_MAX_WIDTH: f32 = 520.0;

static NOTE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct WebState {
    notes: Vec<WebNote>,
    selected_note_id: Option<String>,
    search_query: String,
    markdown_render_mode: bool,
    focus_mode: bool,
    list_panel_width: f32,
}

impl Default for WebState {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            selected_note_id: None,
            search_query: String::new(),
            markdown_render_mode: false,
            focus_mode: false,
            list_panel_width: LIST_PANEL_DEFAULT_WIDTH,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebNote {
    id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
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
    status_line: String,
    dirty_since: Option<Instant>,
    boot_status_cleared: bool,
}

impl WebMemoApp {
    fn new() -> Self {
        let mut state = load_state();
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
            let note = make_empty_note();
            state.selected_note_id = Some(note.id.clone());
            state.notes.push(note);
            save_state(&state);
        }

        let (editor_body, editor_tags) = if let Some(note) = selected_note(&state) {
            (note.body.clone(), note.tags.join(" "))
        } else {
            (String::new(), String::new())
        };

        Self {
            state,
            editor_body,
            editor_tags,
            status_line: "ready".to_string(),
            dirty_since: None,
            boot_status_cleared: false,
        }
    }

    fn create_note(&mut self) {
        self.flush_editor_now();
        let note = make_empty_note();
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
        self.state.notes.retain(|note| note.id != id);
        if self.state.notes.is_empty() {
            let note = make_empty_note();
            self.state.selected_note_id = Some(note.id.clone());
            self.editor_body.clear();
            self.editor_tags.clear();
            self.state.notes.push(note);
        } else {
            self.state.selected_note_id = self.state.notes.first().map(|note| note.id.clone());
        }
        if let Some(note) = selected_note(&self.state) {
            self.editor_body = note.body.clone();
            self.editor_tags = note.tags.join(" ");
        } else {
            self.editor_body.clear();
            self.editor_tags.clear();
        }
        self.dirty_since = None;
        self.status_line = "deleted".to_string();
        save_state(&self.state);
    }

    fn select_note(&mut self, note_id: String) {
        if self.state.selected_note_id.as_deref() == Some(note_id.as_str()) {
            return;
        }
        self.flush_editor_now();
        self.state.selected_note_id = Some(note_id);
        if let Some(note) = selected_note(&self.state) {
            self.editor_body = note.body.clone();
            self.editor_tags = note.tags.join(" ");
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
        let Some(index) = self.state.notes.iter().position(|note| note.id == selected_id) else {
            return false;
        };
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
        if query.is_empty() {
            return self.state.notes.iter().map(|note| note.id.clone()).collect();
        }
        let terms: Vec<&str> = query.split_whitespace().collect();
        self.state
            .notes
            .iter()
            .filter(|note| note_matches_query(note, &terms))
            .map(|note| note.id.clone())
            .collect()
    }
}

impl eframe::App for WebMemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.boot_status_cleared {
            set_boot_status("");
            self.boot_status_cleared = true;
        }
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = egui::Color32::from_rgb(244, 244, 246);
        visuals.window_fill = egui::Color32::from_rgb(244, 244, 246);
        visuals.override_text_color = Some(egui::Color32::from_rgb(36, 36, 36));
        ctx.set_visuals(visuals);
        self.autosave_if_needed();

        egui::TopBottomPanel::top("top_toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let search_width = (ui.available_width() - 240.0).max(140.0);
                let search = ui.add_sized(
                    [search_width, 30.0],
                    egui::TextEdit::singleline(&mut self.state.search_query).hint_text("Search or #tag"),
                );
                if search.changed() {
                    self.status_line = "search updated".to_string();
                }
                if ui.button("+ New").clicked() {
                    self.create_note();
                }
                ui.toggle_value(&mut self.state.markdown_render_mode, "M");
                ui.toggle_value(&mut self.state.focus_mode, "Focus");
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
                    let ids = self.filtered_note_ids();
                    ui.label(format!("Notes: {}", ids.len()));
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for note_id in ids {
                                let Some(note) = self.state.notes.iter().find(|n| n.id == note_id) else {
                                    continue;
                                };
                                let selected =
                                    self.state.selected_note_id.as_deref() == Some(note.id.as_str());
                                let title = truncate_chars(safe_title(&note.title), TITLE_MAX_PREVIEW_CHARS);
                                let snippet = truncate_chars(
                                    &note.body.lines().next().unwrap_or_default().replace('\t', " "),
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
                    ui.label("Create a note with + New");
                });
                return;
            };

            let selected_title = self
                .state
                .notes
                .iter()
                .find(|n| n.id == selected_id)
                .map(|n| truncate_chars(safe_title(&n.title), 64))
                .unwrap_or_else(|| "(untitled)".to_string());

            ui.horizontal(|ui| {
                ui.heading(selected_title);
                ui.separator();
                if ui.button("Delete").clicked() {
                    self.delete_selected_note();
                }
            });

            ui.horizontal(|ui| {
                ui.label("Tags:");
                let tags_resp = ui.add_sized(
                    [ui.available_width(), 26.0],
                    egui::TextEdit::singleline(&mut self.editor_tags).hint_text("work idea rust"),
                );
                if tags_resp.changed() {
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
            let edit_resp = ui.add_sized(
                [available.x, editor_height],
                egui::TextEdit::multiline(&mut self.editor_body)
                    .hint_text("Write your memo...")
                    .desired_width(f32::INFINITY),
            );
            if edit_resp.changed() {
                self.mark_dirty();
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

fn make_empty_note() -> WebNote {
    let now = now_millis();
    let id = format!("n{now}-{}", NOTE_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    WebNote {
        id,
        title: String::new(),
        body: String::new(),
        tags: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
    }
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
