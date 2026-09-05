use crate::{
    config::AppConfig,
    db::{self, CreateMissing, OpenedLedger},
    startup_error::StartupError,
};

pub async fn open(config: &AppConfig) -> Result<OpenedLedger, StartupError> {
    let create = if config.create_ledger_if_missing {
        CreateMissing::Yes
    } else {
        CreateMissing::No
    };
    let opened = db::open(&config.ledger, create)
        .await
        .map_err(|source| StartupError::ledger_open(&config.ledger, config.mode, source))?;
    db::migrate(&opened.pool)
        .await
        .map_err(|source| StartupError::MigrationFailed {
            path: config.ledger.path.clone(),
            source: Box::new(source),
        })?;
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr},
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config::{AppConfig, Mode},
        ledger::resolve,
    };

    #[tokio::test]
    async fn creates_and_migrates_when_creation_is_explicit() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = PathBuf::from("target/test-ledger-open");
        fs::create_dir_all(&directory).expect("test directory");
        let path = directory.join(format!("created-{unique}.sqlite"));
        let config = AppConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8480,
            ledger: resolve(
                &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
                Mode::Production,
            )
            .expect("location"),
            static_assets_dir: directory.clone(),
            mode: Mode::Production,
            create_ledger_if_missing: true,
            market_data_refresh_enabled: false,
            launch_refresh_enabled: false,
        };

        let opened = super::open(&config).await.expect("ledger should open");
        assert!(opened.created_now);
        assert_eq!(
            crate::db::pending_migrations(&opened.pool)
                .await
                .expect("migration status"),
            0
        );
        opened.pool.close().await;
        fs::remove_file(path).expect("remove test ledger");
    }
}
