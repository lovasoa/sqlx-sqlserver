use percent_encoding::percent_decode_str;
use sqlx_core::connection::ConnectOptions;
use sqlx_core::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use url::Url;

use crate::MssqlConnection;

/// SQL Server connection encryption preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encrypt {
    /// Encryption is not supported by the client.
    NotSupported,
    /// Use encryption when the server supports it.
    Off,
    /// Require encryption.
    On,
    /// Require encryption and certificate validation.
    Required,
}

/// SQL Server connection options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlConnectOptions {
    host: String,
    port: Option<u16>,
    username: String,
    password: Option<String>,
    database: String,
    instance: Option<String>,
    encrypt: Encrypt,
    trust_server_certificate: bool,
    hostname_in_certificate: Option<String>,
    ssl_root_cert: Option<PathBuf>,
    requested_packet_size: u32,
    client_program_version: u32,
    client_pid: u32,
    hostname: String,
    app_name: String,
    server_name: String,
    client_interface_name: String,
    language: String,
}

impl Default for MssqlConnectOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl MssqlConnectOptions {
    /// Creates options with SQL Server defaults.
    pub fn new() -> Self {
        Self {
            host: "localhost".to_owned(),
            port: None,
            username: "sa".to_owned(),
            password: None,
            database: "master".to_owned(),
            instance: None,
            encrypt: Encrypt::On,
            trust_server_certificate: true,
            hostname_in_certificate: None,
            ssl_root_cert: None,
            requested_packet_size: 4096,
            client_program_version: 0,
            client_pid: 0,
            hostname: String::new(),
            app_name: String::new(),
            server_name: String::new(),
            client_interface_name: String::new(),
            language: String::new(),
        }
    }

    /// Parses SQL Server connection options while preserving detailed parser errors.
    pub fn parse_url(input: &str) -> Result<Self, MssqlInvalidOption> {
        parse_url(input)
    }

    /// Returns the configured host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the configured port, if one was explicitly set.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Returns the configured username.
    pub fn username(&self) -> &str {
        &self.username
    }

    /// Returns the configured password.
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Returns the configured database.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// Returns the configured named instance, if any.
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// Returns the encryption preference.
    pub fn encrypt(&self) -> Encrypt {
        self.encrypt
    }

    /// Returns whether server certificate validation is bypassed.
    pub fn trust_server_certificate(&self) -> bool {
        self.trust_server_certificate
    }

    /// Returns the hostname expected in the server certificate.
    pub fn hostname_in_certificate(&self) -> Option<&str> {
        self.hostname_in_certificate.as_deref()
    }

    /// Returns the configured root certificate path.
    pub fn ssl_root_cert(&self) -> Option<&Path> {
        self.ssl_root_cert.as_deref()
    }

    /// Returns the requested TDS packet size.
    pub fn requested_packet_size(&self) -> u32 {
        self.requested_packet_size
    }

    /// Returns the client program version sent during login.
    pub fn client_program_version(&self) -> u32 {
        self.client_program_version
    }

    /// Returns the client process ID sent during login.
    pub fn client_pid(&self) -> u32 {
        self.client_pid
    }

    /// Returns the client host name sent during login.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Returns the application name sent during login.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Returns the server name sent during login.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Returns the client interface name sent during login.
    pub fn client_interface_name(&self) -> &str {
        &self.client_interface_name
    }

    /// Returns the language sent during login.
    pub fn language(&self) -> &str {
        &self.language
    }

    fn set_requested_packet_size(&mut self, size: u32) -> Result<(), MssqlInvalidOption> {
        if size < 512 {
            return Err(MssqlInvalidOption::InvalidValue {
                key: "packet_size".to_owned(),
                value: size.to_string(),
                message: "packet_size must be at least 512 bytes".to_owned(),
            });
        }

        self.requested_packet_size = size;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_hostname_for_test(&mut self, hostname: String) {
        self.hostname = hostname;
    }

    #[cfg(feature = "migrate")]
    pub(crate) fn set_database_for_maintenance(&mut self) {
        self.database = "master".to_owned();
    }
}

impl FromStr for MssqlConnectOptions {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse_url(input).map_err(Error::config)
    }
}

