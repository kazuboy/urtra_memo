use crate::model::safe_title;
use crate::{AppPaths, AppState, AppStateStore, AutosaveCoordinator, NoteStore, SortOrder};
use anyhow::Result;
use arboard::Clipboard;
use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

const ICON_FAMILY_NAME: &str = "windows-icons";
const ICON_ADD: &str = "\u{E710}";
const ICON_DOCS: &str = "\u{E8A5}";
const ICON_TRASH: &str = "\u{E74D}";
const ICON_RESTORE: &str = "\u{E777}";
const ICON_MENU: &str = "\u{E700}";
const SPECIAL_CLIPBOARD_NOTE_ID: &str = "__special_clipboard_history__";
const SPECIAL_IMAGE_CLIPBOARD_NOTE_ID: &str = "__special_image_clipboard_history__";
// Search input debounce and clipboard polling tune UI responsiveness.
const SEARCH_DEBOUNCE_MS: u64 = 120;
const CLIPBOARD_POLL_MS: u64 = 250;
const CLIPBOARD_IMAGE_POLL_MS: u64 = 900;
const CLIPBOARD_HISTORY_MAX_ITEMS: usize = 400;
const CLIPBOARD_ITEM_MAX_CHARS: usize = 8000;
const IMAGE_CLIPBOARD_HISTORY_MAX_ITEMS: usize = 300;
const IMAGE_THUMB_MAX_EDGE: usize = 224;
const IMAGE_THUMB_CACHE_CAP: usize = 96;
// List/search caps keep UI and memory usage bounded on large datasets.
const LIST_FETCH_LIMIT: usize = 200;
const SEARCH_FETCH_LIMIT: usize = 120;
const SEARCH_FETCH_LIMIT_SHORT_QUERY: usize = 40;
const SEARCH_CACHE_CAP: usize = 48;
const UI_ZOOM_MIN: f32 = 0.85;
const UI_ZOOM_MAX: f32 = 1.35;
const WINDOW_SMALL_SIZE: [f32; 2] = [840.0, 560.0];
const WINDOW_MEDIUM_SIZE: [f32; 2] = [960.0, 620.0];
const FONT_CANDIDATES: [(&str, &str, &str); 3] = [
    (
        "yu-gothic-ui",
        "Yu Gothic UI",
        r"C:\Windows\Fonts\YuGothM.ttc",
    ),
    ("meiryo", "Meiryo", r"C:\Windows\Fonts\meiryo.ttc"),
    ("msgothic", "MS Gothic", r"C:\Windows\Fonts\msgothic.ttc"),
];
const TEXT_COLOR_PRESETS: [(&str, [u8; 3]); 6] = [
    ("黒", [32, 32, 32]),
    ("濃灰", [64, 64, 64]),
    ("青", [33, 74, 128]),
    ("緑", [26, 104, 71]),
    ("茶", [116, 78, 36]),
    ("赤", [136, 40, 40]),
];
const BG_COLOR_PRESETS: [(&str, [u8; 3]); 5] = [
    ("標準", [245, 245, 246]),
    ("白", [252, 252, 252]),
    ("セピア", [245, 236, 221]),
    ("薄灰", [236, 238, 241]),
    ("薄青", [232, 240, 247]),
];

mod app_actions;
mod app_clipboard;
mod app_list;
mod app_panels;
mod app_runtime;
mod app_search;
mod clipboard;
mod markdown;
mod panel_center;
mod search_worker;
mod types;
mod ui_style;

use self::clipboard::{
    load_clipboard_history, load_image_clipboard_history, normalize_clipboard_text,
    save_image_clipboard_history,
};
use self::search_worker::{spawn_search_worker, SearchWorkerRequest, SearchWorkerResponse};
use self::types::{ClipboardHistoryEntry, ImageClipboardEntry, ListItem, PerfStats};
use self::ui_style::{
    clamp_zoom_pct, load_app_icon_data, setup_visuals_and_fonts, zoom_from_pct, zoom_to_pct,
};

