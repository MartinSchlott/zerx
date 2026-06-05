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
        #[cfg(feature = "mlua")]
        ZerxValue::HostOpaque(_) => "host_opaque",
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
}
