use std::{
    env,
    error::Error,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

pub use crate::mode::Mode;

use crate::ledger::{resolve, LedgerLocation, LedgerLocationError};

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 8480;
const DEFAULT_STATIC_ASSETS_DIR: &str = "../frontend/dist";
pub const MODE_ENV: &str = "TTTB_MODE";
pub const DATABASE_URL_ENV: &str = "TTTB_DATABASE_URL";
pub const CREATE_LEDGER_IF_MISSING_ENV: &str = "TTTB_CREATE_LEDGER_IF_MISSING";
const HOST_ENV: &str = "TTTB_HOST";
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";
const MARKET_DATA_REFRESH_ENABLED_ENV: &str = "TTTB_MARKET_DATA_REFRESH_ENABLED";
const MARKET_DATA_LAUNCH_REFRESH_ENABLED_ENV: &str = "TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED";
const PORT_ENV: &str = "TTTB_PORT";
const HOSTING_PORT_ENV: &str = "PORT";
const STATIC_ASSETS_DIR_ENV: &str = "TTTB_STATIC_DIR";
pub const BACKUP_ENABLED_ENV: &str = "TTTB_BACKUP_ENABLED";
pub const BACKUP_DIR_ENV: &str = "TTTB_BACKUP_DIR";
pub const LOG_FILE_ENV: &str = "TTTB_LOG_FILE";
const ONE_DRIVE_ENV: &str = "OneDrive";

impl Mode {
    pub fn asset_policy(self) -> AssetPolicy {
        match self {
            Self::Production => AssetPolicy::Required,
            Self::Development | Self::Demo => AssetPolicy::Optional,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetPolicy {
    Required,
    Optional,
}

/// A backup destination which may intentionally remain unresolved until startup.
/// Missing OneDrive is a backup failure, not a configuration parsing failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupDirectory {
    Resolved(PathBuf),
    Unresolved(String),
}

impl BackupDirectory {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Resolved(path) => Some(path),
            Self::Unresolved(_) => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Resolved(path) => path.display().to_string(),
            Self::Unresolved(reason) => format!("unresolved: {reason}"),
        }
    }
}

/// A log destination which may intentionally remain unresolved until startup.
/// Logging failures degrade to terminal output instead of rejecting a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogFile {
    Resolved(PathBuf),
    Unresolved(String),
}

impl LogFile {
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Resolved(path) => Some(path),
            Self::Unresolved(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub host: IpAddr,
    pub port: u16,
    pub ledger: LedgerLocation,
    pub static_assets_dir: PathBuf,
    pub mode: Mode,
    pub create_ledger_if_missing: bool,
    pub backup_enabled: bool,
    pub backup_dir: BackupDirectory,
    pub log_file: LogFile,
    pub market_data_refresh_enabled: bool,
    pub launch_refresh_enabled: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let host = read_optional(HOST_ENV)?
            .map(|value| parse_host(HOST_ENV, &value))
            .transpose()?
            .unwrap_or(DEFAULT_HOST);

        let port = match read_optional(PORT_ENV)? {
            Some(value) => parse_port(PORT_ENV, &value)?,
            // PORT is a hosting-platform fallback; TTTB_PORT always wins locally.
            None => read_optional(HOSTING_PORT_ENV)?
                .map(|value| parse_port(HOSTING_PORT_ENV, &value))
                .transpose()?
                .unwrap_or(DEFAULT_PORT),
        };

        let mode = read_optional(MODE_ENV)?
            .map(|value| parse_mode(MODE_ENV, &value))
            .transpose()?
            .unwrap_or(Mode::Production);

        let database_url = match read_optional(DATABASE_URL_ENV)? {
            Some(url) => url,
            None if mode.is_demo() => "sqlite::memory:".to_owned(),
            None => default_database_url(mode)?,
        };

        let ledger = resolve(&database_url, mode).map_err(ConfigError::from_location)?;

        let create_ledger_if_missing = read_optional(CREATE_LEDGER_IF_MISSING_ENV)?
            .map(|value| parse_bool(CREATE_LEDGER_IF_MISSING_ENV, &value))
            .transpose()?
            .unwrap_or(false);

        let backup_enabled = read_optional(BACKUP_ENABLED_ENV)?
            .map(|value| parse_bool(BACKUP_ENABLED_ENV, &value))
            .transpose()?
            .unwrap_or(!mode.is_demo());
        let backup_dir = resolve_backup_dir(mode);
        let log_file = resolve_log_file(mode);

        let market_data_refresh_enabled = read_optional(MARKET_DATA_REFRESH_ENABLED_ENV)?
            .map(|value| parse_bool(MARKET_DATA_REFRESH_ENABLED_ENV, &value))
            .transpose()?
            .unwrap_or(true);

        let launch_refresh_enabled = read_optional(MARKET_DATA_LAUNCH_REFRESH_ENABLED_ENV)?
            .map(|value| parse_bool(MARKET_DATA_LAUNCH_REFRESH_ENABLED_ENV, &value))
            .transpose()?
            .unwrap_or(true);

        let static_assets_dir = read_optional(STATIC_ASSETS_DIR_ENV)?
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATIC_ASSETS_DIR));

        Ok(Self {
            host,
            port,
            ledger,
            static_assets_dir,
            mode,
            create_ledger_if_missing,
            backup_enabled,
            backup_dir,
            log_file,
            market_data_refresh_enabled,
            launch_refresh_enabled,
        })
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }

    pub fn static_assets_dir(&self) -> &Path {
        &self.static_assets_dir
    }

    pub fn asset_policy(&self) -> AssetPolicy {
        self.mode.asset_policy()
    }
}

