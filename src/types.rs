// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::rc::Rc;

use crate::schema::{BuilderInner, Schema, SchemaKind, Validator};
use crate::{ErrorCode, ZerxError, ZerxValue};

// ---------------------------------------------------------------------------
// Open error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const STRING_TOO_SHORT: ErrorCode = ErrorCode::new("string_too_short");
    pub const STRING_TOO_LONG: ErrorCode = ErrorCode::new("string_too_long");
    pub const PATTERN_MISMATCH: ErrorCode = ErrorCode::new("pattern_mismatch");
    pub const PATTERN_INVALID: ErrorCode = ErrorCode::new("pattern_invalid");
    pub const INVALID_EMAIL: ErrorCode = ErrorCode::new("invalid_email");
    pub const INVALID_UUID: ErrorCode = ErrorCode::new("invalid_uuid");
    pub const NUMBER_TOO_SMALL: ErrorCode = ErrorCode::new("number_too_small");
    pub const NUMBER_TOO_LARGE: ErrorCode = ErrorCode::new("number_too_large");
    pub const NOT_INTEGER: ErrorCode = ErrorCode::new("not_integer");
    pub const INVALID_ENUM_VALUE: ErrorCode = ErrorCode::new("invalid_enum_value");
}

// ---------------------------------------------------------------------------
// type_tag helper
// ---------------------------------------------------------------------------

