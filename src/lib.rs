//! zerx — structured schema validation for Rust.

mod error;
mod value;
mod schema;
mod types;

pub use error::{ErrorCode, ZerxError};
pub use value::{Map, ZerxValue};
pub use schema::{Schema, Modify, Validator, AnySchema, LazySchema, any, lazy, MAX_PARSE_DEPTH};
pub use types::{string, number, boolean, enumerate, null, StringSchema, NumberSchema, BooleanSchema, EnumSchema, NullSchema};
pub use types::{object, array, record, tuple, union, discriminated_union, literal, ObjectSchema, ArraySchema, RecordSchema, TupleSchema, UnionSchema, DiscriminatedUnionSchema, LiteralSchema};
