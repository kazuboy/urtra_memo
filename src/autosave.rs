use crate::model::Note;
use crate::note_store::NoteStore;
use anyhow::Result;
use std::time::{Duration, Instant};

/// Coordinates debounce-based autosave for a single pending edit.
pub struct AutosaveCoordinator {
    debounce: Duration,
    pending: Option<PendingEdit>,
}

struct PendingEdit {
    note_id: String,
    body: String,
    due_at: Instant,
}

impl AutosaveCoordinator {
    /// Creates a coordinator with the given debounce window.
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            pending: None,
        }
    }

    /// Schedules an autosave for `note_id` with latest `body`.
    pub fn schedule(&mut self, note_id: impl Into<String>, body: impl Into<String>) {
        self.pending = Some(PendingEdit {
            note_id: note_id.into(),
            body: body.into(),
            due_at: Instant::now() + self.debounce,
        });
    }

    /// Returns true when an autosave is currently pending.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Flushes pending edit when debounce time has elapsed.
    pub fn flush_due(&mut self, store: &mut NoteStore) -> Result<Option<Note>> {
        let should_flush = match &self.pending {
            Some(pending) => Instant::now() >= pending.due_at,
            None => false,
        };
        if should_flush {
            return self.flush_now(store);
        }
        Ok(None)
    }

    /// Immediately flushes pending edit if it exists.
    pub fn flush_now(&mut self, store: &mut NoteStore) -> Result<Option<Note>> {
        let pending = match self.pending.take() {
            Some(value) => value,
            None => return Ok(None),
        };
        let note = store.update_note(&pending.note_id, &pending.body)?;
        Ok(Some(note))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppPaths, NoteStore};
    use tempfile::tempdir;

    #[test]
    fn autosave_flushes_pending_edit() {
        let temp = tempdir().expect("tempdir");
        let paths = AppPaths::from_root(temp.path().join("memo-data"));
        let mut store = NoteStore::open(paths).expect("open store");
        let note = store.create_note("before").expect("create");

        let mut autosave = AutosaveCoordinator::new(Duration::from_millis(0));
        autosave.schedule(note.id.clone(), "after");
        let flushed = autosave.flush_due(&mut store).expect("flush");

        let flushed_note = flushed.expect("note");
        assert_eq!(flushed_note.body, "after");
        assert!(!autosave.has_pending());
    }
}
