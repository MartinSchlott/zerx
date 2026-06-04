use std::borrow::Cow;

use serde::Serialize;

/// Stable machine-readable error code.
///
/// The catalogue is open: each plan declares its own codes as associated constants
/// in an `impl ErrorCode` block next to the code that raises them, e.g.:
/// ```rust,ignore
/// impl ErrorCode {
///     pub const UNKNOWN_PROPERTY: ErrorCode = ErrorCode::new("unknown_property");
/// }
/// ```
/// No edit to `src/error.rs` is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ErrorCode(Cow<'static, str>);

impl ErrorCode {
    pub const fn new(s: &'static str) -> Self {
        ErrorCode(Cow::Borrowed(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Seed codes owned by the `errors` concern.
impl ErrorCode {
    /// Generic catch-all when no more specific code applies.
    pub const UNKNOWN_ERROR: ErrorCode = ErrorCode::new("unknown_error");
    /// A value's type did not match the schema's expectation.
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode::new("type_mismatch");
    /// No union variant matched; produced by [`ZerxError::union`].
    pub const UNION_MISMATCH: ErrorCode = ErrorCode::new("union_mismatch");
}

impl From<&'static str> for ErrorCode {
    fn from(s: &'static str) -> Self {
        ErrorCode(Cow::Borrowed(s))
    }
}

impl From<String> for ErrorCode {
    fn from(s: String) -> Self {
        ErrorCode(Cow::Owned(s))
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------

/// Structured, serialisable validation error returned by every fallible zerx
/// operation.
///
/// `received` and `expected` are plain descriptor strings, not raw values, so
/// `ZerxError` is always serialisable regardless of the host value type (C8).
#[derive(Debug, Clone, Serialize)]
pub struct ZerxError {
    pub path: Vec<String>,
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inner_errors: Vec<ZerxError>,
}

impl ZerxError {
    /// Minimal constructor. `path` is empty; `received`/`expected` are `None`;
    /// `inner_errors` is empty.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ZerxError {
            path: Vec::new(),
            code,
            message: message.into(),
            received: None,
            expected: None,
            inner_errors: Vec::new(),
        }
    }

    /// Set the path segments.
    pub fn at(mut self, path: Vec<String>) -> Self {
        self.path = path;
        self
    }

    /// Set the `received` descriptor.
    pub fn received(mut self, received: impl Into<String>) -> Self {
        self.received = Some(received.into());
        self
    }

    /// Set the `expected` descriptor.
    pub fn expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// Attach nested / per-variant errors.
    pub fn with_inner(mut self, inner: Vec<ZerxError>) -> Self {
        self.inner_errors = inner;
        self
    }

    /// Serialise to a [`serde_json::Value`].  Infallible because every field
    /// is a plain serialisable type (C8 guarantee).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self)
            .expect("ZerxError serialization is infallible: all fields are plain serialisable types")
    }

    /// Aggregate multiple per-variant errors into a single `UNION_MISMATCH`
    /// error.  The mechanism (attach inner errors, produce combined message)
    /// lives here; the variant-selection strategy belongs to the union type
    /// (`PLAN_T2_complex_types`).
    pub fn union(path: Vec<String>, inner: Vec<ZerxError>) -> ZerxError {
        let n = inner.len();
        ZerxError {
            path,
            code: ErrorCode::UNION_MISMATCH,
            message: format!("no union variant matched ({n} alternatives)"),
            received: None,
            expected: Some("one of the union variants".to_string()),
            inner_errors: inner,
        }
    }
}

impl std::fmt::Display for ZerxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() {
            write!(f, "(root): {}", self.message)
        } else {
            write!(f, "{}: {}", self.path.join("/"), self.message)
        }
    }
}

