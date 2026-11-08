#!/usr/bin/env python3
"""README に依存の版が書かれていないかを見る（handball-project#783）。

Gradle のビルド定義（`*.gradle.kts` と `gradle-wrapper.properties`）が宣言している依存の版を集め、
README と、ビルド定義のコメント行に、その版がそのまま書かれていたら落とす。

**なぜ版を書かせないか。** 依存の版を上げるのは Dependabot で、上げるのはビルド定義だけ。README の
表や本文に版を写すと、上げるたびに古いまま残る（2026-10〜11 に 3 回続いた）。版の持ち主は
ビルド定義だけにして、README には「どのファイルのどの行で決まるか」と「なぜそうなっているか」を書く。
README に版を書いた PR はその場で落ちるので、Dependabot の PR が README のせいで赤くなることはない。

集める版:
  - プラグイン       id("<id>") version "<版>"
  - Maven の依存     "<group>:<artifact>:<版>"（`@aar` などの拡張子つきも）
  - ファイル参照     files("libs/<名前>-<版>.aar")
  - Gradle           gradle-wrapper.properties の gradle-<版>-bin.zip / -all.zip

集めないもの: compileSdk / targetSdk / minSdk / buildToolsVersion / versionName。Dependabot は上げず、
SDK の入れ方（`sdkmanager "platforms;android-37.0"` など）を書くのに数字が要るため。

使い方:
  python3 <このスクリプト> --root <Gradle プロジェクトのディレクトリ> <README> [<README> ...]

handball-recorder-android と handball-toolkit（examples/android）に同じ中身で置いてある。直すときは両方を揃える。

終了コード: 0 = 書かれていない / 1 = 書かれている / 2 = 検査できなかった（版を 1 つも集められない・
README が無い）。2 を「異常なし」と読まないこと。

標準ライブラリだけで動く（CI の runner に入っている python3 でそのまま回る）。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ビルドの出力やキャッシュの下にある *.gradle.kts は見ない。
SKIP_DIRS = {"build", ".gradle", ".kotlin", ".git", "node_modules"}

PLUGIN = re.compile(r'id\("([^"]+)"\)\s*version\s*"([0-9][^"]*)"')
MAVEN = re.compile(r'"([A-Za-z0-9_.\-]+):([A-Za-z0-9_.\-]+):([0-9][^"@:\s]*)(?:@[A-Za-z]+)?"')
FILE_AAR = re.compile(r'files\("libs/([^"/]*?)-([0-9][^"/]*)\.aar"\)')
WRAPPER = re.compile(r"^distributionUrl=.*gradle-([0-9][^-/]*)-(?:bin|all)\.zip")


@dataclass(frozen=True)
class Declared:
    version: str
    what: str  # 何の版か（プラグイン id・座標・Gradle）
    where: str  # <ファイル>:<行>


@dataclass(frozen=True)
class Hit:
    where: str
    declared: Declared
    text: str


def is_comment(line: str) -> bool:
    s = line.lstrip()
    return s.startswith(("//", "/*", "*"))


def build_files(root: Path) -> list[Path]:
    found = []
    for path in sorted(root.rglob("*.gradle.kts")):
        if not SKIP_DIRS.intersection(path.relative_to(root).parts[:-1]):
            found.append(path)
    return found


def collect(root: Path, cwd: Path) -> tuple[list[Declared], list[Path]]:
    declared: list[Declared] = []
    kts = build_files(root)
    for path in kts:
        rel = rel_to(path, cwd)
        for no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if is_comment(line):
                continue
            where = f"{rel}:{no}"
            for m in PLUGIN.finditer(line):
                declared.append(Declared(m.group(2), m.group(1), where))
            for m in MAVEN.finditer(line):
                declared.append(Declared(m.group(3), f"{m.group(1)}:{m.group(2)}", where))
            for m in FILE_AAR.finditer(line):
                declared.append(Declared(m.group(2), f"{m.group(1)}.aar", where))
    wrapper = root / "gradle" / "wrapper" / "gradle-wrapper.properties"
    if wrapper.is_file():
        rel = rel_to(wrapper, cwd)
        for no, line in enumerate(wrapper.read_text(encoding="utf-8").splitlines(), start=1):
            m = WRAPPER.match(line)
            if m:
                declared.append(Declared(m.group(1), "Gradle", f"{rel}:{no}"))
    return declared, kts


def pattern_for(version: str) -> re.Pattern[str]:
    # 前後に数字や「.数字」が続くものは別の版（2.4.21 と 12.4.21 / 2.4.210）。
    # 前の英字は許す（`v0.11.0` のタグ・`-0.11.0.aar` のファイル名も版の写しなので拾う）。
    return re.compile(r"(?<![0-9.])" + re.escape(version) + r"(?![0-9]|\.[0-9])")


def scan(path: Path, cwd: Path, declared: list[Declared], comments_only: bool) -> list[Hit]:
    patterns = [(d, pattern_for(d.version)) for d in declared]
    rel = rel_to(path, cwd)
    hits = []
    for no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if comments_only and not is_comment(line):
            continue
        seen = set()
        for d, pat in patterns:
            if d.version in seen:
                continue
            if pat.search(line):
                seen.add(d.version)
                hits.append(Hit(f"{rel}:{no}", d, line.strip()))
    return hits


def rel_to(path: Path, cwd: Path) -> str:
    try:
        return str(path.resolve().relative_to(cwd))
    except ValueError:
        return str(path)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="README に依存の版が書かれていないかを見る")
    parser.add_argument("--root", type=Path, default=Path("."), help="Gradle プロジェクトのディレクトリ")
    parser.add_argument("readmes", nargs="+", type=Path, help="検査する README")
    args = parser.parse_args(argv)

    cwd = Path.cwd().resolve()
    root = args.root.resolve()
    declared, kts = collect(root, cwd)
    if not declared:
        print(
            f"error: {rel_to(root, cwd)} のビルド定義から依存の版を 1 つも集められませんでした。"
            "--root の指定か、版の書き方（id(...) version \"...\" / \"group:artifact:版\"）を確かめてください。",
            file=sys.stderr,
        )
        return 2
    missing = [p for p in args.readmes if not p.is_file()]
    if missing:
        for p in missing:
            print(f"error: {p} がありません。", file=sys.stderr)
        return 2

    hits: list[Hit] = []
    for readme in args.readmes:
        hits += scan(readme, cwd, declared, comments_only=False)
    for path in kts:
        hits += scan(path, cwd, declared, comments_only=True)

    print(f"集めた版: {len(declared)} 件（{', '.join(sorted({d.what for d in declared}))}）", flush=True)
    if not hits:
        print("README とビルド定義のコメントに、依存の版は書かれていません。")
        return 0

    for h in hits:
        print(f"{h.where}: 「{h.declared.version}」は {h.declared.where} の {h.declared.what} の版です", file=sys.stderr)
        print(f"    {h.text}", file=sys.stderr)
    print(
        "\n"
        "README とビルド定義のコメントに依存の版を書かないでください。版を上げるのは Dependabot で、\n"
        "上げるのはビルド定義だけなので、写した版は上げるたびに古くなります。\n"
        "README には「どのファイルのどの行で決まるか」を書き、コメントは版の数字を使わない文にしてください。\n"
        "コマンドの例に版が要るときは、ビルド定義から読む形にします（例: .aar の版を sed で files(\"libs/...\") の行から読む）。",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