pub fn run_gui(paths: AppPaths) -> Result<()> {
    let state_store = AppStateStore::new(paths.state_path.clone());
    let state = state_store.load()?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([state.window.width as f32, state.window.height as f32])
        .with_position([state.window.x as f32, state.window.y as f32]);
    if let Some(icon) = load_app_icon_data() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let title = "Ultra Memo";
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            setup_visuals_and_fonts(
                &cc.egui_ctx,
                &state.ui_font_family,
                state.ui_text_color_rgb,
                state.ui_background_color_rgb,
            );
            let app = MemoGuiApp::new(paths, state).unwrap_or_else(MemoGuiApp::new_fallback);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("failed to start gui: {e}"))?;
    Ok(())
}

struct MemoGuiApp {
    paths: Option<AppPaths>,
    store: Option<NoteStore>,
    state_store: Option<AppStateStore>,
    app_state: AppState,
    last_saved_state: AppState,
    last_state_save_at: Instant,
    autosave: AutosaveCoordinator,
    search_query: String,
    pending_restore_note_id: Option<String>,
    selected_note_id: Option<String>,
    selected_note_deleted: bool,
    editor_body: String,
    editor_tags: String,
    status_line: String,
    focus_search_requested: bool,
    show_trash: bool,
    show_recent: bool,
    show_menu: bool,
    show_tag_editor: bool,
    tag_editor_note_id: Option<String>,
    tag_editor_input: String,
    list_items: Vec<ListItem>,
    list_last_query: String,
    list_last_trash: bool,
    list_last_recent: bool,
    list_sort: SortOrder,
    list_last_sort: SortOrder,
    search_debounce_until: Option<Instant>,
    search_cache: HashMap<String, Vec<ListItem>>,
    search_cache_order: VecDeque<String>,
    search_tx: Option<Sender<SearchWorkerRequest>>,
    search_rx: Option<Receiver<SearchWorkerResponse>>,
    search_next_seq: u64,
    search_inflight: Option<(u64, String, u64)>,
    search_ready: Option<SearchWorkerResponse>,
    search_generation: u64,
    search_deleted_ids: Vec<String>,
    search_deleted_ids_generation: u64,
    rebuild_index_rx: Option<Receiver<std::result::Result<(), String>>>,
    rebuild_index_started_at: Option<Instant>,
    list_dirty: bool,
    open_ms: f64,
    list_perf: PerfStats,
    search_perf: PerfStats,
    show_perf_line: bool,
    ui_zoom: f32,
    appearance_dirty: bool,
    reset_layout_once: bool,
    markdown_render_mode: bool,
    focus_mode: bool,
    always_on_top: bool,
    export_path: String,
    import_path: String,
    delete_armed_for: Option<String>,
    purge_armed_for: Option<String>,
    purge_all_trash_armed: bool,
    clipboard_history_path: PathBuf,
    clipboard_history: Vec<ClipboardHistoryEntry>,
    clipboard_last_seen: Option<String>,
    clipboard_last_poll_at: Instant,
    clipboard_image_history_path: PathBuf,
    clipboard_image_dir: PathBuf,
    clipboard_image_history: Vec<ImageClipboardEntry>,
    clipboard_image_last_seen_hash: Option<u64>,
    clipboard_image_last_poll_at: Instant,
    image_thumb_cache: HashMap<String, egui::TextureHandle>,
    image_thumb_order: VecDeque<String>,
    clipboard: Option<Clipboard>,
}

