# Release Readiness

## 1. Exit Criteria

- `cargo check` が成功する
- `cargo test --lib` が成功する
- GUI で以下が動作する
  - 新規作成 / 編集 / 自動保存
  - 検索（インクリメンタル）/ 一覧選択
  - ゴミ箱 / 復元 / 完全削除（二段階確定）
  - 設定メニュー（UI倍率・計測表示）の変更と再起動復元
- クラッシュ復旧（`drafts/`）が動作する
  - 編集途中に強制終了しても次回再オープン時に下書きが復元される

## 2. Performance Baseline (Windows, 5000 notes)

計測コマンド:

```bash
cargo run -- --data-dir ./.bench-5k perf-startup --iterations 50
cargo run -- --data-dir ./.bench-5k perf rust --iterations 50 --limit 200
```

一括実行:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\release-check.ps1
```

直近の定常計測値:

- `state_load_ms` avg/p95: `0.227 / 0.361`
- `store_open_ms` avg/p95: `13.614 / 17.104`
- `resume_note_ms` avg/p95: `1.626 / 5.995`
- `list_ms` avg/p95: `11.224 / 14.090`
- `search_ms` avg/p95: `18.608 / 24.207`

補足:

- `open_ms` は実行環境やコンパイル直後の状況で揺れやすい
- インデックススキーマ変更直後の初回のみ、再構築で一時的に遅くなる

## 3. Manual GUI Checklist

1. 起動して 2 秒以内に入力可能状態になる
2. `Ctrl+N` で新規作成し、数行入力して 1 秒程度待機後に自動保存される
3. 検索欄へ入力し、候補が更新される
4. `↑/↓`（入力フォーカス外）と `Ctrl+↑/Ctrl+↓` で選択移動できる
5. `Delete` で削除二段階、ゴミ箱内で `Shift+Delete` で完全削除二段階
6. `Ctrl+,` でメニューを開き UI倍率を変更、再起動後も反映される

## 4. Data Compatibility Notes

- 旧 `state.json`（UI設定フィールドなし）を読み込んだ場合は既定値を補完する
- `note_fts` スキーマが古い場合は起動時に再生成して移行する
