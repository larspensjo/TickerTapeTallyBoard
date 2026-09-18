use sqlx::{sqlite::SqlitePool, Executor};

/// Identifies the state of the stored data. It changes whenever anything a
/// valuation depends on may have changed: a ledger write, an import, an
/// instrument edit, or a market-data refresh. The ledger owns it so every
/// process sharing that ledger publishes the same value.
#[derive(Clone, Debug)]
pub struct DataRevision {
    pool: SqlitePool,
}

impl DataRevision {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The current ledger-qualified revision. Clients treat this as opaque.
    pub async fn current(&self) -> Result<String, sqlx::Error> {
        current(&self.pool).await
    }

    /// Record that stored data may have changed.
    pub async fn bump(&self) -> Result<(), sqlx::Error> {
        bump(&self.pool).await
    }
}

pub async fn current<'e, E>(executor: E) -> Result<String, sqlx::Error>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    let (ledger_id, counter): (String, i64) =
        sqlx::query_as("SELECT ledger_id, counter FROM data_revision WHERE id = 1")
            .fetch_one(executor)
            .await?;
    Ok(format!("{ledger_id}:{counter}"))
}

pub async fn bump<'e, E>(executor: E) -> Result<(), sqlx::Error>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("UPDATE data_revision SET counter = counter + 1 WHERE id = 1")
        .execute(executor)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_changes_only_when_bumped() {
        let revision = DataRevision::new(crate::db::testing::memory_pool().await);
        let first = revision.current().await.expect("revision reads");
        revision.bump().await.expect("revision bumps");
        assert_ne!(revision.current().await.expect("revision reads"), first);
    }

    #[tokio::test]
    async fn different_ledgers_at_the_same_counter_have_distinct_revisions() {
        let first = DataRevision::new(crate::db::testing::memory_pool().await);
        let second = DataRevision::new(crate::db::testing::memory_pool().await);

        assert_ne!(
            first.current().await.expect("first revision reads"),
            second.current().await.expect("second revision reads")
        );
    }
}
