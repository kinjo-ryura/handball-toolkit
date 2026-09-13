#!/usr/bin/env bash
# 依存ライセンス一覧の生成（handball-project#140）。
#
# 配布バイナリへリンクされる OSS のライセンス本文と著作権表示を Cargo.lock から集め、
# 各シェル（iOS / Android）がそのまま表示できる JSON に整形する。
#
# 成果物（**コミットする**）:
#   - THIRD_PARTY_LICENSES.json … 正。シェルが同梱して画面に出す
#   - THIRD_PARTY_LICENSES.md   … 同じ内容を人が読める形にしたもの。JSON から機械的に作る
#                                 （リンクで表示を届ける配布経路のため。下の render_markdown）
#
# バイナリ非コミット方針（ADR 0004 決定 8）の例外ではない — これはテキストの生成物で、
# 生成 Swift バインディングと同じ「ソースはコミットする」側に属する。コミットするのは
# 配布時に必ず同梱される必要があり、シェル側のビルドが Rust ツールチェーンなしで
# 完結しなければならないため（iOS の bootstrap.sh が cp するだけで済む形にする）。
#
# 使い方:
#   ./scripts/generate_licenses.sh           # 生成して書き出す
#   ./scripts/generate_licenses.sh --check   # 再生成して差分があれば exit 1（CI 用）
#
# 前提: nix develop（または direnv）環境内で実行する。ネットワークが要る
# （cargo がレジストリを引き、cargo-about が本文の無い crate を clearlydefined.io で補完する）。
set -euo pipefail
cd "$(dirname "$0")/.."

readonly OUT=THIRD_PARTY_LICENSES.json
readonly MD_OUT=THIRD_PARTY_LICENSES.md
# 配布物ごとの依存グラフの根。**両方を走らせて統合する**（handball-project#285）。
#   - ffi  … iOS の staticlib / Android の .so の実体。コア crate を feature `uniffi` 込みで引く
#   - wasm … Web デモ（handball-apps-site が配る .wasm）。wasm-bindgen 等は wasm 側にしか無い
# 以前は ffi だけを見ていたため、about.toml の targets に wasm32 を足しても wasm 限定の
# 依存は一覧に載らなかった（グラフの根に無いものは target を足しても現れない）。
# workspace 全体（`--workspace`）にしないのは CLI の依存（clap 等）まで載るため — CLI は
# CI でしか走らず、シェルの画面に出す一覧に混ぜる意味が無い。
readonly MANIFESTS=(
  crates/handball-toolkit-ffi/Cargo.toml
  crates/handball-toolkit-wasm/Cargo.toml
)

check_only=0
if [ "${1:-}" = "--check" ]; then
  check_only=1