impl ConnectOptions for MssqlConnectOptions {
    type Connection = MssqlConnection;

    fn from_url(url: &Url) -> Result<Self, Error> {
        Self::parse_url(url.as_str()).map_err(Error::config)
    }

    async fn connect(&self) -> Result<Self::Connection, Error>
    where
        Self::Connection: Sized,
    {
        MssqlConnection::establish(self).await
    }

    fn log_statements(self, _level: log::LevelFilter) -> Self {
        self
    }

    fn log_slow_statements(self, _level: log::LevelFilter, _duration: Duration) -> Self {
        self
    }
}

fn parse_url(input: &str) -> Result<MssqlConnectOptions, MssqlInvalidOption> {
    let url = Url::parse(input).map_err(MssqlInvalidOption::Url)?;
    match url.scheme() {
        "mssql" | "sqlserver" => {}
        scheme => return Err(MssqlInvalidOption::UnsupportedScheme(scheme.to_owned())),
    }

    let mut options = MssqlConnectOptions::new();

    if let Some(host) = url.host_str() {
        options.host = host.to_owned();
    }

    options.port = url.port();

    let username = url.username();
    if !username.is_empty() {
        options.username = percent_decode_str(username)
            .decode_utf8()
            .map_err(MssqlInvalidOption::Utf8)?
            .into_owned();
    }

    if let Some(password) = url.password() {
        options.password = Some(
            percent_decode_str(password)
                .decode_utf8()
                .map_err(MssqlInvalidOption::Utf8)?
                .into_owned(),
        );
    }

    let path = url.path().trim_start_matches('/');
    if !path.is_empty() {
        options.database = percent_decode_str(path)
            .decode_utf8()
            .map_err(MssqlInvalidOption::Utf8)?
            .into_owned();
    }

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "instance" => options.instance = Some(value.into_owned()),
            "encrypt" => {
                options.encrypt =
                    parse_encrypt(&value).ok_or_else(|| MssqlInvalidOption::InvalidValue {
                        key: "encrypt".to_owned(),
                        value: value.into_owned(),
                        message: "expected strict, mandatory, optional, not_supported, true, false, yes, or no"
                            .to_owned(),
                    })?;
            }
            "sslrootcert" | "ssl-root-cert" | "ssl-ca" => {
                options.ssl_root_cert = Some(PathBuf::from(value.as_ref()));
            }
            "trust_server_certificate" => {
                options.trust_server_certificate =
                    parse_bool(&value).ok_or_else(|| MssqlInvalidOption::InvalidValue {
                        key: key.into_owned(),
                        value: value.into_owned(),
                        message: "expected true, false, yes, or no".to_owned(),
                    })?;
            }
            "hostname_in_certificate" => {
                options.hostname_in_certificate = Some(value.into_owned());
            }
            "packet_size" => {
                let size = value
                    .parse()
                    .map_err(|_| MssqlInvalidOption::InvalidValue {
                        key: "packet_size".to_owned(),
                        value: value.to_string(),
                        message: "expected an integer".to_owned(),
                    })?;
                options.set_requested_packet_size(size)?;
            }
            "client_program_version" => options.client_program_version = parse_u32(&key, &value)?,
            "client_pid" => options.client_pid = parse_u32(&key, &value)?,
            "hostname" => options.hostname = value.into_owned(),
            "app_name" => options.app_name = value.into_owned(),
            "server_name" => options.server_name = value.into_owned(),
            "client_interface_name" => options.client_interface_name = value.into_owned(),
            "language" => options.language = value.into_owned(),
            _ => return Err(MssqlInvalidOption::UnknownOption(key.into_owned())),
        }
    }

    Ok(options)
}

