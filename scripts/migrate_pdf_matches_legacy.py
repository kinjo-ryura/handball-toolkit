#!/usr/bin/env python3
"""pdf-matches の旧 V2 形式 JSON を現行 SAMPLE_DTO_V2 形式へ移行する一時スクリプト。

使い方（stdlib のみ・venv 不要）:
  python3 scripts/migrate_pdf_matches_legacy.py <旧形式.json>:<出力先.json> ...
出力先は tests/golden/local/inputs/timer/（gitignore 済み）を想定。
再生成の全手順は crates/handball-toolkit/tests/golden/README.md を参照。

旧形式（jha-pdf-importer 産・未更新）との差分:
- configuration: captureMethod/contentKind/phaseRules → {kind: "timer", timer: {...}}
- fact の `id` → `factID`
- MatchClock は phase 相対秒 → 試合通算累積秒（secondHalf は +1800）
- play/control の `phase` フィールド廃止、matchClock の `phase` 廃止
- PhaseStart fact が存在しない → phaseRules から合成（regular ×2、start/end 累積秒）
- timeoutStarted → stoppage {stoppageKind: "timeout", end 無し}

出力は cumulative 秒 → recordedAt → factID で整列（永続化順の入力契約）。
恒久対応（importer 自体の現行スキーマ化)は別 Issue 候補。
"""

import json
import sys
import uuid
from datetime import datetime, timedelta, timezone

PHASE_OFFSETS = {"firstHalf": 0.0, "secondHalf": 1800.0}


def cumulative(anchor: dict, phase: str) -> float:
    return anchor["matchClock"]["elapsedSeconds"] + PHASE_OFFSETS[phase]


def migrate(path: str, out_path: str) -> None:
    with open(path) as fp:
        old = json.load(fp)

    config = old["match"]["configuration"]
    rules = config["phaseRules"]["phases"]
    assert [r["phase"] for r in rules] == ["firstHalf", "secondHalf"], rules
    assert all(r["nominalDurationSeconds"] == 1800.0 for r in rules), rules
    assert config["captureMethod"] == "manualClock", config

    date = datetime.fromisoformat(old["match"]["date"].replace("Z", "+00:00"))
    slug = path.rsplit("/", 1)[-1].removesuffix(".json")

    facts = []  # (sort_key, fact)

    # PhaseStart 合成（regular ×2）
    for index, phase in enumerate(["firstHalf", "secondHalf"]):
        start = PHASE_OFFSETS[phase]
        end = start + 1800.0
        fact_id = str(uuid.uuid5(uuid.NAMESPACE_URL, f"pdf-matches/{slug}/phaseStart/{index}"))
        recorded = (date + timedelta(seconds=start)).strftime("%Y-%m-%dT%H:%M:%SZ")
        fact = {
            "factID": fact_id,
            "recordedAt": recorded,
            "payload": {
                "kind": "control",
                "play": None,
                "control": {
                    "kind": "phaseStart",
                    "phaseStart": {"kind": "regular"},
                    "stoppage": None,
                    "anchor": {
                        "kind": "matchClock",
                        "matchClock": {"elapsedSeconds": start},
                        "videoClock": None,
                        "endMatchElapsedSeconds": end,
                        "endVideoElapsedSeconds": None,
                    },
                },
            },
        }
        facts.append(((start, recorded, fact_id), fact))

    for old_fact in old["facts"]:
        payload = old_fact["payload"]
        fact_id = old_fact["id"]
        recorded = old_fact["recordedAt"]
        if payload["kind"] == "play":
            play = payload["play"]
            seconds = cumulative(play["anchor"], play["phase"])
            new_payload = {
                "kind": "play",
                "play": {
                    "kind": play["kind"],
                    "teamKey": play["teamKey"],
                    "playerKey": play["playerKey"],
                    "relatedPlayerKey": play["relatedPlayerKey"],
                    "anchor": {
                        "kind": "matchClock",
                        "matchClock": {"elapsedSeconds": seconds},
                        "videoClock": None,
                        "endMatchElapsedSeconds": None,
                        "endVideoElapsedSeconds": None,
                    },
                    "title": play["title"],
                    "note": play["note"],
                },
                "control": None,
            }
        elif payload["kind"] == "control":
            control = payload["control"]
            assert control["kind"] == "timeoutStarted", control
            seconds = cumulative(control["anchor"], control["phase"])
            new_payload = {
                "kind": "control",
                "play": None,
                "control": {
                    "kind": "stoppage",
                    "phaseStart": None,
                    "stoppage": {"stoppageKind": "timeout", "note": None},
                    "anchor": {
                        "kind": "matchClock",
                        "matchClock": {"elapsedSeconds": seconds},
                        "videoClock": None,
                        "endMatchElapsedSeconds": None,
                        "endVideoElapsedSeconds": None,
                    },
                },
            }
        else:
            raise AssertionError(payload["kind"])
        facts.append(
            ((seconds, recorded, fact_id), {"factID": fact_id, "recordedAt": recorded, "payload": new_payload})
        )

    facts.sort(key=lambda pair: pair[0])

    new = {
        "schemaVersion": 2,
        "match": {
            "displayName": old["match"]["displayName"],
            "date": old["match"]["date"],
            "configuration": {
                "kind": "timer",
                "timer": {"phaseDurationSeconds": 1800.0},
                "video": None,
                "videoHighlight": None,
            },
        },
        "teams": old["teams"],
        "facts": [fact for _, fact in facts],
    }

    with open(out_path, "w") as fp:
        json.dump(new, fp, ensure_ascii=False, indent=2, sort_keys=True)
        fp.write("\n")
    print(f"migrated {slug}: facts {len(old['facts'])} -> {len(new['facts'])} (PhaseStart +2)")


if __name__ == "__main__":
    for arg in sys.argv[1:]:
        src, dst = arg.split(":", 1)
        migrate(src, dst)
