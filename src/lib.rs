//! zerx — structured schema validation for Rust.

mod error;
mod value;
mod schema;

pub use error::{ErrorCode, ZerxError};
pub use value::{Map, ZerxValue};
pub use schema::{Schema, Modify, Validator, AnySchema, LazySchema, any, lazy, MAX_PARSE_DEPTH};
