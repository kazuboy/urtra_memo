# Ultra Memo Web

Ultra Memo は、  
「一瞬で書けて、一瞬で見つかる」を目指した軽量メモアプリです。

このリポジトリで公開しているのは **Web 版（WASM）** です。

## 特徴

- 起動してすぐ書けるシンプルUI
- 自動保存（保存操作を意識しない）
- 全文検索（タイトル / 本文 / `#tag`）
- Markdown入力 + プレビュー切替
- 一覧管理（All / Recent / Trash）
- 論理削除・復元・完全削除
- フォルダ作成・フォルダ移動（任意利用）
- 外観カスタマイズ
  - フォント
  - 文字色 / 背景色 / アクセント色
  - UI拡大率
  - プリセット: `Classic / Warm Journal / Quiet Modern`
- データ入出力
  - Export: JSON（全件） / Markdown（選択メモ）
  - Import: `json / md / markdown / txt / text / log / rst / adoc / org`

## データ保存

- 保存先: ブラウザの `localStorage`
- 同じブラウザ・同じオリジンでデータが維持されます
- クラウド同期は行いません（ローカル完結）

## ローカルで動かす（開発用）

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve
```

起動後: `http://127.0.0.1:8080`

## GitHub Pages 公開

このリポジトリには Pages デプロイ workflow を設定済みです:

- `.github/workflows/deploy-pages.yml`

手順:

1. `main`（または `master`）へ push
2. GitHub の `Settings > Pages` で  
   `Build and deployment: GitHub Actions` を選択
3. Actions の `Deploy Ultra Memo Web` が成功したら公開URLへアクセス

公開URLのパス:

- `username.github.io` リポジトリ: `/`
- それ以外: `/<repo-name>/`

## 向いている用途

- 思いつきを素早く残すメモ
- 日報・作業ログ・日記
- タグ検索で過去メモを再発見する運用
- 重い多機能ノートより、軽快さ優先の運用