impl std::error::Error for ZerxError {}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Simulate what a later plan adds in its own module: a new associated constant
    // in a separate `impl ErrorCode` block, with no edit to `src/error.rs`.
    impl ErrorCode {
        pub const CUSTOM: ErrorCode = ErrorCode::new("custom_code");
    }

    #[test]
    fn minimal_construction() {
        let e = ZerxError::new(ErrorCode::TYPE_MISMATCH, "type error");
        assert!(e.path.is_empty());
        assert_eq!(e.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(e.message, "type error");
        assert!(e.received.is_none());
        assert!(e.expected.is_none());
        assert!(e.inner_errors.is_empty());
    }

    #[test]
    fn builder_chaining() {
        let e = ZerxError::new(ErrorCode::TYPE_MISMATCH, "type error")
            .at(vec!["a".into(), "0".into()])
            .expected("string")
            .received("number");
        assert_eq!(e.path, vec!["a", "0"]);
        assert_eq!(e.received.as_deref(), Some("number"));
        assert_eq!(e.expected.as_deref(), Some("string"));
    }

    #[test]
    fn json_shape_full() {
        let inner = ZerxError::new(ErrorCode::UNKNOWN_ERROR, "inner");
        let e = ZerxError::new(ErrorCode::TYPE_MISMATCH, "outer")
            .at(vec!["x".into()])
            .received("number")
            .expected("string")
            .with_inner(vec![inner]);
        let v = e.to_json();
        let obj = v.as_object().unwrap();
        // Exactly the six approved keys — no more, no less.
        let keys: std::collections::BTreeSet<&str> =
            obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["code", "expected", "inner_errors", "message", "path", "received"]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(obj["code"], serde_json::json!("type_mismatch"));
        assert_eq!(obj["path"], serde_json::json!(["x"]));
    }

    #[test]
    fn json_shape_omission() {
        let e = ZerxError::new(ErrorCode::TYPE_MISMATCH, "plain");
        let v = e.to_json();
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("received"));
        assert!(!obj.contains_key("expected"));
        assert!(!obj.contains_key("inner_errors"));
    }

    #[test]
    fn nested_aggregation() {
        let e1 = ZerxError::new(ErrorCode::TYPE_MISMATCH, "variant 0");
        let e2 = ZerxError::new(ErrorCode::TYPE_MISMATCH, "variant 1");
        let u = ZerxError::union(vec!["root".into()], vec![e1, e2]);
        assert_eq!(u.code, ErrorCode::UNION_MISMATCH);
        assert_eq!(u.inner_errors.len(), 2);
        let v = u.to_json();
        let arr = v["inner_errors"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Both inner errors carry their complete serialised payloads.
        assert_eq!(arr[0]["code"], serde_json::json!("type_mismatch"));
        assert_eq!(arr[0]["message"], serde_json::json!("variant 0"));
        assert_eq!(arr[1]["code"], serde_json::json!("type_mismatch"));
        assert_eq!(arr[1]["message"], serde_json::json!("variant 1"));
    }

    #[test]
    fn display_formatting() {
        let e_root = ZerxError::new(ErrorCode::TYPE_MISMATCH, "bad type");
        assert_eq!(e_root.to_string(), "(root): bad type");

        let e_path = ZerxError::new(ErrorCode::TYPE_MISMATCH, "bad type")
            .at(vec!["a".into(), "b".into(), "0".into()]);
        assert_eq!(e_path.to_string(), "a/b/0: bad type");
    }

    #[test]
    fn error_trait_propagation() {
        fn returns_boxed_error() -> Result<(), Box<dyn std::error::Error>> {
            let e = ZerxError::new(ErrorCode::TYPE_MISMATCH, "oops");
            Err(e)?
        }
        assert!(returns_boxed_error().is_err());
    }

    #[test]
    fn error_code_equality_and_extension() {
        assert_eq!(ErrorCode::CUSTOM.as_str(), "custom_code");
        assert_eq!(ErrorCode::TYPE_MISMATCH, ErrorCode::new("type_mismatch"));
        assert_ne!(ErrorCode::TYPE_MISMATCH, ErrorCode::UNKNOWN_ERROR);
    }
}
