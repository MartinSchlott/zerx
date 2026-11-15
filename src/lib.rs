//! zerx — structured schema validation for Rust.

mod delta;
mod error;
mod json_schema;
#[cfg(feature = "lua")]
pub mod lua;
mod policy;
mod schema;
mod types;
mod value;

pub use error::{ErrorCode, ZerxError};
pub use json_schema::{from_json_schema, ExportOptions, DRAFT_2020_12};
#[cfg(feature = "lua")]
pub use lua::{LuaTable, LuaValue};
pub use policy::{
    apply_type_transforms, from_json_schema_with, register_policy, ImportOptions, Policy,
    RefResolver, SchemaTransform, TypeTransform,
};
pub use schema::{any, lazy, AnySchema, LazySchema, Modify, Schema, Validator, MAX_PARSE_DEPTH};
pub use types::{
    array, discriminated_union, literal, object, record, tuple, union, ArraySchema,
    DiscriminatedUnionSchema, LiteralSchema, ObjectSchema, RecordSchema, TupleSchema, UnionSchema,
};
pub use types::{
    boolean, enumerate, null, number, string, BooleanSchema, EnumSchema, NullSchema, NumberSchema,
    StringSchema,
};
pub use types::{
    buffer, json, jsonschema, uri, url, BufferSchema, JsonSchema, JsonschemaSchema, UriSchema,
    UrlSchema,
};
pub use value::{Map, ZerxValue};
