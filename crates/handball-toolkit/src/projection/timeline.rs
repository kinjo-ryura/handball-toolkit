//! 移植元: `Projection/TimelineProjection.swift`。

use serde::{Deserialize, Serialize};

use crate::clock::{FactAnchor, MatchClock, VideoClock};
use crate::entities::Match;
use crate::facts::MatchFact;
use crate::ids::FactId;

use super::segment_resolver::SegmentResolver;

/// fact log を時系列に並べ、anchor から表示用の MatchClock / VideoClock を解決した
/// 読み取り専用 projection。永続化はしない。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineProjection {
    pub resolved_facts: Vec<ResolvedFact>,
    pub resolver: SegmentResolver,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFact {
    pub fact: MatchFact,
    pub resolved_match_clock: Option<MatchClock>,
    pub resolved_video_clock: Option<VideoClock>,
}

impl TimelineProjection {
    /// Swift 版同様、`match` は未使用だが API 対称性（facts 版 / timeline 版の 2 系統 —
    /// ADR 0001 関数目録）のため引数に保持する。
    pub fn build(_match: &Match, facts: &[MatchFact]) -> TimelineProjection {
        let resolver = SegmentResolver::build(facts);

        let resolved_facts = facts
            .iter()
            .map(|fact| {
                let (mc, vc) = resolve_clocks(fact, &resolver);
                ResolvedFact {
                    fact: fact.clone(),
                    resolved_match_clock: mc,
                    resolved_video_clock: vc,
                }
            })
            .collect();

        TimelineProjection {
            resolved_facts,
            resolver,
        }
    }

    pub fn resolved_fact(&self, id: FactId) -> Option<&ResolvedFact> {
        self.resolved_facts.iter().find(|r| r.fact.id == id)
    }
}

/// fact の anchor から projection 用 matchClock / videoClock を解決する。
fn resolve_clocks(
    fact: &MatchFact,
    resolver: &SegmentResolver,
) -> (Option<MatchClock>, Option<VideoClock>) {
    match fact.anchor() {
        FactAnchor::MatchClock(mc) => {
            // matchClock のみ → video 派生は resolver 経由
            (Some(mc), resolver.resolve_video_clock(mc))
        }
        FactAnchor::VideoClock(vc) => {
            // videoClock のみ → match 派生は resolver 経由
            (resolver.resolve_match_clock(vc), Some(vc))
        }
        FactAnchor::Both {
            match_clock: mc,
            video_clock: vc,
        } => {
            // 明示確定済み → 上書きしない
            (Some(mc), Some(vc))
        }
    }
}
