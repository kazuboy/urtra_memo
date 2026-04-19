# Ultra Memo Web
<img width="1761" height="1015" alt="image" src="https://github.com/user-attachments/assets/c15568a8-2020-4320-a902-76d5b8b7a23d" />

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

## 向いている用途

- 思いつきを素早く残すメモ
- 日報・作業ログ・日記
- タグ検索で過去メモを再発見する運用
- 重い多機能ノートより、軽快さ優先の運用

## 使い方
https://github.com/kazuboy/urtra_memo/blob/main/docs/ultra-memo-web-guide-ja.md
