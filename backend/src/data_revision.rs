use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Identifies the state of the stored data. It changes whenever anything a
/// valuation depends on may have changed: a ledger write, an import, an
/// instrument edit, or a market-data refresh. It is process-local: a restart
/// starts a new session, which clients see as a change and refetch.
#[derive(Clone, Debug)]
pub struct DataRevision {
    session: Arc<str>,
    counter: Arc<AtomicU64>,
}

impl DataRevision {
    pub fn new() -> Self {
        let session = crate::clock::now_utc().timestamp_millis().to_string();
        Self {
            session: Arc::from(session.as_str()),
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The current revision, as `<session>:<counter>`.
    pub fn current(&self) -> String {
        format!("{}:{}", self.session, self.counter.load(Ordering::SeqCst))
    }

    /// Record that stored data may have changed.
    pub fn bump(&self) {
        self.counter.fetch_add(1, Ordering::SeqCst);
    }
}

impl Default for DataRevision {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_changes_only_when_bumped() {
        let revision = DataRevision::new();
        let first = revision.current();

        assert_eq!(revision.current(), first);

        revision.bump();
        let second = revision.current();

        assert_ne!(second, first);
        assert_eq!(revision.current(), second);
    }

    #[test]
    fn clones_share_one_counter() {
        let revision = DataRevision::new();
        let clone = revision.clone();

        clone.bump();

        assert_eq!(revision.current(), clone.current());
    }
}
