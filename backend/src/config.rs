use std::{
    env,
    error::Error,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_PORT: u16 = 8080;
const HOST_ENV: &str = "TTTB_HOST";
const PORT_ENV: &str = "TTTB_PORT";
const HOSTING_PORT_ENV: &str = "PORT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub host: IpAddr,
    pub port: u16,
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

        Ok(Self { host, port })
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST,
            port: DEFAULT_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    variable: &'static str,
    value: String,
    message: &'static str,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} value {:?}: {}",
            self.variable, self.value, self.message
        )
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
        Ok(value) if value.trim().is_empty() => Err(ConfigError {
            variable,
            value,
            message: "must not be empty",
        }),
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(value)) => Err(ConfigError {
            variable,
            value: value.to_string_lossy().into_owned(),
            message: "must be valid Unicode",
        }),
    }
}

fn parse_host(variable: &'static str, value: &str) -> Result<IpAddr, ConfigError> {
    value.parse().map_err(|_| ConfigError {
        variable,
        value: value.to_owned(),
        message: "must be an IP address",
    })
}

fn parse_port(variable: &'static str, value: &str) -> Result<u16, ConfigError> {
    value.parse().map_err(|_| ConfigError {
        variable,
        value: value.to_owned(),
        message: "must be a TCP port number",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        ffi::OsString,
        sync::{Mutex, MutexGuard},
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_config_uses_local_backend_port() {
        let config = AppConfig::default();

        assert_eq!(config.host, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.port, 8080);
        assert_eq!(config.socket_addr().to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn from_env_uses_tttb_port_before_hosting_port() {
        let _guard = TestEnv::new(&[
            (HOST_ENV, None),
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
            (HOST_ENV, Some("0.0.0.0")),
            (PORT_ENV, None),
            (HOSTING_PORT_ENV, Some("3000")),
        ]);

        let config = AppConfig::from_env().expect("config should load");

        assert_eq!(
            config.host,
            "0.0.0.0".parse::<IpAddr>().expect("valid test IP")
        );
        assert_eq!(config.port, 3000);
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
    fn parse_host_rejects_non_ip_values() {
        let error = parse_host(HOST_ENV, "localhost").expect_err("host must be an IP address");

        assert_eq!(error.variable, HOST_ENV);
        assert_eq!(error.value, "localhost");
        assert_eq!(error.message, "must be an IP address");
    }

    #[test]
    fn parse_port_rejects_non_port_values() {
        let error = parse_port(PORT_ENV, "70000").expect_err("port must fit in u16");

        assert_eq!(error.variable, PORT_ENV);
        assert_eq!(error.value, "70000");
        assert_eq!(error.message, "must be a TCP port number");
    }

    struct TestEnv {
        _lock: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl TestEnv {
        fn new(values: &[(&'static str, Option<&str>)]) -> Self {
            let lock = ENV_LOCK.lock().expect("env lock should not be poisoned");
            let saved = values
                .iter()
                .map(|(variable, _)| (*variable, env::var_os(variable)))
                .collect();

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
}
