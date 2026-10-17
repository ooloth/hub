//! Which pull requests one refresh found waiting on me.
use std::collections::{BTreeMap, BTreeSet};

use crate::pr::PullRequest;
use crate::pr_id::PrId;
use crate::pr_turn::{Turn, TurnReading};

/// Whether a refresh heard from every source that reports a turn.
///
/// Only then does a pull request missing from it mean the turn ended rather than that its source
/// failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnCoverage {
    /// Every source that reports a turn answered.
    Complete,
    /// At least one did not, so a missing pull request says nothing.
    Incomplete,
}

/// The pull requests one refresh found waiting on me, and the ones it could not decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnsSeen {
    turns: BTreeMap<PrId, Turn>,
    undecided: BTreeSet<PrId>,
    coverage: TurnCoverage,
}

impl TurnsSeen {
    /// What `prs`, from a refresh with `coverage`, say is waiting on me.
    #[must_use]
    pub fn from_prs<'p>(
        prs: impl IntoIterator<Item = &'p PullRequest>,
        coverage: TurnCoverage,
    ) -> Self {
        let mut turns = BTreeMap::new();
        let mut undecided = BTreeSet::new();
        for pr in prs {
            match TurnReading::of(pr) {
                TurnReading::Turn(turn) => {
                    let _ = turns.insert(turn.pr.clone(), turn);
                }
                TurnReading::Undecided => {
                    let _ = undecided.insert(PrId::of(pr));
                }
                TurnReading::NotMyTurn => {}
            }
        }
        Self {
            turns,
            undecided,
            coverage,
        }
    }

    /// Turns seen exactly as given, for tests that generate them.
    #[cfg(test)]
    pub(crate) const fn new(
        turns: BTreeMap<PrId, Turn>,
        undecided: BTreeSet<PrId>,
        coverage: TurnCoverage,
    ) -> Self {
        Self {
            turns,
            undecided,
            coverage,
        }
    }

    /// The pull requests waiting on me.
    #[must_use]
    pub const fn turns(&self) -> &BTreeMap<PrId, Turn> {
        &self.turns
    }

    /// Whether `pr` might be waiting on me, which this refresh could not tell.
    #[must_use]
    pub fn is_undecided(&self, pr: &PrId) -> bool {
        self.undecided.contains(pr)
    }

    /// Whether every source that reports a turn answered.
    #[must_use]
    pub const fn coverage(&self) -> TurnCoverage {
        self.coverage
    }
}