fn resolve_log_file(mode: Mode) -> LogFile {
    match read_optional_result(LOG_FILE_ENV, env::var(LOG_FILE_ENV)) {
        Ok(Some(path)) => LogFile::Resolved(PathBuf::from(path)),
        Ok(None) => log_file_from_result(env::var(LOCAL_APP_DATA_ENV), mode),
        Err(error) => LogFile::Unresolved(error.to_string()),
    }
}

fn log_file_from_result(result: Result<String, env::VarError>, mode: Mode) -> LogFile {
    match read_optional_result(LOCAL_APP_DATA_ENV, result) {
        Ok(Some(root)) => {
            let file = match mode {
                Mode::Production => "engine.log",
                Mode::Development => "engine-development.log",
                Mode::Demo => "engine-demo.log",
            };
            LogFile::Resolved(
                PathBuf::from(root)
                    .join("TickerTapeTallyBoard")
                    .join("logs")
                    .join(file),
            )
        }
        Ok(None) => LogFile::Unresolved(format!("{LOCAL_APP_DATA_ENV} is not set")),
        Err(error) => LogFile::Unresolved(error.to_string()),
    }
}

fn resolve_backup_dir(mode: Mode) -> BackupDirectory {
    if mode.is_demo() {
        return BackupDirectory::Unresolved("demo mode does not use backups".to_owned());
    }
    if let Some(directory) = backup_directory_from_env(BACKUP_DIR_ENV, &[]) {
        return directory;
    }
    match mode {
        Mode::Production => {
            backup_directory_from_env(ONE_DRIVE_ENV, &["TickerTapeTallyBoard", "Backups"])
                .unwrap_or_else(|| {
                    BackupDirectory::Unresolved(format!("{ONE_DRIVE_ENV} is not set"))
                })
        }
        Mode::Development => {
            backup_directory_from_env(LOCAL_APP_DATA_ENV, &["TickerTapeTallyBoard", "backups-dev"])
                .unwrap_or_else(|| {
                    BackupDirectory::Unresolved(format!("{LOCAL_APP_DATA_ENV} is not set"))
                })
        }
        Mode::Demo => unreachable!(),
    }
}

fn backup_directory_from_env(variable: &'static str, suffix: &[&str]) -> Option<BackupDirectory> {
    backup_directory_from_result(variable, suffix, env::var(variable))
}

fn backup_directory_from_result(
    variable: &'static str,
    suffix: &[&str],
    result: Result<String, env::VarError>,
) -> Option<BackupDirectory> {
    match read_optional_result(variable, result) {
        Ok(Some(root)) => {
            let mut path = PathBuf::from(root);
            for part in suffix {
                path.push(part);
            }
            Some(BackupDirectory::Resolved(path))
        }
        Ok(None) => None,
        Err(error) => Some(BackupDirectory::Unresolved(error.to_string())),
    }
}

