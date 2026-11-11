use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "tttb-analysis-test-{}-{}-{}",
            std::process::id(),
            crate::clock::now_utc().timestamp_nanos_opt().unwrap(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join("repository")).unwrap();
        Self(directory)
    }
    fn repository(&self) -> PathBuf {
        self.0.join("repository")
    }
    fn ledger(&self) -> PathBuf {
        self.0.join("synthetic.sqlite")
    }
    async fn seed(&self, migrate: bool) {
        let options = SqliteConnectOptions::new()
            .filename(self.ledger())
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        if migrate {
            crate::db::migrate(&pool).await.unwrap();
        }
        pool.close().await;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test-created, absolute temporary directory is removed.
        if self.0.is_absolute() && self.0.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn small_groups_suppress_shares_and_medians() {
    for size in 0..5 {
        assert_eq!(aggregate_share(size, size), "suppressed");
        assert_eq!(aggregate_median(&vec![Decimal::ONE; size]), None);
        assert_eq!(aggregate_tenth_percentile(&vec![Decimal::ONE; size]), None);
    }
    assert_eq!(aggregate_share(1, 5), "20.00%");
    assert_eq!(aggregate_share(1, 7), "14.29%");
    assert_eq!(aggregate_share(0, 5), "0.00%");
    assert_eq!(
        aggregate_median(&[5, 1, 4, 2, 3].map(Decimal::from)),
        Some(Decimal::from(3))
    );
    assert_eq!(
        aggregate_median(&[6, 5, 1, 4, 2, 3].map(Decimal::from)),
        Some(Decimal::new(35, 1))
    );
}

#[test]
fn tenth_percentile_is_nearest_rank_for_odd_even_and_tied_samples() {
    for count in [5usize, 6, 10, 11, 20, 21] {
        let values: Vec<_> = (1..=count).rev().map(Decimal::from).collect();
        assert_eq!(
            aggregate_tenth_percentile(&values),
            Some(Decimal::from(count.div_ceil(10)))
        );
    }
    assert_eq!(
        aggregate_tenth_percentile(&[Decimal::ONE; 6]),
        Some(Decimal::ONE)
    );
    assert_eq!(aggregate_median(&[Decimal::MAX; 6]), Some(Decimal::MAX));
    assert_eq!(aggregate_median(&[Decimal::MIN; 6]), Some(Decimal::MIN));
    assert_eq!(
        aggregate_median(&[
            Decimal::MIN,
            Decimal::MIN,
            Decimal::MIN,
            Decimal::MAX,
            Decimal::MAX,
            Decimal::MAX
        ]),
        Some(Decimal::ZERO)
    );
    assert_eq!(
        aggregate_median(&[
            Decimal::new(1, 28),
            Decimal::new(1, 28),
            Decimal::new(1, 28),
            Decimal::new(2, 28),
            Decimal::new(2, 28),
            Decimal::new(2, 28)
        ]),
        Some(Decimal::new(2, 28))
    );
}

#[tokio::test]
async fn path_refusals_happen_before_any_sqlite_open() {
    let fixture = Fixture::new();
    let missing = fixture.ledger();
    assert!(open_checked(&missing, &fixture.repository(), &[])
        .await
        .is_err());
    assert!(!missing.exists());
    let inside = fixture.repository().join("synthetic.sqlite");
    std::fs::write(&inside, b"not a database").unwrap();
    assert!(open_checked(&inside, &fixture.repository(), &[])
        .await
        .unwrap_err()
        .contains("working tree"));
    let traversal = fixture.repository().join("nested/../synthetic.sqlite");
    std::fs::create_dir(fixture.repository().join("nested")).unwrap();
    assert!(open_checked(&traversal, &fixture.repository(), &[])
        .await
        .unwrap_err()
        .contains("working tree"));
    fixture.seed(true).await;
    assert!(open_checked(
        &fixture.ledger(),
        &fixture.repository(),
        &[fixture.ledger()]
    )
    .await
    .unwrap_err()
    .contains("configured ledger"));
    assert!(open_checked(&fixture.0, &fixture.repository(), &[])
        .await
        .is_err());
}

#[tokio::test]
async fn exact_schema_is_required_and_no_file_is_changed_or_created() {
    let fixture = Fixture::new();
    fixture.seed(false).await;
    let original = std::fs::read(fixture.ledger()).unwrap();
    assert!(open_checked(&fixture.ledger(), &fixture.repository(), &[])
        .await
        .unwrap_err()
        .contains("schema"));
    assert_eq!(std::fs::read(fixture.ledger()).unwrap(), original);
    std::fs::remove_file(fixture.ledger()).unwrap();
    fixture.seed(true).await;
    let original = std::fs::read(fixture.ledger()).unwrap();
    let pool = open_checked(&fixture.ledger(), &fixture.repository(), &[])
        .await
        .unwrap();
    assert!(sqlx::query("CREATE TABLE forbidden_write (id INTEGER)")
        .execute(&pool)
        .await
        .is_err());
    // Even disabling the connection's query-only pragma cannot defeat read-only opening.
    sqlx::query("PRAGMA query_only=OFF")
        .execute(&pool)
        .await
        .unwrap();
    assert!(sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&pool)
        .await
        .is_err());
    pool.close().await;
    assert_eq!(std::fs::read(fixture.ledger()).unwrap(), original);
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 2);

    for mutation in [
        "DELETE FROM _sqlx_migrations WHERE version=(SELECT MAX(version) FROM _sqlx_migrations)",
        "UPDATE _sqlx_migrations SET version=999 WHERE version=(SELECT MAX(version) FROM _sqlx_migrations)",
        "UPDATE _sqlx_migrations SET success=0",
        "UPDATE _sqlx_migrations SET checksum=X'00'",
    ] {
        std::fs::write(fixture.ledger(), &original).unwrap();
        let write_pool = SqlitePoolOptions::new().max_connections(1).connect_with(
            SqliteConnectOptions::new().filename(fixture.ledger()).create_if_missing(false)
        ).await.unwrap();
        sqlx::query(mutation).execute(&write_pool).await.unwrap();
        write_pool.close().await;
        let before = std::fs::read(fixture.ledger()).unwrap();
        assert!(open_checked(&fixture.ledger(), &fixture.repository(), &[]).await.unwrap_err().contains("schema"));
        assert_eq!(std::fs::read(fixture.ledger()).unwrap(), before);
    }
}

