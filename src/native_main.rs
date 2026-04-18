use anyhow::{bail, Result};
use chrono::Local;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Instant;
use ultra_memo::model::safe_title;
use ultra_memo::{run_gui, AppPaths, AppStateStore, Note, NoteStore, SortOrder};

#[derive(Parser)]
#[command(name = "ultra-memo", about = "Ultra-light local memo app MVP CLI")]
struct Cli {
    #[arg(long)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Gui,
    Perf {
        query: String,
        #[arg(long, default_value_t = 30)]
        iterations: usize,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    PerfStartup {
        #[arg(long, default_value_t = 30)]
        iterations: usize,
    },
    Seed {
        #[arg(long, default_value_t = 1000)]
        count: usize,
        #[arg(long, default_value_t = 4)]
        lines: usize,
    },
    New {
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        stdin: bool,
    },
    Edit {
        id: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        stdin: bool,
    },
    Open {
        id: String,
    },
    Resume,
    List {
        #[arg(long, value_enum, default_value_t = SortArg::Updated)]
        sort: SortArg,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = false)]
        include_deleted: bool,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Delete {
        id: String,
    },
    Restore {
        id: String,
    },
    Purge {
        id: String,
    },
    Today,
    Recent {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    RebuildIndex,
    Export {
        path: PathBuf,
    },
    Import {
        path: PathBuf,
    },
    State {
        #[arg(long, default_value_t = false)]
        show: bool,
    },
}

#[derive(Copy, Clone, Eq, PartialEq, ValueEnum)]
enum SortArg {
    Updated,
    Created,
}

impl SortArg {
    fn into_sort_order(self) -> SortOrder {
        match self {
            SortArg::Updated => SortOrder::UpdatedDesc,
            SortArg::Created => SortOrder::CreatedDesc,
        }
    }
}

pub fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = if let Some(data_dir) = cli.data_dir {
        AppPaths::from_root(data_dir)
    } else {
        AppPaths::default_user()?
    };

    let command = cli.command.unwrap_or(Command::Gui);

    match command {
        Command::Gui => run_gui(paths),
        Command::Perf {
            query,
            iterations,
            limit,
        } => run_perf(paths, &query, iterations, limit),
        Command::PerfStartup { iterations } => run_perf_startup(paths, iterations),
        command => run_data_command(paths, command),
    }
}