fn default_database_url(mode: Mode) -> Result<String, ConfigError> {
    let root = read_optional(LOCAL_APP_DATA_ENV)?.ok_or_else(|| {
        ConfigError::value(
            LOCAL_APP_DATA_ENV,
            String::new(),
            "must be set when TTTB_DATABASE_URL is not provided outside demo mode",
        )
    })?;
    let file = match mode {
        Mode::Production => "portfolio.sqlite",
        Mode::Development => "portfolio-dev.sqlite",
        Mode::Demo => unreachable!(),
    };
    Ok(format!(
        "sqlite://{}",
        PathBuf::from(root)
            .join("TickerTapeTallyBoard")
            .join(file)
            .to_string_lossy()
            .replace('\\', "/")
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub(crate) variable: &'static str,
    pub(crate) value: String,
    pub(crate) message: &'static str,
    location: Option<LedgerLocationError>,
}

impl ConfigError {
    fn value(variable: &'static str, value: String, message: &'static str) -> Self {
        Self {
            variable,
            value,
            message,
            location: None,
        }
    }

    fn from_location(error: LedgerLocationError) -> Self {
        Self {
            variable: DATABASE_URL_ENV,
            value: String::new(),
            message: "must be a supported ledger location for the selected mode",
            location: Some(error),
        }
    }

    pub(crate) fn into_location(mut self) -> Result<LedgerLocationError, Self> {
        match self.location.take() {
            Some(error) => Ok(error),
            None => Err(self),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(error) => write!(formatter, "{error}"),
            None => write!(
                formatter,
                "invalid {} value {:?}: {}",
                self.variable, self.value, self.message
            ),
        }
    }
}

impl Error for ConfigError {}

fn read_optional(variable: &'static str) -> Result<Option<String>, ConfigError> {
    read_optional_result(variable, env::var(variable))
}

fn read_optional_result(
    variable: &'static str,
    result: Result<String, env::VarError>,
) -> Result<Option<String>, ConfigError> {
    match result {
        Ok(value) if value.trim().is_empty() => {
            Err(ConfigError::value(variable, value, "must not be empty"))
        }
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(value)) => Err(ConfigError::value(
            variable,
            value.to_string_lossy().into_owned(),
            "must be valid Unicode",
        )),
    }
}

fn parse_host(variable: &'static str, value: &str) -> Result<IpAddr, ConfigError> {
    let host: IpAddr = value
        .parse()
        .map_err(|_| ConfigError::value(variable, value.to_owned(), "must be an IP address"))?;

    if host.is_loopback() {
        Ok(host)
    } else {
        Err(ConfigError::value(
            variable,
            value.to_owned(),
            "must be a loopback address; LAN exposure is separate future work",
        ))
    }
}

fn parse_port(variable: &'static str, value: &str) -> Result<u16, ConfigError> {
    value
        .parse()
        .map_err(|_| ConfigError::value(variable, value.to_owned(), "must be a TCP port number"))
}

fn parse_bool(variable: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError::value(
            variable,
            value.to_owned(),
            "must be a boolean value",
        )),
    }
}

