#!/usr/bin/env bash
# 依存ライセンス一覧の生成（handball-project#140）。
#
# 配布バイナリへリンクされる OSS のライセンス本文と著作権表示を Cargo.lock から集め、
# 各シェル（iOS / Android）がそのまま表示できる JSON に整形する。
#
# 成果物（**コミットする**）:
#   - THIRD_PARTY_LICENSES.json
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
readonly MANIFEST=crates/handball-toolkit-ffi/Cargo.toml

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

# 対象は FFI パッケージング crate。これが iOS の staticlib / Android の .so の実体で、
# コア crate を feature `uniffi` 込みで引く（= 配布バイナリの依存グラフそのもの）。
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
generate() {
  cargo-about generate \
    --config about.toml \
    --manifest-path "$MANIFEST" \
    --format json \
    --fail \
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

if [ "$check_only" = 1 ]; then
  if [ ! -f "$OUT" ]; then
    echo "error: $OUT がありません。./scripts/generate_licenses.sh で生成してコミットしてください。" >&2
    exit 1
  fi
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  generate > "$tmp"
  if ! diff -u "$OUT" "$tmp"; then
    cat >&2 <<MSG

error: $OUT が依存の現況と一致しません。

  依存を追加・更新したら ./scripts/generate_licenses.sh を実行して
  生成結果をコミットしてください（一覧を手で直さないこと）。
MSG
    exit 1
  fi
  echo "OK: $OUT は最新です"
  exit 0
fi

generate > "$OUT"
echo "完了: $OUT"
jq -r '"  ライブラリ \(.libraries | length) 件 / ライセンス本文 \(.licenses | length) 件"' "$OUT"
jq -r '.libraries | group_by(.origin) | .[] | "  - origin=\(.[0].origin): \(length) 件"' "$OUT"
jq -r '.licenses | group_by(.id) | .[] | "  - \(.[0].id): 本文 \(length) 件"' "$OUT"