fn run_data_command(paths: AppPaths, command: Command) -> Result<()> {
    let mut store = NoteStore::open(paths.clone())?;
    let state_store = AppStateStore::new(paths.state_path.clone());
    let mut state = state_store.load()?;

    match command {
        Command::Gui | Command::Perf { .. } | Command::PerfStartup { .. } => {
            unreachable!("gui/perf/perf-startup are handled before run_data_command");
        }
        Command::Seed { count, lines } => {
            let count = count.max(1);
            let lines = lines.max(1);
            let mut last_note_id = None;
            for i in 0..count {
                let body = make_seed_body(i, lines);
                let note = store.create_note(&body)?;
                last_note_id = Some(note.id);
                if (i + 1) % 500 == 0 {
                    println!("seeded: {} notes", i + 1);
                }
            }
            state.last_open_note_id = last_note_id;
            println!("seed completed: {count} notes");
        }
        Command::New { body, stdin } => {
            let body = resolve_body_for_new(body, stdin)?;
            let note = store.create_note(&body)?;
            state.last_open_note_id = Some(note.id.clone());
            print_note(&note);
        }
        Command::Edit { id, body, stdin } => {
            let body = resolve_body_for_edit(body, stdin)?;
            let note = store.update_note(&id, &body)?;
            state.last_open_note_id = Some(note.id.clone());
            print_note(&note);
        }
        Command::Open { id } => {
            let note = store.load_note(&id)?;
            state.last_open_note_id = Some(note.id.clone());
            print_note(&note);
        }
        Command::Resume => {
            let id = state
                .last_open_note_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("no last_open_note_id in state"))?;
            let note = store.load_note(&id)?;
            state.last_open_note_id = Some(note.id.clone());
            print_note(&note);
        }
        Command::List {
            sort,
            limit,
            include_deleted,
        } => {
            let notes = store.list_notes(sort.into_sort_order(), limit, include_deleted)?;
            for note in notes {
                println!(
                    "{}\t{}\t{}",
                    note.id,
                    note.updated_at.to_rfc3339(),
                    safe_title(&note.title)
                );
                println!("  {}", note.snippet);
            }
        }
        Command::Search { query, limit } => {
            let hits = store.search_notes(&query, limit)?;
            state.last_query = Some(query);
            for hit in hits {
                println!(
                    "{}\t{}\t{}",
                    hit.id,
                    hit.updated_at.to_rfc3339(),
                    safe_title(&hit.title)
                );
                println!("  {}", hit.snippet);
            }
        }
        Command::Delete { id } => {
            store.soft_delete_note(&id)?;
            if state.last_open_note_id.as_deref() == Some(id.as_str()) {
                state.last_open_note_id = None;
            }
            println!("deleted: {id}");
        }
        Command::Restore { id } => {
            store.restore_note(&id)?;
            println!("restored: {id}");
        }
        Command::Purge { id } => {
            store.purge_note(&id)?;
            if state.last_open_note_id.as_deref() == Some(id.as_str()) {
                state.last_open_note_id = None;
            }
            println!("purged: {id}");
        }
        Command::Today => {
            let today = Local::now().date_naive();
            let note = store.create_or_open_daily_note(today)?;
            state.last_open_note_id = Some(note.id.clone());
            print_note(&note);
        }
        Command::Recent { limit } => {
            let notes = store.list_recent(limit)?;
            for note in notes {
                println!(
                    "{}\t{}\t{}",
                    note.id,
                    note.updated_at.to_rfc3339(),
                    safe_title(&note.title)
                );
                println!("  {}", note.snippet);
            }
        }
        Command::RebuildIndex => {
            store.rebuild_index()?;
            println!("search index rebuilt");
        }
        Command::Export { path } => {
            let count = store.export_to_path(&path, state.last_open_note_id.as_deref())?;
            println!("exported notes: {count}");
            println!("path: {}", path.display());
        }
        Command::Import { path } => {
            let result = store.import_from_path(&path)?;
            println!(
                "imported: created={}, updated={}, skipped={}",
                result.created, result.updated, result.skipped
            );
            println!("path: {}", path.display());
        }
        Command::State { show } => {
            if show {
                let json = serde_json::to_string_pretty(&state)?;
                println!("{json}");
            } else {
                println!("state saved at {}", paths.state_path.display());
            }
        }
    }

    state_store.save(&state)?;
    Ok(())
}

fn run_perf(paths: AppPaths, query: &str, iterations: usize, limit: usize) -> Result<()> {
    if query.trim().is_empty() {
        bail!("query must not be empty");
    }
    let iterations = iterations.max(1);

    let open_start = Instant::now();
    let store = NoteStore::open(paths)?;
    let open_ms = open_start.elapsed().as_secs_f64() * 1000.0;

    let mut list_samples = Vec::with_capacity(iterations);
    let mut search_samples = Vec::with_capacity(iterations);
    let mut list_count = 0usize;
    let mut hit_count = 0usize;

    for i in 0..iterations {
        let t0 = Instant::now();
        let listed = store.list_notes(SortOrder::UpdatedDesc, limit, false)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        list_samples.push(ms);
        if i == 0 {
            list_count = listed.len();
        }
    }

    for i in 0..iterations {
        let t0 = Instant::now();
        let hits = store.search_notes(query, limit)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        search_samples.push(ms);
        if i == 0 {
            hit_count = hits.len();
        }
    }

    let (list_min, list_avg, list_p95, list_max) = summarize_ms(&list_samples);
    let (search_min, search_avg, search_p95, search_max) = summarize_ms(&search_samples);

    println!("perf iterations: {iterations}");
    println!("limit: {limit}");
    println!("query: {query}");
    println!("open_ms: {:.3}", open_ms);
    println!(
        "list_ms  min/avg/p95/max: {:.3} / {:.3} / {:.3} / {:.3} (count={})",
        list_min, list_avg, list_p95, list_max, list_count
    );
    println!(
        "search_ms min/avg/p95/max: {:.3} / {:.3} / {:.3} / {:.3} (hits={})",
        search_min, search_avg, search_p95, search_max, hit_count
    );
    Ok(())
}

