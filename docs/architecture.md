# 超軽量メモアプリ アーキテクチャ（MVP）

## 1. レイヤ構成

- `main.rs`  
  CLI エントリポイント。ユースケース実行と状態更新を担当。

- `note_store.rs`  
  メモの作成・更新・検索・一覧・削除復元・日付メモ・最近使ったメモを扱う中核層。

- `state_store.rs`  
  `state.json` の読み書き（最終表示メモ、ウィンドウ状態、検索語）。

- `paths.rs`  
  データ保存先パス管理（`notes/`, `meta.sqlite3`, `state.json`）。

- `autosave.rs`  
  入力停止後保存のデバウンス制御を行う補助層（UI側から利用）。

## 2. データ配置

- `<data_root>/notes/*.md`: メモ本文
- `<data_root>/meta.sqlite3`: メタデータ + FTS5
- `<data_root>/state.json`: UI 状態

## 3. SQLite スキーマ

- `notes`  
  `id`, `file_name`, `title`, `created_at`, `updated_at`, `deleted`, `is_daily`

- `note_fts` (FTS5)  
  `id`, `title`, `body`

- `recent_notes`  
  `note_id`, `opened_at`

## 4. 主要フロー

1. 新規作成  
   本文を `.md` に保存し、`notes` と `note_fts` を更新。
2. 編集  
   `.md` 更新後、`notes.updated_at` と `note_fts` を更新。
3. 検索  
   FTS5 `MATCH` で検索。構文エラー時は `LIKE` フォールバック。
4. 復元  
   起動時に `state.json` から `last_open_note_id` を読み出し可能。

## 5. 性能上の要点

- SQLite を `WAL` + `synchronous=NORMAL` で運用。
- 検索は FTS5 を優先し、メモ本文ファイルの全件走査を避ける。
- 自動保存は `autosave.rs` のデバウンスで過剰 I/O を抑制する。
