//! Test-only work counters that prove algorithmic invariants (ADR-0072).

use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Work {
    DiscoveryScan,
    GraphCacheLoad,
    GraphCacheSerialization,
    GraphCachePublication,
    GraphCacheBytes,
    SourceRead,
    SourceParse,
    SourceReuse,
    SourceTargetResolution,
    LinkSourceResolution,
    PolicyDefinition,
    PolicyScopeResolution,
    PolicyCompilation,
    AstParse,
    SourcePartitionBuild,
    NotePartitionBuild,
    PolicyPartitionBuild,
    SourceIndexPartitionBuild,
    StateSerialization,
    StateBytes,
}

thread_local! {
    static COUNTS: RefCell<BTreeMap<Work, usize>> = const { RefCell::new(BTreeMap::new()) };
}

pub fn record(work: Work) {
    add(work, 1);
}

pub fn add(work: Work, amount: usize) {
    COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let count = counts.entry(work).or_default();
        *count = count.saturating_add(amount);
    });
}

pub fn reset() {
    COUNTS.with(|counts| counts.borrow_mut().clear());
}

pub fn count(work: Work) -> usize {
    COUNTS.with(|counts| counts.borrow().get(&work).copied().unwrap_or_default())
}

pub fn total() -> usize {
    COUNTS.with(|counts| counts.borrow().values().sum())
}

/// The counts of `works`, in order, for one comparison.
pub fn counts<const N: usize>(works: [Work; N]) -> [usize; N] {
    works.map(count)
}
