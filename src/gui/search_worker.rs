use crate::AppPaths;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use super::types::{ListItem, ListItemKind};

#[derive(Debug)]
pub(crate) struct SearchWorkerRequest {
    pub(crate) seq: u64,
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) limit: usize,
    pub(crate) deleted_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchWorkerResponse {
    pub(crate) seq: u64,
    pub(crate) generation: u64,
    pub(crate) query: String,
    pub(crate) items: Vec<ListItem>,
    pub(crate) elapsed_ms: f64,
    pub(crate) error: Option<String>,
}

pub(crate) fn has_tag_query(query: &str) -> bool {
    query
        .split_whitespace()
        .filter_map(|token| token.strip_prefix('#'))
        .map(|raw| {
            raw.trim()
                .trim_matches(|c: char| c == '#' || c == '|' || c == ',' || c == ';')
        })
        .any(|tag| !tag.is_empty())
}

pub(crate) fn spawn_search_worker(
    paths: AppPaths,
) -> (Sender<SearchWorkerRequest>, Receiver<SearchWorkerResponse>) {
    let (tx_req, rx_req) = mpsc::channel::<SearchWorkerRequest>();
    let (tx_resp, rx_resp) = mpsc::channel::<SearchWorkerResponse>();

    thread::spawn(move || {
        let mut loaded_generation = 0u64;
        let mut corpus = Vec::new();
        loop {
            let mut req = match rx_req.recv() {
                Ok(req) => req,
                Err(_) => break,
            };
            while let Ok(next) = rx_req.try_recv() {
                req = next;
            }

            let start = Instant::now();
            let response = {
                if loaded_generation != req.generation {
                    let deleted_ids: HashSet<String> = req.deleted_ids.iter().cloned().collect();
                    corpus = load_markdown_search_corpus(&paths.notes_dir, &deleted_ids);
                    loaded_generation = req.generation;
                }
                let query = req.query.clone();
                SearchWorkerResponse {
                    seq: req.seq,
                    generation: req.generation,
                    query,
                    items: search_corpus(&corpus, &req.query, req.limit),
                    elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    error: None,
                }
            };

            if tx_resp.send(response).is_err() {
                break;
            }
        }
    });

    (tx_req, rx_resp)
}

#[derive(Clone, Debug)]
struct SearchCorpusEntry {
    id: String,
    title: String,
    body_lower: String,
    updated_at: DateTime<Utc>,
}

fn load_markdown_search_corpus(
    notes_dir: &Path,
    deleted_ids: &HashSet<String>,
) -> Vec<SearchCorpusEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = fs::read_dir(notes_dir) else {
        return entries;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || deleted_ids.contains(&id) {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        let title = crate::model::derive_title(&body);
        entries.push(SearchCorpusEntry {
            id,
            title,
            body_lower: body.to_lowercase(),
            updated_at: modified,
        });
    }
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    entries
}

fn search_corpus(corpus: &[SearchCorpusEntry], query: &str, limit: usize) -> Vec<ListItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let terms: Vec<String> = q
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|s| s.to_string())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for item in corpus {
        let title_l = item.title.to_lowercase();
        let matched = terms
            .iter()
            .all(|t| title_l.contains(t) || item.body_lower.contains(t));
        if matched {
            hits.push(ListItem {
                id: item.id.clone(),
                title: item.title.clone(),
                tags: Vec::new(),
                updated_text: item.updated_at.format("%-m/%-d %H:%M").to_string(),
                deleted: false,
                kind: ListItemKind::Note,
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}
