use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub(crate) struct ListItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) updated_text: String,
    pub(crate) deleted: bool,
    pub(crate) kind: ListItemKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListItemKind {
    Note,
    ClipboardTextHistory,
    ClipboardImageHistory,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ClipboardHistoryEntry {
    pub(crate) copied_at: DateTime<Utc>,
    pub(crate) text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ClipboardHistoryFile {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) entries: Vec<ClipboardHistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImageClipboardEntry {
    pub(crate) id: String,
    pub(crate) file_name: String,
    pub(crate) copied_at: DateTime<Utc>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) byte_size: u64,
    pub(crate) hash: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ImageClipboardHistoryFile {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) entries: Vec<ImageClipboardEntry>,
}

#[derive(Clone)]
pub(crate) struct PerfStats {
    samples: VecDeque<f64>,
    cap: usize,
}

impl PerfStats {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            samples: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub(crate) fn push(&mut self, ms: f64) {
        if self.samples.len() >= self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(ms.max(0.0));
    }

    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }

    pub(crate) fn avg(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    pub(crate) fn p95(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut v = self.samples.iter().copied().collect::<Vec<_>>();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((v.len() - 1) as f64 * 0.95).round() as usize;
        v[idx.min(v.len() - 1)]
    }
}
