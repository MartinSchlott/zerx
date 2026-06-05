// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::rc::Rc;

use crate::{ErrorCode, Map, ZerxError, ZerxValue};

// ---------------------------------------------------------------------------
// Open error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const PARSE_DEPTH_EXCEEDED: ErrorCode = ErrorCode::new("parse_depth_exceeded");
    pub const REQUIRED: ErrorCode = ErrorCode::new("required");
    pub const REFINEMENT_FAILED: ErrorCode = ErrorCode::new("refinement_failed");
    pub const LAZY_REENTRANCE: ErrorCode = ErrorCode::new("lazy_reentrance");
}

// ---------------------------------------------------------------------------
// Parse depth constant
// ---------------------------------------------------------------------------

pub const MAX_PARSE_DEPTH: usize = 100;

// ---------------------------------------------------------------------------
// Validator — storage contract (all concretes owned by types, PLAN_T1)
// ---------------------------------------------------------------------------

pub trait Validator {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError>;
    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value>;
}

// ---------------------------------------------------------------------------
// Refinement
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct Refinement {
    pub(crate) check: Rc<dyn Fn(&ZerxValue) -> bool>,
    pub(crate) message: String,
}

// ---------------------------------------------------------------------------
// Modifiers — universal carrier
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub(crate) struct Modifiers {
    pub(crate) optional: bool,
    pub(crate) nullable: bool,
    pub(crate) default: Option<ZerxValue>,
    pub(crate) description: Option<String>,
    pub(crate) refine: Vec<Refinement>,
    pub(crate) format: Option<String>,
    pub(crate) mime: Option<String>,
    pub(crate) deprecated: bool,
    pub(crate) read_only: bool,
    pub(crate) write_only: bool,
    pub(crate) meta: Map,
    pub(crate) example: Option<ZerxValue>,
    pub(crate) title: Option<String>,
}

impl std::fmt::Debug for Modifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Modifiers")
            .field("optional", &self.optional)
            .field("nullable", &self.nullable)
            .field("default", &self.default)
            .field("description", &self.description)
            .field("refine_count", &self.refine.len())
            .field("format", &self.format)
            .field("mime", &self.mime)
            .field("deprecated", &self.deprecated)
            .field("read_only", &self.read_only)
            .field("write_only", &self.write_only)
            .field("meta", &self.meta)
            .field("example", &self.example)
            .field("title", &self.title)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Lazy body + state
// ---------------------------------------------------------------------------

enum LazyState {
    Unresolved,
    Resolving,
    Resolved(Rc<Schema>),
}

#[derive(Clone)]
pub(crate) struct Lazy {
    thunk: Rc<dyn Fn() -> Schema>,
    state: Rc<RefCell<LazyState>>,
}

impl Lazy {
    fn new<F: Fn() -> Schema + 'static>(f: F) -> Self {
        Lazy {
            thunk: Rc::new(f),
            state: Rc::new(RefCell::new(LazyState::Unresolved)),
        }
    }

    /// Stable per-instance identity for export `$defs` dedup. All clones of the
    /// same `lazy` share one resolution cell, hence one identity.
    pub(crate) fn export_id(&self) -> usize {
        Rc::as_ptr(&self.state) as *const () as usize
    }

    pub(crate) fn resolve(&self) -> Result<Rc<Schema>, ZerxError> {
        {
            let state = self.state.borrow();
            match &*state {
                LazyState::Resolved(s) => return Ok(s.clone()),
                LazyState::Resolving => {
                    return Err(ZerxError::new(
                        ErrorCode::LAZY_REENTRANCE,
                        "lazy schema re-entered its own resolution",
                    ))
                }
                LazyState::Unresolved => {}
            }
        }
        *self.state.borrow_mut() = LazyState::Resolving;
        let schema = (self.thunk)();
        let rc = Rc::new(schema);
        *self.state.borrow_mut() = LazyState::Resolved(rc.clone());
        Ok(rc)
    }
}

// ---------------------------------------------------------------------------
// SchemaKind — the closed enum container (C1)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) enum SchemaKind {
    Any,
    Lazy(Lazy),
    String,
    Number,
    Boolean,
    Enum(Vec<std::string::String>),
    Null,
    Object(crate::types::ObjectBody),
    Array(Box<Schema>),
    Record(Box<Schema>),
    Tuple(Vec<Schema>),
    Union(Vec<Schema>),
    DiscriminatedUnion(crate::types::DiscriminatedUnionBody),
    Literal(ZerxValue),
    Buffer,     // T4
    Uri,        // T4
    Url,        // T4
    Json,       // T4
    JsonSchema, // T4
}

