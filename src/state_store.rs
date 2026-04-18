use crate::model::AppState;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// Persists and restores [`AppState`] as JSON.
pub struct AppStateStore {
    path: PathBuf,
}

impl AppStateStore {
    /// Creates a state store bound to `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Loads state from primary file, falling back to backup file.
    pub fn load(&self) -> Result<AppState> {
        let backup_path = self.backup_path();
        let load_path = if self.path.exists() {
            self.path.as_path()
        } else if backup_path.exists() {
            backup_path.as_path()
        } else {
            return Ok(AppState::default());
        };
        let text = fs::read_to_string(load_path)?;
        let state = serde_json::from_str(&text)?;
        Ok(state)
    }

    /// Saves state via temp+backup swap to reduce data-loss risk.
    pub fn save(&self, state: &AppState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(state)?;
        let temp_path = self.path.with_extension("tmp");
        let backup_path = self.backup_path();
        fs::write(&temp_path, text)?;
        if self.path.exists() {
            let _ = fs::remove_file(&backup_path);
            fs::rename(&self.path, &backup_path)?;
        }
        match fs::rename(&temp_path, &self.path) {
            Ok(_) => {
                let _ = fs::remove_file(&backup_path);
            }
            Err(err) => {
                if backup_path.exists() {
                    let _ = fs::rename(&backup_path, &self.path);
                }
                let _ = fs::remove_file(&temp_path);
                return Err(err.into());
            }
        }
        Ok(())
    }

    fn backup_path(&self) -> PathBuf {
        self.path.with_extension("bak")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_and_load_state() {
        let temp = tempdir().expect("tempdir");
        let store = AppStateStore::new(temp.path().join("state.json"));
        let mut state = AppState::default();
        state.last_open_note_id = Some("note-123".to_string());
        state.last_query = Some("rust".to_string());
        state.window.width = 1400;
        state.ui_zoom_pct = 112;
        state.show_perf_line = false;
        state.show_recent = false;
        state.show_trash = true;
        state.list_sort = crate::model::SortOrder::CreatedDesc;
        state.markdown_render_mode = true;
        state.ui_font_family = "meiryo".to_string();
        state.ui_text_color_rgb = [20, 30, 40];
        state.ui_background_color_rgb = [230, 231, 232];
        state.focus_mode = true;
        state.always_on_top = true;

        store.save(&state).expect("save");
        let loaded = store.load().expect("load");

        assert_eq!(loaded.last_open_note_id, Some("note-123".to_string()));
        assert_eq!(loaded.last_query, Some("rust".to_string()));
        assert_eq!(loaded.window.width, 1400);
        assert_eq!(loaded.ui_zoom_pct, 112);
        assert!(!loaded.show_perf_line);
        assert!(!loaded.show_recent);
        assert!(loaded.show_trash);
        assert_eq!(loaded.list_sort, crate::model::SortOrder::CreatedDesc);
        assert!(loaded.markdown_render_mode);
        assert_eq!(loaded.ui_font_family, "meiryo");
        assert_eq!(loaded.ui_text_color_rgb, [20, 30, 40]);
        assert_eq!(loaded.ui_background_color_rgb, [230, 231, 232]);
        assert!(loaded.focus_mode);
        assert!(loaded.always_on_top);
    }

    #[test]
    fn load_legacy_state_uses_defaults_for_new_fields() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state.json");
        fs::write(
            &path,
            r#"{
  "last_open_note_id": "legacy-note",
  "window": { "width": 1280, "height": 720, "x": 10, "y": 20 },
  "last_query": "legacy"
}"#,
        )
        .expect("write legacy");
        let store = AppStateStore::new(path);

        let loaded = store.load().expect("load legacy");
        assert_eq!(loaded.last_open_note_id.as_deref(), Some("legacy-note"));
        assert_eq!(loaded.last_query.as_deref(), Some("legacy"));
        assert_eq!(loaded.window.width, 1280);
        assert_eq!(loaded.ui_zoom_pct, 100);
        assert!(loaded.show_perf_line);
        assert!(loaded.show_recent);
        assert!(!loaded.show_trash);
        assert_eq!(loaded.list_sort, crate::model::SortOrder::UpdatedDesc);
        assert!(!loaded.markdown_render_mode);
        assert_eq!(loaded.ui_font_family, "yu-gothic-ui".to_string());
        assert_eq!(loaded.ui_text_color_rgb, [48, 48, 48]);
        assert_eq!(loaded.ui_background_color_rgb, [245, 245, 246]);
        assert!(!loaded.focus_mode);
        assert!(!loaded.always_on_top);
    }
}
