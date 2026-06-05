// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use crate::schema::{ParseContext, SchemaKind};
use crate::types::{DiscriminantKey, DiscriminatorState, ObjectMode};
use crate::{ErrorCode, Map, Schema, ZerxError, ZerxValue};

// ---------------------------------------------------------------------------
// Open error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const LUA_INVALID_KEY: ErrorCode = ErrorCode::new("lua_invalid_key");
    pub const LUA_SHAPE_MISMATCH: ErrorCode = ErrorCode::new("lua_shape_mismatch");
}

// ---------------------------------------------------------------------------
// Public input types (mirrors endymion src/value.rs, A1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum LuaValue {
    Nil,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Bytes(Vec<u8>),
    Table(LuaTable),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LuaTable {
    pub array: Vec<LuaValue>,
    pub hash: Vec<(LuaValue, LuaValue)>,
}

// ---------------------------------------------------------------------------
// Schema::validate_lua — public entry point
// ---------------------------------------------------------------------------

impl Schema {
    pub fn validate_lua(&self, lua: &LuaValue) -> Result<ZerxValue, ZerxError> {
        let zv = self.lua_transform(lua, &mut ParseContext::new())?;
        self.parse_present(&zv, &mut ParseContext::new())
    }

    // Resolve Lazy layers, then depth-guard the transform body (R3).
    fn lua_transform(&self, lua: &LuaValue, ctx: &mut ParseContext) -> Result<ZerxValue, ZerxError> {
        // Resolve Lazy layers (mirrors delta.rs:resolve_lazy)
        let mut current = self.clone();
        while let SchemaKind::Lazy(lzy) = &current.kind {
            let inner = lzy.resolve()?;
            current = (*inner).clone();
        }

        // One depth level per lua_transform call; on error do NOT call exit (balanced contract).
        ctx.enter()?;
        let result = current.lua_transform_inner(lua, ctx);
        ctx.exit();
        result
    }