impl MemoGuiApp {
    fn new(paths: AppPaths, mut state: AppState) -> Result<Self> {
        let open_start = Instant::now();
        let store = NoteStore::open(paths.clone())?;
        let open_ms = open_start.elapsed().as_secs_f64() * 1000.0;
        let pending_restore_note_id = state.last_open_note_id.clone();
        state.last_open_note_id = None;
        let mut show_trash = state.show_trash;
        let mut show_recent = state.show_recent;
        if show_trash {
            show_recent = false;
        } else if !show_recent {
            show_recent = true;
            show_trash = false;
        }
        let ui_zoom_pct = clamp_zoom_pct(state.ui_zoom_pct);
        let ui_zoom = zoom_from_pct(ui_zoom_pct);
        let export_path = default_export_path(&paths.root);
        let import_path = default_import_path(&paths.root);
        let clipboard_history_path = paths.clipboard_history_path();
        let clipboard_history = load_clipboard_history(&clipboard_history_path);
        let clipboard_image_history_path = paths.clipboard_image_history_path();
        let clipboard_image_dir = paths.clipboard_images_dir();
        let mut clipboard_image_history =
            load_image_clipboard_history(&clipboard_image_history_path);
        let missing_files: Vec<String> = clipboard_image_history
            .iter()
            .filter(|entry| !clipboard_image_dir.join(&entry.file_name).exists())
            .map(|entry| entry.id.clone())
            .collect();
        if !missing_files.is_empty() {
            clipboard_image_history.retain(|entry| !missing_files.iter().any(|id| id == &entry.id));
            let _ = save_image_clipboard_history(
                &clipboard_image_history_path,
                &clipboard_image_history,
            );
        }
        let mut clipboard_last_seen = clipboard_history.first().map(|item| item.text.clone());
        let clipboard_image_last_seen_hash = clipboard_image_history.first().map(|item| item.hash);
        let mut clipboard = Clipboard::new().ok();
        if let Some(handle) = &mut clipboard {
            if let Ok(text) = handle.get_text() {
                if let Some(normalized) = normalize_clipboard_text(&text) {
                    clipboard_last_seen = Some(normalized);
                }
            }
        }
        state.ui_zoom_pct = ui_zoom_pct;
        state.show_recent = show_recent;
        state.show_trash = show_trash;
        let (search_tx, search_rx) = spawn_search_worker(paths.clone());

        Ok(Self {
            paths: Some(paths.clone()),
            store: Some(store),
            state_store: Some(AppStateStore::new(paths.state_path)),
            app_state: state.clone(),
            last_saved_state: state.clone(),
            last_state_save_at: Instant::now(),
            autosave: AutosaveCoordinator::new(Duration::from_millis(900)),
            search_query: state.last_query.unwrap_or_default(),
            pending_restore_note_id,
            selected_note_id: None,
            selected_note_deleted: false,
            editor_body: String::new(),
            editor_tags: String::new(),
            status_line: "ready".to_string(),
            focus_search_requested: false,
            show_trash,
            show_recent,
            show_menu: false,
            show_tag_editor: false,
            tag_editor_note_id: None,
            tag_editor_input: String::new(),
            list_items: Vec::new(),
            list_last_query: String::new(),
            list_last_trash: false,
            list_last_recent: true,
            list_sort: state.list_sort,
            list_last_sort: state.list_sort,
            search_debounce_until: None,
            search_cache: HashMap::new(),
            search_cache_order: VecDeque::new(),
            search_tx: Some(search_tx),
            search_rx: Some(search_rx),
            search_next_seq: 1,
            search_inflight: None,
            search_ready: None,
            search_generation: 1,
            search_deleted_ids: Vec::new(),
            search_deleted_ids_generation: 0,
            rebuild_index_rx: None,
            rebuild_index_started_at: None,
            list_dirty: true,
            open_ms,
            list_perf: PerfStats::new(120),
            search_perf: PerfStats::new(120),
            show_perf_line: state.show_perf_line,
            ui_zoom,
            appearance_dirty: true,
            reset_layout_once: true,
            markdown_render_mode: state.markdown_render_mode,
            focus_mode: state.focus_mode,
            always_on_top: state.always_on_top,
            export_path,
            import_path,
            delete_armed_for: None,
            purge_armed_for: None,
            purge_all_trash_armed: false,
            clipboard_history_path,
            clipboard_history,
            clipboard_last_seen,
            clipboard_last_poll_at: Instant::now(),
            clipboard_image_history_path,
            clipboard_image_dir,
            clipboard_image_history,
            clipboard_image_last_seen_hash,
            clipboard_image_last_poll_at: Instant::now(),
            image_thumb_cache: HashMap::new(),
            image_thumb_order: VecDeque::new(),
            clipboard,
        })
    }

