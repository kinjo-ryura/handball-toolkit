# 出荷したアプリが書いた試合ファイル（読み続ける約束の fixture）

利用者の試合ファイル（`.handrec`。中身は SAMPLE_DTO_V2 の JSON）が端末の外へ出た時点で、
**形式は公約**になった — 自アプリが書いた過去のファイルを将来のアプリが読める、という約束
（HandballRecorder `docs/adr/0002-match-file-transfer.md` 規律 2 / handball-project#298）。
この約束を記憶ではなく機械に持たせるため、**出荷した version のアプリが実際に書いたファイル**を
ここに置き、`tests/shipped_match_files_tests.rs` が parse → convert → validation → 再 encode の
バイト一致を CI で固定する。

`golden/export/` の fixture（Swift オラクルから決定的に生成）と違い、ここにあるのは
**出荷ビルドの実出力**で、`.local` の externalID の実形や fact の並びなど生成時の癖をそのまま持つ。

## 置き方

- ファイル名は `handballrecorder-<version>-<build>.handrec`（同じ version で複数置くなら末尾に `-<n>`）
- **実チームの名前は置かない**（このリポは public）。チーム名・選手名・試合名は文字列を
  差し替えて匿名化する。**構造・数値・並びは変えない**（それを固定するのが目的）
- 出荷した version ごとに 1 つ以上。version を出して書式に影響する変更があったら必ず足す
- 消さない。読めなくなる変更を入れるときは `schemaVersion` を上げ、旧版の読み込み経路を残す

## 出所（provenance）

| ファイル | 書いたアプリ | 出所 | 匿名化 |
|---|---|---|---|
| `handballrecorder-1.6.0-28.handrec` | HandballRecorder 1.6.0 (28)（27 と書き出しコードは同一。`generator` 無し） | 実機で記録した `.video` + `.local` の試合（fact 120 件 = goal 72 / shotMissed 28 / control 20。7m スロー失敗の note あり）を「別の端末へ送る…」で書き出し、2026-09-05 に AirDrop で Mac へ | チーム名 → ホーム / アウェイ、displayName → 「ホーム vs アウェイ」、選手名 → `選手H01`〜 / `選手A01`〜（背番号・key・factID・localIdentifier・時刻はそのまま） |