fn parse_encrypt(value: &str) -> Option<Encrypt> {
    match value.to_ascii_lowercase().as_str() {
        "strict" => Some(Encrypt::Required),
        "mandatory" | "true" | "yes" => Some(Encrypt::On),
        "optional" | "false" | "no" => Some(Encrypt::Off),
        "not_supported" => Some(Encrypt::NotSupported),
        _ => None,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

fn parse_u32(key: &str, value: &str) -> Result<u32, MssqlInvalidOption> {
    value.parse().map_err(|_| MssqlInvalidOption::InvalidValue {
        key: key.to_owned(),
        value: value.to_owned(),
        message: "expected an integer".to_owned(),
    })
}

/// Error returned while parsing SQL Server connection options.
#[derive(Debug, Error)]
pub enum MssqlInvalidOption {
    /// URL syntax was invalid.
    #[error("invalid SQL Server URL: {0}")]
    Url(#[from] url::ParseError),
    /// Percent-decoded URL component was not valid UTF-8.
    #[error("invalid UTF-8 in SQL Server URL component: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// The URL scheme is not supported by this driver.
    #[error("unsupported SQL Server URL scheme `{0}`")]
    UnsupportedScheme(String),
    /// A query parameter is not recognized.
    #[error("unknown SQL Server connection option `{0}`")]
    UnknownOption(String),
    /// A query parameter value is invalid.
    #[error("invalid value `{value}` for SQL Server connection option `{key}`: {message}")]
    InvalidValue {
        /// Option name.
        key: String,
        /// Option value.
        value: String,
        /// Validation message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_username_with_at_sign() {
        let opts =
            MssqlConnectOptions::parse_url("mssql://user%40domain:secret@example.com/database")
                .unwrap();

        assert_eq!("user@domain", opts.username());
        assert_eq!(Some("secret"), opts.password());
    }

    #[test]
    fn parses_password_with_at_sign() {
        let opts =
            MssqlConnectOptions::parse_url("mssql://username:p%40ssw0rd@example.com/database")
                .unwrap();

        assert_eq!(Some("p@ssw0rd"), opts.password());
    }

    #[test]
    fn parses_unescaped_username_with_at_sign() {
        let opts =
            MssqlConnectOptions::parse_url("mssql://user@hostname:password@example.com/database")
                .unwrap();

        assert_eq!("user@hostname", opts.username());
        assert_eq!(Some("password"), opts.password());
    }

    #[test]
    fn parses_unescaped_password_with_at_sign() {
        let opts =
            MssqlConnectOptions::parse_url("mssql://username:p@ssw0rd@example.com/database")
                .unwrap();

        assert_eq!("username", opts.username());
        assert_eq!(Some("p@ssw0rd"), opts.password());
    }

    #[test]
    fn parses_named_instance_without_resolving_port() {
        let opts = MssqlConnectOptions::parse_url(
            "mssql://sa:secret@example.com/master?instance=SQLEXPRESS",
        )
        .unwrap();

        assert_eq!("example.com", opts.host());
        assert_eq!(None, opts.port());
        assert_eq!(Some("SQLEXPRESS"), opts.instance());
    }

    #[test]
    fn keeps_explicit_port_with_named_instance() {
        let opts = MssqlConnectOptions::parse_url(
            "mssql://sa:secret@example.com:1434/master?instance=SQLEXPRESS",
        )
        .unwrap();

        assert_eq!(Some(1434), opts.port());
        assert_eq!(Some("SQLEXPRESS"), opts.instance());
    }

    #[test]
    fn parses_encryption_options() {
        let strict =
            MssqlConnectOptions::parse_url("mssql://localhost/master?encrypt=strict").unwrap();
        let optional =
            MssqlConnectOptions::parse_url("mssql://localhost/master?encrypt=optional").unwrap();
        let disabled =
            MssqlConnectOptions::parse_url("mssql://localhost/master?encrypt=not_supported")
                .unwrap();

        assert_eq!(Encrypt::Required, strict.encrypt());
        assert_eq!(Encrypt::Off, optional.encrypt());
        assert_eq!(Encrypt::NotSupported, disabled.encrypt());
    }

    #[test]
    fn rejects_invalid_packet_size() {
        let err = MssqlConnectOptions::parse_url("mssql://localhost/master?packet_size=128")
            .expect_err("packet_size below 512 should be rejected");

        assert!(err.to_string().contains("packet_size"));
    }

    #[test]
    fn rejects_unknown_options() {
        let err = MssqlConnectOptions::parse_url("mssql://localhost/master?mars=true")
            .expect_err("unsupported options should fail loudly");

        assert!(matches!(err, MssqlInvalidOption::UnknownOption(_)));
    }
}