    // Closed dispatch over the resolved SchemaKind. No catch-all arm so a future
    // variant is a compile error.
    fn lua_transform_inner(
        &self,
        lua: &LuaValue,
        ctx: &mut ParseContext,
    ) -> Result<ZerxValue, ZerxError> {
        match &self.kind {
            SchemaKind::Lazy(_) => unreachable!("Lazy resolved before lua_transform_inner"),

            // String-family: Bytes → UTF-8 decode (non-UTF-8 → Bytes; parse yields TYPE_MISMATCH).
            // Table → free-convert (parse yields TYPE_MISMATCH for non-string result).
            SchemaKind::String | SchemaKind::Enum(_) | SchemaKind::Uri | SchemaKind::Url => {
                match lua {
                    LuaValue::Bytes(b) => Ok(bytes_to_string_or_bytes(b)),
                    LuaValue::Table(_) => free_convert(lua),
                    _ => Ok(base_convert(lua)),
                }
            }

            // Literal: if the constant is a String value, treat Bytes as string-family.
            SchemaKind::Literal(c) => {
                if matches!(c, ZerxValue::String(_)) {
                    match lua {
                        LuaValue::Bytes(b) => Ok(bytes_to_string_or_bytes(b)),
                        LuaValue::Table(_) => free_convert(lua),
                        _ => Ok(base_convert(lua)),
                    }
                } else {
                    match lua {
                        LuaValue::Table(_) => free_convert(lua),
                        _ => Ok(base_convert(lua)),
                    }
                }
            }

            // Buffer: keep Bytes raw (no decode). Table → free-convert.
            SchemaKind::Buffer => match lua {
                LuaValue::Bytes(b) => Ok(ZerxValue::Bytes(b.clone())),
                LuaValue::Table(_) => free_convert(lua),
                _ => Ok(base_convert(lua)),
            },

            // Free-convert kinds: any Lua value goes through free_convert.
            SchemaKind::Any | SchemaKind::Json | SchemaKind::JsonSchema => free_convert(lua),

            // Scalar kinds: non-Table → base-convert; Table → free-convert (TYPE_MISMATCH from parse).
            SchemaKind::Number | SchemaKind::Boolean | SchemaKind::Null => match lua {
                LuaValue::Table(_) => free_convert(lua),
                _ => Ok(base_convert(lua)),
            },

            // Array: no hash part allowed; convert elements with item schema in index order.
            SchemaKind::Array(item) => match lua {
                LuaValue::Table(t) => {
                    if !t.hash.is_empty() {
                        return Err(ZerxError::new(
                            ErrorCode::LUA_SHAPE_MISMATCH,
                            "table has a hash part; expected a pure sequence for array",
                        ));
                    }
                    let mut out = Vec::with_capacity(t.array.len());
                    for (idx, elem) in t.array.iter().enumerate() {
                        match item.lua_transform(elem, ctx) {
                            Ok(v) => out.push(v),
                            Err(e) => return Err(prepend(e, idx.to_string())),
                        }
                    }
                    Ok(ZerxValue::Array(out))
                }
                _ => Ok(base_convert(lua)),
            },

            // Tuple: no hash part; then length gate before element conversion.
            SchemaKind::Tuple(items) => match lua {
                LuaValue::Table(t) => {
                    if !t.hash.is_empty() {
                        return Err(ZerxError::new(
                            ErrorCode::LUA_SHAPE_MISMATCH,
                            "table has a hash part; expected a pure sequence for tuple",
                        ));
                    }
                    if t.array.len() != items.len() {
                        // Null placeholders — parse_tuple raises TUPLE_LENGTH_MISMATCH.
                        // No element is converted so no element error can preempt.
                        let placeholders = vec![ZerxValue::Null; t.array.len()];
                        return Ok(ZerxValue::Array(placeholders));
                    }
                    let mut out = Vec::with_capacity(items.len());
                    for (idx, (schema, elem)) in items.iter().zip(t.array.iter()).enumerate() {
                        match schema.lua_transform(elem, ctx) {
                            Ok(v) => out.push(v),
                            Err(e) => return Err(prepend(e, idx.to_string())),
                        }
                    }
                    Ok(ZerxValue::Array(out))
                }
                _ => Ok(base_convert(lua)),
            },

            // Record: no array part; decode all keys, convert each value with value_schema.
            SchemaKind::Record(value_schema) => match lua {
                LuaValue::Table(t) => {
                    if !t.array.is_empty() {
                        return Err(ZerxError::new(
                            ErrorCode::LUA_SHAPE_MISMATCH,
                            "table has an array part; expected a pure hash for record",
                        ));
                    }
                    let mut decoded: Vec<(String, &LuaValue)> = Vec::with_capacity(t.hash.len());
                    for (idx, (k, v)) in t.hash.iter().enumerate() {
                        let key = decode_key(k).map_err(|e| prepend(e, idx.to_string()))?;
                        decoded.push((key, v));
                    }
                    let mut output = Map::new();
                    for (key, lua_val) in decoded {
                        match value_schema.lua_transform(lua_val, ctx) {
                            Ok(val) => output.insert(key.clone(), val),
                            Err(e) => return Err(prepend(e, key)),
                        }
                    }
                    Ok(ZerxValue::Object(output))
                }
                _ => Ok(base_convert(lua)),
            },

            // Object: no array part; decode all keys; then mode-specific handling.
            // Key decoding precedes the mode branch (11b precedence rule).
            SchemaKind::Object(body) => match lua {
                LuaValue::Table(t) => {
                    if !t.array.is_empty() {
                        return Err(ZerxError::new(
                            ErrorCode::LUA_SHAPE_MISMATCH,
                            "table has an array part; expected a pure hash for object",
                        ));
                    }
                    // Decode all keys before mode branch — a bad key is reported first.
                    let mut decoded: Vec<(String, &LuaValue)> = Vec::with_capacity(t.hash.len());
                    for (idx, (k, v)) in t.hash.iter().enumerate() {
                        let key = decode_key(k).map_err(|e| prepend(e, idx.to_string()))?;
                        decoded.push((key, v));
                    }

                    let strip = crate::types::effective_prestrip(body);
                    let is_stripped = |key: &str| strip.iter().any(|k| k == key);

                    match &body.mode {
                        ObjectMode::Strict => {
                            // Unknown-key gate: if any non-stripped decoded key is unknown,
                            // build Object of all decoded keys → Null without converting any value.
                            // parse_object then raises UNKNOWN_PROPERTY on the first unknown key.
                            let has_unknown = decoded
                                .iter()
                                .any(|(k, _)| !is_stripped(k) && !body.shape.iter().any(|(sk, _)| sk == k));
                            if has_unknown {
                                let mut obj = Map::new();
                                for (k, _) in &decoded {
                                    obj.insert(k.clone(), ZerxValue::Null);
                                }
                                return Ok(ZerxValue::Object(obj));
                            }
                            // All keys known: convert present shape fields in shape order.
                            let mut output = Map::new();
                            for (key, field_schema) in &body.shape {
                                if is_stripped(key) {
                                    continue;
                                }
                                if let Some(lua_val) = decoded.iter().find(|(k, _)| k == key).map(|(_, v)| *v) {
                                    match field_schema.lua_transform(lua_val, ctx) {
                                        Ok(v) => output.insert(key.clone(), v),
                                        Err(e) => return Err(prepend(e, key.clone())),
                                    }
                                }
                            }
                            Ok(ZerxValue::Object(output))
                        }

                        ObjectMode::Strip => {
                            // Known shape fields in shape order; unknown and prestripped dropped.
                            let mut output = Map::new();
                            for (key, field_schema) in &body.shape {
                                if is_stripped(key) {
                                    continue;
                                }
                                if let Some(lua_val) = decoded.iter().find(|(k, _)| k == key).map(|(_, v)| *v) {
                                    match field_schema.lua_transform(lua_val, ctx) {
                                        Ok(v) => output.insert(key.clone(), v),
                                        Err(e) => return Err(prepend(e, key.clone())),
                                    }
                                }
                            }
                            Ok(ZerxValue::Object(output))
                        }

                        ObjectMode::Passthrough => {
                            // Known shape fields in shape order, then unknown in input order via free-convert.
                            let mut output = Map::new();
                            for (key, field_schema) in &body.shape {
                                if is_stripped(key) {
                                    continue;
                                }
                                if let Some(lua_val) = decoded.iter().find(|(k, _)| k == key).map(|(_, v)| *v) {
                                    match field_schema.lua_transform(lua_val, ctx) {
                                        Ok(v) => output.insert(key.clone(), v),
                                        Err(e) => return Err(prepend(e, key.clone())),
                                    }
                                }
                            }
                            for (key, lua_val) in &decoded {
                                if is_stripped(key) {
                                    continue;
                                }
                                if !body.shape.iter().any(|(sk, _)| sk == key) {
                                    match free_convert(lua_val) {
                                        Ok(v) => output.insert(key.clone(), v),
                                        Err(e) => return Err(prepend(e, key.clone())),
                                    }
                                }
                            }
                            Ok(ZerxValue::Object(output))
                        }
                    }
                }
                _ => Ok(base_convert(lua)),
            },

            // Union: try each variant; first where both transform and parse_present succeed.
            // Transform errors are caught per-variant here — the one place they are swallowed.
            SchemaKind::Union(variants) => {
                let mut inner_errors = Vec::with_capacity(variants.len());
                for variant in variants {
                    match variant.lua_transform(lua, ctx) {
                        Ok(transformed) => {
                            match variant.parse_present(&transformed, &mut ParseContext::new()) {
                                Ok(_) => return Ok(transformed),
                                Err(e) => inner_errors.push(e),
                            }
                        }
                        Err(e) => inner_errors.push(e),
                    }
                }
                Err(ZerxError::union(Vec::new(), inner_errors))
            }

            // DiscriminatedUnion: discriminator gate first.
            SchemaKind::DiscriminatedUnion(body) => match lua {
                LuaValue::Table(t) => {
                    let map = match &body.state {
                        DiscriminatorState::Ok { map, .. } => map,
                        DiscriminatorState::Invalid(_) => {
                            // Invalid schema state: return empty Object so parse raises the error.
                            return Ok(ZerxValue::Object(Map::new()));
                        }
                    };

                    // Find the discriminator value in the hash by byte-string key match.
                    let disc_lua = t
                        .hash
                        .iter()
                        .find(|(k, _)| {
                            matches!(k, LuaValue::Bytes(b) if b.as_slice() == body.key.as_bytes())
                        })
                        .map(|(_, v)| v);

                    match disc_lua {
                        None => {
                            // Absent: empty Object → parse_discriminated_union raises INVALID_DISCRIMINANT.
                            Ok(ZerxValue::Object(Map::new()))
                        }
                        Some(disc_val) => {
                            let (disc_key_opt, placeholder) = lua_discriminant_map(disc_val);
                            match disc_key_opt {
                                None => {
                                    // Non-keyable: Null placeholder → parse raises INVALID_DISCRIMINANT.
                                    let mut obj = Map::new();
                                    obj.insert(body.key.clone(), placeholder);
                                    Ok(ZerxValue::Object(obj))
                                }
                                Some(disc_key) => match map.get(&disc_key) {
                                    Some(&idx) => {
                                        // Hit: transform whole table against the matched variant.
                                        body.variants[idx].lua_transform(lua, ctx)
                                    }
                                    None => {
                                        // Miss: placeholder → parse raises INVALID_DISCRIMINANT.
                                        let mut obj = Map::new();
                                        obj.insert(body.key.clone(), placeholder);
                                        Ok(ZerxValue::Object(obj))
                                    }
                                },
                            }
                        }
                    }
                }
                _ => Ok(base_convert(lua)),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Step 11a: discriminator mapping (Boolean/Integer/UTF-8 Bytes → keyable)
// ---------------------------------------------------------------------------

fn lua_discriminant_map(lua: &LuaValue) -> (Option<DiscriminantKey>, ZerxValue) {
    match lua {
        LuaValue::Boolean(b) => (Some(DiscriminantKey::Bool(*b)), ZerxValue::Bool(*b)),
        LuaValue::Integer(n) => (Some(DiscriminantKey::Int(*n as i128)), ZerxValue::I64(*n)),
        LuaValue::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => (
                Some(DiscriminantKey::Str(s.to_string())),
                ZerxValue::String(s.to_string()),
            ),
            Err(_) => (None, ZerxValue::Null),
        },
        // Float, Table, Nil, non-UTF-8 Bytes → non-keyable; Null is the infallible sentinel.
        _ => (None, ZerxValue::Null),
    }
}

// ---------------------------------------------------------------------------
// Step 11b: key decoding — Bytes with valid UTF-8 only
// ---------------------------------------------------------------------------

fn decode_key(key: &LuaValue) -> Result<String, ZerxError> {
    match key {
        LuaValue::Bytes(b) => std::str::from_utf8(b).map(|s| s.to_string()).map_err(|_| {
            ZerxError::new(
                ErrorCode::LUA_INVALID_KEY,
                format!("hash key is not valid UTF-8 (len={})", b.len()),
            )
        }),
        other => Err(ZerxError::new(
            ErrorCode::LUA_INVALID_KEY,
            format!(
                "hash key must be a UTF-8 byte string, got {}",
                lua_type_name(other)
            ),
        )),
    }
}

fn lua_type_name(v: &LuaValue) -> &'static str {
    match v {
        LuaValue::Nil => "nil",
        LuaValue::Boolean(_) => "boolean",
        LuaValue::Integer(_) => "integer",
        LuaValue::Float(_) => "float",
        LuaValue::Bytes(_) => "bytes",
        LuaValue::Table(_) => "table",
    }
}

// ---------------------------------------------------------------------------
// Step 12: base_convert — schema-independent leaf mapping
// ---------------------------------------------------------------------------

fn base_convert(lua: &LuaValue) -> ZerxValue {
    match lua {
        LuaValue::Nil => ZerxValue::Null,
        LuaValue::Boolean(b) => ZerxValue::Bool(*b),
        LuaValue::Integer(n) => ZerxValue::I64(*n),
        LuaValue::Float(f) => ZerxValue::F64(*f),
        LuaValue::Bytes(b) => ZerxValue::Bytes(b.clone()),
        LuaValue::Table(_) => unreachable!("Table must not reach base_convert"),
    }
}

// ---------------------------------------------------------------------------
// Step 13: free_convert — schema-independent recursive mapping
// ---------------------------------------------------------------------------

fn free_convert(lua: &LuaValue) -> Result<ZerxValue, ZerxError> {
    match lua {
        LuaValue::Nil => Ok(ZerxValue::Null),
        LuaValue::Boolean(b) => Ok(ZerxValue::Bool(*b)),
        LuaValue::Integer(n) => Ok(ZerxValue::I64(*n)),
        LuaValue::Float(f) => Ok(ZerxValue::F64(*f)),
        LuaValue::Bytes(b) => Ok(bytes_to_string_or_bytes(b)),
        LuaValue::Table(t) => {
            let has_array = !t.array.is_empty();
            let has_hash = !t.hash.is_empty();
            if has_array && has_hash {
                // Mixed table cannot be represented as a single JSON-shaped value (Finding 1).
                return Err(ZerxError::new(
                    ErrorCode::LUA_SHAPE_MISMATCH,
                    "mixed table (non-empty array and hash parts) cannot be represented as a single value",
                ));
            }
            if has_hash {
                // Hash-only → Object; decode keys and free-convert values.
                let mut output = Map::new();
                for (idx, (k, v)) in t.hash.iter().enumerate() {
                    let key = decode_key(k).map_err(|e| prepend(e, idx.to_string()))?;
                    match free_convert(v) {
                        Ok(val) => output.insert(key.clone(), val),
                        Err(e) => return Err(prepend(e, key)),
                    }
                }
                Ok(ZerxValue::Object(output))
            } else {
                // Array-only or empty → Array (empty table bias: empty → empty Array).
                let mut out = Vec::with_capacity(t.array.len());
                for (idx, elem) in t.array.iter().enumerate() {
                    match free_convert(elem) {
                        Ok(v) => out.push(v),
                        Err(e) => return Err(prepend(e, idx.to_string())),
                    }
                }
                Ok(ZerxValue::Array(out))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bytes_to_string_or_bytes(b: &[u8]) -> ZerxValue {
    match std::str::from_utf8(b) {
        Ok(s) => ZerxValue::String(s.to_string()),
        Err(_) => ZerxValue::Bytes(b.to_vec()),
    }
}

fn prepend(mut e: ZerxError, segment: impl Into<String>) -> ZerxError {
    e.path.insert(0, segment.into());
    e
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        array, boolean, buffer, discriminated_union, json, lazy, literal, null,
        number, object, record, string, tuple, union, Modify, Schema, MAX_PARSE_DEPTH,
    };

    fn bytes(b: &[u8]) -> LuaValue {
        LuaValue::Bytes(b.to_vec())
    }

    fn table(array: Vec<LuaValue>, hash: Vec<(LuaValue, LuaValue)>) -> LuaValue {
        LuaValue::Table(LuaTable { array, hash })
    }

    fn hash_entry(key: &str, val: LuaValue) -> (LuaValue, LuaValue) {
        (LuaValue::Bytes(key.as_bytes().to_vec()), val)
    }

    // --- string vs buffer disambiguation ---

    #[test]
    fn string_accepts_utf8_bytes() {
        let schema: Schema = string().into();
        let result = schema.validate_lua(&bytes(b"hello")).unwrap();
        assert_eq!(result, ZerxValue::String("hello".to_string()));
    }

    #[test]
    fn string_on_non_utf8_bytes_yields_type_mismatch() {
        let schema: Schema = string().into();
        let result = schema.validate_lua(&bytes(&[0xFF, 0xFE]));
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.received.as_deref(), Some("bytes"));
    }

    #[test]
    fn buffer_accepts_non_utf8_bytes() {
        let schema: Schema = buffer().into();
        let raw = vec![0xFF, 0xFE, 0x00];
        let result = schema.validate_lua(&bytes(&raw)).unwrap();
        assert_eq!(result, ZerxValue::Bytes(raw));
    }

    #[test]
    fn buffer_accepts_utf8_bytes_unchanged() {
        let schema: Schema = buffer().into();
        let raw = b"hello".to_vec();
        let result = schema.validate_lua(&bytes(&raw)).unwrap();
        assert_eq!(result, ZerxValue::Bytes(raw));
    }

    // --- table → array ---

    #[test]
    fn array_from_pure_sequence() {
        let schema: Schema = array(number()).into();
        let t = table(vec![LuaValue::Integer(1), LuaValue::Integer(2)], vec![]);
        let result = schema.validate_lua(&t).unwrap();
        assert_eq!(
            result,
            ZerxValue::Array(vec![ZerxValue::I64(1), ZerxValue::I64(2)])
        );
    }

    #[test]
    fn array_with_hash_part_yields_shape_mismatch() {
        let schema: Schema = array(number()).into();
        let t = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("x", LuaValue::Integer(2))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_SHAPE_MISMATCH);
    }

    // --- table → object / record ---

    #[test]
    fn object_from_hash_only_table() {
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(vec![], vec![hash_entry("x", LuaValue::Integer(42))]);
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("x".to_string(), ZerxValue::I64(42));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn object_with_array_part_yields_shape_mismatch() {
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("x", LuaValue::Integer(2))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_SHAPE_MISMATCH);
    }

    #[test]
    fn object_non_string_hash_key_yields_invalid_key() {
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(
            vec![],
            vec![(LuaValue::Integer(1), LuaValue::Integer(42))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_INVALID_KEY);
    }

    #[test]
    fn object_non_utf8_hash_key_yields_invalid_key() {
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(
            vec![],
            vec![(LuaValue::Bytes(vec![0xFF, 0xFE]), LuaValue::Integer(42))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_INVALID_KEY);
    }

    #[test]
    fn record_from_hash_only_table() {
        let schema: Schema = record(number()).into();
        let t = table(
            vec![],
            vec![hash_entry("a", LuaValue::Integer(1)), hash_entry("b", LuaValue::Integer(2))],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("a".to_string(), ZerxValue::I64(1));
        expected.insert("b".to_string(), ZerxValue::I64(2));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn record_with_array_part_yields_shape_mismatch() {
        let schema: Schema = record(number()).into();
        let t = table(vec![LuaValue::Integer(1)], vec![]);
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_SHAPE_MISMATCH);
    }

    // --- object semantics via parse flow ---

    #[test]
    fn unknown_key_strict_yields_unknown_property() {
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(vec![], vec![hash_entry("y", LuaValue::Integer(1))]);
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["y"]);
    }

    #[test]
    fn missing_optional_field_omitted() {
        let schema: Schema = object([("x", number().optional().into())]).into();
        let t = table(vec![], vec![]);
        let result = schema.validate_lua(&t).unwrap();
        assert_eq!(result, ZerxValue::Object(crate::Map::new()));
    }

    #[test]
    fn missing_defaulted_field_gets_default() {
        let schema: Schema =
            object([("x", number().default(ZerxValue::I64(99)).into())]).into();
        let t = table(vec![], vec![]);
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("x".to_string(), ZerxValue::I64(99));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    // --- precedence: unknown key beats nested value error (Finding 3-i) ---

    #[test]
    fn precedence_unknown_key_beats_nested_value_error() {
        // Strict object with field "a" (inner object) and unknown key "x".
        // The value of "a" is a mixed table (unrepresentable), but "x" is unknown.
        // UNKNOWN_PROPERTY on "x" must win.
        let inner = object([("z", number().into())]).into();
        let schema: Schema = object([("a", inner)]).into();
        let mixed_table = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("z", LuaValue::Integer(2))],
        );
        let t = table(
            vec![],
            vec![
                hash_entry("x", LuaValue::Integer(99)),  // unknown key
                hash_entry("a", mixed_table),             // value is unrepresentable
            ],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY, "expected UNKNOWN_PROPERTY on 'x', got {:?}", err);
        assert_eq!(err.path, vec!["x"]);
    }

    // --- precedence: tuple length beats element error (Finding 3-ii) ---

    #[test]
    fn precedence_tuple_length_beats_element_error() {
        // tuple([string(), number()]) fed a 3-element array — TUPLE_LENGTH_MISMATCH,
        // even though one element is a Table (which might error in element conversion).
        let schema: Schema = tuple([string().into(), number().into()]).into();
        let t = table(
            vec![
                bytes(b"hello"),
                LuaValue::Integer(1),
                table(vec![LuaValue::Integer(1)], vec![hash_entry("x", LuaValue::Integer(2))]), // mixed — unrepresentable
            ],
            vec![],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::TUPLE_LENGTH_MISMATCH, "expected TUPLE_LENGTH_MISMATCH, got {:?}", err);
    }

    // --- precedence: discriminator beats body error (Finding 3-iii) ---

    #[test]
    fn precedence_discriminator_beats_body_error() {
        // discriminated_union fed a table with an absent discriminator AND an
        // unrepresentable (mixed) field value — INVALID_DISCRIMINANT must win.
        let schema: Schema = discriminated_union(
            "kind",
            [
                object([
                    ("kind", literal("a").into()),
                    ("data", number().into()),
                ])
                .into(),
            ],
        )
        .into();
        let mixed = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("x", LuaValue::Integer(2))],
        );
        let t = table(
            vec![],
            vec![
                // "kind" is absent; "data" has an unrepresentable value
                hash_entry("data", mixed),
            ],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT, "expected INVALID_DISCRIMINANT, got {:?}", err);
    }

    // --- scalars ---

    #[test]
    fn integer_to_number() {
        let schema: Schema = number().into();
        let result = schema.validate_lua(&LuaValue::Integer(7)).unwrap();
        assert_eq!(result, ZerxValue::I64(7));
    }

    #[test]
    fn integer_to_number_int() {
        let schema: Schema = number().int().into();
        let result = schema.validate_lua(&LuaValue::Integer(7)).unwrap();
        assert_eq!(result, ZerxValue::I64(7));
    }

    #[test]
    fn float_with_fract_under_int_constraint_fails() {
        let schema: Schema = number().int().into();
        let err = schema.validate_lua(&LuaValue::Float(1.5)).unwrap_err();
        assert_eq!(err.code, ErrorCode::NOT_INTEGER);
    }

    #[test]
    fn boolean_value() {
        let schema: Schema = boolean().into();
        let result = schema.validate_lua(&LuaValue::Boolean(true)).unwrap();
        assert_eq!(result, ZerxValue::Bool(true));
    }

    #[test]
    fn nil_under_nullable_string() {
        let schema: Schema = string().nullable().into();
        let result = schema.validate_lua(&LuaValue::Nil).unwrap();
        assert_eq!(result, ZerxValue::Null);
    }

    #[test]
    fn nil_under_non_nullable_string_fails() {
        let schema: Schema = string().into();
        let err = schema.validate_lua(&LuaValue::Nil).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
    }

    // --- nested: object containing buffer and string fields ---

    #[test]
    fn nested_buffer_and_string_fields() {
        let schema: Schema = object([
            ("name", string().into()),
            ("data", buffer().into()),
        ])
        .into();
        let raw = vec![0xFF, 0xFE];
        let t = table(
            vec![],
            vec![
                hash_entry("name", bytes(b"alice")),
                (LuaValue::Bytes(b"data".to_vec()), LuaValue::Bytes(raw.clone())),
            ],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("name".to_string(), ZerxValue::String("alice".to_string()));
        expected.insert("data".to_string(), ZerxValue::Bytes(raw));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    // --- union ---

    #[test]
    fn union_utf8_bytes_resolves_string_first() {
        let schema: Schema = union([string().into(), buffer().into()]).into();
        let result = schema.validate_lua(&bytes(b"hello")).unwrap();
        assert_eq!(result, ZerxValue::String("hello".to_string()));
    }

    #[test]
    fn union_non_utf8_bytes_falls_through_to_buffer() {
        let schema: Schema = union([string().into(), buffer().into()]).into();
        let raw = vec![0xFF, 0xFE];
        let result = schema.validate_lua(&bytes(&raw)).unwrap();
        assert_eq!(result, ZerxValue::Bytes(raw));
    }

    // --- discriminated_union with all key kinds (Finding 2) ---

    #[test]
    fn discriminated_union_bytes_string_discriminator() {
        let schema: Schema = discriminated_union(
            "kind",
            [
                object([
                    ("kind", literal("foo").into()),
                    ("val", number().into()),
                ])
                .into(),
                object([
                    ("kind", literal("bar").into()),
                    ("val", string().into()),
                ])
                .into(),
            ],
        )
        .into();
        let t = table(
            vec![],
            vec![
                hash_entry("kind", bytes(b"foo")),
                hash_entry("val", LuaValue::Integer(42)),
            ],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("kind".to_string(), ZerxValue::String("foo".to_string()));
        expected.insert("val".to_string(), ZerxValue::I64(42));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn discriminated_union_boolean_discriminator() {
        let schema: Schema = discriminated_union(
            "active",
            [
                object([
                    ("active", literal(true).into()),
                    ("val", number().into()),
                ])
                .into(),
                object([
                    ("active", literal(false).into()),
                    ("val", string().into()),
                ])
                .into(),
            ],
        )
        .into();
        let t = table(
            vec![],
            vec![
                (LuaValue::Bytes(b"active".to_vec()), LuaValue::Boolean(true)),
                hash_entry("val", LuaValue::Integer(99)),
            ],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("active".to_string(), ZerxValue::Bool(true));
        expected.insert("val".to_string(), ZerxValue::I64(99));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn discriminated_union_integer_discriminator() {
        let schema: Schema = discriminated_union(
            "code",
            [
                object([
                    ("code", literal(1i64).into()),
                    ("msg", string().into()),
                ])
                .into(),
                object([
                    ("code", literal(2i64).into()),
                    ("msg", string().into()),
                ])
                .into(),
            ],
        )
        .into();
        let t = table(
            vec![],
            vec![
                (LuaValue::Bytes(b"code".to_vec()), LuaValue::Integer(2)),
                hash_entry("msg", bytes(b"ok")),
            ],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("code".to_string(), ZerxValue::I64(2));
        expected.insert("msg".to_string(), ZerxValue::String("ok".to_string()));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn discriminated_union_wrong_discriminator_yields_invalid_discriminant() {
        let schema: Schema = discriminated_union(
            "kind",
            [
                object([("kind", literal("foo").into()), ("val", number().into())]).into(),
            ],
        )
        .into();
        let t = table(
            vec![],
            vec![hash_entry("kind", bytes(b"bar")), hash_entry("val", LuaValue::Integer(1))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT);
    }

    #[test]
    fn discriminated_union_absent_discriminator_yields_invalid_discriminant() {
        let schema: Schema = discriminated_union(
            "kind",
            [object([("kind", literal("foo").into())]).into()],
        )
        .into();
        let t = table(vec![], vec![]);
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT);
    }

    // --- discriminator Miss path cannot fail with representability error (Finding 3 follow-up) ---

    #[test]
    fn discriminator_mixed_table_value_yields_invalid_discriminant_not_shape_mismatch() {
        let schema: Schema = discriminated_union(
            "kind",
            [object([("kind", literal("foo").into()), ("val", number().into())]).into()],
        )
        .into();
        // The discriminator field value is a mixed Table — non-keyable → Null placeholder.
        let mixed = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("x", LuaValue::Integer(2))],
        );
        let t = table(
            vec![],
            vec![(LuaValue::Bytes(b"kind".to_vec()), mixed)],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT, "expected INVALID_DISCRIMINANT, got {:?}", err);
    }

    #[test]
    fn discriminator_float_value_yields_invalid_discriminant() {
        let schema: Schema = discriminated_union(
            "kind",
            [object([("kind", literal("foo").into())]).into()],
        )
        .into();
        let t = table(
            vec![],
            vec![(LuaValue::Bytes(b"kind".to_vec()), LuaValue::Float(1.5))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT);
    }

    // --- lazy + depth ---

    #[test]
    fn lazy_wrapped_object_validates() {
        let schema: Schema = lazy(|| {
            object([("x", number().into())]).into()
        })
        .into();
        let t = table(vec![], vec![hash_entry("x", LuaValue::Integer(5))]);
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("x".to_string(), ZerxValue::I64(5));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn depth_exceeded_yields_parse_depth_exceeded() {
        // Build a schema: array(array(array(...))) nested MAX_PARSE_DEPTH + 2 deep.
        // Build a matching nested Lua table the same depth deep.
        let mut schema: Schema = null().into();
        for _ in 0..=(MAX_PARSE_DEPTH + 1) {
            schema = array(schema).into();
        }

        let mut val = LuaValue::Nil;
        for _ in 0..=(MAX_PARSE_DEPTH + 1) {
            val = table(vec![val], vec![]);
        }

        let err = schema.validate_lua(&val).unwrap_err();
        assert_eq!(err.code, ErrorCode::PARSE_DEPTH_EXCEEDED, "expected PARSE_DEPTH_EXCEEDED, got {:?}", err);
    }

    // --- any / json free-convert ---

    #[test]
    fn any_hash_only_nested_table_free_converts() {
        let schema: Schema = json().into();
        let inner = table(vec![], vec![hash_entry("n", LuaValue::Integer(1))]);
        let t = table(vec![], vec![hash_entry("inner", inner)]);
        let result = schema.validate_lua(&t).unwrap();
        let mut inner_expected = crate::Map::new();
        inner_expected.insert("n".to_string(), ZerxValue::I64(1));
        let mut expected = crate::Map::new();
        expected.insert(
            "inner".to_string(),
            ZerxValue::Object(inner_expected),
        );
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn any_bytes_key_table_yields_invalid_key() {
        let schema: Schema = json().into();
        let t = table(
            vec![],
            vec![(LuaValue::Integer(1), LuaValue::Integer(2))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_INVALID_KEY);
    }

    #[test]
    fn any_mixed_table_yields_shape_mismatch() {
        let schema: Schema = json().into();
        let t = table(
            vec![LuaValue::Integer(1)],
            vec![hash_entry("x", LuaValue::Integer(2))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_SHAPE_MISMATCH);
    }

    #[test]
    fn any_empty_table_yields_empty_array() {
        let schema: Schema = json().into();
        let t = table(vec![], vec![]);
        let result = schema.validate_lua(&t).unwrap();
        assert_eq!(result, ZerxValue::Array(vec![]));
    }

    // --- Zex port: string/buffer disambiguation (R1 stricter expectations) ---

    #[test]
    fn port_bytes_decode_utf8_for_string_schema() {
        let schema: Schema = string().into();
        assert!(schema.validate_lua(&bytes(b"zerx")).is_ok());
    }

    #[test]
    fn port_buffer_keeps_raw_bytes() {
        let schema: Schema = buffer().into();
        let raw = b"\x00\x01\x02".to_vec();
        let result = schema.validate_lua(&bytes(&raw)).unwrap();
        assert_eq!(result, ZerxValue::Bytes(raw));
    }

    #[test]
    fn port_discriminant_bytes_selects_string_literal_variant() {
        // Zex: lua-union-literal-discriminant-bytes
        let schema: Schema = discriminated_union(
            "t",
            [
                object([("t", literal("ok").into()), ("v", number().into())]).into(),
                object([("t", literal("err").into()), ("v", string().into())]).into(),
            ],
        )
        .into();
        let t = table(
            vec![],
            vec![
                hash_entry("t", bytes(b"err")),
                hash_entry("v", bytes(b"bad")),
            ],
        );
        let result = schema.validate_lua(&t).unwrap();
        let mut expected = crate::Map::new();
        expected.insert("t".to_string(), ZerxValue::String("err".to_string()));
        expected.insert("v".to_string(), ZerxValue::String("bad".to_string()));
        assert_eq!(result, ZerxValue::Object(expected));
    }

    #[test]
    fn port_strict_rejects_non_string_key() {
        // R1: zerx errors on non-string keys; Zex silently ignores them
        let schema: Schema = object([("x", number().into())]).into();
        let t = table(
            vec![],
            vec![(LuaValue::Integer(1), LuaValue::Integer(42))],
        );
        let err = schema.validate_lua(&t).unwrap_err();
        assert_eq!(err.code, ErrorCode::LUA_INVALID_KEY);
    }
}