#[tokio::test]
async fn other_checkouts_and_backup_directories_are_refused_before_opening() {
    let fixture = Fixture::new();
    fixture.seed(true).await;
    for git_entry_is_directory in [false, true] {
        let checkout = fixture.0.join(if git_entry_is_directory {
            "clone"
        } else {
            "worktree"
        });
        let nested = checkout.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        if git_entry_is_directory {
            std::fs::create_dir(checkout.join(".git")).unwrap();
        } else {
            std::fs::write(checkout.join(".git"), b"synthetic worktree marker").unwrap();
        }
        // A hard link retains its own path: the ancestor guard must still reject it.
        let linked = nested.join("synthetic.sqlite");
        std::fs::hard_link(fixture.ledger(), &linked).unwrap();
        assert!(open_checked(&linked, &fixture.repository(), &[])
            .await
            .unwrap_err()
            .contains("working tree"));
    }
    let backup = fixture.0.join("backups");
    let nested = backup.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let original = nested.join("synthetic.sqlite");
    std::fs::hard_link(fixture.ledger(), &original).unwrap();
    assert!(open_checked_with_backups(
        &original,
        &fixture.repository(),
        &[],
        std::slice::from_ref(&backup)
    )
    .await
    .unwrap_err()
    .contains("backup directory"));
    let pool = open_checked_with_backups(&fixture.ledger(), &fixture.repository(), &[], &[backup])
        .await
        .unwrap();
    pool.close().await;
}

#[test]
fn replay_statistics_require_both_event_and_instrument_floors() {
    assert!(!replay_statistics_allowed(4, 3));
    assert!(!replay_statistics_allowed(50, 2));
    assert!(replay_statistics_allowed(5, 3));
}