pub(crate) fn type_tag(value: &ZerxValue) -> &'static str {
    match value {
        ZerxValue::Null => "null",
        ZerxValue::Bool(_) => "boolean",
        ZerxValue::I64(_)
        | ZerxValue::U64(_)
        | ZerxValue::I128(_)
        | ZerxValue::U128(_)
        | ZerxValue::F64(_) => "number",
        ZerxValue::String(_) => "string",
        ZerxValue::Bytes(_) => "bytes",
        ZerxValue::Array(_) => "array",
        ZerxValue::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// numeric_as_f64 helper
// ---------------------------------------------------------------------------

fn numeric_as_f64(value: &ZerxValue) -> Option<f64> {
    match value {
        ZerxValue::I64(n) => Some(*n as f64),
        ZerxValue::U64(n) => Some(*n as f64),
        ZerxValue::I128(n) => Some(*n as f64),
        ZerxValue::U128(n) => Some(*n as f64),
        ZerxValue::F64(n) => Some(*n),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type-check delegate functions (called from SchemaKind dispatch)
// ---------------------------------------------------------------------------

pub(crate) fn check_string(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::String(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected string")
            .expected("string")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_number(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::I64(_) | ZerxValue::U64(_) | ZerxValue::I128(_) | ZerxValue::U128(_) => Ok(()),
        ZerxValue::F64(n) if n.is_finite() => Ok(()),
        ZerxValue::F64(_) => Err(ZerxError::new(
            ErrorCode::TYPE_MISMATCH,
            "expected finite number",
        )
        .expected("number")
        .received(type_tag(value))),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected number")
            .expected("number")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_boolean(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Bool(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected boolean")
            .expected("boolean")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_enum(value: &ZerxValue, set: &[String]) -> Result<(), ZerxError> {
    match value {
        ZerxValue::String(s) => {
            if set.iter().any(|v| v == s) {
                Ok(())
            } else {
                let allowed = set.join(", ");
                Err(
                    ZerxError::new(ErrorCode::INVALID_ENUM_VALUE, "value is not an allowed enum value")
                        .expected(allowed)
                        .received(type_tag(value)),
                )
            }
        }
        _ => Err(
            ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected string (enum)")
                .expected("string (enum)")
                .received(type_tag(value)),
        ),
    }
}

pub(crate) fn check_null(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Null => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected null")
            .expected("null")
            .received(type_tag(value))),
    }
}

// ---------------------------------------------------------------------------
// Concrete validators — String
// ---------------------------------------------------------------------------

pub(crate) struct MinLength(pub usize);
pub(crate) struct MaxLength(pub usize);

impl Validator for MinLength {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::String(s) = value {
            if s.chars().count() < self.0 {
                return Err(ZerxError::new(
                    ErrorCode::STRING_TOO_SHORT,
                    format!("string length is less than minimum {}", self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("minLength".to_owned(), serde_json::Value::Number(self.0.into()));
        m
    }
}

impl Validator for MaxLength {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::String(s) = value {
            if s.chars().count() > self.0 {
                return Err(ZerxError::new(
                    ErrorCode::STRING_TOO_LONG,
                    format!("string length exceeds maximum {}", self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("maxLength".to_owned(), serde_json::Value::Number(self.0.into()));
        m
    }
}

// ---------------------------------------------------------------------------
// Pattern validator
// ---------------------------------------------------------------------------

enum PatternState {
    Ok(regex::Regex),
    Invalid(String),
}

pub(crate) struct Pattern {
    source: String,
    compiled: PatternState,
}

impl Pattern {
    fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let compiled = match regex::Regex::new(&source) {
            Ok(re) => PatternState::Ok(re),
            Err(e) => PatternState::Invalid(e.to_string()),
        };
        Pattern { source, compiled }
    }
}

impl Validator for Pattern {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::String(s) = value {
            match &self.compiled {
                PatternState::Invalid(err) => {
                    return Err(ZerxError::new(
                        ErrorCode::PATTERN_INVALID,
                        format!("pattern is invalid: {err}"),
                    ));
                }
                PatternState::Ok(re) => {
                    if !re.is_match(s) {
                        return Err(ZerxError::new(
                            ErrorCode::PATTERN_MISMATCH,
                            "string does not match pattern",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "pattern".to_owned(),
            serde_json::Value::String(self.source.clone()),
        );
        m
    }
}

// ---------------------------------------------------------------------------
// Email validator (hand-rolled)
// ---------------------------------------------------------------------------

pub(crate) struct Email;

fn is_valid_email(s: &str) -> bool {
    // Non-empty run, @, non-empty run, ., non-empty run; no whitespace or @ in runs.
    let at = match s.find('@') {
        Some(i) => i,
        None => return false,
    };
    let local = &s[..at];
    let rest = &s[at + 1..];
    if local.is_empty() || local.contains('@') || local.contains(char::is_whitespace) {
        return false;
    }
    let dot = match rest.rfind('.') {
        Some(i) => i,
        None => return false,
    };
    let domain = &rest[..dot];
    let tld = &rest[dot + 1..];
    !domain.is_empty()
        && !tld.is_empty()
        && !domain.contains('@')
        && !domain.contains(char::is_whitespace)
        && !tld.contains('@')
        && !tld.contains(char::is_whitespace)
}

impl Validator for Email {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::String(s) = value {
            if !is_valid_email(s) {
                return Err(ZerxError::new(
                    ErrorCode::INVALID_EMAIL,
                    "string is not a valid email address",
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "format".to_owned(),
            serde_json::Value::String("email".to_owned()),
        );
        m
    }
}

// ---------------------------------------------------------------------------
// Uuid validator (hand-rolled)
// ---------------------------------------------------------------------------

pub(crate) struct Uuid;

fn is_valid_uuid(s: &str) -> bool {
    // 8-4-4-4-12 hex chars with dashes at positions 8, 13, 18, 23
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, &byte) in b.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

impl Validator for Uuid {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::String(s) = value {
            if !is_valid_uuid(s) {
                return Err(ZerxError::new(
                    ErrorCode::INVALID_UUID,
                    "string is not a valid UUID",
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "format".to_owned(),
            serde_json::Value::String("uuid".to_owned()),
        );
        m
    }
}

// ---------------------------------------------------------------------------
// Concrete validators — Number
// ---------------------------------------------------------------------------

pub(crate) struct MinValue(pub f64);
pub(crate) struct MaxValue(pub f64);
pub(crate) struct IntValidator;

impl Validator for MinValue {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let Some(n) = numeric_as_f64(value) {
            if n < self.0 {
                return Err(ZerxError::new(
                    ErrorCode::NUMBER_TOO_SMALL,
                    format!("number is less than minimum {}", self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        let n = serde_json::Number::from_f64(self.0)
            .unwrap_or_else(|| serde_json::Number::from(0));
        m.insert("minimum".to_owned(), serde_json::Value::Number(n));
        m
    }
}

impl Validator for MaxValue {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let Some(n) = numeric_as_f64(value) {
            if n > self.0 {
                return Err(ZerxError::new(
                    ErrorCode::NUMBER_TOO_LARGE,
                    format!("number exceeds maximum {}", self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        let n = serde_json::Number::from_f64(self.0)
            .unwrap_or_else(|| serde_json::Number::from(0));
        m.insert("maximum".to_owned(), serde_json::Value::Number(n));
        m
    }
}

impl Validator for IntValidator {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        match value {
            ZerxValue::I64(_) | ZerxValue::U64(_) | ZerxValue::I128(_) | ZerxValue::U128(_) => {
                Ok(())
            }
            ZerxValue::F64(n) => {
                if n.fract() == 0.0 {
                    Ok(())
                } else {
                    Err(ZerxError::new(
                        ErrorCode::NOT_INTEGER,
                        "number is not an integer",
                    ))
                }
            }
            _ => Ok(()),
        }
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "type".to_owned(),
            serde_json::Value::String("integer".to_owned()),
        );
        m
    }
}

// ---------------------------------------------------------------------------
// Builder newtypes
// ---------------------------------------------------------------------------

/// Builder for `string` schemas. Inherent methods add string-specific validators.
#[derive(Clone)]
pub struct StringSchema(Schema);

/// Builder for `number` schemas. Inherent methods add number-specific validators.
#[derive(Clone)]
pub struct NumberSchema(Schema);

/// Builder for `boolean` schemas.
///
/// ```compile_fail
/// use zerx::boolean;
/// let _ = boolean().min(3);   // ERROR: no method `min` on BooleanSchema
/// ```
#[derive(Clone)]
pub struct BooleanSchema(Schema);

/// Builder for `enumerate` schemas.
#[derive(Clone)]
pub struct EnumSchema(Schema);

/// Builder for `null` schemas.
#[derive(Clone)]
pub struct NullSchema(Schema);

// BuilderInner impls (grants blanket Modify)

impl BuilderInner for StringSchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl BuilderInner for NumberSchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl BuilderInner for BooleanSchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl BuilderInner for EnumSchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl BuilderInner for NullSchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

// From<Builder> for Schema

impl From<StringSchema> for Schema {
    fn from(b: StringSchema) -> Schema {
        b.0
    }
}

impl From<NumberSchema> for Schema {
    fn from(b: NumberSchema) -> Schema {
        b.0
    }
}

impl From<BooleanSchema> for Schema {
    fn from(b: BooleanSchema) -> Schema {
        b.0
    }
}

impl From<EnumSchema> for Schema {
    fn from(b: EnumSchema) -> Schema {
        b.0
    }
}

impl From<NullSchema> for Schema {
    fn from(b: NullSchema) -> Schema {
        b.0
    }
}

// ---------------------------------------------------------------------------
// Inherent methods
// ---------------------------------------------------------------------------

impl StringSchema {
    pub fn min(mut self, n: usize) -> Self {
        self.0.validators.push(Rc::new(MinLength(n)));
        self
    }

    pub fn max(mut self, n: usize) -> Self {
        self.0.validators.push(Rc::new(MaxLength(n)));
        self
    }

    pub fn regex(mut self, pattern: impl Into<String>) -> Self {
        self.0.validators.push(Rc::new(Pattern::new(pattern)));
        self
    }

    pub fn pattern(self, p: impl Into<String>) -> Self {
        self.regex(p)
    }

    pub fn email(mut self) -> Self {
        self.0.validators.push(Rc::new(Email));
        self
    }

    pub fn uuid(mut self) -> Self {
        self.0.validators.push(Rc::new(Uuid));
        self
    }

    pub fn multiline(mut self, lines: u32) -> Self {
        self.0
            .modifiers
            .meta
            .insert("x-ui-multiline", ZerxValue::U64(lines as u64));
        self
    }
}

impl NumberSchema {
    pub fn min(mut self, n: impl Into<f64>) -> Self {
        self.0.validators.push(Rc::new(MinValue(n.into())));
        self
    }

    pub fn max(mut self, n: impl Into<f64>) -> Self {
        self.0.validators.push(Rc::new(MaxValue(n.into())));
        self
    }

    pub fn int(mut self) -> Self {
        self.0.validators.push(Rc::new(IntValidator));
        self
    }
}

// ---------------------------------------------------------------------------
// T2 error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const UNKNOWN_PROPERTY: ErrorCode            = ErrorCode::new("unknown_property");
    pub const ARRAY_TOO_SHORT: ErrorCode             = ErrorCode::new("array_too_short");
    pub const ARRAY_TOO_LONG: ErrorCode              = ErrorCode::new("array_too_long");
    pub const TUPLE_LENGTH_MISMATCH: ErrorCode       = ErrorCode::new("tuple_length_mismatch");
    pub const INVALID_LITERAL: ErrorCode             = ErrorCode::new("invalid_literal");
    pub const INVALID_DISCRIMINANT: ErrorCode        = ErrorCode::new("invalid_discriminant");
    pub const INVALID_DISCRIMINATED_UNION: ErrorCode = ErrorCode::new("invalid_discriminated_union");
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
pub(crate) enum ObjectMode {
    Strict,
    Passthrough,
    Strip,
}

#[derive(Clone)]
pub(crate) struct ObjectBody {
    pub(crate) shape: Vec<(String, Schema)>,
    pub(crate) mode: ObjectMode,
    pub(crate) all_optional: bool,
    pub(crate) prestrip_keys: Vec<String>,
    pub(crate) prestrip_read_only: bool,
    pub(crate) prestrip_write_only: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum DiscriminantKey {
    Bool(bool),
    Int(i128),
    Str(String),
}

#[derive(Clone)]
pub(crate) enum DiscriminatorState {
    Ok {
        map: std::collections::HashMap<DiscriminantKey, usize>,
        allowed: Vec<String>,
    },
    Invalid(String),
}

#[derive(Clone)]
pub(crate) struct DiscriminatedUnionBody {
    pub(crate) key: String,
    pub(crate) variants: Vec<Schema>,
    pub(crate) state: DiscriminatorState,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn discriminant_key(value: &ZerxValue) -> Option<DiscriminantKey> {
    match value {
        ZerxValue::Bool(b) => Some(DiscriminantKey::Bool(*b)),
        ZerxValue::String(s) => Some(DiscriminantKey::Str(s.clone())),
        ZerxValue::I64(n) => Some(DiscriminantKey::Int(*n as i128)),
        ZerxValue::U64(n) => Some(DiscriminantKey::Int(i128::from(*n))),
        ZerxValue::I128(n) => Some(DiscriminantKey::Int(*n)),
        ZerxValue::U128(n) => i128::try_from(*n).ok().map(DiscriminantKey::Int),
        _ => None,
    }
}

fn literal_descriptor(value: &ZerxValue) -> String {
    match value {
        ZerxValue::Bool(b) => b.to_string(),
        ZerxValue::String(s) => format!("\"{}\"", s),
        ZerxValue::I64(n) => n.to_string(),
        ZerxValue::U64(n) => n.to_string(),
        ZerxValue::I128(n) => n.to_string(),
        ZerxValue::U128(n) => n.to_string(),
        ZerxValue::F64(n) => n.to_string(),
        _ => type_tag(value).to_string(),
    }
}

fn prefix_path(mut e: ZerxError, segment: impl Into<String>) -> ZerxError {
    e.path.insert(0, segment.into());
    e
}

// ---------------------------------------------------------------------------
// T2 type-check delegates
// ---------------------------------------------------------------------------

pub(crate) fn check_object(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Object(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected object")
            .expected("object")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_array(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Array(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected array")
            .expected("array")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_record(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Object(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected object")
            .expected("object")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_tuple(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Array(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected array")
            .expected("array")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_union() -> Result<(), ZerxError> {
    Ok(())
}

pub(crate) fn check_discriminated_union(
    value: &ZerxValue,
    body: &DiscriminatedUnionBody,
) -> Result<(), ZerxError> {
    if let DiscriminatorState::Invalid(msg) = &body.state {
        return Err(ZerxError::new(ErrorCode::INVALID_DISCRIMINATED_UNION, msg.clone()));
    }
    match value {
        ZerxValue::Object(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected object")
            .expected("object")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_literal(value: &ZerxValue, constant: &ZerxValue) -> Result<(), ZerxError> {
    if value == constant {
        Ok(())
    } else {
        Err(ZerxError::new(ErrorCode::INVALID_LITERAL, "value does not match literal constant")
            .expected(literal_descriptor(constant))
            .received(type_tag(value)))
    }
}

// ---------------------------------------------------------------------------
// T2 structural parse delegates
// ---------------------------------------------------------------------------

pub(crate) fn effective_prestrip(body: &ObjectBody) -> Vec<String> {
    let mut keys = body.prestrip_keys.clone();
    if body.prestrip_read_only {
        for (k, fs) in &body.shape {
            if fs.modifiers.read_only {
                keys.push(k.clone());
            }
        }
    }
    if body.prestrip_write_only {
        for (k, fs) in &body.shape {
            if fs.modifiers.write_only {
                keys.push(k.clone());
            }
        }
    }
    keys
}

pub(crate) fn parse_object(
    body: &ObjectBody,
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let input = match value.as_object() {
        Some(m) => m,
        None => return Ok(value.clone()),
    };

    let strip = effective_prestrip(body);
    let is_stripped = |key: &str| strip.iter().any(|k| k == key);

    if body.mode == ObjectMode::Strict {
        for (key, val) in input.iter() {
            if is_stripped(key) {
                continue;
            }
            if !body.shape.iter().any(|(k, _)| k == key) {
                return Err(prefix_path(
                    ZerxError::new(
                        ErrorCode::UNKNOWN_PROPERTY,
                        format!("unknown property '{key}'"),
                    )
                    .expected("property not in schema")
                    .received(type_tag(val)),
                    key.clone(),
                ));
            }
        }
    }

    let mut output = crate::Map::new();

    for (key, field_schema) in &body.shape {
        let raw = if is_stripped(key) { None } else { input.get(key) };
        match field_schema.parse_field(raw, ctx) {
            Ok(Some(v)) => output.insert(key.clone(), v),
            Ok(None) => {}
            Err(e) => {
                if body.all_optional && raw.is_none() && e.code == ErrorCode::REQUIRED {
                    // partial: missing required field is treated as optional → omit
                } else {
                    return Err(prefix_path(e, key.clone()));
                }
            }
        }
    }

    if body.mode == ObjectMode::Passthrough {
        for (key, val) in input.iter() {
            if is_stripped(key) {
                continue;
            }
            if !body.shape.iter().any(|(k, _)| k == key) {
                output.insert(key.clone(), val.clone());
            }
        }
    }

    Ok(ZerxValue::Object(output))
}

pub(crate) fn parse_array(
    item: &Schema,
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Ok(value.clone()),
    };

    let mut out = Vec::with_capacity(arr.len());
    for (index, elem) in arr.iter().enumerate() {
        match item.parse_present(elem, ctx) {
            Ok(v) => out.push(v),
            Err(e) => return Err(prefix_path(e, index.to_string())),
        }
    }
    Ok(ZerxValue::Array(out))
}

pub(crate) fn parse_record(
    value_schema: &Schema,
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let input = match value.as_object() {
        Some(m) => m,
        None => return Ok(value.clone()),
    };

    let mut output = crate::Map::new();
    for (key, val) in input.iter() {
        match value_schema.parse_present(val, ctx) {
            Ok(v) => output.insert(key.clone(), v),
            Err(e) => return Err(prefix_path(e, key.clone())),
        }
    }
    Ok(ZerxValue::Object(output))
}

pub(crate) fn parse_tuple(
    items: &[Schema],
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let arr = match value.as_array() {
        Some(a) => a,
        None => return Ok(value.clone()),
    };

    if arr.len() != items.len() {
        return Err(ZerxError::new(
            ErrorCode::TUPLE_LENGTH_MISMATCH,
            format!("expected array of length {}, got {}", items.len(), arr.len()),
        )
        .expected(format!("array of length {}", items.len()))
        .received(format!("array of length {}", arr.len())));
    }

    let mut out = Vec::with_capacity(items.len());
    for (i, (schema, elem)) in items.iter().zip(arr.iter()).enumerate() {
        match schema.parse_present(elem, ctx) {
            Ok(v) => out.push(v),
            Err(e) => return Err(prefix_path(e, i.to_string())),
        }
    }
    Ok(ZerxValue::Array(out))
}

pub(crate) fn parse_union(
    variants: &[Schema],
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let mut inner_errors = Vec::with_capacity(variants.len());
    for variant in variants {
        match variant.parse_present(value, ctx) {
            Ok(v) => return Ok(v),
            Err(e) => inner_errors.push(e),
        }
    }
    Err(ZerxError::union(Vec::new(), inner_errors))
}

pub(crate) fn parse_discriminated_union(
    body: &DiscriminatedUnionBody,
    value: &ZerxValue,
    ctx: &mut crate::schema::ParseContext,
) -> Result<ZerxValue, ZerxError> {
    let (map, allowed) = match &body.state {
        DiscriminatorState::Ok { map, allowed } => (map, allowed),
        DiscriminatorState::Invalid(msg) => {
            return Err(ZerxError::new(ErrorCode::INVALID_DISCRIMINATED_UNION, msg.clone()));
        }
    };

    let input = match value.as_object() {
        Some(m) => m,
        None => return Ok(value.clone()),
    };

    let disc_value = match input.get(&body.key) {
        Some(v) => v,
        None => {
            return Err(ZerxError::new(
                ErrorCode::INVALID_DISCRIMINANT,
                format!("discriminator field '{}' is missing", body.key),
            )
            .expected(allowed.join(", ")));
        }
    };

    let disc_key = match discriminant_key(disc_value) {
        Some(k) => k,
        None => {
            return Err(ZerxError::new(
                ErrorCode::INVALID_DISCRIMINANT,
                format!("discriminator field '{}' has a non-keyable value", body.key),
            )
            .expected(allowed.join(", "))
            .received(type_tag(disc_value)));
        }
    };

    let index = match map.get(&disc_key) {
        Some(&i) => i,
        None => {
            return Err(ZerxError::new(
                ErrorCode::INVALID_DISCRIMINANT,
                format!("no variant matched discriminator '{}'", body.key),
            )
            .expected(allowed.join(", "))
            .received(type_tag(disc_value)));
        }
    };

    body.variants[index].parse_present(value, ctx)
}

// ---------------------------------------------------------------------------
// Array-length validators
// ---------------------------------------------------------------------------

pub(crate) struct ArrayMinLength(pub usize);
pub(crate) struct ArrayMaxLength(pub usize);

impl Validator for ArrayMinLength {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::Array(a) = value {
            if a.len() < self.0 {
                return Err(ZerxError::new(
                    ErrorCode::ARRAY_TOO_SHORT,
                    format!("array length {} is less than minimum {}", a.len(), self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("minItems".to_owned(), serde_json::Value::Number(self.0.into()));
        m
    }
}

impl Validator for ArrayMaxLength {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError> {
        if let ZerxValue::Array(a) = value {
            if a.len() > self.0 {
                return Err(ZerxError::new(
                    ErrorCode::ARRAY_TOO_LONG,
                    format!("array length {} exceeds maximum {}", a.len(), self.0),
                ));
            }
        }
        Ok(())
    }

    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("maxItems".to_owned(), serde_json::Value::Number(self.0.into()));
        m
    }
}

// ---------------------------------------------------------------------------
// T2 builder newtypes
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ObjectSchema(Schema);

#[derive(Clone)]
pub struct ArraySchema(Schema);

#[derive(Clone)]
pub struct RecordSchema(Schema);

#[derive(Clone)]
pub struct TupleSchema(Schema);

#[derive(Clone)]
pub struct UnionSchema(Schema);

#[derive(Clone)]
pub struct DiscriminatedUnionSchema(Schema);

#[derive(Clone)]
pub struct LiteralSchema(Schema);

impl BuilderInner for ObjectSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for ArraySchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for RecordSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for TupleSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for UnionSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for DiscriminatedUnionSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for LiteralSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}

impl From<ObjectSchema> for Schema {
    fn from(b: ObjectSchema) -> Schema { b.0 }
}
impl From<ArraySchema> for Schema {
    fn from(b: ArraySchema) -> Schema { b.0 }
}
impl From<RecordSchema> for Schema {
    fn from(b: RecordSchema) -> Schema { b.0 }
}
impl From<TupleSchema> for Schema {
    fn from(b: TupleSchema) -> Schema { b.0 }
}
impl From<UnionSchema> for Schema {
    fn from(b: UnionSchema) -> Schema { b.0 }
}
impl From<DiscriminatedUnionSchema> for Schema {
    fn from(b: DiscriminatedUnionSchema) -> Schema { b.0 }
}
impl From<LiteralSchema> for Schema {
    fn from(b: LiteralSchema) -> Schema { b.0 }
}

// ---------------------------------------------------------------------------
// T2 inherent methods
// ---------------------------------------------------------------------------

impl ObjectSchema {
    pub fn strict(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.mode = ObjectMode::Strict;
        }
        self
    }

    pub fn passthrough(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.mode = ObjectMode::Passthrough;
        }
        self
    }

    pub fn strip(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.mode = ObjectMode::Strip;
        }
        self
    }

    pub fn partial(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.all_optional = true;
        }
        self
    }

    pub fn extend<I, K>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = (K, Schema)>,
        K: Into<String>,
    {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            for (k, v) in fields.into_iter().map(|(k, v)| (k.into(), v)) {
                if let Some(entry) = body.shape.iter_mut().find(|(key, _)| key == &k) {
                    entry.1 = v;
                } else {
                    body.shape.push((k, v));
                }
            }
        }
        self
    }

    pub fn omit<I, K>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            let drop: Vec<String> = keys.into_iter().map(|k| k.into()).collect();
            body.shape.retain(|(k, _)| !drop.contains(k));
        }
        self
    }

    pub fn omit_read_only(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.shape.retain(|(_, fs)| !fs.modifiers.read_only);
        }
        self
    }

    pub fn omit_write_only(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.shape.retain(|(_, fs)| !fs.modifiers.write_only);
        }
        self
    }

    pub fn strip_only<I, K>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            for k in keys.into_iter().map(|k| k.into()) {
                if !body.prestrip_keys.contains(&k) {
                    body.prestrip_keys.push(k);
                }
            }
        }
        self
    }

    pub fn strip_read_only(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.prestrip_read_only = true;
        }
        self
    }

    pub fn strip_write_only(mut self) -> Self {
        if let SchemaKind::Object(ref mut body) = self.0.kind {
            body.prestrip_write_only = true;
        }
        self
    }
}

impl ArraySchema {
    pub fn min(mut self, n: usize) -> Self {
        self.0.validators.push(Rc::new(ArrayMinLength(n)));
        self
    }

    pub fn max(mut self, n: usize) -> Self {
        self.0.validators.push(Rc::new(ArrayMaxLength(n)));
        self
    }
}

// ---------------------------------------------------------------------------
// T2 constructors
// ---------------------------------------------------------------------------

pub fn object<I, K>(fields: I) -> ObjectSchema
where
    I: IntoIterator<Item = (K, Schema)>,
    K: Into<String>,
{
    let mut shape: Vec<(String, Schema)> = Vec::new();
    for (k, v) in fields.into_iter().map(|(k, v)| (k.into(), v)) {
        if let Some(entry) = shape.iter_mut().find(|(key, _)| key == &k) {
            entry.1 = v;
        } else {
            shape.push((k, v));
        }
    }
    let body = ObjectBody {
        shape,
        mode: ObjectMode::Strict,
        all_optional: false,
        prestrip_keys: Vec::new(),
        prestrip_read_only: false,
        prestrip_write_only: false,
    };
    ObjectSchema(Schema::new(SchemaKind::Object(body)))
}

pub fn array(item: impl Into<Schema>) -> ArraySchema {
    ArraySchema(Schema::new(SchemaKind::Array(Box::new(item.into()))))
}

pub fn record(value: impl Into<Schema>) -> RecordSchema {
    RecordSchema(Schema::new(SchemaKind::Record(Box::new(value.into()))))
}

pub fn tuple<I>(items: I) -> TupleSchema
where
    I: IntoIterator<Item = Schema>,
{
    TupleSchema(Schema::new(SchemaKind::Tuple(items.into_iter().collect())))
}

pub fn union<I>(variants: I) -> UnionSchema
where
    I: IntoIterator<Item = Schema>,
{
    UnionSchema(Schema::new(SchemaKind::Union(variants.into_iter().collect())))
}

pub fn discriminated_union<K, I>(key: K, variants: I) -> DiscriminatedUnionSchema
where
    K: Into<String>,
    I: IntoIterator<Item = Schema>,
{
    let key: String = key.into();
    let variants: Vec<Schema> = variants.into_iter().collect();
    let state = build_discriminator_state(&key, &variants);
    let body = DiscriminatedUnionBody { key, variants, state };
    DiscriminatedUnionSchema(Schema::new(SchemaKind::DiscriminatedUnion(body)))
}

fn build_discriminator_state(key: &str, variants: &[Schema]) -> DiscriminatorState {
    let mut map = std::collections::HashMap::new();
    let mut allowed = Vec::new();

    for (index, variant) in variants.iter().enumerate() {
        let body = match &variant.kind {
            SchemaKind::Object(b) => b,
            _ => {
                return DiscriminatorState::Invalid(format!(
                    "variant {} is not an object schema",
                    index
                ));
            }
        };

        let field_schema = match body.shape.iter().find(|(k, _)| k == key) {
            Some((_, s)) => s,
            None => {
                return DiscriminatorState::Invalid(format!(
                    "variant {} is missing discriminator field '{}'",
                    index, key
                ));
            }
        };

        let constant = match &field_schema.kind {
            SchemaKind::Literal(c) => c,
            _ => {
                return DiscriminatorState::Invalid(format!(
                    "variant {}: discriminator field '{}' is not a literal",
                    index, key
                ));
            }
        };

        if field_schema.modifiers.optional {
            return DiscriminatorState::Invalid(format!(
                "variant {}: discriminator field '{}' must not be optional",
                index, key
            ));
        }
        if field_schema.modifiers.default.is_some() {
            return DiscriminatorState::Invalid(format!(
                "variant {}: discriminator field '{}' must not have a default",
                index, key
            ));
        }
        if field_schema.modifiers.nullable {
            return DiscriminatorState::Invalid(format!(
                "variant {}: discriminator field '{}' must not be nullable",
                index, key
            ));
        }

        let disc_key = match discriminant_key(constant) {
            Some(k) => k,
            None => {
                return DiscriminatorState::Invalid(format!(
                    "variant {}: discriminator literal is not a keyable type",
                    index
                ));
            }
        };

        let display = literal_descriptor(constant);

        if map.contains_key(&disc_key) {
            return DiscriminatorState::Invalid(format!(
                "duplicate discriminator value '{}' in variant {}",
                display, index
            ));
        }

        map.insert(disc_key, index);
        allowed.push(display);
    }

    DiscriminatorState::Ok { map, allowed }
}

pub fn literal(value: impl Into<ZerxValue>) -> LiteralSchema {
    LiteralSchema(Schema::new(SchemaKind::Literal(value.into())))
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

pub fn string() -> StringSchema {
    StringSchema(Schema::new(SchemaKind::String))
}

pub fn number() -> NumberSchema {
    NumberSchema(Schema::new(SchemaKind::Number))
}

pub fn boolean() -> BooleanSchema {
    BooleanSchema(Schema::new(SchemaKind::Boolean))
}

pub fn enumerate<I, S>(values: I) -> EnumSchema
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let set: Vec<String> = values.into_iter().map(Into::into).collect();
    EnumSchema(Schema::new(SchemaKind::Enum(set)))
}

pub fn null() -> NullSchema {
    NullSchema(Schema::new(SchemaKind::Null))
}

// ---------------------------------------------------------------------------
// T4 error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const INVALID_URI: ErrorCode = ErrorCode::new("invalid_uri");
    pub const INVALID_URL: ErrorCode = ErrorCode::new("invalid_url");
}

// ---------------------------------------------------------------------------
// T4 structural validators
// ---------------------------------------------------------------------------

fn is_valid_uri(s: &str) -> bool {
    // scheme ':' path  — scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ); path non-empty
    let colon = match s.find(':') {
        Some(i) => i,
        None => return false,
    };
    let scheme = &s[..colon];
    let rest = &s[colon + 1..];
    if scheme.is_empty() || rest.is_empty() {
        return false;
    }
    let mut chars = scheme.chars();
    if !chars.next().unwrap().is_ascii_alphabetic() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
}

fn url_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)^https?://(?:[-\w.])+(?::[0-9]+)?(?:/(?:[\w/_.-])*)?(?:\?(?:[\w&=%.~!$'()*+,;:@/-])*)?(?:#(?:[\w.~!$'()*+,;:@/-])*)?$"
        ).expect("url regex is valid")
    })
}

fn is_valid_url(s: &str) -> bool {
    if !url_regex().is_match(s) {
        return false;
    }
    // The regex guarantees the string starts with http(s)://, so "://" is present.
    let after_scheme = &s[s.find("://").unwrap() + 3..];
    let host_port_end = after_scheme
        .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
        .unwrap_or(after_scheme.len());
    let host_port = &after_scheme[..host_port_end];
    if host_port.is_empty() {
        return false;
    }
    // Split hostname and optional port (trailing ":digits")
    let (hostname, port_opt) = if let Some(colon_pos) = host_port.rfind(':') {
        let candidate = &host_port[colon_pos + 1..];
        if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_digit()) {
            (&host_port[..colon_pos], Some(candidate))
        } else {
            (host_port, None)
        }
    } else {
        (host_port, None)
    };
    if hostname.is_empty() {
        return false;
    }
    if hostname.contains("..") || hostname.starts_with('.') || hostname.ends_with('.') {
        return false;
    }
    if !hostname.contains('.') && hostname != "localhost" {
        return false;
    }
    if let Some(p) = port_opt {
        match p.parse::<u32>() {
            Ok(n) if (1..=65535).contains(&n) => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// T4 type-check delegates
// ---------------------------------------------------------------------------

pub(crate) fn check_buffer(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::Bytes(_) => Ok(()),
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected buffer")
            .expected("buffer")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_uri(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::String(s) => {
            if is_valid_uri(s) {
                Ok(())
            } else {
                Err(ZerxError::new(ErrorCode::INVALID_URI, "string is not a valid URI")
                    .expected("uri"))
            }
        }
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected uri")
            .expected("uri")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_url(value: &ZerxValue) -> Result<(), ZerxError> {
    match value {
        ZerxValue::String(s) => {
            if is_valid_url(s) {
                Ok(())
            } else {
                Err(ZerxError::new(ErrorCode::INVALID_URL, "string is not a valid URL")
                    .expected("url"))
            }
        }
        _ => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected url")
            .expected("url")
            .received(type_tag(value))),
    }
}

pub(crate) fn check_json(_value: &ZerxValue) -> Result<(), ZerxError> {
    Ok(())
}

pub(crate) fn check_jsonschema(_value: &ZerxValue) -> Result<(), ZerxError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// T4 builder newtypes
// ---------------------------------------------------------------------------

/// Builder for `buffer` schemas. Accepts only `ZerxValue::Bytes` (no coercion, C2).
///
/// ```compile_fail
/// use zerx::buffer;
/// let _ = buffer().min(3);   // ERROR: no method `min` on BufferSchema
/// ```
#[derive(Clone)]
pub struct BufferSchema(Schema);

/// Builder for `uri` schemas. Validates the RFC 3986 scheme/path structure.
///
/// ```compile_fail
/// use zerx::uri;
/// let _ = uri().min(3);   // ERROR: no method `min` on UriSchema
/// ```
#[derive(Clone)]
pub struct UriSchema(Schema);

/// Builder for `url` schemas. Validates HTTP/HTTPS URL structure.
///
/// ```compile_fail
/// use zerx::url;
/// let _ = url().min(3);   // ERROR: no method `min` on UrlSchema
/// ```
#[derive(Clone)]
pub struct UrlSchema(Schema);

/// Builder for `json` schemas. Accepts any serde-bridgeable value; identity parse.
///
/// ```compile_fail
/// use zerx::json;
/// let _ = json().min(3);   // ERROR: no method `min` on JsonSchema
/// ```
#[derive(Clone)]
pub struct JsonSchema(Schema);

/// Builder for `jsonschema` schemas. Accepts any serde-bridgeable value; identity parse.
///
/// ```compile_fail
/// use zerx::jsonschema;
/// let _ = jsonschema().min(3);   // ERROR: no method `min` on JsonschemaSchema
/// ```
#[derive(Clone)]
pub struct JsonschemaSchema(Schema);

impl BuilderInner for BufferSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for UriSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for UrlSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for JsonSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}
impl BuilderInner for JsonschemaSchema {
    fn schema_mut(&mut self) -> &mut Schema { &mut self.0 }
}

impl From<BufferSchema> for Schema {
    fn from(b: BufferSchema) -> Schema { b.0 }
}
impl From<UriSchema> for Schema {
    fn from(b: UriSchema) -> Schema { b.0 }
}
impl From<UrlSchema> for Schema {
    fn from(b: UrlSchema) -> Schema { b.0 }
}
impl From<JsonSchema> for Schema {
    fn from(b: JsonSchema) -> Schema { b.0 }
}
impl From<JsonschemaSchema> for Schema {
    fn from(b: JsonschemaSchema) -> Schema { b.0 }
}

// ---------------------------------------------------------------------------
// T4 inherent methods
// ---------------------------------------------------------------------------

impl BufferSchema {
    /// Set the MIME type for this buffer. Writes `modifiers.mime` (same field as
    /// `mime_format`); read by J1 to emit `contentMediaType`.
    pub fn mime(mut self, mime_type: impl Into<String>) -> Self {
        self.0.modifiers.mime = Some(mime_type.into());
        self
    }
}

// ---------------------------------------------------------------------------
// T4 constructors
// ---------------------------------------------------------------------------

pub fn buffer() -> BufferSchema {
    BufferSchema(Schema::new(SchemaKind::Buffer))
}

pub fn uri() -> UriSchema {
    UriSchema(Schema::new(SchemaKind::Uri))
}

pub fn url() -> UrlSchema {
    UrlSchema(Schema::new(SchemaKind::Url))
}

pub fn json() -> JsonSchema {
    JsonSchema(Schema::new(SchemaKind::Json))
}

pub fn jsonschema() -> JsonschemaSchema {
    JsonschemaSchema(Schema::new(SchemaKind::JsonSchema))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ParseContext;
    use crate::{Modify, Schema};

    // Helper: validate a builder against a serializable value
    fn val<B: Into<Schema>>(builder: B, v: &(impl serde::Serialize + ?Sized)) -> Result<ZerxValue, ZerxError> {
        let s: Schema = builder.into();
        s.validate(v)
    }

    // 1. string type check
    #[test]
    fn string_type_check() {
        assert!(val(string(), "hi").is_ok());
        let err = val(string(), &7i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("string"));
        assert_eq!(err.received.as_deref(), Some("number"));

        let err2 = val(string(), &true).unwrap_err();
        assert_eq!(err2.received.as_deref(), Some("boolean"));

        let err3 = val(string(), &()).unwrap_err();
        assert_eq!(err3.received.as_deref(), Some("null"));
    }

    // 2. string min/max
    #[test]
    fn string_min_max() {
        // min
        assert!(val(string().min(2), "hi").is_ok());
        assert!(val(string().min(2), "ab").is_ok()); // boundary passes
        let err = val(string().min(2), "h").unwrap_err();
        assert_eq!(err.code, ErrorCode::STRING_TOO_SHORT);

        // max
        assert!(val(string().max(3), "hey").is_ok());
        assert!(val(string().max(3), "hey").is_ok()); // boundary passes
        let err2 = val(string().max(3), "heyy").unwrap_err();
        assert_eq!(err2.code, ErrorCode::STRING_TOO_LONG);

        // json_schema fragments
        let frag_min = MinLength(2).json_schema();
        assert_eq!(frag_min["minLength"], serde_json::json!(2));
        let frag_max = MaxLength(3).json_schema();
        assert_eq!(frag_max["maxLength"], serde_json::json!(3));
    }

    // 3. string regex / pattern
    #[test]
    fn string_regex_and_pattern() {
        let re_schema = string().regex(r"^\d{5}$");
        assert!(val(re_schema, "12345").is_ok());
        let err = val(string().regex(r"^\d{5}$"), "abc").unwrap_err();
        assert_eq!(err.code, ErrorCode::PATTERN_MISMATCH);

        // pattern alias behaves identically
        assert!(val(string().pattern(r"^\d{5}$"), "12345").is_ok());
        let err2 = val(string().pattern(r"^\d{5}$"), "abc").unwrap_err();
        assert_eq!(err2.code, ErrorCode::PATTERN_MISMATCH);

        // invalid pattern → PATTERN_INVALID at validate time
        let err3 = val(string().regex("("), "any").unwrap_err();
        assert_eq!(err3.code, ErrorCode::PATTERN_INVALID);

        // json_schema preserves source even for invalid pattern
        let p = Pattern::new(r"^\d{5}$");
        let frag = p.json_schema();
        assert_eq!(frag["pattern"], serde_json::json!(r"^\d{5}$"));

        let p_inv = Pattern::new("(");
        let frag_inv = p_inv.json_schema();
        assert_eq!(frag_inv["pattern"], serde_json::json!("("));
    }

    // 4. string email / uuid
    #[test]
    fn string_email_uuid() {
        assert!(val(string().email(), "a@b.co").is_ok());
        assert_eq!(
            val(string().email(), "a@b").unwrap_err().code,
            ErrorCode::INVALID_EMAIL
        );
        assert_eq!(
            val(string().email(), "a b@c.d").unwrap_err().code,
            ErrorCode::INVALID_EMAIL
        );
        assert_eq!(Email.json_schema()["format"], serde_json::json!("email"));

        let uuid_ok = "550e8400-e29b-41d4-a716-446655440000";
        let uuid_upper = "550E8400-E29B-41D4-A716-446655440000";
        assert!(val(string().uuid(), uuid_ok).is_ok());
        assert!(val(string().uuid(), uuid_upper).is_ok());
        assert_eq!(
            val(string().uuid(), "too-short").unwrap_err().code,
            ErrorCode::INVALID_UUID
        );
        assert_eq!(
            val(string().uuid(), "550e8400-e29b-41d4-a716-44665544000X").unwrap_err().code,
            ErrorCode::INVALID_UUID
        );
        assert_eq!(Uuid.json_schema()["format"], serde_json::json!("uuid"));
    }

    // 5. multiline is a meta hint, not a validator
    #[test]
    fn multiline_is_meta_hint() {
        let b = string().multiline(3);
        assert_eq!(
            b.0.modifiers.meta.get("x-ui-multiline"),
            Some(&ZerxValue::U64(3))
        );
        assert_eq!(b.0.validators.len(), 0);
        // still validates successfully
        assert!(val(string().multiline(3), "x").is_ok());
    }

    // 6. number type check
    #[test]
    fn number_type_check() {
        assert!(val(number(), &7i32).is_ok());
        assert!(val(number(), &7u64).is_ok());
        assert!(val(number(), &i128::MAX).is_ok());
        assert!(val(number(), &u128::MAX).is_ok());
        assert!(val(number(), &2.5f64).is_ok());

        let err = val(number(), "hi").unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("number"));

        let err2 = val(number(), &true).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);

        // non-finite F64 via parse_present
        let s: Schema = number().into();
        let mut ctx = ParseContext::new();
        let nan_err = s
            .parse_present(&ZerxValue::F64(f64::NAN), &mut ctx)
            .unwrap_err();
        assert_eq!(nan_err.code, ErrorCode::TYPE_MISMATCH);

        let inf_err = s
            .parse_present(&ZerxValue::F64(f64::INFINITY), &mut ctx)
            .unwrap_err();
        assert_eq!(inf_err.code, ErrorCode::TYPE_MISMATCH);
    }

    // 7. number min/max with integer-literal ergonomics
    #[test]
    fn number_min_max() {
        assert!(val(number().min(0), &0i32).is_ok());
        assert!(val(number().min(0), &5i32).is_ok());
        let err = val(number().min(0), &-1i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::NUMBER_TOO_SMALL);

        assert!(val(number().max(9), &9i32).is_ok());
        let err2 = val(number().max(9), &10i32).unwrap_err();
        assert_eq!(err2.code, ErrorCode::NUMBER_TOO_LARGE);

        let frag_min = MinValue(0.0).json_schema();
        assert_eq!(frag_min["minimum"], serde_json::json!(0.0));
        let frag_max = MaxValue(9.0).json_schema();
        assert_eq!(frag_max["maximum"], serde_json::json!(9.0));
    }

    // 8. number int
    #[test]
    fn number_int() {
        assert!(val(number().int(), &7i64).is_ok());
        assert!(val(number().int(), &4.0f64).is_ok());
        let err = val(number().int(), &2.5f64).unwrap_err();
        assert_eq!(err.code, ErrorCode::NOT_INTEGER);

        let frag = IntValidator.json_schema();
        assert_eq!(frag["type"], serde_json::json!("integer"));
    }

    // 9. boolean type check
    #[test]
    fn boolean_type_check() {
        assert!(val(boolean(), &true).is_ok());
        assert!(val(boolean(), &false).is_ok());
        let err = val(boolean(), &1i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("boolean"));
        let err2 = val(boolean(), "true").unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
    }

    // 10. enumerate
    #[test]
    fn enumerate_check() {
        let e = enumerate(["user", "assistant", "system"]);
        assert!(val(e, "assistant").is_ok());

        let err = val(enumerate(["user", "assistant", "system"]), "other").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_ENUM_VALUE);
        // expected lists the allowed values
        let exp = err.expected.as_deref().unwrap_or("");
        assert!(exp.contains("user"));
        assert!(exp.contains("assistant"));
        // received is a descriptor, not the raw value
        assert_eq!(err.received.as_deref(), Some("string"));

        let err2 = val(enumerate(["user"]), &7i32).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
    }

    // 11. null type
    #[test]
    fn null_type_check() {
        assert!(val(null(), &()).is_ok());
        let err = val(null(), &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("null"));
        let err2 = val(null(), &false).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
    }

    // 12. type-specific-method composition
    #[test]
    fn composition() {
        // string().min(3).optional() and reverse order
        let s1: Schema = string().min(3).optional().into();
        let s2: Schema = string().optional().min(3).into();

        let err1 = s1.validate("hi").unwrap_err();
        assert_eq!(err1.code, ErrorCode::STRING_TOO_SHORT);
        let err2 = s2.validate("hi").unwrap_err();
        assert_eq!(err2.code, ErrorCode::STRING_TOO_SHORT);

        assert!(s1.validate("hello").is_ok());
        assert!(s2.validate("hello").is_ok());

        // optional: missing yields Ok(None)
        let mut ctx = ParseContext::new();
        assert_eq!(s1.parse_field(None, &mut ctx).unwrap(), None);

        // number().int().min(0).max(9)
        let n: Schema = number().int().min(0).max(9).into();
        assert!(n.validate(&5i32).is_ok());
        let err3 = n.validate(&-1i32).unwrap_err();
        assert_eq!(err3.code, ErrorCode::NUMBER_TOO_SMALL);
        let err4 = n.validate(&2.5f64).unwrap_err();
        assert_eq!(err4.code, ErrorCode::NOT_INTEGER);
    }

    // 13. negative compile-guarantee is in the doc comment on BooleanSchema

    // 14. check_type → validators ordering observable
    #[test]
    fn check_type_before_validators() {
        let s: Schema = number().min(5).into();
        let err = s.validate("hi").unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
    }

    // 15. Clone immutability with validators
    #[test]
    fn clone_immutability_with_validators() {
        let base = string().min(1);
        let s2: Schema = base.clone().max(5).into();
        let s1: Schema = base.into();
        assert_eq!(s1.validators.len(), 1);
        assert_eq!(s2.validators.len(), 2);
    }

    // 16. SchemaKind dispatch + Debug
    #[test]
    fn schema_kind_debug() {
        let cases: &[(&str, Schema)] = &[
            ("String", string().into()),
            ("Number", number().into()),
            ("Boolean", boolean().into()),
            ("Enum", enumerate(["a"]).into()),
            ("Null", null().into()),
        ];
        for (expected, s) in cases {
            let debug = format!("{:?}", s);
            assert!(
                debug.contains(expected),
                "expected kind {expected} in debug output: {debug}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // T2 tests
    // -----------------------------------------------------------------------

    // T2-1: object type check
    #[test]
    fn object_type_check() {
        let o = object([("a", number().into())]);
        let err = val(o.clone(), &7i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("object"));

        assert_eq!(val(o.clone(), "hi").unwrap_err().code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(val(o, &true).unwrap_err().code, ErrorCode::TYPE_MISMATCH);
    }

    // T2-2a: object constructor duplicate-key semantics (last-write-wins, first-occurrence order)
    #[test]
    fn object_duplicate_key_semantics() {
        // Duplicate key: second entry's schema replaces first, but key stays at first position.
        // The second schema is number(), so "a" must be validated as a number.
        let o: Schema = object([
            ("a", string().into()),
            ("b", number().into()),
            ("a", number().into()),  // replaces the string() at first-occurrence position
        ]).into();

        // Valid: a=1 (number), b=2 — the string() schema was replaced
        assert!(o.validate(&serde_json::json!({"a": 1, "b": 2})).is_ok());

        // Key order: "a" at first-occurrence position (0), "b" at position 1
        let ok = o.validate(&serde_json::json!({"a": 1, "b": 2})).unwrap();
        let keys: Vec<&str> = ok.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b"]);

        // Shape has only two entries (the duplicate was merged)
        if let crate::schema::SchemaKind::Object(ref body) = o.kind {
            assert_eq!(body.shape.len(), 2);
        }
    }

    // T2-2: object strict (default)
    #[test]
    fn object_strict_default() {
        #[derive(serde::Serialize)]
        struct WithExtra { a: i64, extra: i64 }
        #[derive(serde::Serialize)]
        struct JustA { a: i64 }

        let o = object([("a", number().into())]);
        let err = val(o.clone(), &WithExtra { a: 1, extra: 2 }).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert!(!err.path.is_empty());

        let ok = val(o, &JustA { a: 1 }).unwrap();
        let map = ok.as_object().unwrap();
        assert_eq!(map.get("a"), Some(&ZerxValue::I64(1)));
        assert_eq!(map.len(), 1);
    }

    // T2-3: object passthrough
    #[test]
    fn object_passthrough() {
        use serde_json::json;

        #[derive(serde::Serialize)]
        struct Input { a: i64, extra: &'static str }

        let o = object([("a", number().into())]).passthrough();
        let ok = val(o, &Input { a: 1, extra: "x" }).unwrap();
        let map = ok.as_object().unwrap();
        assert!(map.get("a").is_some());
        assert!(map.get("extra").is_some());
        // Key order: shape first, then passthrough
        let keys: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "extra"]);
        // Extra field preserved verbatim
        assert_eq!(map.get("extra"), Some(&ZerxValue::from_serialize(&json!("x")).unwrap()));
    }

    // T2-4: object strip
    #[test]
    fn object_strip() {
        #[derive(serde::Serialize)]
        struct Input { a: i64, extra: &'static str }

        let o = object([("a", number().into())]).strip();
        let ok = val(o, &Input { a: 1, extra: "x" }).unwrap();
        let map = ok.as_object().unwrap();
        assert!(map.get("a").is_some());
        assert!(map.get("extra").is_none());
    }

    // T2-5: object field errors carry path
    #[test]
    fn object_field_error_path() {
        #[derive(serde::Serialize)]
        struct AgeInput { age: &'static str }

        let o = object([("age", number().into())]);
        let err = val(o, &AgeInput { age: "x" }).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.path, vec!["age"]);

        // Nested
        #[derive(serde::Serialize)]
        struct Nested { user: AgeInput }
        let nested = object([("user", object([("age", number().into())]).into())]);
        let err2 = val(nested, &Nested { user: AgeInput { age: "x" } }).unwrap_err();
        assert_eq!(err2.path, vec!["user", "age"]);
    }

    // T2-6: object optional / default / required
    #[test]
    fn object_optional_default_required() {
        use serde_json::json;

        // {a: optional, b: default(5), c: required}
        let schema = object([
            ("a", number().optional().into()),
            ("b", number().default(5i64).into()),
            ("c", number().into()),
        ]);

        #[derive(serde::Serialize)]
        struct JustC { c: i64 }

        let ok = val(schema.clone(), &JustC { c: 1 }).unwrap();
        let map = ok.as_object().unwrap();
        assert!(map.get("a").is_none(), "optional absent field should be omitted");
        assert_eq!(map.get("b"), Some(&ZerxValue::I64(5)));
        assert_eq!(map.get("c"), Some(&ZerxValue::I64(1)));

        // Missing required field
        let schema2 = object([
            ("a", number().optional().into()),
            ("b", number().default(5i64).into()),
            ("c", number().into()),
        ]);
        let err = val(schema2, &json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::REQUIRED);
        assert_eq!(err.path, vec!["c"]);
    }

    // T2-7: array
    #[test]
    fn array_type_and_element() {
        let a = array(number());
        assert!(val(a.clone(), &vec![1i64, 2, 3]).is_ok());

        let err = val(a.clone(), &7i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("array"));

        // Element error with path
        let mixed: Vec<serde_json::Value> = vec![
            serde_json::json!(1),
            serde_json::json!("x"),
            serde_json::json!(3),
        ];
        let err2 = val(array(number()), &mixed).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err2.path, vec!["1"]);
    }

    // T2-8: array min/max and json_schema fragments
    #[test]
    fn array_min_max() {
        assert!(val(array(number()).min(2), &vec![1i64, 2]).is_ok());
        let err = val(array(number()).min(2), &vec![1i64]).unwrap_err();
        assert_eq!(err.code, ErrorCode::ARRAY_TOO_SHORT);

        assert!(val(array(number()).max(2), &vec![1i64, 2]).is_ok());
        let err2 = val(array(number()).max(2), &vec![1i64, 2, 3]).unwrap_err();
        assert_eq!(err2.code, ErrorCode::ARRAY_TOO_LONG);

        assert_eq!(ArrayMinLength(2).json_schema()["minItems"], serde_json::json!(2));
        assert_eq!(ArrayMaxLength(2).json_schema()["maxItems"], serde_json::json!(2));
    }

    // T2-9: record
    #[test]
    fn record_type_and_values() {
        use serde_json::json;

        let r = record(number());
        let ok = val(r.clone(), &json!({"x": 1, "y": 2})).unwrap();
        let map = ok.as_object().unwrap();
        assert!(map.get("x").is_some());
        assert!(map.get("y").is_some());

        let err = val(record(number()), &7i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("object"));

        let mixed = json!({"x": 1, "y": "z"});
        let err2 = val(record(number()), &mixed).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err2.path, vec!["y"]);
    }

    // T2-10: tuple
    #[test]
    fn tuple_validation() {
        let t = tuple([string().into(), number().into()]);
        let ok_input: (&str, i64) = ("a", 1);
        assert!(val(t.clone(), &ok_input).is_ok());

        // Wrong length
        let err = val(tuple([string().into(), number().into()]), &vec![serde_json::json!("a")]).unwrap_err();
        assert_eq!(err.code, ErrorCode::TUPLE_LENGTH_MISMATCH);

        // Element type error
        let bad: (&str, &str) = ("a", "b");
        let err2 = val(tuple([string().into(), number().into()]), &bad).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err2.path, vec!["1"]);

        // Non-array
        let err3 = val(tuple([string().into()]), &7i32).unwrap_err();
        assert_eq!(err3.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err3.expected.as_deref(), Some("array"));
    }

    // T2-11: union
    #[test]
    fn union_matching() {
        let u = union([number().into(), string().into()]);
        assert!(val(u.clone(), &1i64).is_ok());
        assert!(val(u.clone(), "x").is_ok());

        let err = val(u, &true).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNION_MISMATCH);
        assert_eq!(err.inner_errors.len(), 2);
    }

    // T2-12: literal
    #[test]
    fn literal_matching() {
        assert!(val(literal("human"), "human").is_ok());
        assert_eq!(val(literal("human"), "tool").unwrap_err().code, ErrorCode::INVALID_LITERAL);
        assert_eq!(val(literal("human"), &7i32).unwrap_err().code, ErrorCode::INVALID_LITERAL);

        assert!(val(literal(true), &true).is_ok());
        assert_eq!(val(literal(true), &false).unwrap_err().code, ErrorCode::INVALID_LITERAL);

        // Numeric exact-match: literal(5i64) matches I64(5) but not U64(5)
        let v_i64 = ZerxValue::from_serialize(&5i64).unwrap();
        let s: Schema = literal(5i64).into();
        assert!(s.parse_present(&v_i64, &mut ParseContext::new()).is_ok());
    }

    // T2-13: discriminated_union happy path
    #[test]
    fn discriminated_union_happy_path() {
        let du = discriminated_union("kind", [
            object([("kind", literal("human").into()), ("session", string().into())]).into(),
            object([("kind", literal("tool").into()), ("tool", string().into())]).into(),
        ]);

        use serde_json::json;
        assert!(val(du.clone(), &json!({"kind": "human", "session": "s"})).is_ok());
        assert!(val(du.clone(), &json!({"kind": "tool", "tool": "t"})).is_ok());

        let err = val(du.clone(), &json!({"kind": "other"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT);

        let err2 = val(du.clone(), &json!({"session": "s"})).unwrap_err();
        assert_eq!(err2.code, ErrorCode::INVALID_DISCRIMINANT);

        let err3 = val(du.clone(), &7i32).unwrap_err();
        assert_eq!(err3.code, ErrorCode::TYPE_MISMATCH);

        // Field error inside matched variant propagates with path
        let err4 = val(du, &json!({"kind": "human", "session": 7})).unwrap_err();
        assert_eq!(err4.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err4.path, vec!["session"]);
    }

    // T2-14: discriminated_union config error (deferred, C7)
    #[test]
    fn discriminated_union_config_errors() {
        // (a) variant not an object
        let du_a = discriminated_union("kind", [number().into()]);
        assert_eq!(val(du_a.clone(), &7i32).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);
        assert_eq!(
            val(du_a, &serde_json::json!({"kind": "x"})).unwrap_err().code,
            ErrorCode::INVALID_DISCRIMINATED_UNION
        );

        // (b) discriminator field not a literal
        let du_b = discriminated_union("kind", [
            object([("kind", string().into())]).into(),
        ]);
        assert_eq!(val(du_b, &serde_json::json!({"kind": "x"})).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);

        // (c) discriminator optional
        let du_c = discriminated_union("kind", [
            object([("kind", literal("human").optional().into())]).into(),
        ]);
        assert_eq!(val(du_c, &serde_json::json!({"kind": "human"})).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);

        // (c2) discriminator with default
        let du_c2 = discriminated_union("kind", [
            object([("kind", literal("human").default("human").into())]).into(),
        ]);
        assert_eq!(val(du_c2, &serde_json::json!({"kind": "human"})).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);

        // (c3) discriminator nullable
        let du_c3 = discriminated_union("kind", [
            object([("kind", literal("human").nullable().into())]).into(),
        ]);
        assert_eq!(val(du_c3, &serde_json::json!({"kind": "human"})).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);

        // (d) duplicate discriminator value
        let du_d = discriminated_union("kind", [
            object([("kind", literal("human").into())]).into(),
            object([("kind", literal("human").into())]).into(),
        ]);
        assert_eq!(val(du_d, &serde_json::json!({"kind": "human"})).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);

        // Non-object input against mis-configured DU → INVALID_DISCRIMINATED_UNION, not TYPE_MISMATCH
        let du_e = discriminated_union("kind", [number().into()]);
        assert_eq!(val(du_e, &7i32).unwrap_err().code, ErrorCode::INVALID_DISCRIMINATED_UNION);
    }

    // T2-15: nesting and composition with deep path
    #[test]
    fn nesting_and_composition() {
        use serde_json::json;

        let schema = object([
            ("tags", array(string()).max(2).into()),
            ("pair", tuple([number().into(), number().into()]).into()),
            ("meta", record(number()).into()),
        ]).strip();

        let ok = val(schema.clone(), &json!({
            "tags": ["a", "b"],
            "pair": [1, 2],
            "meta": {"x": 1},
            "extra": "dropped"
        })).unwrap();
        assert!(ok.as_object().unwrap().get("extra").is_none());

        // Deep error path: pair[1] is wrong type
        let err = val(schema, &json!({
            "tags": ["a", "b"],
            "pair": [1, "x"],
            "meta": {"x": 1}
        })).unwrap_err();
        assert_eq!(err.path, vec!["pair", "1"]);
    }

    // T2-16: SchemaKind dispatch + Debug for new variants
    #[test]
    fn schema_kind_debug_t2() {
        let cases: &[(&str, Schema)] = &[
            ("Object", object([("a", number().into())]).into()),
            ("Array", array(number()).into()),
            ("Record", record(number()).into()),
            ("Tuple", tuple([number().into()]).into()),
            ("Union", union([number().into()]).into()),
            ("DiscriminatedUnion", discriminated_union("k", [
                object([("k", literal("v").into())]).into()
            ]).into()),
            ("Literal", literal("x").into()),
        ];
        for (expected, s) in cases {
            let debug = format!("{:?}", s);
            assert!(debug.contains(expected), "expected {expected} in: {debug}");
        }
    }

    // T2-17: depth guard composes with containers
    #[test]
    fn depth_guard_with_containers() {
        use crate::schema::{ParseContext, MAX_PARSE_DEPTH};

        // array(array(array(...))) 101 deep exceeds limit
        let mut s: Schema = array(number()).into();
        for _ in 0..MAX_PARSE_DEPTH {
            let prev = s.clone();
            s = array(prev).into();
        }
        // Build a deeply nested array value
        let mut v = serde_json::json!([]);
        for _ in 0..MAX_PARSE_DEPTH {
            v = serde_json::json!([v]);
        }
        let err = s.validate(&v).unwrap_err();
        assert_eq!(err.code, ErrorCode::PARSE_DEPTH_EXCEEDED);

        // Same context not poisoned
        let mut ctx = ParseContext::new();
        let shallow: Schema = array(number()).into();
        assert!(shallow.parse_present(&ZerxValue::Array(vec![ZerxValue::I64(1)]), &mut ctx).is_ok());
    }

    // T2-18: clone immutability with object modes
    #[test]
    fn clone_immutability_object_mode() {
        let base = object([("a", number().into())]);
        let passthrough: Schema = base.clone().passthrough().into();
        let strict: Schema = Schema::from(base);

        // passthrough accepts unknown keys
        #[derive(serde::Serialize)]
        struct WithExtra { a: i64, x: i64 }
        assert!(passthrough.validate(&WithExtra { a: 1, x: 2 }).is_ok());
        // strict rejects them
        assert_eq!(
            strict.validate(&WithExtra { a: 1, x: 2 }).unwrap_err().code,
            ErrorCode::UNKNOWN_PROPERTY
        );
    }

    // ---------------------------------------------------------------------------
    // T3 — object utilities
    // ---------------------------------------------------------------------------

    // T3-1: partial makes required fields optional
    #[test]
    fn partial_makes_required_optional() {
        use serde_json::json;
        let s = object([("a", number().into()), ("b", string().into())]).partial();
        // all fields missing → Ok with empty output
        let ok = val(s.clone(), &json!({})).unwrap();
        assert!(ok.as_object().unwrap().is_empty());
        // one field present, one absent → Ok with only the present one
        let ok2 = val(s, &json!({"a": 1})).unwrap();
        let m = ok2.as_object().unwrap();
        assert!(m.get("a").is_some());
        assert!(m.get("b").is_none());
    }

    // T3-2: partial still validates present fields
    #[test]
    fn partial_validates_present_fields() {
        use serde_json::json;
        let s = object([("a", number().into()), ("b", string().into())]).partial();
        let err = val(s, &json!({"a": "x"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.path, vec!["a"]);
    }

    // T3-3: partial preserves default precedence (contract-aligned)
    #[test]
    fn partial_preserves_default_precedence() {
        use serde_json::json;
        let s = object([
            ("a", number().default(5i64).into()),
            ("b", number().into()),
        ]).partial();
        // a has a default → applied; b is plain required → omitted under partial
        let ok = val(s, &json!({})).unwrap();
        let m = ok.as_object().unwrap();
        assert_eq!(m.get("a").and_then(|v| v.as_i64()), Some(5));
        assert!(m.get("b").is_none());
    }

    // T3-4: extend adds a required field
    #[test]
    fn extend_adds_required_field() {
        use serde_json::json;
        let s = object([("a", number().into())]).extend([("b", string().into())]);
        let err = val(s.clone(), &json!({"a": 1})).unwrap_err();
        assert_eq!(err.code, ErrorCode::REQUIRED);
        assert_eq!(err.path, vec!["b"]);
        let ok = val(s, &json!({"a": 1, "b": "x"})).unwrap();
        let m = ok.as_object().unwrap();
        assert!(m.get("a").is_some());
        assert!(m.get("b").is_some());
    }

    // T3-5: extend overwrites a duplicate key in place
    #[test]
    fn extend_overwrites_duplicate_key() {
        use serde_json::json;
        let s = object([("a", number().into())]).extend([("a", string().into())]);
        assert!(val(s.clone(), &json!({"a": "x"})).is_ok());
        let err = val(s, &json!({"a": 1})).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.path, vec!["a"]);
    }

    // T3-6: extend preserves mode
    #[test]
    fn extend_preserves_mode() {
        use serde_json::json;
        let s = object([("a", number().into())]).strip().extend([("b", string().into())]);
        let ok = val(s, &json!({"a": 1, "b": "x", "extra": 9})).unwrap();
        let m = ok.as_object().unwrap();
        assert!(m.get("extra").is_none());
        assert!(m.get("a").is_some());
        assert!(m.get("b").is_some());
    }

    // T3-7: omit removes a field; strict makes it unknown
    #[test]
    fn omit_removes_field() {
        use serde_json::json;
        let s = object([("a", number().into()), ("id", string().into())]).omit(["id"]);
        let ok = val(s.clone(), &json!({"a": 1})).unwrap();
        assert!(ok.as_object().unwrap().get("a").is_some());
        let err = val(s.clone(), &json!({"a": 1, "id": "x"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["id"]);
        // omitting an absent key is a no-op
        let s2 = s.omit(["nope"]);
        assert!(val(s2, &json!({"a": 1})).is_ok());
    }

    // T3-8: omit_read_only / omit_write_only remove by modifier
    #[test]
    fn omit_read_only_write_only() {
        use serde_json::json;
        // read_only
        let s_ro = object([("a", number().into()), ("ro", string().read_only().into())]).omit_read_only();
        assert!(val(s_ro.clone(), &json!({"a": 1})).is_ok());
        let err = val(s_ro, &json!({"a": 1, "ro": "x"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["ro"]);
        // write_only
        let s_wo = object([("a", number().into()), ("wo", string().write_only().into())]).omit_write_only();
        assert!(val(s_wo.clone(), &json!({"a": 1})).is_ok());
        let err2 = val(s_wo, &json!({"a": 1, "wo": "x"})).unwrap_err();
        assert_eq!(err2.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err2.path, vec!["wo"]);
    }

    // T3-9: strip_only drops an unknown key silently in strict
    #[test]
    fn strip_only_drops_unknown_key() {
        use serde_json::json;
        let s = object([("a", number().into())]).strip_only(["junk"]);
        let ok = val(s.clone(), &json!({"a": 1, "junk": true})).unwrap();
        assert!(ok.as_object().unwrap().get("junk").is_none());
        let err = val(s, &json!({"a": 1, "other": 1})).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["other"]);
    }

    // T3-10: strip_only re-defaults a stripped defaulted field
    #[test]
    fn strip_only_redefaults_stripped_field() {
        use serde_json::json;
        let s = object([("a", number().default(5i64).into())]).strip_only(["a"]);
        let ok = val(s, &json!({"a": 99})).unwrap();
        let m = ok.as_object().unwrap();
        assert_eq!(m.get("a").and_then(|v| v.as_i64()), Some(5));
    }

    // T3-11: strip_only starves a stripped required field
    #[test]
    fn strip_only_starves_required_field() {
        use serde_json::json;
        let s = object([("a", number().into())]).strip_only(["a"]);
        let err = val(s, &json!({"a": 1})).unwrap_err();
        assert_eq!(err.code, ErrorCode::REQUIRED);
        assert_eq!(err.path, vec!["a"]);
    }

    // T3-12: strip_read_only / strip_write_only drops modifier-tagged field from input
    #[test]
    fn strip_read_only_write_only() {
        use serde_json::json;
        // read_only
        let s_ro = object([
            ("a", number().into()),
            ("ro", string().read_only().optional().into()),
        ]).strip_read_only();
        let ok = val(s_ro, &json!({"a": 1, "ro": "x"})).unwrap();
        let m = ok.as_object().unwrap();
        assert!(m.get("ro").is_none());
        assert!(m.get("a").is_some());
        // write_only
        let s_wo = object([
            ("a", number().into()),
            ("wo", string().write_only().optional().into()),
        ]).strip_write_only();
        let ok2 = val(s_wo, &json!({"a": 1, "wo": "x"})).unwrap();
        let m2 = ok2.as_object().unwrap();
        assert!(m2.get("wo").is_none());
        assert!(m2.get("a").is_some());
    }

    // T3-13: omit_read_only then strip_read_only does NOT auto-strip (v1 contract)
    #[test]
    fn omit_read_only_then_strip_read_only_no_auto_strip() {
        use serde_json::json;
        // ro removed from shape by omit_read_only → strip_read_only has nothing to strip
        let s = object([
            ("a", number().into()),
            ("ro", string().read_only().into()),
        ]).omit_read_only().strip_read_only();
        let err = val(s, &json!({"a": 1, "ro": "x"})).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["ro"]);
        // explicit escape hatch: strip_only names the key directly
        let s2 = object([
            ("a", number().into()),
            ("ro", string().read_only().into()),
        ]).omit_read_only().strip_only(["ro"]);
        let ok = val(s2, &json!({"a": 1, "ro": "x"})).unwrap();
        assert!(ok.as_object().unwrap().get("ro").is_none());
        assert!(ok.as_object().unwrap().get("a").is_some());
    }

    // T3-14: clone immutability
    #[test]
    fn clone_immutability_partial() {
        use serde_json::json;
        let base = object([("a", number().into())]);
        // clone().partial() → Ok on empty input
        assert!(val(base.clone().partial(), &json!({})).is_ok());
        // original → Err(REQUIRED) on empty input
        let err = val(base, &json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::REQUIRED);
    }

    // T3-15: composition
    #[test]
    fn object_utility_composition() {
        use serde_json::json;
        // omit + extend + strip
        let s = object([("a", number().into()), ("id", string().into())])
            .omit(["id"])
            .extend([("b", string().into())])
            .strip();
        let ok = val(s, &json!({"a": 1, "b": "x", "id": "ignored", "extra": 7})).unwrap();
        let m = ok.as_object().unwrap();
        assert!(m.get("a").is_some());
        assert!(m.get("b").is_some());
        assert!(m.get("id").is_none());
        assert!(m.get("extra").is_none());

        // deep field error path is preserved under partial
        let s2 = object([("inner", object([("n", number().into())]).into())]).partial();
        let err = val(s2, &json!({"inner": {"n": "x"}})).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.path, vec!["inner", "n"]);
    }

    // -----------------------------------------------------------------------
    // T4 — special types
    // -----------------------------------------------------------------------

    // Local Blob helper: Serialize calls serialize_bytes (replicates value.rs test helper).
    struct Blob(Vec<u8>);
    impl serde::Serialize for Blob {
        fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            ser.serialize_bytes(&self.0)
        }
    }

    // T4-1: buffer accepts native bytes
    #[test]
    fn buffer_accepts_bytes() {
        let ok = val(buffer(), &Blob(vec![1, 2, 3])).unwrap();
        assert_eq!(ok, ZerxValue::Bytes(vec![1, 2, 3]));
    }

    // T4-2: buffer rejects non-bytes (C2 no-coercion)
    #[test]
    fn buffer_rejects_non_bytes() {
        // A plain Vec<u8> is NOT emitted via serialize_bytes → becomes Array
        let err = val(buffer(), &vec![1u8, 2, 3]).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err.expected.as_deref(), Some("buffer"));
        assert_eq!(err.received.as_deref(), Some("array"));

        let err2 = val(buffer(), "hi").unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err2.received.as_deref(), Some("string"));

        let err3 = val(buffer(), &7i32).unwrap_err();
        assert_eq!(err3.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err3.received.as_deref(), Some("number"));
    }

    // T4-3: buffer().mime(...) stores MIME on the modifier
    #[test]
    fn buffer_mime_stores_modifier() {
        let b = buffer().mime("image/png");
        assert_eq!(b.0.modifiers.mime.as_deref(), Some("image/png"));
        // Still validates Ok
        assert!(val(b, &Blob(vec![0])).is_ok());
        // mime and mime_format write the same field
        let b2 = buffer().mime_format("image/png");
        assert_eq!(b2.0.modifiers.mime.as_deref(), Some("image/png"));
    }

    // T4-4: uri happy + sad paths
    #[test]
    fn uri_happy_and_sad() {
        assert!(val(uri(), "https://example.com").is_ok());
        assert!(val(uri(), "mailto:a@b.co").is_ok());
        assert!(val(uri(), "urn:isbn:123").is_ok());

        // Malformed URI strings → INVALID_URI
        let sad_cases = &["not a uri", "://nohost", ":path"];
        for s in sad_cases {
            let err = val(uri(), *s).unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_URI, "expected INVALID_URI for {:?}", s);
            assert_eq!(err.expected.as_deref(), Some("uri"));
        }

        // Non-string → TYPE_MISMATCH
        let err_num = val(uri(), &7i32).unwrap_err();
        assert_eq!(err_num.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err_num.expected.as_deref(), Some("uri"));
    }

    // T4-5: url happy + sad paths
    #[test]
    fn url_happy_and_sad() {
        assert!(val(url(), "http://example.com").is_ok());
        assert!(val(url(), "https://example.com:8080/path?q=1#frag").is_ok());
        assert!(val(url(), "http://localhost").is_ok());

        let sad_cases: &[&str] = &[
            "ftp://example.com",       // wrong scheme
            "http://exa..mple.com",    // consecutive dots
            "http://nodot",            // no dot and not localhost
            "http://example.com:99999", // port out of range
        ];
        for s in sad_cases {
            let err = val(url(), *s).unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_URL, "expected INVALID_URL for {:?}", s);
            assert_eq!(err.expected.as_deref(), Some("url"));
        }

        // Non-string → TYPE_MISMATCH
        let err_bool = val(url(), &true).unwrap_err();
        assert_eq!(err_bool.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err_bool.expected.as_deref(), Some("url"));
    }

    // T4-6: json accepts any serde-bridgeable value, identity parse
    #[test]
    fn json_accepts_anything() {
        assert_eq!(val(json(), &7i32).unwrap(), ZerxValue::from_serialize(&7i32).unwrap());
        assert_eq!(val(json(), "hi").unwrap(), ZerxValue::from_serialize("hi").unwrap());
        assert_eq!(val(json(), &true).unwrap(), ZerxValue::from_serialize(&true).unwrap());
        assert_eq!(val(json(), &()).unwrap(), ZerxValue::from_serialize(&()).unwrap());
        assert_eq!(val(json(), &vec![1i64, 2, 3]).unwrap(), ZerxValue::from_serialize(&vec![1i64, 2, 3]).unwrap());
        // Bytes (via Blob) also accepted
        let b_ok = val(json(), &Blob(vec![9, 8])).unwrap();
        assert_eq!(b_ok, ZerxValue::Bytes(vec![9, 8]));
    }

    // T4-7: jsonschema accepts any serde-bridgeable value, identity parse
    #[test]
    fn jsonschema_accepts_anything() {
        use serde_json::json;
        let schema_obj = json!({"type": "string"});
        assert_eq!(
            val(jsonschema(), &schema_obj).unwrap(),
            ZerxValue::from_serialize(&schema_obj).unwrap()
        );
        // A bare boolean is a valid 2020-12 schema
        assert_eq!(
            val(jsonschema(), &true).unwrap(),
            ZerxValue::from_serialize(&true).unwrap()
        );
        // No structural JSON-Schema validation: any value passes (v1 accept-all)
        assert_eq!(
            val(jsonschema(), &42i64).unwrap(),
            ZerxValue::from_serialize(&42i64).unwrap()
        );
    }

    // T4-8: negative compile-guarantee is covered by compile_fail doctests on the builder types

    // T4-9: modifiers compose via blanket trait
    #[test]
    fn special_type_modifiers_compose() {
        use crate::schema::ParseContext;
        // optional buffer: missing field → None
        let s: Schema = buffer().optional().into();
        let mut ctx = ParseContext::new();
        assert_eq!(s.parse_field(None, &mut ctx).unwrap(), None);

        // describe on uri
        let u: Schema = uri().describe("a URI field").into();
        assert_eq!(u.modifiers.description.as_deref(), Some("a URI field"));

        // default on json
        let default_val = ZerxValue::Object(crate::Map::new());
        let j: Schema = json().default(default_val.clone()).into();
        assert_eq!(j.modifiers.default, Some(default_val));
    }

    // T4-10: SchemaKind dispatch + Debug renders expected strings
    #[test]
    fn schema_kind_debug_t4() {
        let cases: &[(&str, Schema)] = &[
            ("Buffer", buffer().into()),
            ("Uri", uri().into()),
            ("Url", url().into()),
            ("Json", json().into()),
            ("JsonSchema", jsonschema().into()),
        ];
        for (expected, s) in cases {
            let debug = format!("{:?}", s);
            assert!(debug.contains(expected), "expected {expected} in: {debug}");
        }
    }

    // T4-11: composition — object mixing special types with complex types
    #[test]
    fn special_type_composition() {
        use serde_json::json;

        let schema = object([
            ("avatar", buffer().mime("image/png").optional().into()),
            ("home", url().into()),
            ("meta", record(json()).into()),
        ]);

        // Valid input: avatar absent, home valid URL, meta a record of json values
        let ok = val(
            schema.clone(),
            &json!({"home": "http://example.com", "meta": {"k": 1}}),
        )
        .unwrap();
        let m = ok.as_object().unwrap();
        assert!(m.get("avatar").is_none());
        assert!(m.get("home").is_some());

        // Malformed home URL → INVALID_URL with path = ["home"]
        let err = val(
            schema,
            &json!({"home": "ftp://bad", "meta": {}}),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_URL);
        assert_eq!(err.path, vec!["home"]);
    }

    // T4-12: clone immutability for buffer
    #[test]
    fn buffer_clone_immutability() {
        let base = buffer();
        let with_mime: Schema = base.clone().mime("image/png").into();
        let without: Schema = Schema::from(base);
        assert_eq!(with_mime.modifiers.mime.as_deref(), Some("image/png"));
        assert_eq!(without.modifiers.mime, None);
    }
}
