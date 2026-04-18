//! Ultra Memo core library.

pub mod model;

#[cfg(not(target_arch = "wasm32"))]
pub mod autosave;
#[cfg(not(target_arch = "wasm32"))]
pub mod gui;
#[cfg(not(target_arch = "wasm32"))]
pub mod note_store;
#[cfg(not(target_arch = "wasm32"))]
pub mod paths;
#[cfg(not(target_arch = "wasm32"))]
pub mod state_store;
#[cfg(target_arch = "wasm32")]
pub mod wasm_app;

/// Shared data models.
pub use model::{AppState, Note, NoteSummary, SearchResult, SortOrder, WindowState};

#[cfg(not(target_arch = "wasm32"))]
/// Debounced autosave scheduler for editor changes.
pub use autosave::AutosaveCoordinator;
#[cfg(not(target_arch = "wasm32"))]
/// Launches the native GUI app.
pub use gui::run_gui;
#[cfg(not(target_arch = "wasm32"))]
/// Persistent note storage and search operations.
pub use note_store::NoteStore;
#[cfg(not(target_arch = "wasm32"))]
/// Resolved filesystem paths for app data.
pub use paths::AppPaths;
#[cfg(not(target_arch = "wasm32"))]
/// JSON-backed app state persistence.
pub use state_store::AppStateStore;
#[cfg(target_arch = "wasm32")]
/// Launches the web app.
pub use wasm_app::run_web;