impl SchemaKind {
    /// Leaf type check — intercepted for Lazy before this is called.
    pub(crate) fn check_type(
        &self,
        value: &ZerxValue,
        _ctx: &mut ParseContext,
    ) -> Result<(), ZerxError> {
        match self {
            SchemaKind::Any => Ok(()),
            SchemaKind::Lazy(_) => unreachable!("Lazy is intercepted before check_type"),
            SchemaKind::String => crate::types::check_string(value),
            SchemaKind::Number => crate::types::check_number(value),
            SchemaKind::Boolean => crate::types::check_boolean(value),
            SchemaKind::Enum(set) => crate::types::check_enum(value, set),
            SchemaKind::Null => crate::types::check_null(value),
            SchemaKind::Object(_) => crate::types::check_object(value),
            SchemaKind::Array(_) => crate::types::check_array(value),
            SchemaKind::Record(_) => crate::types::check_record(value),
            SchemaKind::Tuple(_) => crate::types::check_tuple(value),
            SchemaKind::Union(_) => crate::types::check_union(),
            SchemaKind::DiscriminatedUnion(body) => crate::types::check_discriminated_union(value, body),
            SchemaKind::Literal(c) => crate::types::check_literal(value, c),
            SchemaKind::Buffer => crate::types::check_buffer(value),
            SchemaKind::Uri => crate::types::check_uri(value),
            SchemaKind::Url => crate::types::check_url(value),
            SchemaKind::Json => crate::types::check_json(value),
            SchemaKind::JsonSchema => crate::types::check_jsonschema(value),
        }
    }