elif [ $# -gt 0 ]; then
  echo "error: 不明な引数: $1（使えるのは --check のみ）" >&2
  exit 1
fi

# 配布物のバージョン。生成物がどのコアのものかを追えるようにする。
version=$(grep -m1 '^version = ' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')

# workspace メンバの名前一覧。libraries[].origin の判定に使う（handball-project#145）。
# 読み手に crate 名を持たせないための情報で、判定はここで一度だけ行う。
workspace_members=$(cargo metadata --no-deps --format-version 1 | jq -c '[ .packages[].name ]')

# 対象は上の MANIFESTS（配布物の依存グラフそのもの）。manifest ごとに cargo-about を
# 回し、licenses[] を (id, 本文) で畳んで used_by を合併してから整形する。ffi と wasm は
# コア crate を共有するので大半は重なり、wasm 側で増えるのは wasm-bindgen 一式だけ。
# --fail: ライセンス式を読めない / accepted に無い crate があれば止める。
#
# 整形方針:
#   - licenses[]  … ライセンス本文（同一本文は 1 件に集約済み。MIT は著作権表示が
#                   crate ごとに違うため本文も別々に立つ）
#   - libraries[] … crate 一覧。licenseIndexes で licenses[] を参照する。
#                   本文を crate ごとに複製すると 3 倍近く太るため間接参照にする。
#   - sourceUrl   … crates.io の**当該バージョン**を指す。MPL-2.0 §3.2 の
#                   「ソース入手方法の告知」をこれで満たす。
#   - origin      … "workspace"（この repo の crate）か "registry"（外部）か。
#                   **「自作かどうか」ではない** — 誰から見て自作かは配布経路で変わる
#                   （`.aar` を受け取った外部シェルにとって handball-toolkit は third party）。
#                   ここには視点に依存しない事実だけを載せ、どう見せるかは各シェルに委ねる。
#   - 並び順は全段で固定する（--check の差分が実質変更のときだけ出るように）。
generate_one() {
  cargo-about generate \
    --config about.toml \
    --manifest-path "$1" \
    --format json \
    --fail
}

# 複数 manifest の出力を 1 つの cargo-about 形（licenses[].used_by[]）に統合する。
# 同じライセンス本文は 1 件に畳み、同じ crate（name, version）は 1 回だけ数える。
merge() {
  jq -s '
    { licenses: (
        [ .[].licenses[] ]
        | group_by([ .id, .text ])
        | map({
            id: .[0].id,
            name: .[0].name,
            text: .[0].text,
            used_by: ([ .[].used_by[] ] | unique_by([ .crate.name, .crate.version ]))
          })
      )
    }'
}

generate() {
  for manifest in "${MANIFESTS[@]}"; do
    generate_one "$manifest"
  done \
  | merge \
  | jq --arg version "$version" --argjson workspace "$workspace_members" '
      def origin:
        .name as $n
        | if ($workspace | index($n)) then "workspace" else "registry" end;

      def source_url:
        if ((.source // "") | startswith("registry+https://github.com/rust-lang/crates.io-index"))
        then "https://crates.io/crates/\(.name)/\(.version)"
        else .repository
        end;

      # 本文を並べ替えてから index を確定させる（libraries[] が参照するため順序が先）。
      ( [ .licenses[] | { id, name, text, crates: [ .used_by[].crate ] } ]
        | map(. + { sortKey: ([ .crates[].name ] | sort | join(",")) })
        | sort_by(.id, .sortKey)
      ) as $ls
      | {
          schemaVersion: 1,
          toolkitVersion: $version,
          licenses: [ $ls[] | { id, name, text } ],
          libraries: (
            [ range(0; ($ls | length)) as $i
              | $ls[$i].crates[]
              | { name, version, origin: origin, sourceUrl: source_url, licenseIndex: $i }
            ]
            # 1 crate が複数ライセンスに服することがある（例: unicode-ident は
            # "(MIT OR Apache-2.0) AND Unicode-3.0" で MIT と Unicode-3.0 の両方に載る）。
            # 一覧に同じ crate を 2 行出さないよう畳んで、本文を複数持たせる。
            | group_by([ .name, .version ])
            | map({
                name: .[0].name,
                version: .[0].version,
                origin: .[0].origin,
                sourceUrl: .[0].sourceUrl,
                licenseIndexes: ([ .[].licenseIndex ] | sort)
              })
            | sort_by(.name, .version)
          )
        }
    '
}

# 標準入力の JSON（generate の出力）を、人が読める Markdown に写す。
#
# **なぜ要るか**: 表示義務は「受領者に届ける」ことで、画面に出せないシェルはリンクで届ける
# （handball-apps-site の Web デモは自前で一覧を持たず、このファイルを指す — 写しを持つと
# リリースのたびに古くなるため。handball-project#384）。生の JSON は本文が 1 行の
# エスケープ文字列で、読める形とは言いにくい。
#
# - **JSON だけから作る**。cargo も cargo-about も呼ばないので、JSON と食い違う余地が無い
# - 本文はコードフェンスに入れる（Markdown として解釈させない）。フェンスは本文中の
#   最長のバッククォート列より 1 本長くする。改行は LF に揃える（CRLF の本文がある）
# - アンカーは `license-<licenses[] の index>`。見出しの自動アンカーは同名（MIT が 20 件）で
#   連番になり、並びが変わると別の本文を指すため使わない
render_markdown() {
  jq -r '
    . as $root
    | $root.licenses as $ls
    | def fence($text):
        ([ $text | scan("`+") | length ] | max // 0) as $m
        | "`" * ([ 3, $m + 1 ] | max);
      def license_links:
        [ .licenseIndexes[] as $i | "[\($ls[$i].id)](#license-\($i))" ] | join("・");
      [
        "# OSS ライセンス一覧",
        "",
        "<!-- scripts/generate_licenses.sh が THIRD_PARTY_LICENSES.json から生成する。直接編集しない。 -->",
        "",
        "handball-toolkit \($root.toolkitVersion) の配布物（iOS / macOS の staticlib・Android の `.so`・Web の `.wasm`）にリンクされるオープンソースソフトウェアと、そのライセンス本文。",
        "",
        "- 配布物ごとの依存を統合した一覧のため、配布物によっては含まれないライブラリも載っている",
        "- 各ライブラリのソースコードは「入手先」から入手できる",
        "- 同じ内容の機械可読版は [`THIRD_PARTY_LICENSES.json`](THIRD_PARTY_LICENSES.json)",
        "",
        "## ライブラリ（\($root.libraries | length) 件）",
        "",
        "| ライブラリ | バージョン | ライセンス | 入手先 |",
        "| --- | --- | --- | --- |",
        ( $root.libraries[]
          | "| \(.name) | \(.version) | \(license_links) | [ソース](\(.sourceUrl)) |" ),
        "",
        "## ライセンス本文（\($ls | length) 件）",
        ( range(0; $ls | length) as $i
          | $ls[$i] as $lic
          | ($lic.text | gsub("\r\n"; "\n") | gsub("\r"; "\n") | sub("\n+$"; "")) as $text
          | fence($text) as $f
          | [ $root.libraries[] | select(any(.licenseIndexes[]; . == $i)) | "\(.name) \(.version)" ]
            | join("、") as $users
          | "",
            "### <a id=\"license-\($i)\"></a>\($lic.name)（\($lic.id)）",
            "",
            "適用: \($users)",
            "",
            $f,
            $text,
            $f )
      ]
      | join("\n")
  '
}

if [ "$check_only" = 1 ]; then
  for f in "$OUT" "$MD_OUT"; do
    if [ ! -f "$f" ]; then
      echo "error: $f がありません。./scripts/generate_licenses.sh で生成してコミットしてください。" >&2
      exit 1
    fi
  done
  tmp=$(mktemp)
  tmp_md=$(mktemp)
  trap 'rm -f "$tmp" "$tmp_md"' EXIT
  generate > "$tmp"
  render_markdown < "$tmp" > "$tmp_md"
  stale=()
  diff -u "$OUT" "$tmp" || stale+=("$OUT")
  diff -u "$MD_OUT" "$tmp_md" || stale+=("$MD_OUT")
  if [ ${#stale[@]} -gt 0 ]; then
    cat >&2 <<MSG

error: ${stale[*]} が依存の現況と一致しません。

  依存を追加・更新したら ./scripts/generate_licenses.sh を実行して
  生成結果をコミットしてください（一覧を手で直さないこと）。
MSG
    exit 1
  fi
  echo "OK: $OUT / $MD_OUT は最新です"
  exit 0
fi

generate > "$OUT"
render_markdown < "$OUT" > "$MD_OUT"
echo "完了: $OUT / $MD_OUT"
jq -r '"  ライブラリ \(.libraries | length) 件 / ライセンス本文 \(.licenses | length) 件"' "$OUT"
jq -r '.libraries | group_by(.origin) | .[] | "  - origin=\(.[0].origin): \(length) 件"' "$OUT"
jq -r '.licenses | group_by(.id) | .[] | "  - \(.[0].id): 本文 \(length) 件"' "$OUT"
