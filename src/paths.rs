use anyhow::Result;
use directories::ProjectDirs;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
/// Canonical app storage paths resolved from user environment.
pub struct AppPaths {
    pub root: PathBuf,
    pub notes_dir: PathBuf,
    pub db_path: PathBuf,
    pub state_path: PathBuf,
}

impl AppPaths {
    /// Builds paths rooted at `root`.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            notes_dir: root.join("notes"),
            db_path: root.join("meta.sqlite3"),
            state_path: root.join("state.json"),
            root,
        }
    }

    /// Resolves default user-scoped storage path.
    pub fn default_user() -> Result<Self> {
        if let Some(project_dirs) = ProjectDirs::from("jp", "memo", "UltraLightMemo") {
            return Ok(Self::from_root(project_dirs.data_local_dir()));
        }
        let fallback = std::env::current_dir()?.join("data");
        Ok(Self::from_root(fallback))
    }

    /// Creates all required data directories.
    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(&self.notes_dir)?;
        fs::create_dir_all(self.drafts_dir())?;
        fs::create_dir_all(self.clipboard_images_dir())?;
        Ok(())
    }

    /// Returns path to a note file under `notes/`.
    pub fn note_path(&self, file_name: &str) -> PathBuf {
        self.notes_dir.join(file_name)
    }

    /// Returns drafts directory path.
    pub fn drafts_dir(&self) -> PathBuf {
        self.root.join("drafts")
    }

    /// Returns per-note crash-recovery draft path.
    pub fn draft_path(&self, note_id: &str) -> PathBuf {
        self.drafts_dir().join(format!("{note_id}.draft.md"))
    }

    /// Returns text clipboard history JSON path.
    pub fn clipboard_history_path(&self) -> PathBuf {
        self.root.join("clipboard_history.json")
    }

    /// Returns image clipboard history JSON path.
    pub fn clipboard_image_history_path(&self) -> PathBuf {
        self.root.join("clipboard_image_history.json")
    }

    /// Returns directory for persisted clipboard images.
    pub fn clipboard_images_dir(&self) -> PathBuf {
        self.root.join("clipboard_images")
    }
}