fn parse_mode(variable: &'static str, value: &str) -> Result<Mode, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" => Ok(Mode::Production),
        "development" => Ok(Mode::Development),
        "demo" => Ok(Mode::Demo),
        _ => Err(ConfigError::value(
            variable,
            value.to_owned(),
            "must be production, development, or demo",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        ffi::OsString,
        fs,
        sync::{Mutex, MutexGuard},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::startup_error::StartupError;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ALL: &[&str] = &[
        HOST_ENV,
        DATABASE_URL_ENV,
        MODE_ENV,
        CREATE_LEDGER_IF_MISSING_ENV,
        LOCAL_APP_DATA_ENV,
        MARKET_DATA_REFRESH_ENABLED_ENV,
        MARKET_DATA_LAUNCH_REFRESH_ENABLED_ENV,
        PORT_ENV,
        HOSTING_PORT_ENV,
        STATIC_ASSETS_DIR_ENV,
        BACKUP_ENABLED_ENV,
        BACKUP_DIR_ENV,
        LOG_FILE_ENV,
        ONE_DRIVE_ENV,
    ];

    #[test]
    fn mode_defaults_and_asset_policy() {
        let _guard = TestEnv::new(&[(LOCAL_APP_DATA_ENV, Some("C:/temp/appdata"))]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(config.host, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.port, 8480);
        assert_eq!(config.socket_addr().to_string(), "127.0.0.1:8480");
        assert_eq!(config.mode, Mode::Production);
        assert_eq!(config.asset_policy(), AssetPolicy::Required);
        assert_eq!(Mode::Development.asset_policy(), AssetPolicy::Optional);
        assert_eq!(Mode::Demo.asset_policy(), AssetPolicy::Optional);
        assert_eq!(
            config.static_assets_dir,
            PathBuf::from(DEFAULT_STATIC_ASSETS_DIR)
        );
        assert!(!config.create_ledger_if_missing);
        assert!(config.market_data_refresh_enabled);
        assert!(config.launch_refresh_enabled);
        assert_eq!(
            config.log_file.path(),
            Some(Path::new(
                "C:/temp/appdata/TickerTapeTallyBoard/logs/engine.log"
            ))
        );
    }

    #[test]
    fn modes_have_expected_defaults_and_demo_ignores_url_without_touching_it() {
        for (mode, file) in [
            ("production", "portfolio.sqlite"),
            ("development", "portfolio-dev.sqlite"),
        ] {
            let _guard = TestEnv::new(&[
                (MODE_ENV, Some(mode)),
                (LOCAL_APP_DATA_ENV, Some("C:/temp/appdata")),
            ]);

            assert!(AppConfig::from_env()
                .expect("config should load")
                .ledger
                .url
                .ends_with(file));
        }

        let path = unique_path("ignored-demo-ledger", "sqlite");
        let database_url = sqlite_url(&path);
        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (DATABASE_URL_ENV, Some(&database_url)),
        ]);

        let config = AppConfig::from_env().expect("demo config should load");

        assert!(config.ledger.path.is_none());
        assert_ne!(config.ledger.url, database_url);
        assert!(!path.exists());
    }

    #[test]
    fn from_env_uses_tttb_port_before_hosting_port() {
        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (PORT_ENV, Some("9090")),
            (HOSTING_PORT_ENV, Some("3000")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(config.host, DEFAULT_HOST);
        assert_eq!(config.port, 9090);
    }

    #[test]
    fn from_env_uses_hosting_port_when_tttb_port_is_missing() {
        let _guard = TestEnv::new(&[
            (HOST_ENV, Some("127.0.0.2")),
            (MODE_ENV, Some("demo")),
            (HOSTING_PORT_ENV, Some("3000")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(
            config.host,
            "127.0.0.2".parse::<IpAddr>().expect("valid test IP")
        );
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn from_env_uses_static_assets_dir_override() {
        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (STATIC_ASSETS_DIR_ENV, Some("C:/tttb/static")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(config.static_assets_dir, PathBuf::from("C:/tttb/static"));
    }

    #[test]
    fn from_env_uses_database_url_override() {
        let database_url = sqlite_url(&unique_path("database-override", "sqlite"));
        let _guard = TestEnv::new(&[(DATABASE_URL_ENV, Some(&database_url))]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(config.ledger.url, database_url);
    }

    #[test]
    fn from_env_uses_create_ledger_flag() {
        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (CREATE_LEDGER_IF_MISSING_ENV, Some("1")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert!(config.create_ledger_if_missing);
    }

    #[test]
    fn read_optional_rejects_empty_values() {
        let error = read_optional_result(HOST_ENV, Ok("  ".to_owned()))
            .expect_err("empty value should fail");

        assert_eq!(error.variable, HOST_ENV);
        assert_eq!(error.message, "must not be empty");
    }

    #[test]
    fn read_optional_rejects_non_unicode_values() {
        let error = read_optional_result(
            HOST_ENV,
            Err(env::VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode value should fail");

        assert_eq!(error.variable, HOST_ENV);
        assert_eq!(error.value, "not-unicode");
        assert_eq!(error.message, "must be valid Unicode");
    }

    #[test]
    fn invalid_backup_path_variables_become_unresolved_instead_of_config_errors() {
        let cases = [
            (BACKUP_DIR_ENV, Mode::Production, DATABASE_URL_ENV),
            (ONE_DRIVE_ENV, Mode::Production, DATABASE_URL_ENV),
            (LOCAL_APP_DATA_ENV, Mode::Development, DATABASE_URL_ENV),
        ];

        for (variable, mode, database_variable) in cases {
            let database_url = sqlite_url(&unique_path("backup-config", "sqlite"));
            let mode_value = mode.as_str();
            let _guard = TestEnv::new(&[
                (MODE_ENV, Some(mode_value)),
                (database_variable, Some(&database_url)),
                (variable, Some("   ")),
            ]);

            let config = AppConfig::from_env().expect("backup path error must not reject config");

            assert!(matches!(config.backup_dir, BackupDirectory::Unresolved(_)));
        }

        for variable in [BACKUP_DIR_ENV, ONE_DRIVE_ENV, LOCAL_APP_DATA_ENV] {
            let directory = backup_directory_from_result(
                variable,
                &[],
                Err(env::VarError::NotUnicode(OsString::from("not-unicode"))),
            )
            .expect("invalid variable should produce an unresolved directory");
            assert!(matches!(directory, BackupDirectory::Unresolved(_)));
        }
    }

    #[test]
    fn log_file_uses_mode_specific_app_data_defaults_and_allows_an_override() {
        for (mode, file) in [
            ("production", "engine.log"),
            ("development", "engine-development.log"),
            ("demo", "engine-demo.log"),
        ] {
            let database_url = sqlite_url(&unique_path("log-config", "sqlite"));
            let _guard = TestEnv::new(&[
                (MODE_ENV, Some(mode)),
                (DATABASE_URL_ENV, Some(&database_url)),
                (LOCAL_APP_DATA_ENV, Some("C:/temp/appdata")),
            ]);
            let config = AppConfig::from_env().expect("config should load");
            assert_eq!(
                config.log_file.path(),
                Some(Path::new(&format!(
                    "C:/temp/appdata/TickerTapeTallyBoard/logs/{file}"
                )))
            );
        }

        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (LOG_FILE_ENV, Some("C:/logs/override.log")),
        ]);
        let config = AppConfig::from_env().expect("override should load");
        assert_eq!(
            config.log_file.path(),
            Some(Path::new("C:/logs/override.log"))
        );
    }

    #[test]
    fn unresolved_default_log_path_degrades_instead_of_rejecting_config() {
        let database_url = sqlite_url(&unique_path("log-unresolved", "sqlite"));
        let _guard = TestEnv::new(&[
            (DATABASE_URL_ENV, Some(&database_url)),
            (LOCAL_APP_DATA_ENV, Some("   ")),
        ]);
        let config = AppConfig::from_env().expect("log path failure must not reject config");
        assert!(matches!(config.log_file, LogFile::Unresolved(_)));
    }

    #[test]
    fn blank_log_file_override_degrades_instead_of_rejecting_config() {
        let _guard = TestEnv::new(&[(MODE_ENV, Some("demo")), (LOG_FILE_ENV, Some("   "))]);

        let config = AppConfig::from_env().expect("blank log path must not reject config");

        assert!(matches!(
            config.log_file,
            LogFile::Unresolved(reason) if reason.contains(LOG_FILE_ENV)
        ));
    }

    #[test]
    fn parse_host_rejects_non_ip_values() {
        let error = parse_host(HOST_ENV, "localhost").expect_err("host must be an IP address");

        assert_eq!(error.variable, HOST_ENV);
        assert_eq!(error.value, "localhost");
        assert_eq!(error.message, "must be an IP address");
    }

    #[test]
    fn parse_host_rejects_non_loopback_with_guidance() {
        let error = parse_host(HOST_ENV, "0.0.0.0").expect_err("host must be loopback");
        let message = error.to_string();

        assert!(message.contains("0.0.0.0"));
        assert!(message.contains("loopback"));
        assert!(message.contains("LAN exposure is separate future work"));
    }

    #[test]
    fn parse_host_accepts_ipv4_and_ipv6_loopback() {
        for host in ["127.0.0.1", "127.0.0.2", "::1"] {
            assert!(parse_host(HOST_ENV, host).is_ok(), "{host}");
        }
    }

    #[test]
    fn parse_port_rejects_non_port_values() {
        let error = parse_port(PORT_ENV, "70000").expect_err("port must fit in u16");

        assert_eq!(error.variable, PORT_ENV);
        assert_eq!(error.value, "70000");
        assert_eq!(error.message, "must be a TCP port number");
    }

    #[test]
    fn parse_bool_rejects_invalid_values() {
        let error = parse_bool(MARKET_DATA_REFRESH_ENABLED_ENV, "sometimes")
            .expect_err("invalid boolean value should fail");

        assert_eq!(error.variable, MARKET_DATA_REFRESH_ENABLED_ENV);
        assert_eq!(error.value, "sometimes");
        assert_eq!(error.message, "must be a boolean value");
    }

    #[test]
    fn unknown_mode_is_rejected() {
        let _guard = TestEnv::new(&[(MODE_ENV, Some("staging"))]);

        let error = AppConfig::from_env().expect_err("unknown mode should fail");

        assert_eq!(error.variable, MODE_ENV);
        assert_eq!(error.value, "staging");
        assert_eq!(error.message, "must be production, development, or demo");
    }

    #[test]
    fn from_env_uses_refresh_flags() {
        let _guard = TestEnv::new(&[
            (MODE_ENV, Some("demo")),
            (MARKET_DATA_REFRESH_ENABLED_ENV, Some("false")),
            (MARKET_DATA_LAUNCH_REFRESH_ENABLED_ENV, Some("1")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert!(!config.market_data_refresh_enabled);
        assert!(config.launch_refresh_enabled);
    }

    #[test]
    fn local_app_data_is_required_outside_demo_without_override() {
        let _guard = TestEnv::new(&[]);

        let error = AppConfig::from_env().expect_err("missing app data should fail");
        let message = error.to_string();

        assert!(message.contains("LOCALAPPDATA"));
        assert!(message.contains("TTTB_DATABASE_URL"));
    }

    #[test]
    fn in_memory_production_url_becomes_typed_startup_error() {
        let _guard = TestEnv::new(&[(DATABASE_URL_ENV, Some("sqlite::memory:"))]);

        let error = StartupError::from(AppConfig::from_env().expect_err("memory should fail"));

        assert!(matches!(
            error,
            StartupError::LedgerMustBeFileBacked {
                mode: Mode::Production,
                ..
            }
        ));
    }

    #[test]
    fn unsupported_url_becomes_typed_startup_error() {
        let _guard = TestEnv::new(&[(DATABASE_URL_ENV, Some("postgres://ledger"))]);

        let error = StartupError::from(AppConfig::from_env().expect_err("URL should fail"));

        assert!(matches!(error, StartupError::LedgerUnsupportedUrl { .. }));
    }

    #[test]
    fn directory_path_becomes_typed_startup_error() {
        let directory = unique_path("ledger-directory", "dir");
        fs::create_dir_all(&directory).expect("test directory should be created");
        let database_url = sqlite_url(&directory);
        let _guard = TestEnv::new(&[(DATABASE_URL_ENV, Some(&database_url))]);

        let error = StartupError::from(AppConfig::from_env().expect_err("directory should fail"));

        assert!(matches!(error, StartupError::LedgerNotAFile { .. }));
        fs::remove_dir(directory).expect("test directory should be removed");
    }

    struct TestEnv {
        _lock: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl TestEnv {
        fn new(values: &[(&'static str, Option<&str>)]) -> Self {
            let lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
            let saved = ALL
                .iter()
                .map(|variable| (*variable, env::var_os(variable)))
                .collect();

            for variable in ALL {
                env::remove_var(variable);
            }

            for (variable, value) in values {
                match value {
                    Some(value) => env::set_var(variable, value),
                    None => env::remove_var(variable),
                }
            }

            Self { _lock: lock, saved }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            for (variable, value) in self.saved.drain(..) {
                match value {
                    Some(value) => env::set_var(variable, value),
                    None => env::remove_var(variable),
                }
            }
        }
    }

    fn unique_path(stem: &str, extension: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        env::current_dir()
            .expect("current directory should resolve")
            .join("target")
            .join("test-config")
            .join(format!("{stem}-{unique}.{extension}"))
    }

    fn sqlite_url(path: &Path) -> String {
        format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"))
    }
}
