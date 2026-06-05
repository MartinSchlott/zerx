//! zerx — structured schema validation for Rust.

mod error;
mod value;
mod schema;
mod types;
mod json_schema;
mod delta;
mod policy;
#[cfg(feature = "lua")]
pub mod lua;

pub use error::{ErrorCode, ZerxError};
pub use value::{Map, ZerxValue};
pub use schema::{Schema, Modify, Validator, AnySchema, LazySchema, any, lazy, MAX_PARSE_DEPTH};
pub use types::{string, number, boolean, enumerate, null, StringSchema, NumberSchema, BooleanSchema, EnumSchema, NullSchema};
pub use types::{object, array, record, tuple, union, discriminated_union, literal, ObjectSchema, ArraySchema, RecordSchema, TupleSchema, UnionSchema, DiscriminatedUnionSchema, LiteralSchema};
pub use types::{buffer, uri, url, json, jsonschema, BufferSchema, UriSchema, UrlSchema, JsonSchema, JsonschemaSchema};
pub use json_schema::{ExportOptions, DRAFT_2020_12, from_json_schema};
pub use policy::{register_policy, from_json_schema_with, apply_type_transforms,
    Policy, ImportOptions, SchemaTransform, TypeTransform, RefResolver};
#[cfg(feature = "lua")]
pub use lua::{LuaValue, LuaTable};
