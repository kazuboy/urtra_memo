# 実装計画（MVP）

## 完了済み

1. `docs/product-spec.md` を作成し、MVPスコープと受け入れ基準を固定
2. Rust プロジェクト初期化
3. コア実装
   - Markdown保存
   - SQLiteメタデータ管理
   - FTS5全文検索
   - 削除/復元
   - 日付メモ
   - 最近使ったメモ
   - 状態保存
4. CLI 実装
   - `new/edit/open/list/search/delete/restore/today/recent/rebuild-index/state`
5. 単体テスト実装

## 次の実装候補

1. GUI 実装（左ペイン検索・一覧、右ペイン編集）
2. `AutosaveCoordinator` をUI入力イベントに統合
3. 起動時に `last_open_note_id` を自動オープン
4. 検索スニペット強化（ハイライト位置最適化）
5. キーボードショートカット最適化
