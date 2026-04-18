# Ultra Memo

超軽量メモアプリ（Rust / egui）。

## 起動

```bash
cargo run -- gui
```

## Web (WASM) 実行

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve
```

`http://127.0.0.1:8080` で Web 版を開けます。

### Web版の仕様メモ

- データ保存: ブラウザ `localStorage`（ローカル端末内のみ）
- 検索: 起動中メモリ上で本文/タイトル/`#tag` を検索
- Markdown: 編集中テキストの簡易プレビュー表示に対応
- 非対応: SQLite インデックス、ネイティブクリップボード画像履歴、ファイルダイアログ連携

## GitHub Pages 公開

このリポジトリには Pages デプロイ workflow を追加済みです:

- `.github/workflows/deploy-pages.yml`

手順:

1. GitHub に push（`main` または `master`）
2. リポジトリ設定の `Pages` で `Build and deployment: GitHub Actions` を選択
3. Actions の `Deploy Ultra Memo Web` が成功したら公開URLにアクセス

公開URLのベースパスは自動判定されます:

- `username.github.io` リポジトリ: `/`
- それ以外のリポジトリ: `/<repo-name>/`

## 主なCLI

```bash
cargo run -- list --limit 20
cargo run -- search "keyword" --limit 20
cargo run -- today
cargo run -- rebuild-index
```

## プロジェクト分離

太閤立志伝5DXツールは別プロジェクトへ分離済みです:

- `../taiko5dx-tool`

この `Rust` フォルダはメモアプリ専用です。