fn run_perf_startup(paths: AppPaths, iterations: usize) -> Result<()> {
    let iterations = iterations.max(1);
    let mut state_samples = Vec::with_capacity(iterations);
    let mut open_samples = Vec::with_capacity(iterations);
    let mut resume_samples = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let state_store = AppStateStore::new(paths.state_path.clone());

        let t0 = Instant::now();
        let state = state_store.load()?;
        state_samples.push(t0.elapsed().as_secs_f64() * 1000.0);

        let t1 = Instant::now();
        let mut store = NoteStore::open(paths.clone())?;
        open_samples.push(t1.elapsed().as_secs_f64() * 1000.0);

        let t2 = Instant::now();
        if let Some(id) = state.last_open_note_id {
            let _ = store.load_note(&id);
        }
        resume_samples.push(t2.elapsed().as_secs_f64() * 1000.0);
    }

    let (state_min, state_avg, state_p95, state_max) = summarize_ms(&state_samples);
    let (open_min, open_avg, open_p95, open_max) = summarize_ms(&open_samples);
    let (resume_min, resume_avg, resume_p95, resume_max) = summarize_ms(&resume_samples);

    println!("startup perf iterations: {iterations}");
    println!(
        "state_load_ms min/avg/p95/max: {:.3} / {:.3} / {:.3} / {:.3}",
        state_min, state_avg, state_p95, state_max
    );
    println!(
        "store_open_ms min/avg/p95/max: {:.3} / {:.3} / {:.3} / {:.3}",
        open_min, open_avg, open_p95, open_max
    );
    println!(
        "resume_note_ms min/avg/p95/max: {:.3} / {:.3} / {:.3} / {:.3}",
        resume_min, resume_avg, resume_p95, resume_max
    );
    Ok(())
}

fn summarize_ms(samples: &[f64]) -> (f64, f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = sorted[0];
    let max = *sorted.last().unwrap_or(&min);
    let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let p95_index = ((sorted.len() - 1) as f64 * 0.95).round() as usize;
    let p95 = sorted[p95_index.min(sorted.len() - 1)];
    (min, avg, p95, max)
}

fn resolve_body_for_new(body: Option<String>, from_stdin: bool) -> Result<String> {
    if from_stdin {
        return read_stdin_body();
    }
    Ok(body.unwrap_or_default())
}

fn resolve_body_for_edit(body: Option<String>, from_stdin: bool) -> Result<String> {
    if from_stdin {
        return read_stdin_body();
    }
    body.ok_or_else(|| anyhow::anyhow!("--body or --stdin is required"))
}

fn read_stdin_body() -> Result<String> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    if input.trim().is_empty() {
        bail!("stdin is empty");
    }
    Ok(input)
}

fn print_note(note: &Note) {
    println!("id: {}", note.id);
    println!("title: {}", safe_title(&note.title));
    if !note.tags.is_empty() {
        println!("tags: {}", note.tags.join(" "));
    }
    println!("created_at: {}", note.created_at.to_rfc3339());
    println!("updated_at: {}", note.updated_at.to_rfc3339());
    println!("deleted: {}", note.deleted);
    println!("---");
    println!("{}", note.body);
}

fn make_seed_body(index: usize, lines: usize) -> String {
    let fragments = [
        "idea", "memo", "scene", "draft", "search", "task", "log", "note", "rust", "design",
        "story", "tag",
    ];
    let mut body = String::new();
    body.push_str(&format!("# Seed Note {:05}\n", index + 1));
    for line in 0..lines {
        let a = fragments[(index + line) % fragments.len()];
        let b = fragments[(index * 3 + line + 1) % fragments.len()];
        let c = fragments[(index * 7 + line + 2) % fragments.len()];
        body.push_str(&format!("- {a} {b} {c}\n"));
    }
    if index % 3 == 0 {
        body.push_str("rust performance incremental search\n");
    }
    body
}