    /// Type-specific structural step — intercepted for Lazy before this is called.
    pub(crate) fn parse_inner(
        &self,
        value: &ZerxValue,
        ctx: &mut ParseContext,
    ) -> Result<ZerxValue, ZerxError> {
        match self {
            SchemaKind::Any
            | SchemaKind::String
            | SchemaKind::Number
            | SchemaKind::Boolean
            | SchemaKind::Enum(_)
            | SchemaKind::Null
            | SchemaKind::Buffer
            | SchemaKind::Uri
            | SchemaKind::Url
            | SchemaKind::Json
            | SchemaKind::JsonSchema => Ok(value.clone()),
            SchemaKind::Lazy(_) => unreachable!("Lazy is intercepted before parse_inner"),
            SchemaKind::Object(body) => crate::types::parse_object(body, value, ctx),
            SchemaKind::Array(item) => crate::types::parse_array(item, value, ctx),
            SchemaKind::Record(vs) => crate::types::parse_record(vs, value, ctx),
            SchemaKind::Tuple(items) => crate::types::parse_tuple(items, value, ctx),
            SchemaKind::Union(variants) => crate::types::parse_union(variants, value, ctx),
            SchemaKind::DiscriminatedUnion(body) => crate::types::parse_discriminated_union(body, value, ctx),
            SchemaKind::Literal(_) => Ok(value.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// ParseContext — depth counter + future seam for path and data-cycle guard
// ---------------------------------------------------------------------------

pub(crate) struct ParseContext {
    depth: usize,
}

impl ParseContext {
    pub(crate) fn new() -> Self {
        ParseContext { depth: 0 }
    }

    pub(crate) fn enter(&mut self) -> Result<(), ZerxError> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            self.depth -= 1; // restore so ctx stays balanced; exit() is not called on this path
            Err(ZerxError::new(
                ErrorCode::PARSE_DEPTH_EXCEEDED,
                format!("parse depth exceeded maximum of {MAX_PARSE_DEPTH}"),
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn exit(&mut self) {
        self.depth -= 1;
    }
}

// ---------------------------------------------------------------------------
// Schema — the erased, cloneable value (C1)
// ---------------------------------------------------------------------------

pub struct Schema {
    pub(crate) kind: SchemaKind,
    pub(crate) modifiers: Modifiers,
    pub(crate) validators: Vec<Rc<dyn Validator>>,
}

impl Clone for Schema {
    fn clone(&self) -> Self {
        Schema {
            kind: self.kind.clone(),
            modifiers: self.modifiers.clone(),
            validators: self.validators.clone(),
        }
    }
}

impl std::fmt::Debug for Schema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind_str = match &self.kind {
            SchemaKind::Any => "Any",
            SchemaKind::Lazy(_) => "Lazy(<thunk>)",
            SchemaKind::String => "String",
            SchemaKind::Number => "Number",
            SchemaKind::Boolean => "Boolean",
            SchemaKind::Enum(_) => "Enum",
            SchemaKind::Null => "Null",
            SchemaKind::Object(_) => "Object",
            SchemaKind::Array(_) => "Array",
            SchemaKind::Record(_) => "Record",
            SchemaKind::Tuple(_) => "Tuple",
            SchemaKind::Union(_) => "Union",
            SchemaKind::DiscriminatedUnion(_) => "DiscriminatedUnion",
            SchemaKind::Literal(_) => "Literal",
            SchemaKind::Buffer => "Buffer",
            SchemaKind::Uri => "Uri",
            SchemaKind::Url => "Url",
            SchemaKind::Json => "Json",
            SchemaKind::JsonSchema => "JsonSchema",
        };
        f.debug_struct("Schema")
            .field("kind", &kind_str)
            .field("modifiers", &self.modifiers)
            .field("validator_count", &self.validators.len())
            .finish()
    }
}

impl Schema {
    pub(crate) fn new(kind: SchemaKind) -> Self {
        Schema {
            kind,
            modifiers: Modifiers::default(),
            validators: Vec::new(),
        }
    }

    /// Public entry: serialize value and run the parse flow.
    pub fn validate<T: serde::Serialize + ?Sized>(&self, value: &T) -> Result<ZerxValue, ZerxError> {
        let v = ZerxValue::from_serialize(value)?;
        self.parse_present(&v, &mut ParseContext::new())
    }

    /// Parse a present value. Called recursively by container types (PLAN_T2).
    pub(crate) fn parse_present(
        &self,
        value: &ZerxValue,
        ctx: &mut ParseContext,
    ) -> Result<ZerxValue, ZerxError> {
        ctx.enter()?;
        let result = self.parse_present_body(value, ctx);
        ctx.exit();
        result
    }

    fn parse_present_body(
        &self,
        value: &ZerxValue,
        ctx: &mut ParseContext,
    ) -> Result<ZerxValue, ZerxError> {
        // Lazy: resolve and delegate; outer lazy's refine still runs on the result
        if let SchemaKind::Lazy(lzy) = &self.kind {
            let inner = lzy.resolve()?;
            let output = inner.parse_present(value, ctx)?;
            return self.run_refine(output);
        }

        // nullable: explicit null accepted without type check or validators
        if value.is_null() && self.modifiers.nullable {
            return Ok(ZerxValue::Null);
        }

        // type check → validators → type-specific logic → refine
        self.kind.check_type(value, ctx)?;
        for v in &self.validators {
            v.validate(value)?;
        }
        let output = self.kind.parse_inner(value, ctx)?;
        self.run_refine(output)
    }

    fn run_refine(&self, value: ZerxValue) -> Result<ZerxValue, ZerxError> {
        for r in &self.modifiers.refine {
            if !(r.check)(&value) {
                return Err(ZerxError::new(ErrorCode::REFINEMENT_FAILED, r.message.clone()));
            }
        }
        Ok(value)
    }

    /// Missing/present field wrapper for container property iteration (PLAN_T2).
    /// Returns `None` = omit from output.
    #[allow(dead_code)]
    pub(crate) fn parse_field(
        &self,
        value: Option<&ZerxValue>,
        ctx: &mut ParseContext,
    ) -> Result<Option<ZerxValue>, ZerxError> {
        match value {
            None => {
                if let Some(def) = &self.modifiers.default {
                    // Clone to avoid holding an immutable borrow through parse_present
                    let def = def.clone();
                    Ok(Some(self.parse_present(&def, ctx)?))
                } else if self.modifiers.optional {
                    Ok(None)
                } else {
                    Err(ZerxError::new(ErrorCode::REQUIRED, "required field is missing"))
                }
            }
            // Explicit null does NOT trigger default; handled by nullable in parse_present
            Some(v) => Ok(Some(self.parse_present(v, ctx)?)),
        }
    }
}

// ---------------------------------------------------------------------------
// BuilderInner — private driver for the blanket Modify impl
// ---------------------------------------------------------------------------

pub(crate) trait BuilderInner {
    fn schema_mut(&mut self) -> &mut Schema;
}

impl BuilderInner for Schema {
    fn schema_mut(&mut self) -> &mut Schema {
        self
    }
}

// ---------------------------------------------------------------------------
// Modify — the blanket universal-modifier trait (C1)
// ---------------------------------------------------------------------------

pub trait Modify: Sized {
    fn optional(self) -> Self;
    fn nullable(self) -> Self;
    fn default(self, value: impl Into<ZerxValue>) -> Self;
    fn describe(self, text: impl Into<String>) -> Self;
    fn refine<F: Fn(&ZerxValue) -> bool + 'static>(self, f: F, message: impl Into<String>) -> Self;
    fn format(self, fmt: impl Into<String>) -> Self;
    fn mime_format(self, mime: impl Into<String>) -> Self;
    fn deprecated(self) -> Self;
    fn read_only(self) -> Self;
    fn write_only(self) -> Self;
    fn meta(self, key: impl Into<String>, value: impl Into<ZerxValue>) -> Self;
    fn example(self, value: impl Into<ZerxValue>) -> Self;
    fn title(self, text: impl Into<String>) -> Self;
}

impl<T: BuilderInner> Modify for T {
    fn optional(mut self) -> Self {
        self.schema_mut().modifiers.optional = true;
        self
    }
    fn nullable(mut self) -> Self {
        self.schema_mut().modifiers.nullable = true;
        self
    }
    fn default(mut self, value: impl Into<ZerxValue>) -> Self {
        self.schema_mut().modifiers.default = Some(value.into());
        self
    }
    fn describe(mut self, text: impl Into<String>) -> Self {
        self.schema_mut().modifiers.description = Some(text.into());
        self
    }
    fn refine<F: Fn(&ZerxValue) -> bool + 'static>(
        mut self,
        f: F,
        message: impl Into<String>,
    ) -> Self {
        self.schema_mut().modifiers.refine.push(Refinement {
            check: Rc::new(f),
            message: message.into(),
        });
        self
    }
    fn format(mut self, fmt: impl Into<String>) -> Self {
        self.schema_mut().modifiers.format = Some(fmt.into());
        self
    }
    fn mime_format(mut self, mime: impl Into<String>) -> Self {
        self.schema_mut().modifiers.mime = Some(mime.into());
        self
    }
    fn deprecated(mut self) -> Self {
        self.schema_mut().modifiers.deprecated = true;
        self
    }
    fn read_only(mut self) -> Self {
        self.schema_mut().modifiers.read_only = true;
        self
    }
    fn write_only(mut self) -> Self {
        self.schema_mut().modifiers.write_only = true;
        self
    }
    fn meta(mut self, key: impl Into<String>, value: impl Into<ZerxValue>) -> Self {
        self.schema_mut().modifiers.meta.insert(key.into(), value.into());
        self
    }
    fn example(mut self, value: impl Into<ZerxValue>) -> Self {
        self.schema_mut().modifiers.example = Some(value.into());
        self
    }
    fn title(mut self, text: impl Into<String>) -> Self {
        self.schema_mut().modifiers.title = Some(text.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Typed builders + constructors (C1)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AnySchema(Schema);

#[derive(Clone)]
pub struct LazySchema(Schema);

impl BuilderInner for AnySchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl BuilderInner for LazySchema {
    fn schema_mut(&mut self) -> &mut Schema {
        &mut self.0
    }
}

impl From<AnySchema> for Schema {
    fn from(b: AnySchema) -> Schema {
        b.0
    }
}

impl From<LazySchema> for Schema {
    fn from(b: LazySchema) -> Schema {
        b.0
    }
}

pub fn any() -> AnySchema {
    AnySchema(Schema::new(SchemaKind::Any))
}

pub fn lazy<F: Fn() -> Schema + 'static>(f: F) -> LazySchema {
    LazySchema(Schema::new(SchemaKind::Lazy(Lazy::new(f))))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // Test-only validator that always rejects with a chosen error code
    struct RejectAll {
        code: ErrorCode,
    }

    impl Validator for RejectAll {
        fn validate(&self, _value: &ZerxValue) -> Result<(), ZerxError> {
            Err(ZerxError::new(self.code.clone(), "rejected by RejectAll"))
        }
        fn json_schema(&self) -> serde_json::Map<String, serde_json::Value> {
            serde_json::Map::new()
        }
    }

    // 1. any accepts every scalar/container
    #[test]
    fn any_accepts_all() {
        let s: Schema = any().into();
        assert!(s.validate(&true).is_ok());
        assert!(s.validate(&7i32).is_ok());
        assert!(s.validate(&2.5f64).is_ok());
        assert!(s.validate("hi").is_ok());
        assert!(s.validate(&()).is_ok());
        assert!(s.validate(&vec![1i64, 2, 3]).is_ok());

        #[derive(serde::Serialize)]
        struct Small {
            x: i32,
        }
        assert!(s.validate(&Small { x: 1 }).is_ok());

        // validate = from_serialize + identity parse_inner
        let result = s.validate(&7i32).unwrap();
        assert_eq!(result, ZerxValue::from_serialize(&7i32).unwrap());
    }

    // 2. Blanket Modify chains return the builder type
    #[test]
    fn modify_chain_preserves_type() {
        let b: AnySchema = any()
            .optional()
            .nullable()
            .describe("d")
            .deprecated()
            .read_only()
            .meta("x", true)
            .title("t");

        let s: Schema = b.into();
        assert!(s.modifiers.optional);
        assert!(s.modifiers.nullable);
        assert!(s.modifiers.deprecated);
        assert!(s.modifiers.read_only);
        assert_eq!(s.modifiers.description.as_deref(), Some("d"));
        assert_eq!(s.modifiers.meta.get("x"), Some(&ZerxValue::Bool(true)));
        assert_eq!(s.modifiers.title.as_deref(), Some("t"));

        // Same chain on LazySchema type-checks as LazySchema
        let _lb: LazySchema = lazy(|| any().into()).optional().nullable().describe("d");
    }

    // 3. Into<Schema> and Clone immutability
    #[test]
    fn clone_immutability() {
        let base = any().describe("x");
        let opt: Schema = base.clone().optional().into();
        let orig: Schema = Schema::from(base);

        assert!(opt.modifiers.optional);
        assert_eq!(opt.modifiers.description.as_deref(), Some("x"));
        assert!(!orig.modifiers.optional);
        assert_eq!(orig.modifiers.description.as_deref(), Some("x"));
    }

    // 4. Default precedes optional on a missing value
    #[test]
    fn default_precedes_optional() {
        let mut ctx = ParseContext::new();

        let s_default: Schema = any().default(7i64).into();
        assert_eq!(
            s_default.parse_field(None, &mut ctx).unwrap(),
            Some(ZerxValue::I64(7))
        );

        let s_optional: Schema = any().optional().into();
        assert_eq!(s_optional.parse_field(None, &mut ctx).unwrap(), None);

        let s_required: Schema = any().into();
        let err = s_required.parse_field(None, &mut ctx).unwrap_err();
        assert_eq!(err.code, ErrorCode::REQUIRED);
    }

    // 5. Nullable and explicit null
    #[test]
    fn nullable_and_explicit_null() {
        let mut ctx = ParseContext::new();

        let s: Schema = any().nullable().into();
        assert_eq!(
            s.parse_present(&ZerxValue::Null, &mut ctx).unwrap(),
            ZerxValue::Null
        );

        // Explicit null does NOT trigger default
        let s2: Schema = any().default(7i64).nullable().into();
        assert_eq!(
            s2.parse_field(Some(&ZerxValue::Null), &mut ctx).unwrap(),
            Some(ZerxValue::Null)
        );
    }

    // 6. Validators stored, executed in order, and run before refine
    #[test]
    fn validators_order_and_before_refine() {
        // Single validator rejects
        let mut s = Schema::new(SchemaKind::Any);
        s.validators.push(Rc::new(RejectAll { code: ErrorCode::new("v1") }));
        assert_eq!(s.validate(&42i32).unwrap_err().code, ErrorCode::new("v1"));

        // Two validators: first failure wins
        let mut s2 = Schema::new(SchemaKind::Any);
        s2.validators.push(Rc::new(RejectAll { code: ErrorCode::new("first") }));
        s2.validators.push(Rc::new(RejectAll { code: ErrorCode::new("second") }));
        assert_eq!(s2.validate(&42i32).unwrap_err().code, ErrorCode::new("first"));

        // Validator short-circuits before refine
        let flag = Rc::new(Cell::new(false));
        let flag_clone = flag.clone();
        let mut s3 = Schema::new(SchemaKind::Any);
        s3.validators.push(Rc::new(RejectAll { code: ErrorCode::new("v3") }));
        s3.modifiers.refine.push(Refinement {
            check: Rc::new(move |_| {
                flag_clone.set(true);
                true
            }),
            message: "should not run".into(),
        });
        assert_eq!(s3.validate(&42i32).unwrap_err().code, ErrorCode::new("v3"));
        assert!(!flag.get());
    }

    // 7. Refine runs and aggregates
    #[test]
    fn refine_aggregates() {
        let s: Schema = any().refine(|v| v.as_i64() == Some(5), "must be 5").into();
        assert!(s.validate(&5i64).is_ok());

        let err = s.validate(&6i64).unwrap_err();
        assert_eq!(err.code, ErrorCode::REFINEMENT_FAILED);
        assert_eq!(err.message, "must be 5");

        // First failing predicate's message wins
        let s2: Schema = any()
            .refine(|v| v.as_i64() == Some(5), "must be 5")
            .refine(|_| false, "second refine")
            .into();
        let err2 = s2.validate(&6i64).unwrap_err();
        assert_eq!(err2.message, "must be 5");
    }

    // 8. Depth limit counted in recursion frames
    #[test]
    fn depth_limit_frames() {
        // 100 lazy wrappers around any = 101 total frames → exceeds MAX_PARSE_DEPTH (100)
        let mut s: Schema = any().into();
        for _ in 0..MAX_PARSE_DEPTH {
            let prev = s.clone();
            s = lazy(move || prev.clone()).into();
        }
        assert_eq!(
            s.validate(&42i32).unwrap_err().code,
            ErrorCode::PARSE_DEPTH_EXCEEDED
        );

        // 99 lazy wrappers around any = 100 total frames → exactly at limit, succeeds
        let mut s2: Schema = any().into();
        for _ in 0..(MAX_PARSE_DEPTH - 1) {
            let prev = s2.clone();
            s2 = lazy(move || prev.clone()).into();
        }
        assert!(s2.validate(&42i32).is_ok());
    }

    // 8b. Depth limit does not poison a reused context
    #[test]
    fn depth_limit_does_not_poison_context() {
        let mut ctx = ParseContext::new();

        let mut deep: Schema = any().into();
        for _ in 0..MAX_PARSE_DEPTH {
            let prev = deep.clone();
            deep = lazy(move || prev.clone()).into();
        }

        // Fails: 101 frames exceeds MAX_PARSE_DEPTH
        assert_eq!(
            deep.parse_present(&ZerxValue::Bool(true), &mut ctx)
                .unwrap_err()
                .code,
            ErrorCode::PARSE_DEPTH_EXCEEDED
        );

        // The same context is not poisoned: a shallow parse on the same ctx succeeds
        let shallow: Schema = any().into();
        assert!(shallow
            .parse_present(&ZerxValue::Bool(true), &mut ctx)
            .is_ok());
    }

    // 9. lazy memoisation
    #[test]
    fn lazy_memoisation() {
        let counter = Rc::new(Cell::new(0usize));
        let c = counter.clone();
        let s: Schema = lazy(move || {
            c.set(c.get() + 1);
            any().into()
        })
        .into();

        assert!(s.validate(&1i32).is_ok());
        assert!(s.validate(&2i32).is_ok());
        assert_eq!(counter.get(), 1); // thunk ran once

        // Across a clone — state shared via Rc
        let s2 = s.clone();
        assert!(s2.validate(&3i32).is_ok());
        assert_eq!(counter.get(), 1); // still cached
    }

    // 10. lazy reentrance guard (white-box)
    #[test]
    fn lazy_reentrance_guard() {
        let s = lazy(|| any().into());
        if let SchemaKind::Lazy(ref lzy) = s.0.kind {
            *lzy.state.borrow_mut() = LazyState::Resolving;
            let err = lzy.resolve().unwrap_err();
            assert_eq!(err.code, ErrorCode::LAZY_REENTRANCE);
        } else {
            panic!("expected Lazy kind");
        }
    }
}
