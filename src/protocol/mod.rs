//! Small protocol helpers covered by fast unit tests.

/// LOGIN7 packet construction helpers.
pub mod login;
/// TDS packet framing helpers.
pub mod packet;
/// PRELOGIN packet construction and parsing helpers.
pub mod pre_login;
pub(crate) mod query;
pub(crate) mod rpc;
/// Bounded tabular-result token parsing helpers.
pub mod token;
/// TDS TYPE_INFO parsing and value length-prefix helpers.
#[allow(dead_code)]
pub mod type_info;
