# セキュリティ上の問題の報告

このリポジトリ（Rust コア・生成バインディング・配布物の `.aar` / XCFramework / wasm）に
脆弱性を見つけたら、**公開 Issue ではなく GitHub の Private vulnerability reporting で
報告してください**。Issue は誰でも読めるので、修正が出る前に手口が広まります。

- 報告先: リポジトリの **Security** タブ → **Report a vulnerability**
  （<https://github.com/kinjo-ryura/handball-toolkit/security/advisories/new>）
- 書いてほしいこと: 影響を受ける版（Release のタグ、または main のコミット）、再現手順、
  想定される影響。PoC があれば添えてください
- 対象: このリポジトリの成果物。利用側アプリ（ハンド記録 iOS / Android 版）の問題は
  それぞれのリポジトリ、または <https://hand-plus.com/handball-recorder/support/> へ

個人で運営しているため即応の約束はできませんが、**受領の返事は 7 日以内**を目安にし、
修正が出たら Release notes と advisory で公表します。報告者名は希望があれば掲載します。

## 対象となる版

最新の Release と `main` のみ。古い Release への backport は行いません。

## 対象外

- 依存クレートそのものの脆弱性（各クレートへ直接報告してください。この CI は
  RustSec advisory-db との照合を回しており、公表された advisory は検知します）
- ネットワークアクセス・認証・秘密情報の取り扱い（このリポジトリは持っていません。
  コアは純粋計算で、ネットワークにも保存先にも触れません）