    fn new_fallback(err: anyhow::Error) -> Self {
        Self {
            paths: None,
            store: None,
            state_store: None,
            app_state: AppState::default(),
            last_saved_state: AppState::default(),
            last_state_save_at: Instant::now(),
            autosave: AutosaveCoordinator::new(Duration::from_millis(900)),
            search_query: String::new(),
            pending_restore_note_id: None,
            selected_note_id: None,
            selected_note_deleted: false,
            editor_body: String::new(),
            editor_tags: String::new(),
            status_line: format!("failed to initialize app: {err}"),
            focus_search_requested: false,
            show_trash: false,
            show_recent: true,
            show_menu: false,
            show_tag_editor: false,
            tag_editor_note_id: None,
            tag_editor_input: String::new(),
            list_items: Vec::new(),
            list_last_query: String::new(),
            list_last_trash: false,
            list_last_recent: true,
            list_sort: SortOrder::UpdatedDesc,
            list_last_sort: SortOrder::UpdatedDesc,
            search_debounce_until: None,
            search_cache: HashMap::new(),
            search_cache_order: VecDeque::new(),
            search_tx: None,
            search_rx: None,
            search_next_seq: 1,
            search_inflight: None,
            search_ready: None,
            search_generation: 1,
            search_deleted_ids: Vec::new(),
            search_deleted_ids_generation: 0,
            rebuild_index_rx: None,
            rebuild_index_started_at: None,
            list_dirty: true,
            open_ms: 0.0,
            list_perf: PerfStats::new(120),
            search_perf: PerfStats::new(120),
            show_perf_line: true,
            ui_zoom: 1.0,
            appearance_dirty: true,
            reset_layout_once: true,
            markdown_render_mode: false,
            focus_mode: false,
            always_on_top: false,
            export_path: String::new(),
            import_path: String::new(),
            delete_armed_for: None,
            purge_armed_for: None,
            purge_all_trash_armed: false,
            clipboard_history_path: PathBuf::new(),
            clipboard_history: Vec::new(),
            clipboard_last_seen: None,
            clipboard_last_poll_at: Instant::now(),
            clipboard_image_history_path: PathBuf::new(),
            clipboard_image_dir: PathBuf::new(),
            clipboard_image_history: Vec::new(),
            clipboard_image_last_seen_hash: None,
            clipboard_image_last_poll_at: Instant::now(),
            image_thumb_cache: HashMap::new(),
            image_thumb_order: VecDeque::new(),
            clipboard: Clipboard::new().ok(),
        }
    }

    fn save_state_if_due(&mut self, force: bool) {
        let Some(store) = &self.state_store else {
            return;
        };
        self.app_state.last_open_note_id = self.selected_note_id.clone();
        self.app_state.last_query = if self.search_query.trim().is_empty() {
            None
        } else {
            Some(self.search_query.clone())
        };
        self.app_state.ui_zoom_pct = zoom_to_pct(self.ui_zoom);
        self.app_state.show_perf_line = self.show_perf_line;
        self.app_state.show_recent = self.show_recent;
        self.app_state.show_trash = self.show_trash;
        self.app_state.list_sort = self.list_sort;
        self.app_state.markdown_render_mode = self.markdown_render_mode;
        self.app_state.focus_mode = self.focus_mode;
        self.app_state.always_on_top = self.always_on_top;

        let changed = self.app_state != self.last_saved_state;
        if !changed {
            return;
        }
        let min_interval = Duration::from_millis(700);
        if !force && self.last_state_save_at.elapsed() < min_interval {
            return;
        }
        if let Err(err) = store.save(&self.app_state) {
            self.status_line = format!("状態保存エラー: {err}");
            return;
        }
        self.last_saved_state = self.app_state.clone();
        self.last_state_save_at = Instant::now();
    }

    fn flush_pending_now(&mut self) {
        let result = if let Some(store) = &mut self.store {
            self.autosave.flush_now(store)
        } else {
            Ok(None)
        };
        match result {
            Ok(Some(note)) => {
                self.clear_recovery_draft(&note.id);
                self.clear_search_cache();
                self.list_dirty = true;
            }
            Ok(None) => {}
            Err(err) => {
                self.status_line = format!("自動保存に失敗: {err}");
            }
        }
    }

