# Ultra Memo コードレビュー結果 & 修正計画

## 対象範囲

メモアプリ部分のみ（`taiko5dx.rs`, `taiko5dx_gui.rs`, 5DX 関連 CLI は対象外）

**対象ファイル**: `main.rs`, `lib.rs`, `model.rs`, `gui.rs`, `note_store.rs`, `paths.rs`, `state_store.rs`, `autosave.rs`, `Cargo.toml`

> [!NOTE]
> 全体として **非常によく書かれたプロジェクト** です。`unsafe` ブロックはゼロ、エラーハンドリングは `anyhow::Result` で一貫しています。以下は「さらに良くするための改善提案」です。

---

## レビュー結果

### 2-1. 安全性 (Safety)

| # | 重大度 | 対象箇所 | 問題内容 | 放置した場合のリスク |
|---|--------|---------|---------|-------------------|
| S1 | 中 | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L2443-L2448) `hash_clipboard_image` | `DefaultHasher` は **安定性の保証なし**。画像の同一判定には不向き。 | ハッシュ衝突で**新画像が保存されない**可能性。起動ごとにハッシュが変わり、同じ画像が二重登録され得る。 |
| S2 | 低 | [note_store.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/note_store.rs#L767) `search_notes_with_like` | LIKE フォールバックで `%{query}%` を直接展開。 | `query` に `%` や `_` が含まれると**意図しないワイルドカード一致**。 |

### 2-2. 堅牢性 (Robustness)

| # | 重大度 | 対象箇所 | 問題内容 | 放置した場合のリスク |
|---|--------|---------|---------|-------------------|
| R1 | **高** | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L2611-L2620) `load_deleted_id_set` | 検索ワーカースレッド内で**毎回 `NoteStore::open()` を実行**し新たに SQLite 接続を開く。`list_deleted_notes(1_000_000)` も重い。 | 大量メモ時に**検索ワーカーが遅延**、DB ロック競合もあり得る。 |
| R2 | 中 | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L102-L159) `MemoGuiApp` | **35+ フィールド**の巨大構造体。`new_fallback` のフィールド列挙が手動で管理困難。 | フィールド追加時に `new_fallback` の同期漏れ → **コンパイルは通るが初期値が不正**。 |
| R3 | 中 | [state_store.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/state_store.rs#L31-L34) `save` | `remove_file` → `rename` の 2 ステップ。間にクラッシュすると**ファイル消失**。 | 電源断時に状態ファイルや下書きが失われる。 |
| R4 | 中 | [note_store.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/note_store.rs#L944-L965) `ensure_note_fts_schema` | FTS テーブルを DROP して再作成後 `rebuild_index()` を呼ぶ。 | 1 万メモ超で**初回起動が数十秒〜数分**かかる。進捗表示もなし。 |
| R5 | 低 | [model.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/model.rs#L141) / [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L2464) | `truncate_chars` が **2 箇所に重複**。挙動も微妙に異なる（`...` vs `…`）。 | 修正漏れやバグの温床。 |

### 2-3. パフォーマンス (Performance)

| # | 重大度 | 対象箇所 | 問題内容 |
|---|--------|---------|---------|
| P1 | 中 | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L1864) クリップ履歴表示 | `editor_body.clone()` で読み取り専用テキスト表示時に**毎フレーム不要な String クローン**。 |
| P2 | 中 | [note_store.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/note_store.rs#L1104-L1120) `normalize_snippet` | `chars().count()` を **2 回呼んでいる**（L1116 で再カウント）。マルチバイト文字列では線形スキャン 2 倍。 |
| P3 | 低 | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs#L2569-L2608) `load_markdown_search_corpus` | 全 `.md` ファイルを読み込み `to_lowercase()` で丸ごと小文字化。大量メモ時にメモリ圧迫。 |

### 2-4. 保守性 (Maintainability)

| # | 重大度 | 対象箇所 | 問題内容 |
|---|--------|---------|---------|
| M1 | **高** | [gui.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/gui.rs) (2,890 行) | **500 行の目安を大幅超過**。GUI・クリップボード・検索ワーカー・フォント管理がすべて 1 ファイル。 |
| M2 | 中 | [model.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/model.rs#L27-L45) `NoteSummary` / `SearchResult` | **フィールドがほぼ同一**。共通化の余地あり。 |
| M3 | 中 | [model.rs](file:///c:/Users/user/.gemini/antigravity/scratch/.agent/project/Rust/src/model.rs#L54) `sql_order_by` | モデル層に **SQL 断片が混入**。`note_store.rs` 側で組み立てるべき。 |
| M4 | 低 | テスト | `model.rs` の `derive_title` / `truncate_chars` にユニットテストなし。 |
| M5 | 低 | ドキュメント | `pub` アイテムに `///` ドキュメントコメントがほぼ皆無。 |

### 2-5. スタイル (Style)

| # | 重大度 | 対象箇所 | 問題内容 |
|---|--------|---------|---------|
| Y1 | 低 | `main.rs` L875 / `gui.rs` L2179 | `safe_title` 関数が**2 箇所に重複**定義。 |
| Y2 | 低 | `Cargo.toml` L4 | `edition = "2024"` は nightly。安定リリースでは `2021` が推奨。 |
| Y3 | 低 | `gui.rs` L29-38 | 一部の定数に**説明コメントがない**。 |

---

## 修正計画（優先度順）

| # | 対象箇所 | 重大度 | 修正方針 | 影響範囲 | 検証方法 |
|---|---------|:------:|---------|---------|---------|
| R1 | `gui.rs` `load_deleted_id_set` | 高 | 削除 ID セットをメインスレッドからチャネル経由で渡し、DB 再オープンを廃止 | `gui.rs` 検索関連 | `cargo test`, 検索パフォーマンステスト |
| M1 | `gui.rs` 2,890行 | 高 | `gui/` ディレクトリに分割（`app.rs`, `clipboard.rs`, `search_worker.rs`, `fonts.rs`, `markdown.rs`） | `lib.rs`, `gui.rs` | `cargo check`, `cargo clippy`, `cargo test` |
| R3 | `state_store.rs` `save` | 中 | アトミック書き込みのエラー復旧を改善 | `state_store.rs`, `gui.rs` | `cargo test` |
| S1 | `gui.rs` `hash_clipboard_image` | 中 | 安定したハッシュ（FNV or `ahash`）に変更 | `gui.rs` | 手動確認 |
| S2 | `note_store.rs` LIKE フォールバック | 低 | `%` と `_` をエスケープ | `note_store.rs` | テスト追加 |
| P2 | `note_store.rs` `normalize_snippet` | 中 | `chars().count()` 結果をキャッシュ | `note_store.rs` | `cargo test` |
| R5 | `truncate_chars` 重複 | 低 | `model.rs` に統一 | `gui.rs`, `model.rs` | `cargo test` |
| Y1 | `safe_title` 重複 | 低 | `model.rs` に移動して共有 | `main.rs`, `gui.rs` | `cargo check` |
| M2 | `NoteSummary`/`SearchResult` | 低 | 型統合 or type alias | `model.rs`, `note_store.rs` | `cargo check`, `cargo test` |
| M3 | `sql_order_by` | 低 | `note_store.rs` に移動 | `model.rs`, `note_store.rs` | `cargo check` |
| M4 | テスト追加 | 低 | `derive_title`, `truncate_chars` のテスト追加 | `model.rs` | `cargo test` |

---

## 次のステップ

1. このレビュー結果を確認してください
2. 修正したい項目を教えてください（重大度：高 の R1, M1 から着手を推奨）
3. 許可をいただいた後、段階的にコードを修正します
