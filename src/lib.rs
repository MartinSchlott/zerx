//! zerx — structured schema validation for Rust.

mod error;
mod value;

pub use error::{ErrorCode, ZerxError};
pub use value::{Map, ZerxValue};