    fn try_autosave(&mut self) {
        let result = if let Some(store) = &mut self.store {
            self.autosave.flush_due(store)
        } else {
            Ok(None)
        };
        match result {
            Ok(Some(note)) => {
                self.clear_recovery_draft(&note.id);
                self.clear_search_cache();
                self.list_dirty = true;
                self.status_line = format!(
                    "{} を {} に自動保存",
                    safe_title(&note.title),
                    note.updated_at.format("%H:%M:%S")
                );
            }
            Ok(None) => {}
            Err(err) => {
                self.status_line = format!("自動保存に失敗: {err}");
            }
        }
    }

    fn read_recovery_draft(&self, note_id: &str) -> Option<String> {
        let store = self.store.as_ref()?;
        let path = store.paths().draft_path(note_id);
        let body = fs::read_to_string(path).ok()?;
        if body.is_empty() {
            None
        } else {
            Some(body)
        }
    }

    fn write_recovery_draft(&mut self, note_id: &str, body: &str) {
        let Some(store) = &self.store else {
            return;
        };
        let path = store.paths().draft_path(note_id);
        let temp_path = path.with_extension("tmp");
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                self.status_line = format!("下書き保存に失敗: {err}");
                return;
            }
        }
        if let Err(err) = fs::write(&temp_path, body) {
            self.status_line = format!("下書き保存に失敗: {err}");
            return;
        }
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        if let Err(err) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            self.status_line = format!("下書き保存に失敗: {err}");
        }
    }

    fn clear_recovery_draft(&mut self, note_id: &str) {
        let Some(store) = &self.store else {
            return;
        };
        let path = store.paths().draft_path(note_id);
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }

    fn apply_pending_appearance(&mut self, ctx: &egui::Context) {
        if !self.appearance_dirty {
            return;
        }
        setup_visuals_and_fonts(
            ctx,
            &self.app_state.ui_font_family,
            self.app_state.ui_text_color_rgb,
            self.app_state.ui_background_color_rgb,
        );
        self.appearance_dirty = false;
    }

    fn reset_layout_if_needed(&mut self, ctx: &egui::Context) {
        if !self.reset_layout_once {
            return;
        }
        ctx.memory_mut(|mem| mem.reset_areas());
        self.reset_layout_once = false;
    }
}

impl eframe::App for MemoGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_pending_appearance(ctx);
        self.reset_layout_if_needed(ctx);
        self.try_autosave();
        self.poll_clipboard_history();
        self.poll_search_worker();
        self.poll_rebuild_search_index();
        self.restore_last_note_if_needed();
        ctx.set_zoom_factor(self.ui_zoom);
        self.apply_window_level(ctx);
        self.schedule_repaint_if_needed(ctx);
        self.process_shortcuts(ctx);
        self.refresh_list_if_needed();
        self.draw_left_panel(ctx);
        self.draw_central_panel(ctx);
        self.draw_focus_mode_button(ctx);
        self.draw_menu_window(ctx);
        self.draw_tag_editor_window(ctx);
        self.sync_window_state(ctx);
        self.save_state_if_due(false);
    }
}

impl Drop for MemoGuiApp {
    fn drop(&mut self) {
        if let Some(store) = &mut self.store {
            if let Ok(Some(note)) = self.autosave.flush_now(store) {
                self.clear_recovery_draft(&note.id);
            }
        }
        self.save_state_if_due(true);
    }
}

fn clamp_list_title(title: &str) -> String {
    let max = 24usize;
    let count = title.chars().count();
    if count <= max {
        return title.to_string();
    }
    let mut out: String = title.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn default_export_path(root: &Path) -> String {
    root.join("exports")
        .join("ultra-memo-export.json")
        .display()
        .to_string()
}

fn default_import_path(root: &Path) -> String {
    root.join("imports")
        .join("ultra-memo-import.json")
        .display()
        .to_string()
}
