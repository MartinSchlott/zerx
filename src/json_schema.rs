// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ecma_regex::{self, EcmaError};
use crate::schema::{Schema, SchemaKind, MAX_PARSE_DEPTH};
use crate::types::{DiscriminatorState, ObjectBody, ObjectMode, StringSchema};
use crate::{ErrorCode, Modify, ZerxError, ZerxValue};

// ---------------------------------------------------------------------------
// Public surface
// ---------------------------------------------------------------------------

/// Draft 2020-12 dialect URI, for `ExportOptions::dialect`.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Options for JSON Schema export. `Default` emits no `$schema`.
#[derive(Default, Clone)]
pub struct ExportOptions {
    /// When set, the root schema gains `"$schema": <dialect>`.
    pub dialect: Option<String>,
}

impl Schema {
    /// Export this schema to JSON Schema (Draft 2020-12). Infallible.
    pub fn to_json_schema(&self) -> serde_json::Value {
        self.to_json_schema_with(&ExportOptions::default())
    }

    /// Export with options. Attaches `$schema` when `opts.dialect` is set.
    pub fn to_json_schema_with(&self, opts: &ExportOptions) -> serde_json::Value {
        let mut ctx = ExportContext::new();
        let mut root = export_node(self, &mut ctx);

        if let serde_json::Value::Object(ref mut map) = root {
            if !ctx.defs.is_empty() {
                map.insert("$defs".to_owned(), serde_json::Value::Object(ctx.defs));
            }
            if let Some(ref dialect) = opts.dialect {
                map.insert(
                    "$schema".to_owned(),
                    serde_json::Value::String(dialect.clone()),
                );
            }
        }
        root
    }
}

// ---------------------------------------------------------------------------
// Export-reserved keyword set — meta entries colliding with these are skipped
// ---------------------------------------------------------------------------

const RESERVED: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "prefixItems",
    "minItems",
    "maxItems",
    "anyOf",
    "oneOf",
    "discriminator",
    "const",
    "enum",
    "$ref",
    "$defs",
    "$schema",
    "format",
    "contentMediaType",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "title",
    // Every remaining assertion keyword is export-reserved: `meta` must never
    // carry a keyword whose JSON Schema meaning zerx would not enforce.
    "allOf",
    "not",
    "if",
    "then",
    "else",
    "$dynamicRef",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "contains",
    "minContains",
    "maxContains",
    "uniqueItems",
    "unevaluatedItems",
    "patternProperties",
    "propertyNames",
    "dependentRequired",
    "dependentSchemas",
    "minProperties",
    "maxProperties",
    "unevaluatedProperties",
];

fn is_reserved(key: &str) -> bool {
    RESERVED.contains(&key)
}

// ---------------------------------------------------------------------------
// ExportContext — $defs tracker
// ---------------------------------------------------------------------------

struct ExportContext {
    defs: serde_json::Map<String, serde_json::Value>,
    ids: HashMap<usize, String>,
    seq: usize,
}

impl ExportContext {
    fn new() -> Self {
        ExportContext {
            defs: serde_json::Map::new(),
            ids: HashMap::new(),
            seq: 0,
        }
    }

    /// Returns (stable_id, is_newly_assigned).
    fn id_for(&mut self, identity: usize) -> (String, bool) {
        if let Some(id) = self.ids.get(&identity) {
            return (id.clone(), false);
        }
        self.seq += 1;
        let id = format!("S{}", self.seq);
        self.ids.insert(identity, id.clone());
        (id, true)
    }
}

// ---------------------------------------------------------------------------
// ZerxValue → serde_json::Value helper
// ---------------------------------------------------------------------------

fn zerx_to_json(v: &crate::ZerxValue) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// is_plain_any — Record arm helper
// ---------------------------------------------------------------------------

fn is_plain_any(s: &Schema) -> bool {
    !s.modifiers.defective
        && matches!(s.kind, SchemaKind::Any)
        && s.validators.is_empty()
        && !s.modifiers.nullable
        && s.modifiers.description.is_none()
        && s.modifiers.default.is_none()
        && s.modifiers.example.is_none()
        && s.modifiers.format.is_none()
        && s.modifiers.mime.is_none()
        && !s.modifiers.deprecated
        && !s.modifiers.read_only
        && !s.modifiers.write_only
        && s.modifiers.title.is_none()
        && s.modifiers.meta.is_empty()
}

// ---------------------------------------------------------------------------
// Export walk
// ---------------------------------------------------------------------------

// The export of a node that must accept nothing JSON Schema could describe:
// `false`, or only `null` under `.nullable()`, which the runtime accepts first.
fn never_export(schema: &Schema) -> serde_json::Value {
    if schema.modifiers.nullable {
        serde_json::json!({"type": "null"})
    } else {
        serde_json::Value::Bool(false)
    }
}

// Whether the runtime rejects every non-null value of this node in a way JSON
// Schema keywords cannot express.
fn accepts_nothing(schema: &Schema) -> bool {
    let kind_rejects_all = match &schema.kind {
        // A constant JSON cannot hold: `const` would carry a stand-in
        // (`null` for a non-finite float or a 128-bit integer, a number array
        // for bytes) that the literal itself rejects.
        SchemaKind::Literal(c) => !json_representable(c),
        // A structurally invalid discriminated union rejects every value.
        SchemaKind::DiscriminatedUnion(body) => {
            matches!(body.state, DiscriminatorState::Invalid(_))
        }
        _ => false,
    };
    schema.modifiers.defective
        || kind_rejects_all
        || schema.validators.iter().any(|v| v.accepts_nothing())
}

// Whether `v` converts to JSON without a stand-in value.
fn json_representable(v: &ZerxValue) -> bool {
    match v {
        ZerxValue::Null
        | ZerxValue::Bool(_)
        | ZerxValue::I64(_)
        | ZerxValue::U64(_)
        | ZerxValue::String(_) => true,
        ZerxValue::I128(n) => i64::try_from(*n).is_ok() || u64::try_from(*n).is_ok(),
        ZerxValue::U128(n) => u64::try_from(*n).is_ok(),
        ZerxValue::F64(f) => f.is_finite(),
        ZerxValue::Bytes(_) => false,
        ZerxValue::Array(items) => items.iter().all(json_representable),
        ZerxValue::Object(map) => map.iter().all(|(_, v)| json_representable(v)),
    }
}

// An integral float within the i64/u64 range as the equal JSON integer; any
// other value (a JSON integer already, fractional, or out of range) is `None`.
fn integral_float_as_integer(v: &serde_json::Value) -> Option<serde_json::Value> {
    let n = v.as_number()?;
    if n.is_i64() || n.is_u64() {
        return None;
    }
    let f = n.as_f64()?;
    if f.fract() != 0.0 {
        return None;
    }
    // -2^63 and 2^64 are exact floats; within [-2^63, 2^64) the casts are exact.
    if (-9_223_372_036_854_775_808.0..0.0).contains(&f) {
        Some(serde_json::Value::from(f as i64))
    } else if (0.0..18_446_744_073_709_551_616.0).contains(&f) {
        Some(serde_json::Value::from(f as u64))
    } else {
        None
    }
}

fn export_node(schema: &Schema, ctx: &mut ExportContext) -> serde_json::Value {
    // A defective node fails closed before the Lazy short-circuit and before
    // any `$defs` registration.
    if schema.modifiers.defective {
        return never_export(schema);
    }

    // Step 1 — Lazy short-circuit
    if let SchemaKind::Lazy(lzy) = &schema.kind {
        let (id, is_new) = ctx.id_for(lzy.export_id());
        if is_new {
            // Placeholder prevents infinite recursion on cyclic schemas
            ctx.defs.insert(
                id.clone(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
            // A lazy that cannot resolve rejects every value at runtime, so
            // its definition fails closed instead of keeping the accepting
            // placeholder.
            let exported = match lzy.resolve() {
                Ok(inner) => export_node(&inner, ctx),
                Err(_) => serde_json::Value::Bool(false),
            };
            ctx.defs.insert(id.clone(), exported);
        }
        let mut core = serde_json::Map::new();
        core.insert(
            "$ref".to_owned(),
            serde_json::Value::String(format!("#/$defs/{id}")),
        );
        // Annotations/nullable still applied below (step 4+)
        apply_annotations(schema, ctx, &mut core);
        return wrap_nullable(schema, core);
    }

    // A node that accepts nothing exports as `false` rather than as a wider
    // schema; `null` stays accepted under `.nullable()`, which precedes it.
    if accepts_nothing(schema) {
        return never_export(schema);
    }

    // Step 2 — Base map
    let mut core = base_schema(&schema.kind, ctx);

    // Step 3 — Validator fragments. The runtime enforces every validator, so a
    // repeated lower or upper bound keeps the tightest value, and two different
    // `pattern`/`format` fragments, which one keyword cannot carry, make the
    // node export as `false` (fail closed; `allOf` is not imported).
    for v in &schema.validators {
        for (k, val) in v.json_schema() {
            if matches!(k.as_str(), "pattern" | "format")
                && core.get(&k).is_some_and(|existing| existing != &val)
            {
                return never_export(schema);
            }
            // Exact comparison: neighbouring integers beyond 2^53 share one
            // f64, so an f64 comparison could keep the wider bound.
            use std::cmp::Ordering;
            let order = core.get(&k).and_then(|old| compare_json_numbers(old, &val));
            let keep_existing = match (k.as_str(), order) {
                ("minLength" | "minItems" | "minimum", Some(o)) => o != Ordering::Less,
                ("maxLength" | "maxItems" | "maximum", Some(o)) => o != Ordering::Greater,
                _ => false,
            };
            if !keep_existing {
                core.insert(k, val);
            }
        }
    }

    // On an integer node an integral float bound is an exact integer: export
    // it as a JSON integer, because the importer refuses float literals of
    // magnitude 2^53 or more on integer nodes (the parser may have rounded
    // them) and would otherwise refuse zerx's own export.
    if core.get("type").and_then(|t| t.as_str()) == Some("integer") {
        for key in ["minimum", "maximum"] {
            if let Some(int) = core.get(key).and_then(integral_float_as_integer) {
                core.insert(key.to_owned(), int);
            }
        }
    }

    // Steps 4+5 — Modifier annotations + meta
    apply_annotations(schema, ctx, &mut core);

    // Step 6 — Nullable wrap
    wrap_nullable(schema, core)
}

fn apply_annotations(
    schema: &Schema,
    _ctx: &mut ExportContext,
    core: &mut serde_json::Map<String, serde_json::Value>,
) {
    let m = &schema.modifiers;

    if let Some(ref d) = m.description {
        core.insert(
            "description".to_owned(),
            serde_json::Value::String(d.clone()),
        );
    }
    if let Some(ref def) = m.default {
        core.insert("default".to_owned(), zerx_to_json(def));
    }
    if let Some(ref ex) = m.example {
        core.insert(
            "examples".to_owned(),
            serde_json::Value::Array(vec![zerx_to_json(ex)]),
        );
    }
    if m.deprecated {
        core.insert("deprecated".to_owned(), serde_json::Value::Bool(true));
    }
    if m.read_only {
        core.insert("readOnly".to_owned(), serde_json::Value::Bool(true));
    }
    if m.write_only {
        core.insert("writeOnly".to_owned(), serde_json::Value::Bool(true));
    }
    if let Some(ref t) = m.title {
        core.insert("title".to_owned(), serde_json::Value::String(t.clone()));
    }
    if let Some(ref mime) = m.mime {
        core.insert(
            "contentMediaType".to_owned(),
            serde_json::Value::String(mime.clone()),
        );
    }
    // User format: only written if no semantic format already present
    if let Some(ref fmt) = m.format {
        if !core.contains_key("format") {
            core.insert("format".to_owned(), serde_json::Value::String(fmt.clone()));
        }
    }

    // Step 5 — meta: skip reserved keys and already-set keys
    for (k, v) in m.meta.iter() {
        if !is_reserved(k) && !core.contains_key(k) {
            core.insert(k.clone(), zerx_to_json(v));
        }
    }
}

fn wrap_nullable(
    schema: &Schema,
    core: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    if schema.modifiers.nullable {
        serde_json::Value::Object({
            let mut m = serde_json::Map::new();
            m.insert(
                "anyOf".to_owned(),
                serde_json::Value::Array(vec![
                    serde_json::Value::Object(core),
                    serde_json::json!({"type": "null"}),
                ]),
            );
            m
        })
    } else {
        serde_json::Value::Object(core)
    }
}

// ---------------------------------------------------------------------------
// base_schema — central exhaustive match over SchemaKind
// ---------------------------------------------------------------------------

fn base_schema(
    kind: &SchemaKind,
    ctx: &mut ExportContext,
) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    match kind {
        SchemaKind::Any => {}

        SchemaKind::String => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("string".to_owned()),
            );
        }

        SchemaKind::Number => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("number".to_owned()),
            );
        }

        SchemaKind::Boolean => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("boolean".to_owned()),
            );
        }

        SchemaKind::Null => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("null".to_owned()),
            );
        }

        SchemaKind::Enum(set) => {
            m.insert(
                "enum".to_owned(),
                serde_json::Value::Array(
                    set.iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

        SchemaKind::Literal(c) => {
            m.insert("const".to_owned(), zerx_to_json(c));
        }

        SchemaKind::Object(body) => {
            object_schema(body, ctx, &mut m);
        }

        SchemaKind::Array(item) => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("array".to_owned()),
            );
            m.insert("items".to_owned(), export_node(item, ctx));
        }

        SchemaKind::Record(value) => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("object".to_owned()),
            );
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("record".to_owned()),
            );
            m.insert(
                "properties".to_owned(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
            let additional = if is_plain_any(value) {
                serde_json::Value::Bool(true)
            } else {
                export_node(value, ctx)
            };
            m.insert("additionalProperties".to_owned(), additional);
        }

        SchemaKind::Tuple(items) => {
            let n = items.len();
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("array".to_owned()),
            );
            m.insert(
                "prefixItems".to_owned(),
                serde_json::Value::Array(items.iter().map(|s| export_node(s, ctx)).collect()),
            );
            m.insert("items".to_owned(), serde_json::Value::Bool(false));
            m.insert("minItems".to_owned(), serde_json::Value::Number(n.into()));
            m.insert("maxItems".to_owned(), serde_json::Value::Number(n.into()));
        }

        SchemaKind::Union(variants) => {
            m.insert(
                "anyOf".to_owned(),
                serde_json::Value::Array(variants.iter().map(|s| export_node(s, ctx)).collect()),
            );
        }

        SchemaKind::OneOf(variants) => {
            m.insert(
                "oneOf".to_owned(),
                serde_json::Value::Array(variants.iter().map(|s| export_node(s, ctx)).collect()),
            );
        }

        SchemaKind::DiscriminatedUnion(body) => {
            m.insert(
                "oneOf".to_owned(),
                serde_json::Value::Array(
                    body.variants.iter().map(|s| export_node(s, ctx)).collect(),
                ),
            );
            let mut disc = serde_json::Map::new();
            disc.insert(
                "propertyName".to_owned(),
                serde_json::Value::String(body.key.clone()),
            );
            m.insert("discriminator".to_owned(), serde_json::Value::Object(disc));
        }

        SchemaKind::Buffer => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("string".to_owned()),
            );
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("buffer".to_owned()),
            );
        }

        SchemaKind::Uri => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("string".to_owned()),
            );
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("uri".to_owned()),
            );
        }

        SchemaKind::Url => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("string".to_owned()),
            );
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("url".to_owned()),
            );
        }

        SchemaKind::Json => {
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("json".to_owned()),
            );
        }

        SchemaKind::JsonSchema => {
            m.insert(
                "type".to_owned(),
                serde_json::Value::String("object".to_owned()),
            );
            m.insert(
                "format".to_owned(),
                serde_json::Value::String("jsonschema".to_owned()),
            );
        }

        SchemaKind::Lazy(_) => unreachable!("Lazy is intercepted in export_node"),
    }
    m
}

fn object_schema(
    body: &ObjectBody,
    ctx: &mut ExportContext,
    m: &mut serde_json::Map<String, serde_json::Value>,
) {
    m.insert(
        "type".to_owned(),
        serde_json::Value::String("object".to_owned()),
    );

    let mut properties = serde_json::Map::new();
    let mut required: Vec<serde_json::Value> = Vec::new();

    for (key, field) in &body.shape {
        properties.insert(key.clone(), export_node(field, ctx));
        if !body.all_optional && !field.modifiers.optional && field.modifiers.default.is_none() {
            required.push(serde_json::Value::String(key.clone()));
        }
    }

    m.insert(
        "properties".to_owned(),
        serde_json::Value::Object(properties),
    );

    if !required.is_empty() {
        m.insert("required".to_owned(), serde_json::Value::Array(required));
    }

    let additional = match body.mode {
        ObjectMode::Passthrough => serde_json::Value::Bool(true),
        ObjectMode::Strict | ObjectMode::Strip => serde_json::Value::Bool(false),
    };
    m.insert("additionalProperties".to_owned(), additional);
}

// ---------------------------------------------------------------------------
// Import error codes
// ---------------------------------------------------------------------------

impl ErrorCode {
    /// A JSON Schema feature zerx cannot represent (allOf, not, false-schema,
    /// unknown structural keywords). Well-formed JSON Schema but out of zerx's model.
    pub const IMPORT_UNSUPPORTED: ErrorCode = ErrorCode::new("import_unsupported");
    /// A structurally invalid input (non-object/bool, array missing items,
    /// record with additionalProperties:false, $ref to missing/non-local target).
    pub const IMPORT_MALFORMED: ErrorCode = ErrorCode::new("import_malformed");
}

// ---------------------------------------------------------------------------
// Import document depth bound
// ---------------------------------------------------------------------------

/// Hard bound on the JSON nesting of any raw document an import entry accepts,
/// measured before it is cloned, compared or walked.
///
/// `serde_json`'s text parser stops at 128 levels, but a programmatically built
/// `serde_json::Value` is unbounded, and the import path clones and walks the
/// caller's document long before the import walk's own `MAX_PARSE_DEPTH` guard
/// runs. Four times the walk limit leaves every used part of a valid document
/// under the bound: a schema position spends at most two JSON levels per walk
/// level (`properties`/`$defs`/`anyOf`/`prefixItems` → container → schema), and
/// a `const`/`default`/`enum`/annotation member is capped at `MAX_PARSE_DEPTH`
/// containers by the value bound. Only an *unused* deep part of a document 3.2.1
/// could import can therefore cross it.
pub(crate) const MAX_IMPORT_DOCUMENT_DEPTH: usize = 4 * MAX_PARSE_DEPTH;

/// Whether any container in `value` sits deeper than `limit` JSON levels
/// (arrays and objects count one level, scalars none). Iterative, so the guard
/// is never the recursion that exhausts the stack, and it returns as soon as
/// the limit is crossed instead of walking a whole huge document.
pub(crate) fn json_depth_exceeds(value: &serde_json::Value, limit: usize) -> bool {
    let mut stack: Vec<(&serde_json::Value, usize)> = vec![(value, 0)];
    while let Some((node, depth)) = stack.pop() {
        match node {
            serde_json::Value::Array(items) => {
                let child = depth + 1;
                if child > limit {
                    return true;
                }
                for item in items {
                    stack.push((item, child));
                }
            }
            serde_json::Value::Object(map) => {
                let child = depth + 1;
                if child > limit {
                    return true;
                }
                for entry in map.values() {
                    stack.push((entry, child));
                }
            }
            _ => {}
        }
    }
    false
}

/// Refuse a raw document that exceeds [`MAX_IMPORT_DOCUMENT_DEPTH`].
pub(crate) fn check_import_document_depth(value: &serde_json::Value) -> Result<(), ZerxError> {
    if json_depth_exceeds(value, MAX_IMPORT_DOCUMENT_DEPTH) {
        return Err(import_document_depth_error());
    }
    Ok(())
}

/// Tear an owned `serde_json::Value` down without recursing per nesting level.
/// A value that failed the document bound can be deeper than `Value`'s own drop
/// recursion can take on a 2 MB thread, so every owned over-bound value is
/// disposed this way instead of dropped.
pub(crate) fn dispose_json_value(value: serde_json::Value) {
    let mut stack = vec![value];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(map) => {
                stack.extend(map.into_iter().map(|(_, v)| v));
            }
            _ => {}
        }
    }
}

#[cold]
pub(crate) fn import_document_depth_error() -> ZerxError {
    ZerxError::new(
        ErrorCode::IMPORT_UNSUPPORTED,
        format!(
            "schema document nesting exceeds the import document depth limit of {MAX_IMPORT_DOCUMENT_DEPTH}"
        ),
    )
}

// ---------------------------------------------------------------------------
// Public import surface
// ---------------------------------------------------------------------------

/// Import a JSON Schema (Draft 2020-12) document back into a `Schema`. Fallible.
pub fn from_json_schema(value: &serde_json::Value) -> Result<Schema, ZerxError> {
    from_json_schema_inner(value, false)
}

/// Core import walk, carrying the caller's unknown-property disposition.
pub(crate) fn from_json_schema_inner(
    value: &serde_json::Value,
    strip_unknown: bool,
) -> Result<Schema, ZerxError> {
    // Bound the caller's document before `ImportContext::new` clones `$defs`
    // and the walk recurses: neither may run on an unbounded raw `Value`.
    check_import_document_depth(value)?;
    let ctx = ImportContext::new(value, strip_unknown);
    import_value(value, &ctx, &[])
}

// ---------------------------------------------------------------------------
// Import keyword classification
// ---------------------------------------------------------------------------

const IMPORT_HANDLED: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "prefixItems",
    "enum",
    "const",
    "anyOf",
    "oneOf",
    "$ref",
    "discriminator",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "minItems",
    "maxItems",
    "uniqueItems",
    "format",
    "contentMediaType",
    "description",
    "title",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "$defs",
    "$schema",
];

const IMPORT_UNSUPPORTED_STRUCTURAL: &[&str] = &[
    "allOf",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "minContains",
    "maxContains",
    "patternProperties",
    "propertyNames",
    "dependentSchemas",
    "dependentRequired",
    "unevaluatedProperties",
    "unevaluatedItems",
    "$dynamicRef",
];

// ---------------------------------------------------------------------------
// Assertion keyword coverage — consumed, vacuous, or refused
// ---------------------------------------------------------------------------

/// The instance types a keyword constrains. `Any` is never vacuous.
#[derive(Clone, Copy)]
enum Applicability {
    Any,
    Types(&'static [TypeName]),
}

const STRING_APPLIES: &[TypeName] = &[TypeName::String];
// `Number` covers `Integer`: an integer node is a number node.
const NUMBER_APPLIES: &[TypeName] = &[TypeName::Number];
const ARRAY_APPLIES: &[TypeName] = &[TypeName::Array];
const OBJECT_APPLIES: &[TypeName] = &[TypeName::Object];

/// Draft 2020-12 assertion keywords (core + applicator + validation). Anything
/// else is an annotation, a core identifier, or unknown — and asserts nothing.
fn assertion_applicability(key: &str) -> Option<Applicability> {
    Some(match key {
        "type" | "enum" | "const" | "allOf" | "anyOf" | "oneOf" | "not" | "if" | "then"
        | "else" | "$ref" | "$dynamicRef" => Applicability::Any,
        "minLength" | "maxLength" | "pattern" => Applicability::Types(STRING_APPLIES),
        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" => {
            Applicability::Types(NUMBER_APPLIES)
        }
        "items" | "prefixItems" | "contains" | "minContains" | "maxContains" | "minItems"
        | "maxItems" | "uniqueItems" | "unevaluatedItems" => Applicability::Types(ARRAY_APPLIES),
        "properties"
        | "patternProperties"
        | "additionalProperties"
        | "propertyNames"
        | "required"
        | "dependentRequired"
        | "dependentSchemas"
        | "minProperties"
        | "maxProperties"
        | "unevaluatedProperties" => Applicability::Types(OBJECT_APPLIES),
        _ => return None,
    })
}

fn type_covered(applicable: TypeName, member: TypeName) -> bool {
    member == applicable || (applicable == TypeName::Number && member == TypeName::Integer)
}

fn keyword_applies_to(app: Applicability, member: TypeName) -> bool {
    match app {
        Applicability::Any => true,
        Applicability::Types(types) => types.iter().any(|t| type_covered(*t, member)),
    }
}

fn type_admitted(spec: &TypeSpec, name: TypeName) -> bool {
    match name {
        TypeName::Number => spec.admits(TypeName::Number) || spec.admits(TypeName::Integer),
        _ => spec.admits(name),
    }
}

fn keyword_admitted_by(app: Applicability, spec: &TypeSpec) -> bool {
    match app {
        Applicability::Any => true,
        Applicability::Types(types) => types.iter().any(|t| type_admitted(spec, *t)),
    }
}

/// A non-negative JSON integer. An integral float such as `2.0` counts.
fn as_non_negative_u64(v: &serde_json::Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    let f = v.as_f64()?;
    if f.is_finite() && f.fract() == 0.0 && f >= 0.0 && f <= u64::MAX as f64 {
        Some(f as u64)
    } else {
        None
    }
}

fn json_integer_form(v: &serde_json::Value) -> Option<(bool, u128)> {
    if let Some(i) = v.as_i64() {
        return Some((i < 0, u128::from(i.unsigned_abs())));
    }
    if let Some(u) = v.as_u64() {
        return Some((false, u128::from(u)));
    }
    let f = v.as_f64()?;
    // The bound guards the `as u128` cast: from 2^128 upward an f64 no longer
    // represents distinct integers and the cast would saturate.
    const TWO_POW_128: f64 = 340282366920938463463374607431768211456.0;
    if f.is_finite() && f.fract() == 0.0 && f.abs() < TWO_POW_128 {
        Some((f < 0.0, f.abs() as u128))
    } else {
        None
    }
}

fn compare_integer_forms((an, am): (bool, u128), (bn, bm): (bool, u128)) -> std::cmp::Ordering {
    match (an, bn) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => am.cmp(&bm),
        (true, true) => bm.cmp(&am),
    }
}

/// Exact for integers, `f64` otherwise. Both operands are JSON numbers.
fn compare_json_numbers(
    a: &serde_json::Value,
    b: &serde_json::Value,
) -> Option<std::cmp::Ordering> {
    if let (Some(x), Some(y)) = (json_integer_form(a), json_integer_form(b)) {
        return Some(compare_integer_forms(x, y));
    }
    a.as_f64()?.partial_cmp(&b.as_f64()?)
}

fn required_names(v: &serde_json::Value) -> Option<Vec<&str>> {
    let arr = v.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        out.push(item.as_str()?);
    }
    Some(out)
}

/// Values of keywords the import walk consumes must match the Draft 2020-12
/// grammar; a malformed value is never silently skipped.
fn validate_node_grammar(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
) -> Result<(), ZerxError> {
    for (key, val) in obj.iter() {
        let ok = match key.as_str() {
            "properties" | "$defs" => val.is_object(),
            "required" => required_names(val).is_some(),
            "enum" | "anyOf" | "oneOf" | "prefixItems" => val.is_array(),
            "additionalProperties" | "items" => val.is_boolean() || val.is_object(),
            _ => true,
        };
        if !ok {
            return Err(type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                format!("malformed `{key}` value"),
            ));
        }
    }
    Ok(())
}

fn validate_static_value(
    key: &str,
    val: &serde_json::Value,
    path: &[String],
) -> Result<(), ZerxError> {
    if key == "pattern" {
        // `pattern` is an ECMA-262 source: a source valid there but not
        // translatable is `Unsupported`, not malformed.
        let src = val.as_str().ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "malformed `pattern` value",
            )
        })?;
        return match ecma_regex::compile(src) {
            Ok(_) => Ok(()),
            Err(EcmaError::Malformed(_)) => Err(type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "malformed `pattern` value",
            )),
            Err(EcmaError::Unsupported(_)) => Err(type_error(
                path,
                ErrorCode::IMPORT_UNSUPPORTED,
                "unsupported `pattern` value",
            )),
        };
    }
    let ok = match key {
        "minLength" | "maxLength" | "minItems" | "maxItems" | "minProperties" | "maxProperties" => {
            as_non_negative_u64(val).is_some()
        }
        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" => {
            val.is_number()
        }
        "required" => required_names(val).is_some(),
        "uniqueItems" => val.is_boolean(),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(type_error(
            path,
            ErrorCode::IMPORT_MALFORMED,
            format!("malformed `{key}` value"),
        ))
    }
}

fn static_keyword_is_evaluable(key: &str) -> bool {
    matches!(
        key,
        "minLength"
            | "maxLength"
            | "pattern"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "required"
            | "minProperties"
            | "maxProperties"
    )
}

fn multiple_of(member: &serde_json::Value, divisor: &serde_json::Value) -> Option<bool> {
    let m = member
        .as_i64()
        .map(i128::from)
        .or_else(|| member.as_u64().map(i128::from))?;
    let d = divisor
        .as_i64()
        .map(i128::from)
        .or_else(|| divisor.as_u64().map(i128::from))?;
    if d <= 0 {
        return None;
    }
    Some(m % d == 0)
}

fn static_keyword_holds(
    key: &str,
    val: &serde_json::Value,
    member: &serde_json::Value,
    path: &[String],
) -> Result<bool, ZerxError> {
    use std::cmp::Ordering;

    Ok(match key {
        "minLength" => {
            member.as_str().unwrap_or("").chars().count() as u64
                >= as_non_negative_u64(val).unwrap_or(0)
        }
        "maxLength" => {
            member.as_str().unwrap_or("").chars().count() as u64
                <= as_non_negative_u64(val).unwrap_or(0)
        }
        "pattern" => {
            let src = val.as_str().unwrap_or("");
            let re = ecma_regex::compile(src).map_err(|e| match e {
                EcmaError::Malformed(_) => type_error(
                    path,
                    ErrorCode::IMPORT_MALFORMED,
                    format!("`pattern` value `{src}` is not valid ECMA-262"),
                ),
                EcmaError::Unsupported(_) => type_error(
                    path,
                    ErrorCode::IMPORT_UNSUPPORTED,
                    format!("`pattern` value `{src}` is not supported"),
                ),
            })?;
            re.is_match(member.as_str().unwrap_or(""))
        }
        "minimum" => matches!(
            compare_json_numbers(member, val),
            Some(Ordering::Greater | Ordering::Equal)
        ),
        "maximum" => matches!(
            compare_json_numbers(member, val),
            Some(Ordering::Less | Ordering::Equal)
        ),
        "exclusiveMinimum" => compare_json_numbers(member, val) == Some(Ordering::Greater),
        "exclusiveMaximum" => compare_json_numbers(member, val) == Some(Ordering::Less),
        "minItems" => {
            member.as_array().map(|a| a.len()).unwrap_or(0) as u64
                >= as_non_negative_u64(val).unwrap_or(0)
        }
        "maxItems" => {
            member.as_array().map(|a| a.len()).unwrap_or(0) as u64
                <= as_non_negative_u64(val).unwrap_or(0)
        }
        "uniqueItems" => {
            if !val.as_bool().unwrap_or(false) {
                return Ok(true);
            }
            let arr = member.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            let mut values = Vec::with_capacity(arr.len());
            for item in arr {
                values.push(ZerxValue::from_serialize(item).map_err(|e| e.at(path.to_vec()))?);
            }
            crate::types::first_duplicate(&values).is_none()
        }
        "required" => {
            let names = required_names(val).unwrap_or_default();
            let object = member.as_object();
            names
                .iter()
                .all(|name| object.map(|o| o.contains_key(*name)).unwrap_or(false))
        }
        "minProperties" => {
            member.as_object().map(|o| o.len()).unwrap_or(0) as u64
                >= as_non_negative_u64(val).unwrap_or(0)
        }
        "maxProperties" => {
            member.as_object().map(|o| o.len()).unwrap_or(0) as u64
                <= as_non_negative_u64(val).unwrap_or(0)
        }
        _ => false,
    })
}

/// Evaluate the sibling assertion keywords of an `enum`/`const` node against
/// its member set. Returns the keywords consumed because they are either
/// satisfied by every applicable member or vacuous for the whole enum.
fn evaluate_enum_siblings(
    obj: &serde_json::Map<String, serde_json::Value>,
    members: &[serde_json::Value],
    path: &[String],
) -> Result<Vec<String>, ZerxError> {
    let mut consumed = Vec::new();

    for (key, val) in obj.iter() {
        let k = key.as_str();
        if matches!(k, "type" | "enum" | "const") {
            continue;
        }
        let app = match assertion_applicability(k) {
            Some(a) => a,
            None => continue,
        };
        // A refusal that stands today is not relaxed by the enum evaluation.
        if IMPORT_UNSUPPORTED_STRUCTURAL.contains(&k) {
            return Err(type_error(
                path,
                ErrorCode::IMPORT_UNSUPPORTED,
                format!("`{k}` is not representable in zerx"),
            ));
        }

        let applicable: Vec<(usize, &serde_json::Value)> = members
            .iter()
            .enumerate()
            .filter(|(_, m)| keyword_applies_to(app, value_type_name(m)))
            .collect();

        if applicable.is_empty() {
            // Vacuous for every member: exact to ignore, but its grammar still
            // has to be well-formed because the keyword is consumed here.
            validate_static_value(k, val, path)?;
            consumed.push(k.to_string());
            continue;
        }

        if !static_keyword_is_evaluable(k) {
            return Err(type_error(
                path,
                ErrorCode::IMPORT_UNSUPPORTED,
                format!("`{k}` alongside `enum`/`const` cannot be evaluated statically"),
            ));
        }
        validate_static_value(k, val, path)?;

        for (index, member) in applicable {
            let holds =
                if k == "multipleOf" {
                    match multiple_of(member, val) {
                        Some(result) => result,
                        None => return Err(type_error(
                            path,
                            ErrorCode::IMPORT_UNSUPPORTED,
                            "`multipleOf` alongside `enum`/`const` cannot be evaluated statically",
                        )),
                    }
                } else {
                    static_keyword_holds(k, val, member, path)?
                };
            if !holds {
                return Err(type_error(
                    path,
                    ErrorCode::IMPORT_UNSUPPORTED,
                    format!("`enum`/`const` member {index} violates `{k}`"),
                ));
            }
        }
        consumed.push(k.to_string());
    }

    Ok(consumed)
}

// ---------------------------------------------------------------------------
// ImportContext — $defs table + lazy memoisation + cycle guard
// ---------------------------------------------------------------------------

pub(crate) struct ImportContext {
    defs: serde_json::Map<String, serde_json::Value>,
    resolved: Rc<RefCell<HashMap<String, Schema>>>,
    placeholders: RefCell<HashMap<String, Schema>>,
    importing: RefCell<HashSet<String>>,
    strip_unknown: bool,
    depth: std::cell::Cell<usize>,
}

impl ImportContext {
    fn new(root: &serde_json::Value, strip_unknown: bool) -> Self {
        let defs = root
            .get("$defs")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        ImportContext {
            defs,
            resolved: Rc::new(RefCell::new(HashMap::new())),
            placeholders: RefCell::new(HashMap::new()),
            importing: RefCell::new(HashSet::new()),
            strip_unknown,
            depth: std::cell::Cell::new(0),
        }
    }

    fn ref_schema(&self, id: &str, path: &[String]) -> Result<Schema, ZerxError> {
        if !self.defs.contains_key(id) {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                format!("`$ref` target `#/$defs/{id}` not found"),
            )
            .at(path.to_vec()));
        }

        let existing = self.placeholders.borrow().get(id).cloned();
        let lz = match existing {
            Some(p) => p,
            None => {
                let resolved = self.resolved.clone();
                let id_owned = id.to_string();
                let lz: Schema = crate::lazy(move || {
                    resolved
                        .borrow()
                        .get(&id_owned)
                        .cloned()
                        .expect("def resolved before lazy forced")
                })
                .into();
                self.placeholders
                    .borrow_mut()
                    .insert(id.to_string(), lz.clone());
                lz
            }
        };

        self.ensure_resolved(id, path)?;
        Ok(lz)
    }

    fn ensure_resolved(&self, id: &str, _path: &[String]) -> Result<(), ZerxError> {
        if self.resolved.borrow().contains_key(id) {
            return Ok(());
        }
        if self.importing.borrow().contains(id) {
            return Ok(());
        }

        self.importing.borrow_mut().insert(id.to_string());
        let node = self.defs.get(id).unwrap().clone();
        let def_path = vec!["$defs".to_string(), id.to_string()];
        let s = import_value(&node, self, &def_path)?;
        self.resolved.borrow_mut().insert(id.to_string(), s);
        self.importing.borrow_mut().remove(id);
        Ok(())
    }
}

// Decodes the single JSON Pointer segment after `#/$defs/` (RFC 6901). A
// further `/` would point into a def rather than name one, and a `%` would need
// URI-fragment decoding; both are refused rather than looked up as a literal
// name, which could bind the ref to a different def.
fn decode_defs_ref(raw: &str, path: &[String]) -> Result<String, ZerxError> {
    if raw.contains('/') || raw.contains('%') {
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!(
                "`$ref` `#/$defs/{raw}` must name a single `$defs` entry without percent-encoding"
            ),
        )
        .at(path.to_vec()));
    }
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0') | Some('1')) {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                format!("`$ref` `#/$defs/{raw}` has an invalid JSON Pointer `~` escape"),
            )
            .at(path.to_vec()));
        }
    }
    Ok(raw.replace("~1", "/").replace("~0", "~"))
}

// ---------------------------------------------------------------------------
// is_bare_null_schema — helper for nullable collapse
// ---------------------------------------------------------------------------

fn is_bare_null_schema(v: &serde_json::Value) -> bool {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return false,
    };
    if obj.get("type").and_then(|t| t.as_str()) != Some("null") {
        return false;
    }
    const STRUCTURAL: &[&str] = &[
        "properties",
        "required",
        "additionalProperties",
        "items",
        "prefixItems",
        "enum",
        "const",
        "anyOf",
        "oneOf",
        "$ref",
        "discriminator",
        "minLength",
        "maxLength",
        "pattern",
        "minimum",
        "maximum",
        "minItems",
        "maxItems",
        "allOf",
        "not",
        "format",
        "contentMediaType",
    ];
    !obj.keys()
        .any(|k| k != "type" && STRUCTURAL.contains(&k.as_str()))
}

// ---------------------------------------------------------------------------
// `type` parsing — fail-closed
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TypeName {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    Integer,
    String,
}

#[derive(Clone, Copy, Debug)]
struct TypeSpec {
    name: TypeName,
    nullable: bool,
}

impl TypeSpec {
    /// Whether this spec admits the JSON type `name`. Integer values are a subset
    /// of number; null is admitted only when the type array carried `"null"`.
    fn admits(&self, name: TypeName) -> bool {
        name == self.name
            || (name == TypeName::Integer && self.name == TypeName::Number)
            || (name == TypeName::Null && self.nullable)
    }
}

fn type_name(s: &str) -> Option<TypeName> {
    Some(match s {
        "null" => TypeName::Null,
        "boolean" => TypeName::Boolean,
        "object" => TypeName::Object,
        "array" => TypeName::Array,
        "number" => TypeName::Number,
        "integer" => TypeName::Integer,
        "string" => TypeName::String,
        _ => return None,
    })
}

fn type_error(path: &[String], code: ErrorCode, message: impl Into<String>) -> ZerxError {
    ZerxError::new(code, message).at(path.to_vec())
}

/// Rebuilds the string validators (`minLength`, `maxLength`, `pattern`) a string
/// node carries, in that order, and records each present keyword in `consumed`.
/// Used by both the plain string dispatch and the `uri`/`url` marker branches so
/// the keywords are enforced identically wherever a string node is reconstructed.
fn import_string_validators(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
    consumed: &mut HashSet<String>,
) -> Result<StringSchema, ZerxError> {
    use crate::string;

    let mut sb = string();
    if let Some(raw) = obj.get("minLength") {
        let n = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`minLength` must be a non-negative integer",
            )
        })?;
        sb = sb.min(n as usize);
        consumed.insert("minLength".to_string());
    }
    if let Some(raw) = obj.get("maxLength") {
        let n = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`maxLength` must be a non-negative integer",
            )
        })?;
        sb = sb.max(n as usize);
        consumed.insert("maxLength".to_string());
    }
    if let Some(raw) = obj.get("pattern") {
        let pat = raw.as_str().ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`pattern` must be a string",
            )
        })?;
        // Compiled here as in static evaluation: a refused pattern would
        // otherwise import and reject every string at validation time.
        match ecma_regex::compile(pat) {
            Ok(_) => {}
            Err(EcmaError::Malformed(_)) => {
                return Err(type_error(
                    path,
                    ErrorCode::IMPORT_MALFORMED,
                    "`pattern` is not valid ECMA-262",
                ))
            }
            Err(EcmaError::Unsupported(_)) => {
                return Err(type_error(
                    path,
                    ErrorCode::IMPORT_UNSUPPORTED,
                    "`pattern` uses a construct that cannot be translated",
                ))
            }
        }
        sb = sb.regex(pat);
        consumed.insert("pattern".to_string());
    }
    Ok(sb)
}

/// Parse Draft 2020-12 `type`. Only a single known name or exactly one non-null
/// known name plus `"null"` is accepted; every other well-formed form is
/// `IMPORT_UNSUPPORTED`, every malformed form `IMPORT_MALFORMED`.
fn parse_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
) -> Result<Option<TypeSpec>, ZerxError> {
    let raw = match obj.get("type") {
        Some(v) => v,
        None => return Ok(None),
    };
    match raw {
        serde_json::Value::String(s) => match type_name(s) {
            Some(name) => Ok(Some(TypeSpec {
                name,
                nullable: false,
            })),
            None => Err(type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                format!("unknown `type` name `{s}`"),
            )),
        },
        serde_json::Value::Array(types) => {
            if types.is_empty() {
                return Err(type_error(
                    path,
                    ErrorCode::IMPORT_MALFORMED,
                    "`type` array must not be empty",
                ));
            }
            let mut names: Vec<TypeName> = Vec::with_capacity(types.len());
            for t in types {
                let s = t.as_str().ok_or_else(|| {
                    type_error(
                        path,
                        ErrorCode::IMPORT_MALFORMED,
                        "`type` array must contain only strings",
                    )
                })?;
                let name = type_name(s).ok_or_else(|| {
                    type_error(
                        path,
                        ErrorCode::IMPORT_MALFORMED,
                        format!("unknown `type` name `{s}`"),
                    )
                })?;
                if names.contains(&name) {
                    return Err(type_error(
                        path,
                        ErrorCode::IMPORT_MALFORMED,
                        "`type` array must not contain duplicates",
                    ));
                }
                names.push(name);
            }
            if names.len() == 2 {
                if let Some(pos) = names.iter().position(|n| *n == TypeName::Null) {
                    return Ok(Some(TypeSpec {
                        name: names[1 - pos],
                        nullable: true,
                    }));
                }
            }
            Err(type_error(
                path,
                ErrorCode::IMPORT_UNSUPPORTED,
                "only a single `type` name or `[T, \"null\"]` is accepted",
            ))
        }
        _ => Err(type_error(
            path,
            ErrorCode::IMPORT_MALFORMED,
            "`type` must be a string or an array of strings",
        )),
    }
}

fn value_type_name(v: &serde_json::Value) -> TypeName {
    match v {
        serde_json::Value::Null => TypeName::Null,
        serde_json::Value::Bool(_) => TypeName::Boolean,
        serde_json::Value::Number(n) => {
            let integral = n.as_i64().is_some()
                || n.as_u64().is_some()
                || n.as_f64()
                    .map(|f| f.is_finite() && f.fract() == 0.0)
                    .unwrap_or(false);
            if integral {
                TypeName::Integer
            } else {
                TypeName::Number
            }
        }
        serde_json::Value::String(_) => TypeName::String,
        serde_json::Value::Array(_) => TypeName::Array,
        serde_json::Value::Object(_) => TypeName::Object,
    }
}

fn admits_value(spec: &TypeSpec, v: &serde_json::Value) -> bool {
    spec.admits(value_type_name(v))
}

fn within(inner: &TypeSpec, outer: &TypeSpec) -> bool {
    outer.admits(inner.name) && (!inner.nullable || outer.admits(TypeName::Null))
}

/// A union member under an outer `type` must declare its own type within the
/// outer set, so the intersection is provably the union itself.
fn ensure_member_within(
    member: &serde_json::Value,
    outer: &TypeSpec,
    keyword: &str,
    index: usize,
    path: &[String],
) -> Result<(), ZerxError> {
    let member_obj = member.as_object().ok_or_else(|| {
        type_error(
            path,
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("`{keyword}` member {index} must declare a `type` alongside the outer `type`"),
        )
    })?;
    match parse_type(member_obj, path)? {
        Some(inner) if within(&inner, outer) => Ok(()),
        _ => Err(type_error(
            path,
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("`{keyword}` member {index} must declare a `type` within the outer `type`"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Import walk — pub(crate) so the policy pipeline can wrap it
// ---------------------------------------------------------------------------

pub(crate) fn import_value(
    node: &serde_json::Value,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Schema, ZerxError> {
    // Nesting and `$defs` resolution both recurse through here; a flat chain of
    // refs nests once per link although the document stays shallow, so serde's
    // parse limit does not bound it. Cap it at the parse-depth limit.
    let depth = ctx.depth.get();
    if depth >= MAX_PARSE_DEPTH {
        return Err(import_depth_error(path));
    }
    ctx.depth.set(depth + 1);
    let result = import_node(node, ctx, path);
    ctx.depth.set(depth);
    result
}

#[cold]
fn import_depth_error(path: &[String]) -> ZerxError {
    ZerxError::new(
        ErrorCode::IMPORT_UNSUPPORTED,
        format!("schema nesting exceeds the import depth limit of {MAX_PARSE_DEPTH}"),
    )
    .at(path.to_vec())
}

// The import recurses through `import_node` once per nesting level or `$ref`
// link, so every step lives in its own function and writes its result through
// `base`: a single frame holding every step's temporaries exhausted a 2 MB
// stack well within the depth limit in debug builds.
fn import_node(
    node: &serde_json::Value,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Schema, ZerxError> {
    let (obj, spec) = match import_node_prelude(node, path)? {
        Some(parts) => parts,
        // Step 1: `true` imports as `any()`.
        None => return Ok(true_schema()),
    };

    // Assertion keywords this node's reconstruction actually consumed. The
    // coverage check before `meta` routing proves every one of them was
    // consumed, is vacuous for the declared type, or is refused.
    let mut consumed: HashSet<String> = HashSet::new();
    let mut base: Option<Schema> = None;

    // `allOf`/`not` are refused with their hint on every node, a `$ref`
    // node included, before any base is built.
    refuse_all_of_not(obj, path)?;
    if obj.contains_key("$ref") {
        import_ref_step(obj, spec, ctx, path, &mut consumed, &mut base)?;
    }
    if base.is_none() {
        import_nullable_collapse_step(obj, spec, ctx, path, &mut consumed, &mut base)?;
    }
    if base.is_none() {
        import_union_step(obj, spec, ctx, path, &mut consumed, &mut base)?;
    }
    if base.is_none() && (obj.contains_key("enum") || obj.contains_key("const")) {
        import_enum_const_step(obj, spec, path, &mut consumed, &mut base)?;
    }

    let fmt = obj.get("format").and_then(|v| v.as_str());
    let had_base_before_markers = base.is_some();

    if base.is_none() {
        import_format_marker_step(obj, spec, ctx, path, &mut consumed, fmt, &mut base)?;
    }
    if base.is_none() {
        import_type_step(obj, spec, ctx, path, &mut consumed, fmt, &mut base)?;
    }

    finish_import_node(
        base,
        had_base_before_markers,
        obj,
        spec,
        fmt,
        path,
        &consumed,
    )
}

type NodeParts<'a> = (
    &'a serde_json::Map<String, serde_json::Value>,
    Option<TypeSpec>,
);

fn true_schema() -> Schema {
    crate::any().into()
}

// Steps 1–2: boolean schema, non-object guard, keyword grammar, nested `$id`
// and `type` parsing. `None` stands for the `true` schema.
fn import_node_prelude<'a>(
    node: &'a serde_json::Value,
    path: &[String],
) -> Result<Option<NodeParts<'a>>, ZerxError> {
    // Step 1: Boolean schema.
    match node {
        serde_json::Value::Bool(true) => return Ok(None),
        serde_json::Value::Bool(false) => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "a `false` schema rejects all values; zerx has no never type",
            )
            .at(path.to_vec()));
        }
        _ => {}
    }

    // Step 2: Non-object guard.
    let obj = match node.as_object() {
        Some(o) => o,
        None => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "schema must be an object or boolean",
            )
            .at(path.to_vec()))
        }
    };

    // Step 2 (grammar): grammar of structurally consumed keyword values. A malformed
    // value is refused here rather than silently skipped by a later branch.
    validate_node_grammar(obj, path)?;

    // A nested `$id` re-bases the `$ref`s under it in JSON Schema, but zerx
    // resolves every ref against the root `$defs`; only the root may carry one.
    if obj.contains_key("$id") && !path.is_empty() {
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            "`$id` on a non-root node is not supported",
        )
        .at(path.to_vec()));
    }

    // Step 2 (type): parse `type`. Runs on every object node, `$ref` included, so a
    // malformed `type` is never skipped.
    let spec = parse_type(obj, path)?;
    Ok(Some((obj, spec)))
}

// Step 4: `allOf` / `not` — raise a clear error.
fn refuse_all_of_not(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
) -> Result<(), ZerxError> {
    for kw in &["allOf", "not"] {
        if obj.contains_key(*kw) {
            let hint = if *kw == "allOf" {
                "use extend() for composition"
            } else {
                "use union() alternatives"
            };
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                format!("`{kw}` is not representable in zerx; {hint}"),
            )
            .at(path.to_vec()));
        }
    }
    Ok(())
}

// Step 3: `$ref` — the base is the lazy def placeholder.
fn import_ref_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    {
        if spec.is_some() {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "`type` alongside `$ref` is not supported",
            )
            .at(path.to_vec()));
        }
        let ref_str = obj["$ref"].as_str().unwrap_or("");
        if !ref_str.starts_with("#/$defs/") {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "only local `#/$defs/<id>` refs are supported",
            )
            .at(path.to_vec()));
        }
        let id = decode_defs_ref(&ref_str["#/$defs/".len()..], path)?;
        *base = Some(ctx.ref_schema(&id, path)?);
        consumed.insert("$ref".to_string());
    }
    Ok(())
}

// Step 5: nullable `anyOf` collapse.
fn import_nullable_collapse_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    if let Some(any_of_arr) = obj.get("anyOf").and_then(|v| v.as_array()) {
        if any_of_arr.len() == 2 {
            if let Some(null_i) = any_of_arr.iter().position(is_bare_null_schema) {
                // The null member is dropped from the reconstruction, so it
                // is imported only to hold it to the import rules: a keyword
                // the bare-null check does not know (`if`, `$dynamicRef`,
                // a non-root `$id`) is refused there as anywhere else.
                let mut null_path = path.to_vec();
                null_path.push("anyOf".to_string());
                null_path.push(null_i.to_string());
                import_value(&any_of_arr[null_i], ctx, &null_path)?;
                let other_i = 1 - null_i;
                let mut child_path = path.to_vec();
                child_path.push("anyOf".to_string());
                child_path.push(other_i.to_string());
                let other = import_value(&any_of_arr[other_i], ctx, &child_path)?;
                if let Some(outer) = spec {
                    for (i, member) in any_of_arr.iter().enumerate() {
                        ensure_member_within(member, &outer, "anyOf", i, path)?;
                    }
                }
                *base = Some(other.nullable());
                consumed.insert("anyOf".to_string());
                if spec.is_some() {
                    consumed.insert("type".to_string());
                }
                // Continue to step 10.
            }
        }
    }
    Ok(())
}

// Step 6: `oneOf` / `anyOf` not consumed by the nullable collapse.
fn import_union_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    let (arr_opt, is_one_of) = if let Some(a) = obj.get("oneOf").and_then(|v| v.as_array()) {
        (Some(a), true)
    } else if let Some(a) = obj.get("anyOf").and_then(|v| v.as_array()) {
        (Some(a), false)
    } else {
        (None, false)
    };

    if let Some(arr) = arr_opt {
        if arr.is_empty() {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "an empty `anyOf`/`oneOf` accepts nothing; zerx has no never type",
            )
            .at(path.to_vec()));
        }
        let kw = if is_one_of { "oneOf" } else { "anyOf" };
        let mut variants: Vec<Schema> = Vec::new();
        for (i, v) in arr.iter().enumerate() {
            let mut child_path = path.to_vec();
            child_path.push(kw.to_string());
            child_path.push(i.to_string());
            variants.push(import_value(v, ctx, &child_path)?);
        }

        if let Some(outer) = spec {
            for (i, member) in arr.iter().enumerate() {
                ensure_member_within(member, &outer, kw, i, path)?;
            }
            consumed.insert("type".to_string());
        }

        let disc_key = obj
            .get("discriminator")
            .and_then(|d| d.as_object())
            .and_then(|d| d.get("propertyName"))
            .and_then(|p| p.as_str());

        let schema = if let Some(key) = disc_key {
            let du: Schema = crate::discriminated_union(key, variants.clone()).into();
            let is_ok = matches!(
                &du.kind,
                SchemaKind::DiscriminatedUnion(b)
                    if matches!(b.state, DiscriminatorState::Ok { .. })
            );
            if is_ok {
                du
            } else if is_one_of {
                // A failed discriminator invariant leaves exactly-one
                // semantics intact; `x-oneOf` is never written.
                crate::types::one_of(variants)
            } else {
                crate::union(variants).into()
            }
        } else if is_one_of {
            crate::types::one_of(variants)
        } else {
            crate::union(variants).into()
        };

        consumed.insert(kw.to_string());
        *base = Some(schema);
        // Continue to step 10.
    }
    Ok(())
}

// Step 7: `enum`/`const`. Sibling assertion keywords are evaluated
// statically against the member set: satisfied ones are redundant and
// consumed, violated or non-evaluable ones are refused.
fn import_enum_const_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    let has_enum = obj.contains_key("enum");
    let has_const = obj.contains_key("const");
    let enum_arr = obj.get("enum").and_then(|v| v.as_array());

    // Convert the constant before any deep clone or type walk: the input bound
    // must refuse a too-deep constant before serde_json recurses over it.
    let const_zv = if has_const {
        Some(
            ZerxValue::from_serialize(obj.get("const").expect("`const` present"))
                .map_err(|e| e.at(path.to_vec()))?,
        )
    } else {
        None
    };

    // The `type`-membership rule for enum/const members applies first.
    if let Some(t) = spec {
        if let Some(arr) = enum_arr {
            if let Some((i, _)) = arr.iter().enumerate().find(|(_, v)| !admits_value(&t, v)) {
                return Err(ZerxError::new(
                    ErrorCode::IMPORT_UNSUPPORTED,
                    format!("`enum` member {i} is outside the declared `type`"),
                )
                .at(path.to_vec()));
            }
        }
        if let Some(const_val) = obj.get("const") {
            if !admits_value(&t, const_val) {
                return Err(ZerxError::new(
                    ErrorCode::IMPORT_UNSUPPORTED,
                    "`const` value is outside the declared `type`",
                )
                .at(path.to_vec()));
            }
        }
    }

    // Effective member set: `enum` merged with `const` where present.
    let members: Vec<serde_json::Value> = if has_enum && has_const {
        let arr = enum_arr.expect("`enum` is an array per grammar check");
        let const_val = obj.get("const").expect("`const` present");
        let const_zv = const_zv.as_ref().expect("`const` present");
        let mut is_member = false;
        for member in arr {
            let member_zv = ZerxValue::from_serialize(member).map_err(|e| e.at(path.to_vec()))?;
            if crate::types::literal_eq(&member_zv, const_zv) {
                is_member = true;
                break;
            }
        }
        if !is_member {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "`const` is not a member of the sibling `enum`",
            )
            .at(path.to_vec()));
        }
        consumed.insert("enum".to_string());
        consumed.insert("const".to_string());
        vec![const_val.clone()]
    } else if has_enum {
        consumed.insert("enum".to_string());
        enum_arr
            .expect("`enum` is an array per grammar check")
            .clone()
    } else {
        consumed.insert("const".to_string());
        vec![obj.get("const").expect("`const` present").clone()]
    };
    if spec.is_some() {
        consumed.insert("type".to_string());
    }

    for key in evaluate_enum_siblings(obj, &members, path)? {
        consumed.insert(key);
    }

    if has_const {
        let zv = const_zv.expect("`const` present");
        *base = Some(crate::literal(zv).into());
    } else {
        let arr = enum_arr.expect("`enum` is an array per grammar check");
        let all_strings = arr.iter().all(|v| v.is_string());
        if all_strings {
            let strings: Vec<String> = arr
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            *base = Some(crate::enumerate(strings).into());
        } else {
            let mut lit_variants: Vec<Schema> = Vec::new();
            for v in arr {
                let zv = ZerxValue::from_serialize(v).map_err(|e| e.at(path.to_vec()))?;
                lit_variants.push(crate::literal(zv).into());
            }
            *base = Some(crate::union(lit_variants).into());
        }
    }
    // Continue to step 10.
    Ok(())
}

// Step 8: format-marker kinds (checked before generic type dispatch).
fn import_format_marker_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    fmt: Option<&str>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    // `record` is the only marker with a child schema; the others are leaves
    // and stay out of the frames live across the recursion.
    match (spec.map(|s| s.name), fmt) {
        (Some(TypeName::Object), Some("record")) => {
            import_record_marker(obj, ctx, path, consumed, base)
        }
        _ => import_leaf_marker(obj, spec, path, consumed, fmt, base),
    }
}

// Step 8 for the markers without a child schema.
fn import_leaf_marker(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    path: &[String],
    consumed: &mut HashSet<String>,
    fmt: Option<&str>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    if fmt == Some("json") {
        if spec.is_some() {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "`type` alongside `format: \"json\"` is not supported",
            )
            .at(path.to_vec()));
        }
        *base = Some(crate::json().into());
    } else if let Some(s) = spec {
        match (s.name, fmt) {
            (TypeName::Object, Some("jsonschema")) => {
                *base = Some(crate::jsonschema().into());
                consumed.insert("type".to_string());
            }
            (TypeName::String, Some("buffer")) => {
                // `buffer` holds bytes; the string validators skip non-string
                // values, so consuming them here would not enforce them.
                *base = Some(crate::buffer().into());
                consumed.insert("type".to_string());
            }
            (TypeName::String, Some("uri")) => {
                let mut schema: Schema = crate::uri().into();
                let extra: Schema = import_string_validators(obj, path, consumed)?.into();
                schema.validators.extend(extra.validators);
                *base = Some(schema);
                consumed.insert("type".to_string());
            }
            (TypeName::String, Some("url")) => {
                let mut schema: Schema = crate::url().into();
                let extra: Schema = import_string_validators(obj, path, consumed)?.into();
                schema.validators.extend(extra.validators);
                *base = Some(schema);
                consumed.insert("type".to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

// Step 8, `type: "object"` + `format: "record"`.
fn import_record_marker(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    match obj.get("additionalProperties") {
        Some(serde_json::Value::Bool(false)) => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "record with `additionalProperties: false` is meaningless",
            )
            .at(path.to_vec()));
        }
        Some(serde_json::Value::Bool(true)) | None => {
            *base = Some(crate::record(crate::any()).into());
        }
        Some(ap_schema) => {
            let mut ap_path = path.to_vec();
            ap_path.push("additionalProperties".to_string());
            let inner = import_value(ap_schema, ctx, &ap_path)?;
            *base = Some(crate::record(inner).into());
        }
    }
    consumed.insert("type".to_string());
    if obj.contains_key("additionalProperties") {
        consumed.insert("additionalProperties".to_string());
    }
    // The export form carries `properties: {}`; any other shape
    // leaves `properties` to the coverage check below.
    if obj
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|o| o.is_empty())
        .unwrap_or(false)
    {
        consumed.insert("properties".to_string());
    }
    Ok(())
}

// Imports the child schema at `path` + `key` (+ `index`). Building the child
// path here keeps it out of the frames that stay live across the recursion.
fn import_child(
    node: &serde_json::Value,
    ctx: &ImportContext,
    path: &[String],
    key: &str,
    index: Option<String>,
) -> Result<Schema, ZerxError> {
    let mut child_path = path.to_vec();
    child_path.push(key.to_string());
    if let Some(i) = index {
        child_path.push(i);
    }
    import_value(node, ctx, &child_path)
}

// Step 9: type dispatch. Exhaustive over `TypeName`: every `Some(spec)`
// produces a base here (or in step 8) or has already errored.
fn import_type_step(
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    fmt: Option<&str>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    if let Some(s) = spec {
        consumed.insert("type".to_string());
        match s.name {
            TypeName::String => import_string_type(obj, path, consumed, fmt, base)?,
            TypeName::Number => import_number_type(obj, path, consumed, base)?,
            TypeName::Integer => import_integer_type(obj, path, consumed, base)?,
            TypeName::Boolean | TypeName::Null => import_scalar_type(s.name, base),
            TypeName::Array => import_array_type(obj, ctx, path, consumed, base)?,
            TypeName::Object => import_object_type(obj, ctx, path, consumed, base)?,
        }
    }
    Ok(())
}

// Step 9, `type: "boolean"` / `type: "null"`.
fn import_scalar_type(name: TypeName, base: &mut Option<Schema>) {
    *base = Some(match name {
        TypeName::Boolean => crate::boolean().into(),
        _ => crate::null().into(),
    });
}

// Step 9, `type: "string"`.
fn import_string_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
    consumed: &mut HashSet<String>,
    fmt: Option<&str>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    *base = Some({
        let mut sb = import_string_validators(obj, path, consumed)?;
        match fmt {
            Some("email") => sb = sb.email(),
            Some("uuid") => sb = sb.uuid(),
            _ => {}
        }
        sb.into()
    });
    Ok(())
}

// Step 9, `type: "number"`.
// 2^53: from here on neighbouring integers share one `f64`. Number nodes keep
// this refusal for every bound; integer nodes apply it only to float literals
// (`import_integer_bound`).
const EXACT_F64_INTEGER_LIMIT: f64 = 9_007_199_254_740_992.0;

// Reads an imported `minimum`/`maximum`. Bounds compare in `f64`; a bound of
// magnitude 2^53 or more no longer separates neighbouring integers (2^53 + 1
// rounds to 2^53, so `maximum: 2^53` would accept it). Such a bound is refused
// rather than enforced inexactly; below 2^53 every value orders correctly
// against it, because rounding is monotonic.
fn import_bound(raw: &serde_json::Value, key: &str, path: &[String]) -> Result<f64, ZerxError> {
    let n = raw.as_f64().ok_or_else(|| {
        type_error(
            path,
            ErrorCode::IMPORT_MALFORMED,
            format!("`{key}` must be a number"),
        )
    })?;
    let beyond = match raw
        .as_i64()
        .map(|i| i.unsigned_abs())
        .or_else(|| raw.as_u64())
    {
        Some(m) => m >= 1u64 << 53,
        None => n.abs() >= EXACT_F64_INTEGER_LIMIT,
    };
    if beyond {
        return Err(type_error(
            path,
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("`{key}` of magnitude 2^53 or more cannot be enforced exactly"),
        ));
    }
    Ok(n)
}

// Reads a `minimum`/`maximum` of a `type: integer` node. An integer JSON
// literal is held exactly (any i64/u64 value) and compared exactly at runtime.
// A float literal of magnitude 2^53 or more may already be a rounded form of
// its source text, so it is refused like on a number node.
fn import_integer_bound(
    raw: &serde_json::Value,
    key: &str,
    path: &[String],
) -> Result<crate::NumberBound, ZerxError> {
    if let Some(i) = raw.as_i64() {
        return Ok(i.into());
    }
    if let Some(u) = raw.as_u64() {
        return Ok(u.into());
    }
    Ok(import_bound(raw, key, path)?.into())
}

fn import_number_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    *base = Some({
        let mut nb = crate::number();
        if let Some(raw) = obj.get("minimum") {
            let n = import_bound(raw, "minimum", path)?;
            nb = nb.min(n);
            consumed.insert("minimum".to_string());
        }
        if let Some(raw) = obj.get("maximum") {
            let n = import_bound(raw, "maximum", path)?;
            nb = nb.max(n);
            consumed.insert("maximum".to_string());
        }
        nb.into()
    });
    Ok(())
}

// Step 9, `type: "integer"`.
fn import_integer_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    *base = Some({
        let mut nb = crate::number().int();
        if let Some(raw) = obj.get("minimum") {
            let n = import_integer_bound(raw, "minimum", path)?;
            nb = nb.min_bound(n);
            consumed.insert("minimum".to_string());
        }
        if let Some(raw) = obj.get("maximum") {
            let n = import_integer_bound(raw, "maximum", path)?;
            nb = nb.max_bound(n);
            consumed.insert("maximum".to_string());
        }
        nb.into()
    });
    Ok(())
}

// Step 9, `type: "array"`: a closed tuple or an array.
fn import_array_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    match obj.get("prefixItems").and_then(|v| v.as_array()) {
        Some(prefix) => import_tuple_type(obj, prefix, ctx, path, consumed, base),
        None => import_plain_array(obj, ctx, path, consumed, base),
    }
}

// `type: "array"` without `prefixItems`.
fn import_plain_array(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    let item_schema = import_array_items(obj, ctx, path)?;
    *base = Some(apply_array_bounds(item_schema, obj, path, consumed)?);
    Ok(())
}

// The `items` schema of a plain array.
fn import_array_items(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Schema, ZerxError> {
    let items_val = match obj.get("items") {
        Some(v) => v,
        None => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "array schema missing `items`",
            )
            .at(path.to_vec()));
        }
    };
    Ok(
        if matches!(items_val, serde_json::Value::Bool(true))
            || items_val.as_object().map(|o| o.is_empty()).unwrap_or(false)
        {
            crate::any().into()
        } else {
            import_child(items_val, ctx, path, "items", None)?
        },
    )
}

// `prefixItems`: a tuple, representable only when `items` is `false`.
fn import_tuple_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &[serde_json::Value],
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    if !matches!(obj.get("items"), Some(serde_json::Value::Bool(false))) {
        return Err(open_tuple_error(path));
    }
    import_tuple_items(obj, prefix, ctx, path, consumed, base)
}

#[cold]
fn open_tuple_error(path: &[String]) -> ZerxError {
    ZerxError::new(
        ErrorCode::IMPORT_UNSUPPORTED,
        "`prefixItems` with an open or typed tail is not representable as a zerx tuple",
    )
    .at(path.to_vec())
}

// `prefixItems` with `items: false`: a closed tuple.
fn import_tuple_items(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &[serde_json::Value],
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    let mut items: Vec<Schema> = Vec::new();
    for (i, pi) in prefix.iter().enumerate() {
        items.push(import_child(
            pi,
            ctx,
            path,
            "prefixItems",
            Some(i.to_string()),
        )?);
    }
    check_tuple_bounds(obj, prefix.len() as u64, path, consumed)?;
    set_tuple(items, base);
    Ok(())
}

fn set_tuple(items: Vec<Schema>, base: &mut Option<Schema>) {
    *base = Some(crate::tuple(items).into());
}

// `minItems`/`maxItems` next to a closed tuple: bracketing values are
// consumed, values the tuple length can never meet are refused.
fn check_tuple_bounds(
    obj: &serde_json::Map<String, serde_json::Value>,
    n: u64,
    path: &[String],
    consumed: &mut HashSet<String>,
) -> Result<(), ZerxError> {
    if let Some(raw) = obj.get("minItems") {
        let min = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`minItems` must be a non-negative integer",
            )
        })?;
        if min > n {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "`minItems` exceeds the number of `prefixItems`; the tuple can never match",
            )
            .at(path.to_vec()));
        }
        consumed.insert("minItems".to_string());
    }
    if let Some(raw) = obj.get("maxItems") {
        let max = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`maxItems` must be a non-negative integer",
            )
        })?;
        if max < n {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "`maxItems` is below the number of `prefixItems`; the tuple can never match",
            )
            .at(path.to_vec()));
        }
        consumed.insert("maxItems".to_string());
    }
    if let Some(raw) = obj.get("uniqueItems") {
        let unique = raw.as_bool().ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`uniqueItems` must be a boolean",
            )
        })?;
        // `true` is left unconsumed so the coverage check refuses it: zerx has
        // no unique tuple. `false` asserts nothing.
        if !unique {
            consumed.insert("uniqueItems".to_string());
        }
    }
    consumed.insert("prefixItems".to_string());
    consumed.insert("items".to_string());
    Ok(())
}

fn apply_array_bounds(
    item_schema: Schema,
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &[String],
    consumed: &mut HashSet<String>,
) -> Result<Schema, ZerxError> {
    let mut ab = crate::array(item_schema);
    if let Some(raw) = obj.get("minItems") {
        let n = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`minItems` must be a non-negative integer",
            )
        })?;
        ab = ab.min(n as usize);
        consumed.insert("minItems".to_string());
    }
    if let Some(raw) = obj.get("maxItems") {
        let n = as_non_negative_u64(raw).ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`maxItems` must be a non-negative integer",
            )
        })?;
        ab = ab.max(n as usize);
        consumed.insert("maxItems".to_string());
    }
    if let Some(raw) = obj.get("uniqueItems") {
        let unique = raw.as_bool().ok_or_else(|| {
            type_error(
                path,
                ErrorCode::IMPORT_MALFORMED,
                "`uniqueItems` must be a boolean",
            )
        })?;
        if unique {
            ab = ab.unique_items();
        }
        consumed.insert("uniqueItems".to_string());
    }
    consumed.insert("items".to_string());
    Ok(ab.into())
}

// Step 9, `type: "object"`.
fn import_object_type(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
    consumed: &mut HashSet<String>,
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    import_object(obj, ctx, path, base)?;
    for key in ["properties", "required", "additionalProperties"] {
        if obj.contains_key(key) {
            consumed.insert(key.to_string());
        }
    }
    Ok(())
}

// Steps 10–11 and meta routing: annotation/modifier reconstruction, the assertion coverage
// check, and `meta` routing of the remaining keys.
fn finish_import_node(
    mut base: Option<Schema>,
    had_base_before_markers: bool,
    obj: &serde_json::Map<String, serde_json::Value>,
    spec: Option<TypeSpec>,
    fmt: Option<&str>,
    path: &[String],
    consumed: &HashSet<String>,
) -> Result<Schema, ZerxError> {
    use crate::any;

    // A `[T, "null"]` type array adds `.nullable()` only to a base built by
    // steps 8/9; a base from steps 3/5/6/7 already decides null itself.
    if !had_base_before_markers {
        if let Some(s) = spec {
            if s.nullable {
                if let Some(b) = base.take() {
                    base = Some(b.nullable());
                }
            }
        }
    }
    let mut schema = base.unwrap_or_else(|| any().into());

    // Step 10: Annotation / modifier reconstruction (inverse of apply_annotations).
    if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
        schema = schema.describe(desc);
    }
    if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
        schema = schema.title(title);
    }
    if let Some(def_val) = obj.get("default") {
        let zv = ZerxValue::from_serialize(def_val).map_err(|e| e.at(path.to_vec()))?;
        schema = schema.default(zv);
    }
    if let Some(examples) = obj.get("examples").and_then(|v| v.as_array()) {
        if let Some(first) = examples.first() {
            let zv = ZerxValue::from_serialize(first).map_err(|e| e.at(path.to_vec()))?;
            schema = schema.example(zv);
        }
    }
    if obj.get("deprecated").and_then(|v| v.as_bool()) == Some(true) {
        schema = schema.deprecated();
    }
    if obj.get("readOnly").and_then(|v| v.as_bool()) == Some(true) {
        schema = schema.read_only();
    }
    if obj.get("writeOnly").and_then(|v| v.as_bool()) == Some(true) {
        schema = schema.write_only();
    }
    if let Some(mime) = obj.get("contentMediaType").and_then(|v| v.as_str()) {
        schema = schema.mime_format(mime);
    }
    // Non-marker, non-email, non-uuid format → format modifier.
    const FORMAT_SKIP: &[&str] = &[
        "buffer",
        "record",
        "json",
        "jsonschema",
        "uri",
        "url",
        "email",
        "uuid",
    ];
    if let Some(fmt_str) = fmt {
        if !FORMAT_SKIP.contains(&fmt_str) {
            schema = schema.format(fmt_str);
        }
    }

    // Step 11: consumed-or-vacuous-or-refused coverage. No assertion keyword
    // may reach the `meta` step below.
    for key in obj.keys() {
        let app = match assertion_applicability(key) {
            Some(a) => a,
            None => continue,
        };
        if consumed.contains(key.as_str()) {
            continue;
        }
        if IMPORT_UNSUPPORTED_STRUCTURAL.contains(&key.as_str()) {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                format!("`{key}` is not representable in zerx"),
            )
            .at(path.to_vec()));
        }
        if let Some(s) = spec {
            if !keyword_admitted_by(app, &s) {
                // Vacuous for the declared type: ignoring it forbids nothing.
                continue;
            }
        }
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("`{key}` cannot be enforced on this node"),
        )
        .at(path.to_vec()));
    }

    // Final step: Remaining non-assertion extension keys → meta.
    for (key, val) in obj.iter() {
        if IMPORT_HANDLED.contains(&key.as_str()) || assertion_applicability(key).is_some() {
            continue;
        }
        let zv = ZerxValue::from_serialize(val).map_err(|e| e.at(path.to_vec()))?;
        schema = schema.meta(key.clone(), zv);
    }

    Ok(schema)
}

// ---------------------------------------------------------------------------
// Object reconstruction — step 11
// ---------------------------------------------------------------------------

fn import_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
    base: &mut Option<Schema>,
) -> Result<(), ZerxError> {
    let props = obj.get("properties").and_then(|v| v.as_object());
    let required: HashSet<&str> = obj
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    // A name in `required` that no `properties` entry declares is not enforced by
    // the reconstructed object: strict mode would reject it, so `{}` would pass.
    for name in &required {
        if !props.map(|p| p.contains_key(*name)).unwrap_or(false) {
            return Err(undeclared_required_error(name, path));
        }
    }

    let fields = match props {
        Some(props) => import_object_fields(props, &required, ctx, path)?,
        None => Vec::new(),
    };
    *base = Some(finish_object(fields, obj, ctx, path)?);
    Ok(())
}

#[cold]
fn undeclared_required_error(name: &str, path: &[String]) -> ZerxError {
    ZerxError::new(
        ErrorCode::IMPORT_UNSUPPORTED,
        format!("`required` names `{name}`, which is not declared in `properties`"),
    )
    .at(path.to_vec())
}

// The per-property imports, kept apart from the object reconstruction so the
// frame live across each property's recursion stays small.
fn import_object_fields(
    props: &serde_json::Map<String, serde_json::Value>,
    required: &HashSet<&str>,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Vec<(String, Schema)>, ZerxError> {
    let mut fields: Vec<(String, Schema)> = Vec::new();
    for (key, prop_val) in props.iter() {
        let field_schema = import_child(prop_val, ctx, path, "properties", Some(key.clone()))?;
        fields.push((
            key.clone(),
            import_field_presence(field_schema, key, prop_val, required, path)?,
        ));
    }
    Ok(fields)
}

// Applies `required` to an imported property schema.
fn import_field_presence(
    field_schema: Schema,
    key: &str,
    prop_val: &serde_json::Value,
    required: &HashSet<&str>,
    path: &[String],
) -> Result<Schema, ZerxError> {
    // A default fills a missing key before the required check, so a required
    // field carrying one would accept the key's absence.
    if required.contains(key) && field_schema.modifiers.default.is_some() {
        let mut prop_path = path.to_vec();
        prop_path.push("properties".to_string());
        prop_path.push(key.to_string());
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("required property `{key}` carries a `default`; a default would satisfy the missing key"),
        )
        .at(prop_path));
    }
    if !required.contains(key) && prop_val.get("default").is_none() {
        return Ok(field_schema.optional());
    }
    // Not in required but has default → non-optional (default supplies the value).
    Ok(field_schema)
}

// Builds the object from its imported fields and applies `additionalProperties`.
fn finish_object(
    fields: Vec<(String, Schema)>,
    obj: &serde_json::Map<String, serde_json::Value>,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Schema, ZerxError> {
    use crate::object;

    let mut ob = object(fields);
    match obj.get("additionalProperties") {
        Some(serde_json::Value::Bool(true)) => {
            ob = ob.passthrough();
        }
        Some(serde_json::Value::Object(map)) if map.is_empty() => {
            // The empty schema is `any()`: passthrough keeps the extras exactly.
            ob = ob.passthrough();
        }
        Some(serde_json::Value::Object(_)) => {
            // A per-extra-key value schema is not modelled on a plain object;
            // `passthrough()` would discard it.
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                "a schema-object `additionalProperties` on a plain object is not representable; use a `record`",
            )
            .at(path.to_vec()));
        }
        // `false` and absent reject unknown properties; the caller's strip_unknown
        // flag reinterprets only this outcome, not passthrough.
        Some(serde_json::Value::Bool(false)) | None => {
            if ctx.strip_unknown {
                ob = ob.strip();
            }
        }
        Some(_) => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "`additionalProperties` must be a boolean or a schema object",
            )
            .at(path.to_vec()));
        }
    }

    Ok(ob.into())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn js(schema: &Schema) -> serde_json::Value {
        schema.to_json_schema()
    }

    fn get<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        v.as_object()?.get(key)
    }

    // 1. Scalars
    #[test]
    fn scalars() {
        assert_eq!(js(&string().into()), serde_json::json!({"type": "string"}));
        assert_eq!(js(&number().into()), serde_json::json!({"type": "number"}));
        assert_eq!(
            js(&boolean().into()),
            serde_json::json!({"type": "boolean"})
        );
        assert_eq!(js(&null().into()), serde_json::json!({"type": "null"}));
        assert_eq!(js(&any().into()), serde_json::json!({}));
    }

    // 2. Enum
    #[test]
    fn enum_export() {
        let s: Schema = enumerate(["a", "b"]).into();
        assert_eq!(js(&s), serde_json::json!({"enum": ["a", "b"]}));
    }

    // 3. Literal
    #[test]
    fn literal_export() {
        let sx: Schema = literal("x").into();
        assert_eq!(js(&sx), serde_json::json!({"const": "x"}));

        let si: Schema = literal(5i64).into();
        assert_eq!(js(&si), serde_json::json!({"const": 5}));

        let sn: Schema = literal(ZerxValue::Null).into();
        assert_eq!(js(&sn), serde_json::json!({"const": null}));
    }

    // 4. String validators merge
    #[test]
    fn string_validators_merge() {
        let s: Schema = string().min(2).max(8).regex("^x").email().into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("string")));
        assert_eq!(get(&v, "minLength"), Some(&serde_json::json!(2)));
        assert_eq!(get(&v, "maxLength"), Some(&serde_json::json!(8)));
        assert_eq!(get(&v, "pattern"), Some(&serde_json::json!("^x")));
        assert_eq!(get(&v, "format"), Some(&serde_json::json!("email")));
    }

    // 5. Number validators
    #[test]
    fn number_validators() {
        let s: Schema = number().int().min(0).max(9).into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("integer")));
        // An integer node exports integral bounds as exact JSON integers,
        // whether they came from `min`/`max` (f64) or `min_bound`/`max_bound`.
        assert_eq!(get(&v, "minimum"), Some(&serde_json::json!(0)));
        assert_eq!(get(&v, "maximum"), Some(&serde_json::json!(9)));
        let exact: Schema = number().int().min_bound(0).max_bound(9).into();
        let v = js(&exact);
        assert_eq!(get(&v, "minimum"), Some(&serde_json::json!(0)));
        assert_eq!(get(&v, "maximum"), Some(&serde_json::json!(9)));
    }

    // 6. Array
    #[test]
    fn array_export() {
        let s: Schema = array(string()).min(1).max(3).into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("array")));
        assert_eq!(
            get(&v, "items"),
            Some(&serde_json::json!({"type": "string"}))
        );
        assert_eq!(get(&v, "minItems"), Some(&serde_json::json!(1)));
        assert_eq!(get(&v, "maxItems"), Some(&serde_json::json!(3)));
    }

    // 7. Tuple
    #[test]
    fn tuple_export() {
        let s: Schema = tuple([number().into(), string().into()]).into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("array")));
        assert_eq!(get(&v, "items"), Some(&serde_json::json!(false)));
        assert_eq!(get(&v, "minItems"), Some(&serde_json::json!(2)));
        assert_eq!(get(&v, "maxItems"), Some(&serde_json::json!(2)));
        let prefix = get(&v, "prefixItems").unwrap().as_array().unwrap();
        assert_eq!(prefix.len(), 2);
        assert_eq!(prefix[0], serde_json::json!({"type": "number"}));
        assert_eq!(prefix[1], serde_json::json!({"type": "string"}));
    }

    // 8. Record
    #[test]
    fn record_export() {
        let sj: Schema = record(json()).into();
        let v = js(&sj);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("object")));
        assert_eq!(get(&v, "format"), Some(&serde_json::json!("record")));
        assert_eq!(get(&v, "properties"), Some(&serde_json::json!({})));
        assert_eq!(
            get(&v, "additionalProperties"),
            Some(&serde_json::json!({"format": "json"}))
        );

        let sa: Schema = record(any()).into();
        let va = js(&sa);
        assert_eq!(
            get(&va, "additionalProperties"),
            Some(&serde_json::json!(true))
        );
    }

    // 9. Object required / additionalProperties
    #[test]
    fn object_required_and_additional() {
        // strict (default): required only for non-optional, non-defaulted fields
        let s: Schema = object([
            ("a", string().into()),
            ("b", string().optional().into()),
            ("c", string().default("x").into()),
        ])
        .into();
        let v = js(&s);
        assert_eq!(
            get(&v, "additionalProperties"),
            Some(&serde_json::json!(false))
        );
        let req = get(&v, "required").unwrap().as_array().unwrap();
        assert_eq!(req, &[serde_json::json!("a")]);
        let props = get(&v, "properties").unwrap();
        assert_eq!(props["c"]["default"], serde_json::json!("x"));

        // passthrough
        let sp: Schema = object([("a", string().into())]).passthrough().into();
        let vp = js(&sp);
        assert_eq!(
            get(&vp, "additionalProperties"),
            Some(&serde_json::json!(true))
        );

        // strip
        let ss: Schema = object([("a", string().into())]).strip().into();
        let vs = js(&ss);
        assert_eq!(
            get(&vs, "additionalProperties"),
            Some(&serde_json::json!(false))
        );

        // partial — required omitted entirely
        let sparr: Schema = object([("a", string().into()), ("b", string().into())])
            .partial()
            .into();
        let vparr = js(&sparr);
        assert!(get(&vparr, "required").is_none());
    }

    // 10. Buffer + MIME
    #[test]
    fn buffer_mime() {
        let s: Schema = buffer().mime_format("image/png").into();
        let v = js(&s);
        assert_eq!(
            v,
            serde_json::json!({"type": "string", "format": "buffer", "contentMediaType": "image/png"})
        );
    }

    // 11. uri / url / json / jsonschema markers
    #[test]
    fn special_type_markers() {
        assert_eq!(
            js(&uri().into()),
            serde_json::json!({"type": "string", "format": "uri"})
        );
        assert_eq!(
            js(&url().into()),
            serde_json::json!({"type": "string", "format": "url"})
        );
        assert_eq!(js(&json().into()), serde_json::json!({"format": "json"}));
        assert_eq!(
            js(&jsonschema().into()),
            serde_json::json!({"type": "object", "format": "jsonschema"})
        );
    }

    // 11b. User format cannot clobber markers
    #[test]
    fn user_format_cannot_clobber_markers() {
        // buffer marker takes precedence
        let sb: Schema = buffer().format("date-time").into();
        assert_eq!(get(&js(&sb), "format"), Some(&serde_json::json!("buffer")));

        // json marker takes precedence
        let sj: Schema = json().format("x").into();
        assert_eq!(get(&js(&sj), "format"), Some(&serde_json::json!("json")));

        // email validator format takes precedence
        let se: Schema = string().email().format("phone").into();
        assert_eq!(get(&js(&se), "format"), Some(&serde_json::json!("email")));

        // plain string with no marker — user format is written
        let sp: Schema = string().format("date-time").into();
        assert_eq!(
            get(&js(&sp), "format"),
            Some(&serde_json::json!("date-time"))
        );
    }

    // 12. Modifiers
    #[test]
    fn modifiers_export() {
        let s: Schema = string()
            .describe("desc")
            .deprecated()
            .read_only()
            .write_only()
            .title("Title")
            .example("ex")
            .meta("x-internal", true)
            .multiline(3)
            .into();
        let v = js(&s);
        assert_eq!(get(&v, "description"), Some(&serde_json::json!("desc")));
        assert_eq!(get(&v, "deprecated"), Some(&serde_json::json!(true)));
        assert_eq!(get(&v, "readOnly"), Some(&serde_json::json!(true)));
        assert_eq!(get(&v, "writeOnly"), Some(&serde_json::json!(true)));
        assert_eq!(get(&v, "title"), Some(&serde_json::json!("Title")));
        assert_eq!(get(&v, "examples"), Some(&serde_json::json!(["ex"])));
        assert_eq!(get(&v, "x-internal"), Some(&serde_json::json!(true)));
        assert_eq!(get(&v, "x-ui-multiline"), Some(&serde_json::json!(3u64)));
    }

    // 12b. Meta cannot corrupt the shape
    #[test]
    fn meta_cannot_corrupt_shape() {
        let s: Schema = string()
            .meta("type", "object")
            .meta("description", "evil")
            .meta("x-ok", 1i64)
            .into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("string")));
        // description was not in core before meta step, so it would be set by meta
        // BUT "description" IS in the reserved set, so it is skipped
        assert!(get(&v, "description").is_none());
        assert_eq!(get(&v, "x-ok"), Some(&serde_json::json!(1i64)));

        // $ref and properties also skipped
        let s2: Schema = string()
            .meta("$ref", "evil")
            .meta("properties", "bad")
            .into();
        let v2 = js(&s2);
        assert!(get(&v2, "$ref").is_none());
        assert!(get(&v2, "properties").is_none());
    }

    // 13. Nullable
    #[test]
    fn nullable_export() {
        let s: Schema = string().nullable().into();
        let v = js(&s);
        let any_of = get(&v, "anyOf").unwrap().as_array().unwrap();
        assert_eq!(any_of[0], serde_json::json!({"type": "string"}));
        assert_eq!(any_of[1], serde_json::json!({"type": "null"}));

        // description inside first anyOf branch
        let s2: Schema = string().describe("d").nullable().into();
        let v2 = js(&s2);
        let any_of2 = get(&v2, "anyOf").unwrap().as_array().unwrap();
        assert_eq!(any_of2[0]["description"], serde_json::json!("d"));
    }

    // 14. Discriminated union vs plain union
    #[test]
    fn union_variants() {
        let du: Schema = discriminated_union(
            "kind",
            [
                object([("kind", literal("a").into()), ("x", string().into())]).into(),
                object([("kind", literal("b").into()), ("y", number().into())]).into(),
            ],
        )
        .into();
        let v = js(&du);
        assert!(get(&v, "oneOf").is_some());
        let disc = get(&v, "discriminator").unwrap();
        assert_eq!(disc["propertyName"], serde_json::json!("kind"));

        let pu: Schema = union([string().into(), number().into()]).into();
        let vp = js(&pu);
        assert!(get(&vp, "anyOf").is_some());
        assert!(get(&vp, "oneOf").is_none());
    }

    // 15. Lazy → $defs/$ref
    #[test]
    fn lazy_defs_ref() {
        let s: Schema = lazy(|| object([("id", string().into())]).into()).into();
        let v = js(&s);
        assert_eq!(get(&v, "$ref"), Some(&serde_json::json!("#/$defs/S1")));
        let defs = get(&v, "$defs").unwrap();
        assert!(defs.get("S1").is_some());
        assert_eq!(defs["S1"]["type"], serde_json::json!("object"));
    }

    // 16. Cyclic lazy terminates
    #[test]
    fn cyclic_lazy_terminates() {
        // Build a self-referential tree:
        //   node = object([("next", lazy(|| node.clone()).optional())])
        // We use an Rc<RefCell<Option<Schema>>> as a forward reference.
        let holder: Rc<RefCell<Option<Schema>>> = Rc::new(RefCell::new(None));
        let holder_clone = holder.clone();
        let inner_lazy: Schema = lazy(move || holder_clone.borrow().clone().unwrap())
            .optional()
            .into();

        let node: Schema = object([("next", inner_lazy)]).into();
        *holder.borrow_mut() = Some(node.clone());

        // Must not loop or panic
        let v = node.to_json_schema();

        // Exactly one $defs entry
        let defs = get(&v, "$defs").unwrap().as_object().unwrap();
        assert_eq!(defs.len(), 1);

        // Extract the sole $defs id and the expected $ref string
        let def_id = defs.keys().next().unwrap().clone();
        let expected_ref = format!("#/$defs/{def_id}");

        // The $defs body's "next" property must point back to the same $ref.
        // inner_lazy is .optional() (not nullable), so the Lazy arm emits a plain
        // {"$ref": …} node. If it were nullable an anyOf wrapper would be present.
        let def_body = defs.values().next().unwrap();
        let next_raw = &def_body["properties"]["next"];
        let next_ref_val = if let Some(any_of) = next_raw.get("anyOf") {
            // nullable wrapper — first branch is the $ref node
            &any_of.as_array().unwrap()[0]
        } else {
            next_raw
        };
        assert_eq!(
            next_ref_val.get("$ref"),
            Some(&serde_json::Value::String(expected_ref))
        );
    }

    // 17. $schema option
    #[test]
    fn schema_option() {
        let s: Schema = string().into();
        let v1 = s.to_json_schema_with(&ExportOptions {
            dialect: Some(DRAFT_2020_12.to_owned()),
        });
        assert_eq!(get(&v1, "$schema"), Some(&serde_json::json!(DRAFT_2020_12)));

        let v2 = s.to_json_schema();
        assert!(get(&v2, "$schema").is_none());
    }

    // ---------------------------------------------------------------------------
    // Import tests
    // ---------------------------------------------------------------------------

    fn from(v: serde_json::Value) -> Result<Schema, ZerxError> {
        from_json_schema(&v)
    }

    fn roundtrip_str(schema: &Schema) -> (String, String) {
        let v1 = schema.to_json_schema();
        let imported = from_json_schema(&v1).expect("import failed");
        let v2 = imported.to_json_schema();
        (
            serde_json::to_string(&v1).unwrap(),
            serde_json::to_string(&v2).unwrap(),
        )
    }

    fn validate_zv(schema: &Schema, value: ZerxValue) -> Result<ZerxValue, ZerxError> {
        schema.validate(&value)
    }

    fn obj(entries: &[(&str, ZerxValue)]) -> ZerxValue {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert(*key, value.clone());
        }
        ZerxValue::Object(map)
    }

    // Scalars and empty schema.
    #[test]
    fn import_scalars() {
        assert!(from(serde_json::json!({"type":"string"})).is_ok());
        assert!(from(serde_json::json!({"type":"number"})).is_ok());
        assert!(from(serde_json::json!({"type":"boolean"})).is_ok());
        assert!(from(serde_json::json!({"type":"null"})).is_ok());
        assert!(from(serde_json::json!({})).is_ok());

        let (v1, v2) = roundtrip_str(&string().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&number().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&boolean().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&null().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&any().into());
        assert_eq!(v1, v2);
    }

    // Integer.
    #[test]
    fn import_integer() {
        let (v1, v2) = roundtrip_str(&number().int().into());
        assert_eq!(v1, v2);
        let v = serde_json::json!({"type":"integer"});
        let exported = from(v.clone()).unwrap().to_json_schema();
        assert_eq!(exported["type"], serde_json::json!("integer"));

        let (v1, v2) = roundtrip_str(&number().int().min(0.0).max(9.0).into());
        assert_eq!(v1, v2);
    }

    // String validators.
    #[test]
    fn import_string_validators() {
        let (v1, v2) = roundtrip_str(&string().min(2).max(8).regex("^x").email().into());
        assert_eq!(v1, v2);

        let input = serde_json::json!({
            "type": "string", "minLength": 2, "maxLength": 8,
            "pattern": "^x", "format": "email"
        });
        let exported = from(input).unwrap().to_json_schema();
        assert_eq!(exported["minLength"], serde_json::json!(2));
        assert_eq!(exported["maxLength"], serde_json::json!(8));
        assert_eq!(exported["pattern"], serde_json::json!("^x"));
        assert_eq!(exported["format"], serde_json::json!("email"));
    }

    // UUID and user format.
    #[test]
    fn import_uuid_and_user_format() {
        let (v1, v2) = roundtrip_str(&string().uuid().into());
        assert_eq!(v1, v2);

        let input = serde_json::json!({"type": "string", "format": "date-time"});
        let exported = from(input).unwrap().to_json_schema();
        assert_eq!(exported["format"], serde_json::json!("date-time"));
    }

    // Enum.
    #[test]
    fn import_enum() {
        let (v1, v2) = roundtrip_str(&enumerate(["a", "b"]).into());
        assert_eq!(v1, v2);

        // enum precedes type dispatch
        let input = serde_json::json!({"type": "string", "enum": ["a", "b"]});
        let exported = from(input).unwrap().to_json_schema();
        assert_eq!(exported["enum"], serde_json::json!(["a", "b"]));

        // mixed enum → literal union
        let mixed = serde_json::json!({"enum": ["a", 1]});
        let exported2 = from(mixed).unwrap().to_json_schema();
        assert!(exported2.get("anyOf").is_some());
    }

    // Const / literal.
    #[test]
    fn import_const() {
        let (v1, v2) = roundtrip_str(&literal("x").into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&literal(5i64).into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&literal(ZerxValue::Null).into());
        assert_eq!(v1, v2);
    }

    // Array and tuple.
    #[test]
    fn import_array_tuple() {
        let (v1, v2) = roundtrip_str(&array(string()).min(1).max(3).into());
        assert_eq!(v1, v2);

        let (v1, v2) = roundtrip_str(&tuple([number().into(), string().into()]).into());
        assert_eq!(v1, v2);

        // items:true and items:{} → array(any())
        let v_true = serde_json::json!({"type":"array","items":true});
        let exp_true = from(v_true).unwrap().to_json_schema();
        assert_eq!(exp_true, serde_json::json!({"type":"array","items":{}}));

        let v_empty = serde_json::json!({"type":"array","items":{}});
        let exp_empty = from(v_empty).unwrap().to_json_schema();
        assert_eq!(exp_empty["type"], serde_json::json!("array"));

        // missing items → IMPORT_MALFORMED
        let bad = serde_json::json!({"type":"array"});
        assert_eq!(from(bad).unwrap_err().code, ErrorCode::IMPORT_MALFORMED);

        // prefixItems with open tail → IMPORT_UNSUPPORTED
        let open_tail = serde_json::json!({
            "type":"array","prefixItems":[{"type":"string"}],
            "items":{"type":"number"}
        });
        assert_eq!(
            from(open_tail).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // Record.
    #[test]
    fn import_record() {
        let (v1, v2) = roundtrip_str(&record(json()).into());
        assert_eq!(v1, v2);

        // additionalProperties:true → record(any())
        let input = serde_json::json!({
            "type":"object","format":"record","properties":{},
            "additionalProperties":true
        });
        let exp = from(input).unwrap().to_json_schema();
        assert_eq!(exp["additionalProperties"], serde_json::json!(true));

        // additionalProperties:false → IMPORT_MALFORMED
        let bad = serde_json::json!({
            "type":"object","format":"record","properties":{},
            "additionalProperties":false
        });
        assert_eq!(from(bad).unwrap_err().code, ErrorCode::IMPORT_MALFORMED);
    }

    // Object required / additionalProperties / default.
    #[test]
    fn import_object_required_additional() {
        let input = serde_json::json!({
            "type":"object",
            "properties":{
                "a":{"type":"string"},
                "b":{"type":"string"},
                "c":{"type":"string","default":"x"}
            },
            "required":["a"],
            "additionalProperties":false
        });
        let exported = from(input.clone()).unwrap().to_json_schema();
        let req = exported["required"].as_array().unwrap();
        assert_eq!(req, &[serde_json::json!("a")]);
        assert_eq!(exported["additionalProperties"], serde_json::json!(false));
        assert_eq!(
            exported["properties"]["c"]["default"],
            serde_json::json!("x")
        );

        // passthrough
        let pass = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "additionalProperties":true
        });
        assert_eq!(
            from(pass).unwrap().to_json_schema()["additionalProperties"],
            serde_json::json!(true)
        );

        // absent additionalProperties → strict
        let absent = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "required":["a"]
        });
        assert_eq!(
            from(absent).unwrap().to_json_schema()["additionalProperties"],
            serde_json::json!(false)
        );

        // an empty schema object is `any()`: passthrough keeps extras exactly
        let empty_ap = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "additionalProperties":{}
        });
        let empty_schema = from(empty_ap).unwrap();
        assert_eq!(
            empty_schema.to_json_schema()["additionalProperties"],
            serde_json::json!(true)
        );
        assert!(empty_schema
            .validate(&serde_json::json!({"a":"x","extra":1}))
            .is_ok());

        // a per-extra-key value schema would be discarded by passthrough → refused
        let schema_ap = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "additionalProperties":{"type":"string"}
        });
        assert_eq!(
            from(schema_ap).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );

        // required must name a declared property
        let bad_required = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "required":["b"]
        });
        assert_eq!(
            from(bad_required).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // Buffer + MIME / special markers.
    #[test]
    fn import_special_markers() {
        let (v1, v2) = roundtrip_str(&buffer().mime_format("image/png").into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&uri().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&url().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&json().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&jsonschema().into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&record(json()).into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&record(string()).into());
        assert_eq!(v1, v2);
    }

    // Nullable collapse.
    #[test]
    fn import_nullable_collapse() {
        let (v1, v2) = roundtrip_str(&string().nullable().into());
        assert_eq!(v1, v2);

        // nullable string with description in first branch
        let (v1, v2) = roundtrip_str(&string().describe("d").nullable().into());
        assert_eq!(v1, v2);

        // null first
        let input = serde_json::json!({"anyOf":[{"type":"null"},{"type":"string"}]});
        let exported = from(input).unwrap().to_json_schema();
        let any_of = exported["anyOf"].as_array().unwrap();
        assert_eq!(any_of.len(), 2);
    }

    // Union vs discriminated union.
    #[test]
    fn import_union_discriminated() {
        // anyOf → plain union
        let (v1, v2) = roundtrip_str(&union([string().into(), number().into()]).into());
        assert_eq!(v1, v2);

        // oneOf + discriminator + valid variants → discriminated union
        let du: Schema = discriminated_union(
            "kind",
            [
                object([("kind", literal("a").into()), ("x", string().into())]).into(),
                object([("kind", literal("b").into()), ("y", number().into())]).into(),
            ],
        )
        .into();
        let (v1, v2) = roundtrip_str(&du);
        assert_eq!(v1, v2);
        assert!(
            from_json_schema(&serde_json::from_str::<serde_json::Value>(&v1).unwrap())
                .unwrap()
                .to_json_schema()
                .get("oneOf")
                .is_some()
        );

        // oneOf without discriminator → OneOf, re-exported as oneOf
        let one_of_no_disc = serde_json::json!({
            "oneOf":[{"type":"string"},{"type":"number"}]
        });
        let exported = from(one_of_no_disc).unwrap().to_json_schema();
        assert!(exported.get("oneOf").is_some());
        assert!(exported.get("anyOf").is_none());
        assert!(exported.get("x-oneOf").is_none());

        // oneOf + discriminator but variants not all objects → OneOf fallback
        let bad_du = serde_json::json!({
            "oneOf":[{"type":"string"},{"type":"number"}],
            "discriminator":{"propertyName":"kind"}
        });
        let exported2 = from(bad_du).unwrap().to_json_schema();
        assert!(exported2.get("oneOf").is_some());
        assert!(exported2.get("x-oneOf").is_none());

        // oneOf + discriminator + objects but discriminator field is plain string (not literal)
        let bad_du2 = serde_json::json!({
            "oneOf":[
                {"type":"object","properties":{"kind":{"type":"string"}},"required":["kind"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"type":"string"}},"required":["kind"],"additionalProperties":false}
            ],
            "discriminator":{"propertyName":"kind"}
        });
        let exported3 = from(bad_du2).unwrap().to_json_schema();
        assert!(exported3.get("oneOf").is_some());
        assert!(exported3.get("x-oneOf").is_none());
    }

    // Literal value equality across numeric variants.
    #[test]
    fn import_literal_accepts_equal_numeric_values() {
        let const_one = from(serde_json::json!({"const": 1})).unwrap();
        for value in [ZerxValue::I64(1), ZerxValue::U64(1), ZerxValue::F64(1.0)] {
            assert!(
                validate_zv(&const_one, value.clone()).is_ok(),
                "const 1 rejected {value:?}"
            );
        }

        let enum_num = from(serde_json::json!({"enum": [1, 2]})).unwrap();
        for value in [ZerxValue::I64(2), ZerxValue::U64(2), ZerxValue::F64(2.0)] {
            assert!(validate_zv(&enum_num, value).is_ok());
        }

        let const_float = from(serde_json::json!({"const": 1.0})).unwrap();
        for value in [ZerxValue::I64(1), ZerxValue::U64(1), ZerxValue::F64(1.0)] {
            assert!(validate_zv(&const_float, value).is_ok());
        }

        assert!(validate_zv(
            &from(serde_json::json!({"const": -1})).unwrap(),
            ZerxValue::I64(-1)
        )
        .is_ok());
        assert!(validate_zv(
            &from(serde_json::json!({"const": 1.5})).unwrap(),
            ZerxValue::F64(1.5)
        )
        .is_ok());
    }

    #[test]
    fn import_literal_rejects_unequal_numeric_values() {
        let const_one = from(serde_json::json!({"const": 1})).unwrap();
        for value in [
            ZerxValue::I64(2),
            ZerxValue::U64(2),
            ZerxValue::F64(1.5),
            ZerxValue::F64(f64::NAN),
        ] {
            let err = validate_zv(&const_one, value).unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_LITERAL);
        }

        let enum_num = from(serde_json::json!({"enum": [1, 2]})).unwrap();
        let err = validate_zv(&enum_num, ZerxValue::I64(3)).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNION_MISMATCH);
    }

    #[test]
    fn import_literal_keeps_integer_precision() {
        let schema = from(serde_json::json!({"const": 9007199254740993u64})).unwrap();
        assert!(validate_zv(&schema, ZerxValue::F64(9007199254740992.0)).is_err());
        assert!(validate_zv(&schema, ZerxValue::U64(9007199254740993)).is_ok());
    }

    #[test]
    fn import_literal_nested_value_equality() {
        let object_const = from(serde_json::json!({"const": {"v": 1}})).unwrap();
        for value in [
            obj(&[("v", ZerxValue::I64(1))]),
            obj(&[("v", ZerxValue::U64(1))]),
            obj(&[("v", ZerxValue::F64(1.0))]),
        ] {
            assert!(validate_zv(&object_const, value).is_ok());
        }
        assert!(validate_zv(&object_const, obj(&[("v", ZerxValue::I64(2))])).is_err());
        assert!(validate_zv(&object_const, obj(&[])).is_err());
        assert!(validate_zv(
            &object_const,
            obj(&[("v", ZerxValue::I64(1)), ("x", ZerxValue::I64(0))])
        )
        .is_err());

        let array_const = from(serde_json::json!({"const": [1, {"v": 2.0}]})).unwrap();
        for value in [
            ZerxValue::Array(vec![ZerxValue::I64(1), obj(&[("v", ZerxValue::I64(2))])]),
            ZerxValue::Array(vec![
                ZerxValue::F64(1.0),
                obj(&[("v", ZerxValue::F64(2.0))]),
            ]),
            ZerxValue::Array(vec![ZerxValue::U64(1), obj(&[("v", ZerxValue::U64(2))])]),
        ] {
            assert!(validate_zv(&array_const, value).is_ok());
        }
        // Unequal element, different length, and reordered array all stay strict.
        assert!(validate_zv(
            &array_const,
            ZerxValue::Array(vec![ZerxValue::I64(1), obj(&[("v", ZerxValue::I64(3))])])
        )
        .is_err());
        assert!(validate_zv(&array_const, ZerxValue::Array(vec![ZerxValue::I64(1)])).is_err());
        assert!(validate_zv(
            &array_const,
            ZerxValue::Array(vec![obj(&[("v", ZerxValue::I64(2))]), ZerxValue::I64(1)])
        )
        .is_err());
    }

    #[test]
    fn import_literal_non_numbers_stay_strict() {
        let const_one = from(serde_json::json!({"const": 1})).unwrap();
        for value in [
            ZerxValue::String("1".to_string()),
            ZerxValue::Bool(true),
            ZerxValue::Null,
            ZerxValue::Bytes(vec![1]),
            ZerxValue::Array(vec![ZerxValue::I64(1)]),
        ] {
            assert!(validate_zv(&const_one, value).is_err());
        }

        let const_x = from(serde_json::json!({"const": "x"})).unwrap();
        assert!(validate_zv(&const_x, ZerxValue::Bytes(b"x".to_vec())).is_err());
    }

    #[test]
    fn import_literal_rejection_error_unchanged() {
        let schema = from(serde_json::json!({"const": 1})).unwrap();
        let err = validate_zv(&schema, ZerxValue::I64(2)).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_LITERAL);
        assert_eq!(err.message, "value does not match literal constant");
        assert_eq!(err.expected.as_deref(), Some("1"));
        assert_eq!(err.received.as_deref(), Some("number"));

        let nested =
            from(serde_json::json!({"const": {"name": "SECRET_ARGUMENT", "n": 1}})).unwrap();
        let err = validate_zv(
            &nested,
            obj(&[
                ("name", ZerxValue::String("SECRET_ARGUMENT".to_string())),
                ("n", ZerxValue::I64(2)),
            ]),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_LITERAL);
        assert!(!err.to_json().to_string().contains("SECRET_ARGUMENT"));
    }

    #[test]
    fn import_literal_preserves_input_form() {
        let const_one = from(serde_json::json!({"const": 1})).unwrap();
        assert_eq!(
            validate_zv(&const_one, ZerxValue::I64(1)).unwrap(),
            ZerxValue::I64(1)
        );
        assert_eq!(
            validate_zv(&const_one, ZerxValue::F64(1.0)).unwrap(),
            ZerxValue::F64(1.0)
        );

        let object_const = from(serde_json::json!({"const": {"a": 1, "b": 2}})).unwrap();
        let input = obj(&[("b", ZerxValue::I64(2)), ("a", ZerxValue::I64(1))]);
        let output = validate_zv(&object_const, input).unwrap();
        let keys: Vec<&str> = output
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, vec!["b", "a"]);
    }

    #[test]
    fn import_literal_export_unchanged() {
        assert_eq!(
            js(&from(serde_json::json!({"const": 1})).unwrap()),
            serde_json::json!({"const": 1})
        );
        assert_eq!(
            js(&from(serde_json::json!({"const": 1.0})).unwrap()),
            serde_json::json!({"const": 1.0})
        );
        assert_eq!(js(&literal(5i64).into()), serde_json::json!({"const": 5}));

        let (v1, v2) = roundtrip_str(&from(serde_json::json!({"const": 1})).unwrap());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&from(serde_json::json!({"const": 1.0})).unwrap());
        assert_eq!(v1, v2);
    }

    #[test]
    fn import_discriminated_union_accepts_integral_float_discriminant() {
        let schema = from(serde_json::json!({
            "oneOf": [
                {"type":"object","properties":{"k":{"const":1},"a":{"type":"string"}},"required":["k","a"],"additionalProperties":false},
                {"type":"object","properties":{"k":{"const":2},"b":{"type":"string"}},"required":["k","b"],"additionalProperties":false}
            ],
            "discriminator": {"propertyName": "k"}
        }))
        .unwrap();
        assert!(js(&schema).get("oneOf").is_some());

        for (k, expected) in [
            (ZerxValue::I64(1), ZerxValue::I64(1)),
            (ZerxValue::U64(1), ZerxValue::U64(1)),
            (ZerxValue::F64(1.0), ZerxValue::F64(1.0)),
        ] {
            let out = validate_zv(
                &schema,
                obj(&[("k", k), ("a", ZerxValue::String("x".to_string()))]),
            )
            .unwrap();
            assert_eq!(out.as_object().unwrap().get("k"), Some(&expected));
        }

        for k in [ZerxValue::F64(3.0), ZerxValue::F64(1.5)] {
            let err = validate_zv(
                &schema,
                obj(&[("k", k), ("a", ZerxValue::String("x".to_string()))]),
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_DISCRIMINANT);
        }
    }

    #[test]
    fn import_discriminated_union_float_literals_fall_back_to_one_of() {
        let schema = from(serde_json::json!({
            "oneOf": [
                {"type":"object","properties":{"k":{"const":1.0},"a":{"type":"string"}},"required":["k","a"],"additionalProperties":false},
                {"type":"object","properties":{"k":{"const":2.0},"b":{"type":"string"}},"required":["k","b"],"additionalProperties":false}
            ],
            "discriminator": {"propertyName": "k"}
        }))
        .unwrap();
        let exported = js(&schema);
        assert!(exported.get("oneOf").is_some());
        assert!(exported.get("anyOf").is_none());
        assert!(exported.get("x-oneOf").is_none());
        assert!(exported.get("discriminator").is_none());

        assert!(validate_zv(
            &schema,
            obj(&[
                ("k", ZerxValue::I64(1)),
                ("a", ZerxValue::String("x".to_string()))
            ])
        )
        .is_ok());
        assert!(validate_zv(
            &schema,
            obj(&[
                ("k", ZerxValue::F64(1.0)),
                ("a", ZerxValue::String("x".to_string()))
            ])
        )
        .is_ok());
    }

    // Modifiers round-trip.
    #[test]
    fn import_modifiers_roundtrip() {
        let s: Schema = string()
            .describe("desc")
            .title("T")
            .deprecated()
            .read_only()
            .write_only()
            .example("ex")
            .meta("x-internal", true)
            .multiline(3)
            .into();
        let (v1, v2) = roundtrip_str(&s);
        assert_eq!(v1, v2);

        let v1_val: serde_json::Value = serde_json::from_str(&v1).unwrap();
        assert_eq!(v1_val["description"], serde_json::json!("desc"));
        assert_eq!(v1_val["title"], serde_json::json!("T"));
        assert_eq!(v1_val["deprecated"], serde_json::json!(true));
        assert_eq!(v1_val["readOnly"], serde_json::json!(true));
        assert_eq!(v1_val["writeOnly"], serde_json::json!(true));
        assert_eq!(v1_val["examples"], serde_json::json!(["ex"]));
        assert_eq!(v1_val["x-internal"], serde_json::json!(true));
        assert_eq!(v1_val["x-ui-multiline"], serde_json::json!(3u64));
    }

    // allOf / not / false / unsupported applicators.
    #[test]
    fn import_unsupported() {
        assert_eq!(
            from(serde_json::json!({"allOf":[{"type":"string"}]}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            from(serde_json::json!({"not":{"type":"string"}}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            from(serde_json::Value::Bool(false)).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        // unsupported applicator via step-13 classification
        assert_eq!(
            from(serde_json::json!({"type":"object","if":{},"then":{}}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            from(serde_json::json!({"type":"object","patternProperties":{"^x":{"type":"string"}}}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // $ref / $defs.
    #[test]
    fn import_ref_defs() {
        let input = serde_json::json!({
            "$ref": "#/$defs/S1",
            "$defs": {
                "S1": {
                    "type": "object",
                    "properties": {"id": {"type": "string"}},
                    "required": ["id"],
                    "additionalProperties": false
                }
            }
        });
        let imported = from(input).unwrap();
        let exported = imported.to_json_schema();
        assert!(exported.get("$ref").is_some());
        assert!(exported.get("$defs").is_some());

        // $ref to missing id → IMPORT_MALFORMED
        let bad = serde_json::json!({"$ref": "#/$defs/Missing"});
        assert_eq!(from(bad).unwrap_err().code, ErrorCode::IMPORT_MALFORMED);

        // non-local $ref → IMPORT_MALFORMED
        let non_local = serde_json::json!({"$ref": "http://example.com/schema"});
        assert_eq!(
            from(non_local).unwrap_err().code,
            ErrorCode::IMPORT_MALFORMED
        );
    }

    // Cyclic $ref terminates and re-exports to one $defs entry.
    #[test]
    fn import_cyclic_ref() {
        // Build a self-referential schema and export it first.
        let holder: Rc<RefCell<Option<Schema>>> = Rc::new(RefCell::new(None));
        let holder_clone = holder.clone();
        let inner_lazy: Schema = lazy(move || holder_clone.borrow().clone().unwrap())
            .optional()
            .into();
        let node: Schema = object([("next", inner_lazy)]).into();
        *holder.borrow_mut() = Some(node.clone());
        let exported = node.to_json_schema();

        // Import must terminate.
        let imported = from_json_schema(&exported).expect("import of cyclic schema failed");

        // Re-export must produce exactly one $defs entry.
        let re_exported = imported.to_json_schema();
        let defs = re_exported.get("$defs").unwrap().as_object().unwrap();
        assert_eq!(defs.len(), 1, "expected exactly one $defs entry");

        // The single $defs body's properties must contain a back-reference.
        let def_body = defs.values().next().unwrap();
        assert!(def_body.get("properties").is_some());
    }

    // Full roundtrip — the vision `message` schema (mlua call field omitted).
    #[test]
    fn import_message_schema_roundtrip() {
        let address: Schema = object([
            (
                "geo",
                tuple([number().into(), number().into()]).optional().into(),
            ),
            ("street", string().min(1).into()),
            ("zip", string().regex(r"^\d{5}$").into()),
        ])
        .into();

        let message: Schema = object([
            (
                "attachment",
                union([buffer().into(), url().into()]).optional().into(),
            ),
            (
                "avatar",
                buffer().mime_format("image/png").optional().into(),
            ),
            (
                "content",
                string().min(1).max(8192).describe("message body").into(),
            ),
            ("id", string().uuid().into()),
            (
                "legacy_id",
                string().deprecated().read_only().optional().into(),
            ),
            (
                "metadata",
                record(json())
                    .default(ZerxValue::from_serialize(&serde_json::json!({})).unwrap())
                    .into(),
            ),
            ("origin", address.optional()),
            (
                "priority",
                number()
                    .int()
                    .min(0.0)
                    .max(9.0)
                    .default(5i64)
                    .meta("x-internal", true)
                    .into(),
            ),
            (
                "role",
                enumerate(["user", "assistant", "system"])
                    .describe("who sent it")
                    .into(),
            ),
            (
                "source",
                discriminated_union(
                    "kind",
                    [
                        object([
                            ("kind", literal("human").into()),
                            ("session", string().into()),
                        ])
                        .into(),
                        object([("kind", literal("tool").into()), ("tool", string().into())])
                            .into(),
                    ],
                )
                .into(),
            ),
            ("tags", array(string().min(1)).max(10).nullable().into()),
        ])
        .into();

        let (v1, v2) = roundtrip_str(&message);
        assert_eq!(v1, v2, "message schema roundtrip not byte-identical");
    }

    // Error path determinism.
    #[test]
    fn import_error_path() {
        let input = serde_json::json!({
            "type": "object",
            "properties": {
                "x": {"type": "array"}
            },
            "required": ["x"],
            "additionalProperties": false
        });
        let err = from(input).unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_MALFORMED);
        assert!(
            !err.path.is_empty(),
            "error path should point to offending node"
        );
    }

    // ---------------------------------------------------------------------------
    // Strip-unknown import tests
    // ---------------------------------------------------------------------------

    // Strip applies at the root.
    #[test]
    fn j3_strip_root_object() {
        let input = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "required":["a"],
            "additionalProperties":false
        });
        let value = serde_json::json!({"a":"x","stale":1});

        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema.validate(&value).unwrap();
        let obj = ok.as_object().unwrap();
        assert!(obj.get("a").is_some());
        assert!(obj.get("stale").is_none());
        assert_eq!(obj.len(), 1);

        let strict = from_json_schema_inner(&input, false).unwrap();
        let err = strict.validate(&value).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
    }

    // Strip applies to nested objects (regression guard for the recursive decision).
    #[test]
    fn j3_strip_nested_object() {
        let input = serde_json::json!({
            "type":"object",
            "properties":{
                "inner":{
                    "type":"object",
                    "properties":{"b":{"type":"string"}},
                    "required":["b"],
                    "additionalProperties":false
                }
            },
            "required":["inner"],
            "additionalProperties":false
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"inner":{"b":"x","stale":1}}))
            .unwrap();
        let inner = ok
            .as_object()
            .unwrap()
            .get("inner")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(inner.get("b").is_some());
        assert!(inner.get("stale").is_none());
    }

    // Absent additionalProperties also strips.
    #[test]
    fn j3_strip_absent_additional_properties() {
        let input = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "required":["a"]
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"a":"x","stale":1}))
            .unwrap();
        assert!(ok.as_object().unwrap().get("stale").is_none());
    }

    // additionalProperties:true and {} stay passthrough under strip; a
    // non-empty schema object is refused by the core import.
    #[test]
    fn j3_strip_passthrough_untouched() {
        let bool_true = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "additionalProperties":true
        });
        let schema = from_json_schema_inner(&bool_true, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"a":"x","extra":1}))
            .unwrap();
        assert!(ok.as_object().unwrap().get("extra").is_some());

        let empty_ap = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "additionalProperties":{}
        });
        let schema_empty = from_json_schema_inner(&empty_ap, true).unwrap();
        let ok_empty = schema_empty
            .validate(&serde_json::json!({"a":"x","extra":1}))
            .unwrap();
        assert!(ok_empty.as_object().unwrap().get("extra").is_some());

        let schema_ap = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "additionalProperties":{"type":"string"}
        });
        assert_eq!(
            from_json_schema_inner(&schema_ap, true).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // Union first-match-wins under strip — pins the accepted consequence.
    #[test]
    fn j3_strip_union_first_match_wins() {
        let input = serde_json::json!({
            "anyOf": [
                {
                    "type":"object",
                    "properties":{"a":{"type":"string"}},
                    "required":["a"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{"a":{"type":"string"},"b":{"type":"string"}},
                    "required":["a","b"],
                    "additionalProperties":false
                }
            ]
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"a":"x","b":"y"}))
            .unwrap();
        let obj = ok.as_object().unwrap();
        assert!(obj.get("a").is_some());
        assert!(
            obj.get("b").is_none(),
            "first variant matched under strip and dropped the second variant's field"
        );
    }

    // Discriminated union still builds under strip; dispatch still works.
    #[test]
    fn j3_strip_discriminated_union_builds() {
        let du: Schema = discriminated_union(
            "kind",
            [
                object([("kind", literal("a").into()), ("x", string().into())]).into(),
                object([("kind", literal("b").into()), ("y", number().into())]).into(),
            ],
        )
        .into();
        let exported = du.to_json_schema();

        let schema = from_json_schema_inner(&exported, true).unwrap();
        assert!(schema.to_json_schema().get("oneOf").is_some());

        let ok = schema
            .validate(&serde_json::json!({"kind":"a","x":"hi","stale":1}))
            .unwrap();
        let obj = ok.as_object().unwrap();
        assert!(obj.get("stale").is_none());
        assert_eq!(obj.get("x").and_then(|v| v.as_str()), Some("hi"));
    }

    // `record` is unaffected by strip_unknown.
    #[test]
    fn j3_strip_record_unaffected() {
        let input = serde_json::json!({
            "type":"object","format":"record",
            "additionalProperties":{"type":"string"}
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"a":"x","b":"y"}))
            .unwrap();
        assert_eq!(ok.as_object().unwrap().len(), 2);

        let bad = serde_json::json!({
            "type":"object","format":"record","additionalProperties":false
        });
        let err = from_json_schema_inner(&bad, true).unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_MALFORMED);
    }

    // $ref/$defs reached through a lazy node strip too (regression guard for decision 5).
    #[test]
    fn j3_strip_reaches_through_ref() {
        let input = serde_json::json!({
            "$ref": "#/$defs/S1",
            "$defs": {
                "S1": {
                    "type":"object",
                    "properties":{"a":{"type":"string"}},
                    "required":["a"],
                    "additionalProperties":false
                }
            }
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        let ok = schema
            .validate(&serde_json::json!({"a":"x","stale":1}))
            .unwrap();
        assert!(ok.as_object().unwrap().get("stale").is_none());
    }

    // Export is unchanged — the option does not leak into the document.
    #[test]
    fn j3_strip_export_unchanged() {
        let input = serde_json::json!({
            "type":"object",
            "properties":{"a":{"type":"string"}},
            "required":["a"],
            "additionalProperties":false
        });
        let schema = from_json_schema_inner(&input, true).unwrap();
        assert_eq!(
            schema.to_json_schema()["additionalProperties"],
            serde_json::json!(false)
        );
    }

    // 18. Infallibility / determinism
    #[test]
    fn infallible_and_deterministic() {
        // Uses two distinct lazy subschemas to exercise stable $defs id allocation
        // (S1 for the first-encountered lazy, S2 for the second) across two exports.
        let lzy1: Schema = lazy(|| string().into()).into();
        let lzy2: Schema = lazy(|| number().into()).into();
        let s: Schema = object([("a", lzy1), ("b", lzy2)]).into();

        let v1 = s.to_json_schema();
        let v2 = s.to_json_schema();

        // $defs must be present
        let defs = get(&v1, "$defs").unwrap().as_object().unwrap();
        assert_eq!(defs.len(), 2);

        // Stable id ordering: S1 for "a" (first encountered), S2 for "b"
        assert_eq!(defs["S1"], serde_json::json!({"type": "string"}));
        assert_eq!(defs["S2"], serde_json::json!({"type": "number"}));

        // Byte-identical output across two exports confirms allocation stability
        assert_eq!(
            serde_json::to_string(&v1).unwrap(),
            serde_json::to_string(&v2).unwrap()
        );
    }

    // ---------------------------------------------------------------------------
    // `type` import and nullable lazy
    // ---------------------------------------------------------------------------

    fn s_defs() -> serde_json::Value {
        serde_json::json!({
            "S": {
                "type": "object",
                "properties": {"format": {"type": "string"}},
                "required": ["format"],
                "additionalProperties": false
            }
        })
    }

    fn code_of(v: serde_json::Value) -> ErrorCode {
        from(v).unwrap_err().code
    }

    // Nullable `$ref` keeps admitting null through the collapse.
    #[test]
    fn import_nullable_ref_lazy() {
        let schema = from(serde_json::json!({
            "type": "object",
            "properties": {"sig": {"anyOf": [{"$ref": "#/$defs/S"}, {"type": "null"}]}},
            "additionalProperties": false,
            "$defs": s_defs()
        }))
        .unwrap();

        // Explicit null accepted.
        assert!(validate_zv(&schema, obj(&[("sig", ZerxValue::Null)])).is_ok());
        // Omitted, not required, no default → absent from output.
        let out = validate_zv(&schema, obj(&[])).unwrap();
        assert!(out.as_object().unwrap().get("sig").is_none());
        // Wrong content still fails.
        assert!(validate_zv(
            &schema,
            obj(&[("sig", obj(&[("format", ZerxValue::I64(1))]))])
        )
        .is_err());
        assert!(validate_zv(&schema, obj(&[("sig", ZerxValue::I64(5))])).is_err());

        // Required, omitted, no default → REQUIRED.
        let required = from(serde_json::json!({
            "type": "object",
            "properties": {"sig": {"anyOf": [{"$ref": "#/$defs/S"}, {"type": "null"}]}},
            "required": ["sig"],
            "additionalProperties": false,
            "$defs": s_defs()
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&required, obj(&[])).unwrap_err().code,
            ErrorCode::REQUIRED
        );

        // default:null on the property node materialises Null.
        let defaulted = from(serde_json::json!({
            "type": "object",
            "properties": {
                "sig": {"anyOf": [{"$ref": "#/$defs/S"}, {"type": "null"}], "default": null}
            },
            "additionalProperties": false,
            "$defs": s_defs()
        }))
        .unwrap();
        let out = validate_zv(&defaulted, obj(&[])).unwrap();
        assert_eq!(out.as_object().unwrap().get("sig"), Some(&ZerxValue::Null));

        // The root nullable `$ref` accepts null and a valid S.
        let root = from(serde_json::json!({
            "anyOf": [{"$ref": "#/$defs/S"}, {"type": "null"}],
            "$defs": s_defs()
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&root, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert!(validate_zv(
            &root,
            obj(&[("format", ZerxValue::String("a".to_string()))])
        )
        .is_ok());

        // A non-nullable `$ref` refuses null.
        let plain_ref = from(serde_json::json!({
            "$ref": "#/$defs/S",
            "$defs": s_defs()
        }))
        .unwrap();
        assert!(validate_zv(&plain_ref, ZerxValue::Null).is_err());

        // A definition whose own type array admits null accepts null.
        let nullable_def = from(serde_json::json!({
            "$ref": "#/$defs/S2",
            "$defs": {"S2": {"type": ["object", "null"], "properties": {}}}
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&nullable_def, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
    }

    // Export → import → validate keeps accepting null for a nullable lazy.
    #[test]
    fn import_nullable_lazy_roundtrip_accepts_null() {
        let s: Schema = lazy(|| object([("format", string().into())]).into())
            .nullable()
            .into();
        let exported = s.to_json_schema();
        let reimported = from(exported).unwrap();
        assert_eq!(
            validate_zv(&reimported, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert!(validate_zv(
            &reimported,
            obj(&[("format", ZerxValue::String("a".to_string()))])
        )
        .is_ok());
    }

    // `[T, "null"]` imports as T plus nullable, keeping sibling constraints.
    #[test]
    fn import_type_array_nullable() {
        for schema_json in [
            serde_json::json!({"type": ["string", "null"]}),
            serde_json::json!({"type": ["null", "string"]}),
        ] {
            let schema = from(schema_json).unwrap();
            assert!(validate_zv(&schema, ZerxValue::String("a".to_string())).is_ok());
            assert_eq!(
                validate_zv(&schema, ZerxValue::Null).unwrap(),
                ZerxValue::Null
            );
            assert_eq!(
                validate_zv(&schema, ZerxValue::I64(5)).unwrap_err().code,
                ErrorCode::TYPE_MISMATCH
            );
        }

        let integer = from(serde_json::json!({"type": ["integer", "null"]})).unwrap();
        assert!(validate_zv(&integer, ZerxValue::I64(3)).is_ok());
        assert_eq!(
            validate_zv(&integer, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert_eq!(
            validate_zv(&integer, ZerxValue::String("x".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::TYPE_MISMATCH
        );
        assert!(validate_zv(&integer, ZerxValue::F64(1.5)).is_err());

        // Sibling constraints survive.
        let min_len =
            from(serde_json::json!({"type": ["string", "null"], "minLength": 2})).unwrap();
        assert!(validate_zv(&min_len, ZerxValue::String("a".to_string())).is_err());
        assert_eq!(
            validate_zv(&min_len, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );

        let array_items = from(serde_json::json!({
            "type": ["array", "null"], "items": {"type": "string"}
        }))
        .unwrap();
        assert!(validate_zv(&array_items, ZerxValue::Array(vec![ZerxValue::I64(1)])).is_err());
        assert_eq!(
            validate_zv(&array_items, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );

        let strict_obj = from(serde_json::json!({
            "type": ["object", "null"],
            "properties": {"a": {"type": "string"}},
            "additionalProperties": false
        }))
        .unwrap();
        assert!(validate_zv(&strict_obj, obj(&[("b", ZerxValue::I64(1))])).is_err());
        assert_eq!(
            validate_zv(&strict_obj, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );

        let nullable_buffer =
            from(serde_json::json!({"type": ["string", "null"], "format": "buffer"})).unwrap();
        assert_eq!(
            validate_zv(&nullable_buffer, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert_eq!(
            js(&nullable_buffer)["anyOf"][0]["format"],
            serde_json::json!("buffer")
        );

        // Absent type still imports as any().
        let any_schema = from(serde_json::json!({})).unwrap();
        assert!(validate_zv(&any_schema, ZerxValue::I64(5)).is_ok());
    }

    // Unknown and malformed `type` forms are refused, with a node path.
    #[test]
    fn import_type_malformed_and_unsupported() {
        assert_eq!(
            code_of(serde_json::json!({"type": "str"})),
            ErrorCode::IMPORT_MALFORMED
        );

        let err = from(serde_json::json!({
            "type": "object",
            "properties": {"display_name": {"type": "strin"}}
        }))
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_MALFORMED);
        assert!(
            err.path.iter().any(|p| p == "display_name"),
            "path {:?}",
            err.path
        );

        for bad in [
            serde_json::json!({"type": 5}),
            serde_json::json!({"type": null}),
            serde_json::json!({"type": {}}),
            serde_json::json!({"type": true}),
            serde_json::json!({"type": []}),
            serde_json::json!({"type": ["string", 5]}),
            serde_json::json!({"type": ["string", "string"]}),
            serde_json::json!({"type": ["str", "null"]}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_MALFORMED,
                "input {bad}"
            );
        }

        for unsupported in [
            serde_json::json!({"type": ["string"]}),
            serde_json::json!({"type": ["null"]}),
            serde_json::json!({"type": ["string", "number"]}),
            serde_json::json!({"type": ["string", "number", "null"]}),
        ] {
            assert_eq!(
                code_of(unsupported.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {unsupported}"
            );
        }
    }

    // `enum`/`const` members must lie inside the declared type.
    #[test]
    fn import_enum_const_type_membership() {
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "enum": [5]})),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "const": 5})),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            code_of(serde_json::json!({"type": "integer", "enum": [1, 2.5]})),
            ErrorCode::IMPORT_UNSUPPORTED
        );

        let integral = from(serde_json::json!({"type": "integer", "enum": [1, 2.0]})).unwrap();
        assert!(validate_zv(&integral, ZerxValue::I64(2)).is_ok());

        let number_const = from(serde_json::json!({"type": "number", "const": 3})).unwrap();
        assert!(validate_zv(&number_const, ZerxValue::I64(3)).is_ok());

        for schema_json in [
            serde_json::json!({"type": ["string", "null"], "enum": ["a"]}),
            serde_json::json!({"type": ["string", "null"], "const": "a"}),
        ] {
            let schema = from(schema_json).unwrap();
            assert!(validate_zv(&schema, ZerxValue::String("a".to_string())).is_ok());
            assert!(validate_zv(&schema, ZerxValue::Null).is_err());
        }

        let with_null =
            from(serde_json::json!({"type": ["string", "null"], "enum": ["a", null]})).unwrap();
        assert_eq!(
            validate_zv(&with_null, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
    }

    // Sibling assertions next to `enum`/`const` are evaluated statically: a
    // satisfied keyword is redundant, a violated or non-evaluable one refuses.
    #[test]
    fn import_enum_const_static_evaluation() {
        for bad in [
            serde_json::json!({"type": "string", "enum": ["a", "bb"], "minLength": 2}),
            serde_json::json!({"type": "object", "const": {}, "required": ["x"]}),
            serde_json::json!({"const": {"a": 1}, "properties": {"a": {}}}),
            serde_json::json!({"enum": ["a"], "const": "b"}),
            serde_json::json!({"enum": [1, 2], "exclusiveMinimum": 1}),
            serde_json::json!({"enum": [[1, 1]], "uniqueItems": true}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }

        // A sibling whose value is malformed is IMPORT_MALFORMED.
        assert_eq!(
            code_of(serde_json::json!({"enum": ["a"], "pattern": "("})),
            ErrorCode::IMPORT_MALFORMED
        );

        let min_len = from(serde_json::json!({
            "type": "string", "enum": ["aa", "bbb"], "minLength": 2
        }))
        .unwrap();
        assert!(validate_zv(&min_len, ZerxValue::String("aa".to_string())).is_ok());

        // The pattern is vacuous for the numeric member.
        let mixed = from(serde_json::json!({"enum": ["ab", 5], "pattern": "^a"})).unwrap();
        assert!(validate_zv(&mixed, ZerxValue::String("ab".to_string())).is_ok());
        assert!(validate_zv(&mixed, ZerxValue::I64(5)).is_ok());

        let exclusive = from(serde_json::json!({"enum": [1, 2], "exclusiveMinimum": 0})).unwrap();
        assert!(validate_zv(&exclusive, ZerxValue::I64(1)).is_ok());

        let unique = from(serde_json::json!({"enum": [[1, 2]], "uniqueItems": true})).unwrap();
        assert!(validate_zv(
            &unique,
            ZerxValue::Array(vec![ZerxValue::I64(1), ZerxValue::I64(2)])
        )
        .is_ok());

        let const_required =
            from(serde_json::json!({"const": {"a": 1}, "required": ["a"]})).unwrap();
        assert!(validate_zv(&const_required, obj(&[("a", ZerxValue::I64(1))])).is_ok());

        // enum + const: the const is the intersection and becomes the literal.
        let narrowed = from(serde_json::json!({"enum": ["a", "b"], "const": "a"})).unwrap();
        assert!(validate_zv(&narrowed, ZerxValue::String("a".to_string())).is_ok());
        assert!(validate_zv(&narrowed, ZerxValue::String("b".to_string())).is_err());
        assert!(
            from(serde_json::json!({"type": "string", "enum": ["a", "b"], "const": "a"})).is_ok()
        );

        // Redundant satisfied siblings no longer refuse the import.
        assert!(from(serde_json::json!({"enum": ["a"], "pattern": "^a$"})).is_ok());
        assert!(from(serde_json::json!({
            "type": ["string", "null"], "const": "a", "maxLength": 1
        }))
        .is_ok());
        assert!(from(serde_json::json!({"type": "number", "enum": [1, 2], "minimum": 0})).is_ok());

        // Annotations and format are not validation constraints.
        assert!(from(serde_json::json!({
            "type": "string", "enum": ["a"], "description": "d", "default": "a"
        }))
        .is_ok());
        assert!(
            from(serde_json::json!({"type": "string", "enum": ["a"], "format": "email"})).is_ok()
        );
    }

    // An imported `jsonschema` marker rejects non-objects.
    #[test]
    fn import_jsonschema_marker_rejects_non_objects() {
        let schema = from(serde_json::json!({"type": "object", "format": "jsonschema"})).unwrap();
        assert!(validate_zv(&schema, obj(&[])).is_ok());
        assert!(validate_zv(&schema, obj(&[("a", ZerxValue::I64(1))])).is_ok());
        assert_eq!(
            validate_zv(&schema, ZerxValue::I64(5)).unwrap_err().code,
            ErrorCode::TYPE_MISMATCH
        );
        assert_eq!(
            validate_zv(&schema, ZerxValue::String("x".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::TYPE_MISMATCH
        );

        let nullable =
            from(serde_json::json!({"type": ["object", "null"], "format": "jsonschema"})).unwrap();
        assert_eq!(
            validate_zv(&nullable, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert_eq!(
            validate_zv(&nullable, ZerxValue::I64(5)).unwrap_err().code,
            ErrorCode::TYPE_MISMATCH
        );

        // Programmatic jsonschema(): export → import → export is unchanged.
        let original: Schema = jsonschema().into();
        let exported = js(&original);
        let reimported = from(exported.clone()).unwrap();
        assert_eq!(js(&reimported), exported);
        assert_eq!(
            validate_zv(&reimported, ZerxValue::I64(5))
                .unwrap_err()
                .code,
            ErrorCode::TYPE_MISMATCH
        );
    }

    // `type` next to `$ref` and next to `format:"json"` is refused.
    #[test]
    fn import_type_with_ref_or_json_format_refused() {
        assert_eq!(
            code_of(serde_json::json!({
                "$ref": "#/$defs/S", "type": "string", "$defs": {"S": {}}
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "format": "json"})),
            ErrorCode::IMPORT_UNSUPPORTED
        );

        let json_schema = from(serde_json::json!({"format": "json"})).unwrap();
        assert!(validate_zv(&json_schema, ZerxValue::I64(5)).is_ok());
    }

    // A `type` sibling on `anyOf`/`oneOf` is honoured only when provably exact.
    #[test]
    fn import_union_type_sibling() {
        assert_eq!(
            code_of(serde_json::json!({
                "type": "object", "oneOf": [{"type": "string"}, {"type": "number"}]
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );

        let objects = from(serde_json::json!({
            "type": "object",
            "oneOf": [
                {"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false},
                {"type":"object","properties":{"b":{"type":"number"}},"required":["b"],"additionalProperties":false}
            ]
        }))
        .unwrap();
        assert!(validate_zv(&objects, obj(&[("a", ZerxValue::String("x".to_string()))])).is_ok());

        assert!(from(serde_json::json!({
            "type": "number",
            "anyOf": [{"type": "integer"}, {"type": "number", "minimum": 5}]
        }))
        .is_ok());

        assert_eq!(
            code_of(serde_json::json!({
                "type": "object",
                "anyOf": [{"$ref": "#/$defs/S"}, {"type": "object", "properties": {}}],
                "$defs": {"S": {}}
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            code_of(serde_json::json!({
                "type": "object",
                "anyOf": [{"type": "object", "properties": {}}, {"type": "null"}]
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );

        let nullable_union = from(serde_json::json!({
            "type": ["object", "null"],
            "anyOf": [{"type": "object", "properties": {}}, {"type": "null"}]
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&nullable_union, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
    }

    // Unmodelled validation keywords are refused where they are not vacuous.
    #[test]
    fn import_unmodelled_assertion_keywords() {
        for bad in [
            serde_json::json!({"type": "number", "exclusiveMinimum": 0}),
            serde_json::json!({"type": "integer", "multipleOf": 2}),
            serde_json::json!({"type": "object", "minProperties": 1}),
            serde_json::json!({"type": "object", "maxProperties": 1}),
            serde_json::json!({"type": "number", "exclusiveMaximum": 5}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }

        // Vacuous under the declared type: ignoring forbids nothing.
        assert!(from(serde_json::json!({"type": "string", "exclusiveMinimum": 0})).is_ok());
        assert!(from(serde_json::json!({"type": "number", "uniqueItems": true})).is_ok());

        // A structural refusal that stands today is not relaxed by vacuity.
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "contains": {}})),
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // Assertion keywords next to `$ref` are refused, not skipped.
    #[test]
    fn import_ref_sibling_assertions() {
        for (sibling, value) in [
            ("required", serde_json::json!(["x"])),
            ("allOf", serde_json::json!([{"type": "string"}])),
            ("minLength", serde_json::json!(1)),
        ] {
            let mut node = serde_json::json!({
                "$ref": "#/$defs/S",
                "$defs": {"S": {"type": "string"}}
            });
            node[sibling] = value;
            assert_eq!(
                code_of(node.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {node}"
            );
        }

        // Annotations and extensions next to `$ref` still import.
        let ok = from(serde_json::json!({
            "$ref": "#/$defs/S",
            "description": "d",
            "x-k": 1,
            "$defs": {"S": {"type": "string"}}
        }))
        .unwrap();
        let exported = js(&ok);
        assert_eq!(exported["description"], serde_json::json!("d"));
        assert_eq!(exported["x-k"], serde_json::json!(1));
    }

    // Assertion keywords next to `anyOf`/`oneOf` are refused unless vacuous.
    #[test]
    fn import_union_sibling_assertions() {
        for bad in [
            serde_json::json!({"anyOf": [{"type": "string"}, {"type": "number"}], "minLength": 3}),
            serde_json::json!({"anyOf": [{"type": "string"}, {"type": "null"}], "maxLength": 3}),
            serde_json::json!({
                "anyOf": [{"type": "string"}, {"type": "number"}],
                "oneOf": [{"type": "string"}, {"type": "number"}]
            }),
            serde_json::json!({"anyOf": [{"type": "string"}, {"type": "number"}], "enum": ["a"]}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }

        // `minLength` applies only to strings, so it is vacuous on an object union.
        assert!(from(serde_json::json!({
            "type": "object",
            "oneOf": [
                {"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false},
                {"type":"object","properties":{"b":{"type":"number"}},"required":["b"],"additionalProperties":false}
            ],
            "minLength": 3
        }))
        .is_ok());
    }

    // An untyped node with assertions no branch consumes is refused.
    #[test]
    fn import_untyped_assertions_refused() {
        for bad in [
            serde_json::json!({"properties": {"a": {"type": "string"}}}),
            serde_json::json!({"minLength": 3}),
            serde_json::json!({"required": ["a"]}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }

        // No assertions: still `any()` with annotations.
        assert!(from(serde_json::json!({})).is_ok());
        assert!(from(serde_json::json!({"description": "d"})).is_ok());
    }

    // Malformed consumed keyword values are IMPORT_MALFORMED, never skipped.
    #[test]
    fn import_malformed_consumed_values() {
        for bad in [
            serde_json::json!({"type": "object", "properties": 5}),
            serde_json::json!({"type": "object", "properties": {"a": {}}, "required": [5]}),
            serde_json::json!({"type": "array", "items": {}, "minItems": -1}),
            serde_json::json!({"type": "string", "minLength": "2"}),
            serde_json::json!({"enum": "a"}),
            serde_json::json!({"type": "object", "$defs": 5}),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_MALFORMED,
                "input {bad}"
            );
        }

        assert_eq!(
            code_of(serde_json::json!({"anyOf": []})),
            ErrorCode::IMPORT_UNSUPPORTED
        );

        // An integral float counts as a non-negative integer.
        let s = from(serde_json::json!({"type": "string", "minLength": 2.0})).unwrap();
        assert!(validate_zv(&s, ZerxValue::String("ab".to_string())).is_ok());
        assert!(validate_zv(&s, ZerxValue::String("a".to_string())).is_err());
    }

    #[test]
    fn import_tuple_bounds() {
        for bad in [
            serde_json::json!({
                "type":"array",
                "prefixItems":[{"type":"string"},{"type":"number"}],
                "items":false,
                "minItems":3
            }),
            serde_json::json!({
                "type":"array",
                "prefixItems":[{"type":"string"},{"type":"number"}],
                "items":false,
                "maxItems":1
            }),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }

        let (v1, v2) = roundtrip_str(&tuple([string().into(), number().into()]).into());
        assert_eq!(v1, v2);
    }

    #[test]
    fn import_root_id_kept_and_nested_id_refused() {
        let root = serde_json::json!({"$id": "https://example.com/s", "type": "string"});
        let exported = from(root).unwrap().to_json_schema();
        assert_eq!(exported["$id"], serde_json::json!("https://example.com/s"));
        assert_eq!(from(exported.clone()).unwrap().to_json_schema(), exported);

        assert_eq!(
            code_of(serde_json::json!({
                "type":"object",
                "properties":{"a":{"$id":"nested","type":"string"}},
                "additionalProperties":false
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // A format marker consumes only the keywords its kind reconstructs.
    #[test]
    fn import_format_marker_siblings_refused() {
        for bad in [
            serde_json::json!({"type": "object", "format": "jsonschema", "required": ["x"]}),
            serde_json::json!({
                "type": "object", "format": "record",
                "additionalProperties": {}, "required": ["x"]
            }),
        ] {
            assert_eq!(
                code_of(bad.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {bad}"
            );
        }
    }

    // Strings carrying a `uri` marker enforce their string validators together
    // with the marker check. The import writes them into the marker node's
    // validator storage, so `string().min(1).format("uri")` re-imports.
    #[test]
    fn import_uri_marker_with_string_validators() {
        let s = from(serde_json::json!({"type": "string", "minLength": 1, "format": "uri"}))
            .expect("reported shape imports");
        assert!(validate_zv(&s, ZerxValue::String("a:b".to_string())).is_ok());
        assert_eq!(
            validate_zv(&s, ZerxValue::String("nope".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::INVALID_URI
        );
        assert_eq!(
            validate_zv(&s, ZerxValue::I64(5)).unwrap_err().code,
            ErrorCode::TYPE_MISMATCH
        );

        let s =
            from(serde_json::json!({"type": "string", "maxLength": 5, "format": "uri"})).unwrap();
        assert!(validate_zv(&s, ZerxValue::String("a:b".to_string())).is_ok());
        assert_eq!(
            validate_zv(&s, ZerxValue::String("https://example.com".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::STRING_TOO_LONG
        );
        assert_eq!(
            validate_zv(&s, ZerxValue::String("abc".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::INVALID_URI
        );

        let s = from(serde_json::json!({
            "type": "string", "pattern": "^https:", "format": "uri"
        }))
        .unwrap();
        assert!(validate_zv(&s, ZerxValue::String("https://x".to_string())).is_ok());
        assert_eq!(
            validate_zv(&s, ZerxValue::String("http://x".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::PATTERN_MISMATCH
        );
    }

    // All three string validators apply next to a `url` marker.
    #[test]
    fn import_url_marker_with_string_validators() {
        let s = from(serde_json::json!({
            "type": "string",
            "minLength": 12,
            "maxLength": 40,
            "pattern": "\\.org/",
            "format": "url"
        }))
        .unwrap();
        assert!(validate_zv(&s, ZerxValue::String("https://example.org/".to_string())).is_ok());
        assert_eq!(
            validate_zv(&s, ZerxValue::String("https://example.com/".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::PATTERN_MISMATCH
        );
        assert_eq!(
            validate_zv(&s, ZerxValue::String("ftp://example.org/".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::INVALID_URL
        );
        // Short and long valid URLs surface the length codes: `check_type` runs
        // first, then the validators in `minLength`, `maxLength`, `pattern` order.
        assert_eq!(
            validate_zv(&s, ZerxValue::String("https://a.b".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::STRING_TOO_SHORT
        );
        let long = format!("https://{}.org/", "a".repeat(40));
        assert_eq!(
            validate_zv(&s, ZerxValue::String(long)).unwrap_err().code,
            ErrorCode::STRING_TOO_LONG
        );
    }

    // The `[T, "null"]` nullable wrap still applies to a marker carrying validators.
    #[test]
    fn import_nullable_uri_marker_with_validators() {
        let s = from(serde_json::json!({
            "type": ["string", "null"], "maxLength": 10, "format": "uri"
        }))
        .unwrap();
        assert!(validate_zv(&s, ZerxValue::Null).is_ok());
        assert!(validate_zv(&s, ZerxValue::String("a:b".to_string())).is_ok());
        assert_eq!(
            validate_zv(&s, ZerxValue::String("a:bcdefghijkl".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::STRING_TOO_LONG
        );
    }

    // A malformed validator next to a marker is refused, not skipped.
    #[test]
    fn import_uri_marker_malformed_validator() {
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "minLength": -1, "format": "uri"})),
            ErrorCode::IMPORT_MALFORMED
        );
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "pattern": 5, "format": "uri"})),
            ErrorCode::IMPORT_MALFORMED
        );
    }

    // The export → import → export roundtrip of string validators combined with a
    // marker-colliding `format` is stable, and the re-imported node enforces both.
    #[test]
    fn import_marker_validator_roundtrip_pin() {
        // `minLength: 1` cannot reject a structurally valid URI/URL (the shortest
        // URI is three chars), and `^h` cannot reject a URL (both schemes start
        // with `h`), so those two codes have no rejecting value here; their
        // enforcement is covered by `import_uri_marker_with_string_validators`
        // and `import_url_marker_with_string_validators`.
        let long = format!("https://{}.com", "a".repeat(50));
        for (fmt, marker_code) in [
            ("uri", ErrorCode::INVALID_URI),
            ("url", ErrorCode::INVALID_URL),
        ] {
            let schema: Schema = string().min(1).max(50).regex("^h").format(fmt).into();
            let doc = schema.to_json_schema();
            let reimported = from(doc.clone()).expect("re-import of marker+validators failed");
            assert_eq!(reimported.to_json_schema(), doc, "format {fmt}");

            assert!(
                validate_zv(&reimported, ZerxValue::String("https://x.org".to_string())).is_ok(),
                "format {fmt}"
            );
            assert_eq!(
                validate_zv(&reimported, ZerxValue::String("nope".to_string()))
                    .unwrap_err()
                    .code,
                marker_code,
                "format {fmt}"
            );
            assert_eq!(
                validate_zv(&reimported, ZerxValue::String(long.clone()))
                    .unwrap_err()
                    .code,
                ErrorCode::STRING_TOO_LONG,
                "format {fmt}"
            );
        }

        // The `uri` variant additionally exercises `pattern`.
        let schema: Schema = string().min(1).max(50).regex("^h").format("uri").into();
        let reimported = from(schema.to_json_schema()).unwrap();
        assert_eq!(
            validate_zv(&reimported, ZerxValue::String("ftp://x".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::PATTERN_MISMATCH
        );
    }

    // The plain markers keep their roundtrip and carry no validators.
    #[test]
    fn import_plain_markers_roundtrip_without_validators() {
        let schemas: Vec<Schema> = vec![uri().into(), url().into()];
        for schema in schemas {
            let doc = schema.to_json_schema();
            let reimported = from(doc.clone()).unwrap();
            assert_eq!(reimported.to_json_schema(), doc);
            assert!(reimported.validators.is_empty());
        }
    }

    // `buffer` holds bytes, so string validators are not enforced on it and stay
    // refused instead of being consumed without effect.
    #[test]
    fn import_buffer_marker_keeps_string_validators_refused() {
        for (key, value) in [
            ("minLength", serde_json::json!(1)),
            ("maxLength", serde_json::json!(1)),
            ("pattern", serde_json::json!("^a")),
        ] {
            let mut node = serde_json::json!({"type": "string", "format": "buffer"});
            node[key] = value;
            assert_eq!(
                code_of(node.clone()),
                ErrorCode::IMPORT_UNSUPPORTED,
                "input {node}"
            );
        }

        // Pinned limitation: the export of a buffer with a string validator
        // re-imports as a refusal.
        let exported: Schema = string().min(1).format("buffer").into();
        let doc = exported.to_json_schema();
        assert_eq!(doc["minLength"], serde_json::json!(1));
        assert_eq!(code_of(doc), ErrorCode::IMPORT_UNSUPPORTED);

        let (v1, v2) = roundtrip_str(&buffer().mime("text/plain").into());
        assert_eq!(v1, v2);
    }

    // Non-string assertions and the `$ref` sibling rule are untouched by the
    // marker's string-validator consumption.
    #[test]
    fn import_uri_marker_other_siblings_unchanged() {
        // `minimum` applies to numbers, so it is vacuous on a string node.
        assert!(from(serde_json::json!({"type": "string", "format": "uri", "minimum": 1})).is_ok());

        // `enum` takes its own branch before the marker dispatch.
        let s =
            from(serde_json::json!({"type": "string", "format": "uri", "enum": ["a:b"]})).unwrap();
        assert!(validate_zv(&s, ZerxValue::String("a:b".to_string())).is_ok());
        assert!(validate_zv(&s, ZerxValue::String("c:d".to_string())).is_err());

        // An assertion sibling of `$ref` stays refused, even a string validator.
        assert_eq!(
            code_of(serde_json::json!({
                "$ref": "#/$defs/U",
                "minLength": 1,
                "$defs": {"U": {"type": "string", "format": "uri"}}
            })),
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    // A legacy `anyOf` + `x-oneOf` document keeps its `anyOf` meaning.
    #[test]
    fn import_legacy_x_one_of_document() {
        let schema = from(serde_json::json!({
            "anyOf": [{"type": "string"}, {"type": "number"}],
            "x-oneOf": true
        }))
        .unwrap();
        assert!(validate_zv(&schema, ZerxValue::String("a".to_string())).is_ok());
        assert!(validate_zv(&schema, ZerxValue::I64(1)).is_ok());
        let exported = js(&schema);
        assert!(exported.get("anyOf").is_some());
        assert_eq!(exported.get("x-oneOf"), Some(&serde_json::json!(true)));
    }

    // `oneOf` has real exactly-one semantics.
    #[test]
    fn import_one_of_exactly_one() {
        let schema = from(serde_json::json!({
            "oneOf":[{"type":"number"},{"type":"integer"}]
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&schema, ZerxValue::I64(1)).unwrap_err().code,
            ErrorCode::ONE_OF_AMBIGUOUS
        );
        assert!(validate_zv(&schema, ZerxValue::F64(1.5)).is_ok());
        assert_eq!(
            validate_zv(&schema, ZerxValue::String("x".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::UNION_MISMATCH
        );

        let exported = js(&schema);
        assert!(exported.get("oneOf").is_some());
        assert!(exported.get("x-oneOf").is_none());
        let reimported = from(exported.clone()).unwrap();
        assert_eq!(js(&reimported), exported);
    }

    // No assertion keyword is ever stored as `meta`.
    #[test]
    fn import_assertion_keywords_never_reach_meta() {
        for (node, key) in [
            (
                serde_json::json!({"type": "string", "exclusiveMinimum": 0}),
                "exclusiveMinimum",
            ),
            (
                serde_json::json!({"type": "number", "uniqueItems": true}),
                "uniqueItems",
            ),
            (
                serde_json::json!({"type": "string", "minProperties": 2}),
                "minProperties",
            ),
        ] {
            let schema = from(node).unwrap();
            assert!(
                schema.modifiers.meta.get(key).is_none(),
                "`{key}` reached meta"
            );
            assert!(js(&schema).get(key).is_none(), "`{key}` was exported");
        }

        // Even a programmatic `meta` entry cannot resurface an assertion keyword.
        let s: Schema = string().meta("minimum", 1i64).into();
        assert!(js(&s).get("minimum").is_none());
    }

    #[test]
    fn import_required_property_with_default_is_refused() {
        // A default fills a missing key before the required check runs, so
        // the reconstruction would accept `{}` although `required` forbids it.
        for doc in [
            serde_json::json!({"type": "object", "properties": {"a": {"type": "string", "default": "x"}}, "required": ["a"]}),
            serde_json::json!({"type": "object", "properties": {"a": {"anyOf": [{"type": "string", "default": "x"}, {"type": "null"}]}}, "required": ["a"]}),
        ] {
            let err = from(doc.clone()).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED, "{doc}");
            assert_eq!(err.path, vec!["properties", "a"], "{doc}");
        }
        // Not required: the default supplies the value, as before.
        let s = from(
            serde_json::json!({"type": "object", "properties": {"a": {"type": "string", "default": "x"}}}),
        )
        .unwrap();
        assert_eq!(
            validate_zv(&s, obj(&[])).unwrap(),
            obj(&[("a", ZerxValue::String("x".into()))])
        );
    }

    #[test]
    fn import_nullable_collapse_checks_the_null_member() {
        // The null member of a nullable anyOf is held to the same import
        // rules as any other node: an unsupported applicator or a non-root
        // `$id` there is refused, not skipped.
        for member in [
            serde_json::json!({"type": "null", "if": true, "then": false}),
            serde_json::json!({"type": "null", "$dynamicRef": "#x"}),
            serde_json::json!({"type": "null", "$id": "https://example.com/n"}),
        ] {
            let doc = serde_json::json!({"anyOf": [{"type": "string"}, member]});
            assert!(from(doc.clone()).is_err(), "{doc}");
        }
        // A bare null member with annotations still collapses to nullable.
        let s = from(serde_json::json!({"anyOf": [{"type": "string"}, {"type": "null", "description": "none"}]})).unwrap();
        assert!(validate_zv(&s, ZerxValue::Null).is_ok());
        assert!(validate_zv(&s, ZerxValue::String("a".into())).is_ok());
        assert!(validate_zv(&s, ZerxValue::I64(1)).is_err());
    }

    #[test]
    fn import_ref_decodes_json_pointer_escapes() {
        // `#/$defs/a~1b` names the def `a/b` (RFC 6901), not `a~1b`.
        let doc = serde_json::json!({
            "$defs": {"a/b": {"type": "string"}, "a~1b": {"type": "number"}},
            "$ref": "#/$defs/a~1b"
        });
        let s = from(doc).unwrap();
        assert!(validate_zv(&s, ZerxValue::I64(5)).is_err());
        assert!(validate_zv(&s, ZerxValue::String("x".into())).is_ok());

        let tilde =
            serde_json::json!({"$defs": {"m~n": {"type": "string"}}, "$ref": "#/$defs/m~0n"});
        let s = from(tilde).unwrap();
        assert!(validate_zv(&s, ZerxValue::I64(5)).is_err());

        // A pointer deeper than one `$defs` entry is not followed by name lookup.
        let deep = serde_json::json!({
            "$defs": {
                "A": {"type": "object", "properties": {"x": {"type": "string"}}},
                "A/properties/x": {}
            },
            "$ref": "#/$defs/A/properties/x"
        });
        assert_eq!(from(deep).unwrap_err().code, ErrorCode::IMPORT_UNSUPPORTED);

        // Malformed escapes and percent-encoded fragments are refused.
        for r in ["#/$defs/a~2", "#/$defs/a~", "#/$defs/a%20b"] {
            let doc = serde_json::json!({"$defs": {"a~2": {}, "a~": {}, "a%20b": {}}, "$ref": r});
            assert!(from(doc).is_err(), "{r}");
        }
    }

    #[test]
    fn import_depth_is_bounded() {
        // Nesting and `$ref` chains recurse once per level; past the depth
        // limit the import must refuse rather than exhaust the stack. A flat
        // `$defs` chain nests once per link although the document is shallow.
        // Run on a 2 MB thread so the bound must hold in debug builds too.
        fn build(kind: &str, n: usize) -> serde_json::Value {
            if kind == "chain" {
                let mut defs = serde_json::Map::new();
                for i in 0..n {
                    defs.insert(
                        format!("D{i}"),
                        serde_json::json!({"$ref": format!("#/$defs/D{}", i + 1)}),
                    );
                }
                defs.insert(format!("D{n}"), serde_json::json!({"type": "string"}));
                return serde_json::json!({"$defs": defs, "$ref": "#/$defs/D0"});
            }
            let mut v = serde_json::json!({"type": "string"});
            for _ in 0..n {
                v = match kind {
                    "items" => serde_json::json!({"type": "array", "items": v}),
                    "properties" => serde_json::json!({
                        "type": "object", "properties": {"a": v}, "required": ["a"]
                    }),
                    "anyOf" => serde_json::json!({"anyOf": [v, {"type": "number"}]}),
                    "oneOf" => serde_json::json!({"oneOf": [v, {"type": "number"}]}),
                    "nullable" => serde_json::json!({"anyOf": [v, {"type": "null"}]}),
                    "record" => serde_json::json!({
                        "type": "object", "format": "record", "additionalProperties": v
                    }),
                    "tuple" => {
                        serde_json::json!({"type": "array", "prefixItems": [v], "items": false})
                    }
                    _ => unreachable!(),
                };
            }
            v
        }
        let kinds = [
            "chain",
            "items",
            "properties",
            "anyOf",
            "oneOf",
            "nullable",
            "record",
            "tuple",
        ];
        for kind in kinds {
            let handle = std::thread::Builder::new()
                .stack_size(2 * 1024 * 1024)
                .spawn(move || {
                    let deep = from(build(kind, MAX_PARSE_DEPTH + 50))
                        .map(|_| ())
                        .map_err(|e| e.code);
                    let shallow = from(build(kind, 10)).map(|_| ()).map_err(|e| e.code);
                    (deep, shallow)
                })
                .unwrap();
            let (deep, shallow) = handle.join().unwrap();
            assert_eq!(deep, Err(ErrorCode::IMPORT_UNSUPPORTED), "{kind}");
            assert_eq!(shallow, Ok(()), "{kind}");
        }
    }

    #[test]
    fn import_non_compiling_pattern_is_malformed() {
        // Same outcome as the static path for `{"enum":["a"],"pattern":"["}`.
        for doc in [
            serde_json::json!({"type": "string", "pattern": "["}),
            serde_json::json!({"type": "string", "format": "uri", "pattern": "("}),
        ] {
            assert_eq!(
                from(doc.clone()).unwrap_err().code,
                ErrorCode::IMPORT_MALFORMED,
                "{doc}"
            );
        }
        assert!(from(serde_json::json!({"type": "string", "pattern": "^a+$"})).is_ok());
    }

    #[test]
    fn import_pattern_reads_ecma_262() {
        // The translated `\d` is ASCII-only, so an Arabic-digit value fails.
        let s = from(serde_json::json!({"type": "string", "pattern": "^\\d+$"})).unwrap();
        assert_eq!(
            validate_zv(&s, ZerxValue::String("١٢٣".to_string()))
                .unwrap_err()
                .code,
            ErrorCode::PATTERN_MISMATCH
        );
        assert!(validate_zv(&s, ZerxValue::String("123".to_string())).is_ok());

        // Valid ECMA that cannot be translated is `IMPORT_UNSUPPORTED`;
        // invalid ECMA is `IMPORT_MALFORMED`.
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "pattern": "(?=a)a"})),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            code_of(serde_json::json!({"type": "string", "pattern": "\\e"})),
            ErrorCode::IMPORT_MALFORMED
        );

        // Static `enum` evaluation agrees with the translated runtime meaning.
        assert_eq!(
            code_of(serde_json::json!({"enum": ["١٢٣"], "pattern": "^\\d+$"})),
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert!(from(serde_json::json!({"enum": ["123"], "pattern": "^\\d+$"})).is_ok());
    }

    #[test]
    fn export_repeated_bounds_keep_the_strictest() {
        // The runtime enforces every validator, so the export must state the
        // tightest bound, not whichever validator was added last.
        let s: Schema = string().min(5).min(2).max(3).max(9).into();
        let v = js(&s);
        assert_eq!(get(&v, "minLength"), Some(&serde_json::json!(5)));
        assert_eq!(get(&v, "maxLength"), Some(&serde_json::json!(3)));

        let n: Schema = number().max(10.0).max(20.0).min(-5.0).min(1.0).into();
        let v = js(&n);
        assert_eq!(get(&v, "maximum").and_then(|x| x.as_f64()), Some(10.0));
        assert_eq!(get(&v, "minimum").and_then(|x| x.as_f64()), Some(1.0));

        let a: Schema = array(string()).min(4).min(1).max(2).max(8).into();
        let v = js(&a);
        assert_eq!(get(&v, "minItems"), Some(&serde_json::json!(4)));
        assert_eq!(get(&v, "maxItems"), Some(&serde_json::json!(2)));

        // Round trip stays as strict as the source.
        let re = from_json_schema(&js(&s)).unwrap();
        assert!(re.validate("abc").is_err());
    }

    #[test]
    fn non_finite_number_bounds_fail_closed() {
        // Number values are always finite, so a NaN bound or an infinite bound
        // on the wrong side accepts nothing; JSON cannot hold the bound, so
        // the node exports as `false`.
        for s in [
            Schema::from(number().min(f64::NAN)),
            Schema::from(number().max(f64::NAN)),
            Schema::from(number().min(f64::INFINITY)),
            Schema::from(number().max(f64::NEG_INFINITY)),
        ] {
            assert!(s.validate(&5i32).is_err());
            assert!(s.validate(&-5i32).is_err());
            assert_eq!(js(&s), serde_json::Value::Bool(false));
        }
        // An infinite bound on the open side forbids nothing: no keyword.
        let open: Schema = number().min(f64::NEG_INFINITY).max(f64::INFINITY).into();
        assert!(open.validate(&5i32).is_ok());
        assert_eq!(js(&open), serde_json::json!({"type": "number"}));
        // Nullable keeps `null`, the only value the runtime still accepts.
        let n: Schema = number().min(f64::NAN).nullable().into();
        assert!(n.validate(&()).is_ok());
        assert_eq!(js(&n), serde_json::json!({"type": "null"}));
        // Inside a container the `false` member makes the import refuse.
        let o: Schema = object([("a", number().min(f64::NAN).into())]).into();
        assert_eq!(js(&o)["properties"]["a"], serde_json::Value::Bool(false));
        assert!(from_json_schema(&js(&o)).is_err());
    }

    #[test]
    fn unrepresentable_literal_exports_false() {
        // `{"const": null}` would accept only null, which the literal rejects.
        for s in [
            Schema::from(literal(u128::MAX)),
            Schema::from(literal(i128::MIN)),
            Schema::from(literal(f64::NAN)),
            Schema::from(literal(f64::INFINITY)),
            Schema::from(literal(ZerxValue::Bytes(vec![1, 2]))),
        ] {
            assert!(s.validate(&()).is_err());
            assert_eq!(js(&s), serde_json::Value::Bool(false));
        }
        // Representable literals keep `const`.
        let s: Schema = literal(u64::MAX).into();
        assert_eq!(js(&s), serde_json::json!({"const": u64::MAX}));
        let s: Schema = literal(i64::MIN).into();
        assert_eq!(js(&s), serde_json::json!({"const": i64::MIN}));
    }

    #[test]
    fn invalid_discriminated_union_exports_false() {
        // A structural defect makes the runtime reject every value; the
        // `oneOf` export would accept the variants' values.
        let du: Schema = discriminated_union(
            "k",
            [Schema::from(object([(
                "k",
                literal("a").optional().into(),
            )]))],
        )
        .into();
        assert!(du.validate(&serde_json::json!({"k": "a"})).is_err());
        assert_eq!(js(&du), serde_json::Value::Bool(false));

        let valid: Schema =
            discriminated_union("k", [Schema::from(object([("k", literal("a").into())]))]).into();
        assert!(js(&valid).get("oneOf").is_some());
    }

    #[test]
    fn colliding_pattern_or_format_exports_false() {
        // One `pattern` / `format` keyword cannot carry two different
        // validators; keeping only one would export a wider schema.
        for s in [
            Schema::from(string().regex("^a").regex("b$")),
            Schema::from(string().email().uuid()),
        ] {
            assert_eq!(js(&s), serde_json::Value::Bool(false));
        }
        let n: Schema = string().regex("^a").regex("b$").nullable().into();
        assert_eq!(js(&n), serde_json::json!({"type": "null"}));
        // The same validator twice is no collision.
        let same: Schema = string().regex("^a").regex("^a").min(2).into();
        assert_eq!(
            js(&same),
            serde_json::json!({"type": "string", "pattern": "^a", "minLength": 2})
        );
    }

    #[test]
    fn number_node_bounds_beyond_2_pow_53_are_refused() {
        // A number node compares a bound against fractional and integral
        // values alike; from 2^53 on the bound no longer separates
        // neighbouring integers, so it is refused rather than enforced
        // inexactly. Integer nodes hold integer bounds exactly instead
        // (`integer_node_bounds_import_exactly`).
        for doc in [
            serde_json::json!({"type": "number", "maximum": 9007199254740992u64}),
            serde_json::json!({"type": "number", "minimum": -9007199254740993i64}),
            serde_json::json!({"type": "number", "maximum": 1e300}),
            serde_json::json!({"type": ["number", "null"], "minimum": 18446744073709551615u64}),
        ] {
            assert_eq!(
                from(doc.clone()).unwrap_err().code,
                ErrorCode::IMPORT_UNSUPPORTED,
                "{doc}"
            );
        }
        assert!(
            from(serde_json::json!({"type": "number", "minimum": 0.5, "maximum": 1e15})).is_ok()
        );
        // On an integer node the same literals import and order exactly.
        let s =
            from(serde_json::json!({"type": "integer", "maximum": 9007199254740992u64})).unwrap();
        assert!(validate_zv(&s, ZerxValue::I64(9007199254740991)).is_ok());
        assert!(validate_zv(&s, ZerxValue::I64(9007199254740992)).is_ok());
        assert!(validate_zv(&s, ZerxValue::I64(9007199254740993)).is_err());
        assert!(from(
            serde_json::json!({"type": ["integer", "null"], "minimum": 18446744073709551615u64})
        )
        .is_ok());
    }

    #[test]
    fn integer_node_bounds_import_exactly() {
        // epistole: quota_bytes up to i64::MAX.
        let epistole = serde_json::json!({
            "type": "object",
            "properties": {"quota_bytes": {"type": "integer", "minimum": 0, "maximum": 9223372036854775807i64}},
            "required": ["quota_bytes"]
        });
        // pygmalion: seed over the full i64 range below its maximum.
        let pygmalion = serde_json::json!({
            "type": "object",
            "properties": {"seed": {"type": "integer", "minimum": -9223372036854775808i64, "maximum": 9223372036854774784i64}},
            "required": ["seed"]
        });
        for (doc, key, lo, hi) in [
            (&epistole, "quota_bytes", 0i128, i64::MAX as i128),
            (
                &pygmalion,
                "seed",
                i64::MIN as i128,
                9223372036854774784i128,
            ),
        ] {
            let s = from(doc.clone()).unwrap();
            let v = |n: i128| obj(&[(key, ZerxValue::I128(n))]);
            assert!(validate_zv(&s, v(lo)).is_ok(), "{key} {lo}");
            assert!(validate_zv(&s, v(hi)).is_ok(), "{key} {hi}");
            assert!(validate_zv(&s, v(lo - 1)).is_err(), "{key} {lo}-1");
            assert!(validate_zv(&s, v(hi + 1)).is_err(), "{key} {hi}+1");
            // Neighbours beyond the signed ranges and huge integral floats.
            assert!(validate_zv(&s, obj(&[(key, ZerxValue::U64(u64::MAX))])).is_err());
            assert!(validate_zv(&s, obj(&[(key, ZerxValue::U128(u128::MAX))])).is_err());
            assert!(validate_zv(&s, obj(&[(key, ZerxValue::F64(1e40))])).is_err());
            assert!(validate_zv(&s, obj(&[(key, ZerxValue::F64(-1e40))])).is_err());
            // Round trip: the bounds export unchanged, and the re-import
            // exports the identical integer bounds again.
            let exported = s.to_json_schema();
            assert_eq!(exported["properties"][key], doc["properties"][key], "{key}");
            let again = from(exported).unwrap().to_json_schema();
            assert_eq!(again["properties"][key], doc["properties"][key], "{key}");
            let re = from(again).unwrap();
            assert!(validate_zv(&re, v(lo)).is_ok());
            assert!(validate_zv(&re, v(hi + 1)).is_err());
        }
        // A nullable integer keeps exact bounds and admits null.
        let s = from(
            serde_json::json!({"type": ["integer", "null"], "minimum": -9223372036854775808i64, "maximum": 9223372036854775807i64}),
        )
        .unwrap();
        assert!(validate_zv(&s, ZerxValue::Null).is_ok());
        assert!(validate_zv(&s, ZerxValue::I64(i64::MIN)).is_ok());
        assert!(validate_zv(&s, ZerxValue::I128(i64::MIN as i128 - 1)).is_err());
        assert!(validate_zv(&s, ZerxValue::U64(9223372036854775808)).is_err());
        // An integer node with a float bound of magnitude 2^53 or more may
        // carry a rounded source value and stays refused.
        assert_eq!(
            from(serde_json::json!({"type": "integer", "maximum": 1e300}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        // Number nodes keep the 2^53 refusal.
        assert_eq!(
            from(serde_json::json!({"type": "number", "maximum": 9223372036854775807i64}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    #[test]
    fn export_repeated_bounds_merge_exactly() {
        // Neighbouring bounds beyond 2^53 share one f64; the merge must still
        // keep the tighter one, in either call order, so the re-imported
        // export rejects the value the runtime rejects.
        let p53 = 1i64 << 53;
        let cases: [(i64, i64, &str, i64); 3] = [
            (i64::MAX, i64::MAX - 1, "maximum", i64::MAX),
            (p53 + 1, p53, "maximum", p53 + 1),
            (i64::MIN, i64::MIN + 1, "minimum", i64::MIN),
        ];
        for (wide, tight, key, excluded) in cases {
            for (a, b) in [(wide, tight), (tight, wide)] {
                let s: Schema = if key == "maximum" {
                    number().int().max_bound(a).max_bound(b).into()
                } else {
                    number().int().min_bound(a).min_bound(b).into()
                };
                assert!(s.validate(&excluded).is_err(), "{key} {a} {b}");
                let v = js(&s);
                assert_eq!(v[key], serde_json::json!(tight), "{key} {a} {b}");
                let re = from_json_schema(&v).unwrap();
                assert!(re.validate(&excluded).is_err(), "{key} {a} {b}");
                assert!(re.validate(&tight).is_ok(), "{key} {a} {b}");
            }
        }
    }

    #[test]
    fn integer_node_float_bounds_export_as_integers() {
        // pygmalion builds its seed bounds as f64. An integral f64 is an
        // exact integer, so an integer node exports it as a JSON integer and
        // the export re-imports (the importer refuses float literals >= 2^53
        // on integer nodes, as the parser may have rounded them).
        let seed: Schema = number()
            .int()
            .min(-9223372036854775808.0)
            .max(9223372036854774784.0)
            .into();
        let v = js(&seed);
        assert_eq!(v["minimum"], serde_json::json!(i64::MIN));
        assert_eq!(v["maximum"], serde_json::json!(9223372036854774784i64));
        // The round trip goes through text: serialize, parse, import.
        let text = serde_json::to_string(&v).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let re = from_json_schema(&parsed).unwrap();
        assert!(re.validate(&i64::MIN).is_ok());
        assert!(re.validate(&(i64::MIN as i128 - 1)).is_err());
        assert!(re.validate(&9223372036854774784i64).is_ok());
        assert!(re.validate(&9223372036854774785i64).is_err());
        // Inside a nullable wrap as well.
        let n: Schema = number().int().max(9223372036854774784.0).nullable().into();
        assert!(from_json_schema(&js(&n)).is_ok());
        // Small integral bounds on integer nodes: `0`, not `0.0`.
        let small: Schema = number().int().min(0).max(9.0).into();
        let v = js(&small);
        assert_eq!(v["minimum"], serde_json::json!(0));
        assert_eq!(v["maximum"], serde_json::json!(9));
        // Fractional bounds stay floats on integer nodes.
        let frac: Schema = number().int().min(0.5).into();
        assert_eq!(js(&frac)["minimum"], serde_json::json!(0.5));
        // Negative control: the importer still refuses a float literal of
        // magnitude 2^53 or more on an integer node; serde has already rounded
        // 9007199254740993.0 to 2^53.
        let rounded: serde_json::Value =
            serde_json::from_str(r#"{"type":"integer","minimum":9007199254740993.0}"#).unwrap();
        assert_eq!(
            from_json_schema(&rounded).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        // Bounds beyond the i64/u64 range are not converted: they stay floats
        // on the strict side, and the importer refuses them.
        let wide: Schema = number().int().min(-1e20).max(1e20).into();
        let v = js(&wide);
        assert!(v["minimum"].is_f64() && v["maximum"].is_f64());
        let text = serde_json::to_string(&v).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            from_json_schema(&parsed).unwrap_err().code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        // Number nodes are unchanged.
        let num: Schema = number().min(0).max(9.0).into();
        let v = js(&num);
        assert_eq!(v["minimum"], serde_json::json!(0.0));
        assert_eq!(v["maximum"], serde_json::json!(9.0));
    }

    // --- uniqueItems ---

    #[test]
    fn export_unique_items_fragment() {
        let with: Schema = array(string()).unique_items().into();
        assert_eq!(
            get(&js(&with), "uniqueItems"),
            Some(&serde_json::json!(true))
        );

        let without: Schema = array(string()).into();
        assert!(get(&js(&without), "uniqueItems").is_none());

        let meta_only: Schema = array(string()).meta("uniqueItems", true).into();
        assert!(get(&js(&meta_only), "uniqueItems").is_none());
    }

    #[test]
    fn import_unique_items_true_false_and_malformed() {
        let unique = from(serde_json::json!({
            "type": "array",
            "items": {"type": "string"},
            "uniqueItems": true
        }))
        .unwrap();
        let duplicate = || {
            ZerxValue::Array(vec![
                ZerxValue::String("a".to_string()),
                ZerxValue::String("a".to_string()),
            ])
        };
        let err = validate_zv(&unique, duplicate()).unwrap_err();
        assert_eq!(err.code, ErrorCode::ARRAY_NOT_UNIQUE);
        assert_eq!(err.path, vec!["1"]);

        let not_unique = from(serde_json::json!({
            "type": "array",
            "items": {"type": "string"},
            "uniqueItems": false
        }))
        .unwrap();
        assert!(validate_zv(&not_unique, duplicate()).is_ok());

        for bad in [
            serde_json::json!({"type":"array","items":{"type":"string"},"uniqueItems":"yes"}),
            serde_json::json!({"type":"array","items":{"type":"string"},"uniqueItems":1}),
        ] {
            assert_eq!(code_of(bad.clone()), ErrorCode::IMPORT_MALFORMED, "{bad}");
        }
    }

    #[test]
    fn import_unique_items_nullable_array() {
        let schema = from(serde_json::json!({
            "type": ["array", "null"],
            "items": {"type": "string"},
            "uniqueItems": true
        }))
        .unwrap();
        assert_eq!(
            validate_zv(&schema, ZerxValue::Null).unwrap(),
            ZerxValue::Null
        );
        assert_eq!(
            validate_zv(
                &schema,
                ZerxValue::Array(vec![
                    ZerxValue::String("a".to_string()),
                    ZerxValue::String("a".to_string()),
                ])
            )
            .unwrap_err()
            .code,
            ErrorCode::ARRAY_NOT_UNIQUE
        );
    }

    #[test]
    fn import_unique_items_on_tuple() {
        let tuple_doc = serde_json::json!({
            "type": "array",
            "prefixItems": [{"type": "string"}],
            "items": false,
            "uniqueItems": true
        });
        assert_eq!(code_of(tuple_doc), ErrorCode::IMPORT_UNSUPPORTED);

        let ok = from(serde_json::json!({
            "type": "array",
            "prefixItems": [{"type": "string"}],
            "items": false,
            "uniqueItems": false
        }));
        assert!(ok.is_ok());
    }

    fn harness_schema() -> Schema {
        object([
            (
                "steps",
                array(object([
                    ("id", string().into()),
                    ("kind", enumerate(["a", "b"]).into()),
                ]))
                .unique_items()
                .into(),
            ),
            ("tags", array(string()).unique_items().into()),
        ])
        .into()
    }

    #[test]
    fn unique_items_round_trip_and_native_import_parity() {
        let native = harness_schema();
        let exported = js(&native);
        let imported = from(exported.clone()).unwrap();
        assert_eq!(js(&imported), exported);

        for case in [
            serde_json::json!({
                "tags": ["a", "b"],
                "steps": [{"id": "s1", "kind": "a"}]
            }),
            serde_json::json!({
                "tags": ["a", "a"],
                "steps": [{"id": "s1", "kind": "a"}]
            }),
            serde_json::json!({
                "tags": ["a", "b"],
                "steps": [
                    {"id": "s1", "kind": "a"},
                    {"kind": "a", "id": "s1"}
                ]
            }),
            serde_json::json!({
                "tags": ["a", "b"],
                "steps": [{"id": "s1", "kind": "a", "extra": 1}]
            }),
        ] {
            let n = validate_zv(&native, ZerxValue::from_serialize(&case).unwrap());
            let i = validate_zv(&imported, ZerxValue::from_serialize(&case).unwrap());
            match (&n, &i) {
                (Ok(_), Ok(_)) => {}
                (Err(a), Err(b)) => {
                    assert_eq!(a.code, b.code, "case {case}");
                    assert_eq!(a.path, b.path, "case {case}");
                }
                _ => panic!("native and imported differ on {case}: {n:?} vs {i:?}"),
            }
        }
    }

    // ------------------------------------------------------------------
    // Imported constants/defaults and the value nesting bound
    // ------------------------------------------------------------------

    fn on_2mb_thread<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .expect("spawn deep-test thread")
            .join()
            .expect("deep-test thread panicked");
    }

    fn deep_json_arrays(depth: usize) -> serde_json::Value {
        let mut v = serde_json::json!(0);
        for _ in 0..depth {
            v = serde_json::Value::Array(vec![v]);
        }
        v
    }

    #[test]
    fn import_deep_const_and_default_refused() {
        on_2mb_thread(|| {
            // Build the documents field by field: `json!` would serialise the
            // deep child through serde_json itself and recurse per level.
            let mut const_obj = serde_json::Map::new();
            const_obj.insert("const".to_string(), deep_json_arrays(100_000));
            let const_doc = serde_json::Value::Object(const_obj);
            let err = crate::from_json_schema(&const_doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            std::mem::forget(const_doc);

            let mut property = serde_json::Map::new();
            property.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            property.insert("default".to_string(), deep_json_arrays(100_000));
            let mut properties = serde_json::Map::new();
            properties.insert("a".to_string(), serde_json::Value::Object(property));
            let mut default_obj = serde_json::Map::new();
            default_obj.insert(
                "type".to_string(),
                serde_json::Value::String("object".to_string()),
            );
            default_obj.insert(
                "properties".to_string(),
                serde_json::Value::Object(properties),
            );
            let default_doc = serde_json::Value::Object(default_obj);
            let err = crate::from_json_schema(&default_doc).unwrap_err();
            assert!(
                err.code == ErrorCode::PARSE_DEPTH_EXCEEDED
                    || err.code == ErrorCode::IMPORT_UNSUPPORTED
            );
            std::mem::forget(default_doc);
        });
    }

    // ------------------------------------------------------------------
    // Import document depth bound
    // ------------------------------------------------------------------

    // Iteratively wraps a leaf schema `levels` times, so a deep fixture is
    // built from Maps without `json!` recursing through serde_json per level.
    fn deep_schema_chain(kind: &str, levels: usize) -> serde_json::Value {
        let mut v = serde_json::Value::Object({
            let mut m = serde_json::Map::new();
            m.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            m
        });
        for _ in 0..levels {
            let mut node = serde_json::Map::new();
            match kind {
                "items" => {
                    node.insert(
                        "type".to_string(),
                        serde_json::Value::String("array".to_string()),
                    );
                    node.insert("items".to_string(), v);
                }
                "properties" => {
                    node.insert(
                        "type".to_string(),
                        serde_json::Value::String("object".to_string()),
                    );
                    let mut props = serde_json::Map::new();
                    props.insert("a".to_string(), v);
                    node.insert("properties".to_string(), serde_json::Value::Object(props));
                    node.insert(
                        "required".to_string(),
                        serde_json::Value::Array(vec![serde_json::Value::String("a".to_string())]),
                    );
                }
                _ => unreachable!("unsupported chain kind: {kind}"),
            }
            v = serde_json::Value::Object(node);
        }
        v
    }

    fn unused_defs_document(member: serde_json::Value) -> serde_json::Value {
        let mut entry = serde_json::Map::new();
        entry.insert("const".to_string(), member);
        let mut defs = serde_json::Map::new();
        defs.insert("X".to_string(), serde_json::Value::Object(entry));
        let mut root = serde_json::Map::new();
        root.insert("$defs".to_string(), serde_json::Value::Object(defs));
        serde_json::Value::Object(root)
    }

    #[test]
    fn import_unused_defs_deep_document_refused() {
        on_2mb_thread(|| {
            // `ImportContext::new` clones the whole `$defs` map before any
            // depth guard; an unused deep entry must not reach that clone.
            let doc = unused_defs_document(deep_json_arrays(10_000));
            let err = crate::from_json_schema(&doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            std::mem::forget(doc);
        });
    }

    #[test]
    fn import_ref_to_deep_defs_and_deep_enum_refused() {
        on_2mb_thread(|| {
            // A `$ref` forces the deep definition through the import walk.
            let mut ref_doc = unused_defs_document(deep_json_arrays(10_000));
            ref_doc.as_object_mut().unwrap().insert(
                "$ref".to_string(),
                serde_json::Value::String("#/$defs/X".to_string()),
            );
            let err = crate::from_json_schema(&ref_doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            std::mem::forget(ref_doc);

            // `enum` clones its raw members before conversion.
            let mut root = serde_json::Map::new();
            root.insert(
                "enum".to_string(),
                serde_json::Value::Array(vec![deep_json_arrays(10_000)]),
            );
            let enum_doc = serde_json::Value::Object(root);
            let err = crate::from_json_schema(&enum_doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            std::mem::forget(enum_doc);
        });
    }

    #[test]
    fn import_deep_items_and_properties_chains_refused() {
        on_2mb_thread(|| {
            for kind in ["items", "properties"] {
                let doc = deep_schema_chain(kind, 100_000);
                let err = crate::from_json_schema(&doc).unwrap_err();
                assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED, "{kind}");
                assert!(
                    err.message.contains("document depth limit"),
                    "{kind}: {}",
                    err.message
                );
                std::mem::forget(doc);
            }
        });
    }

    #[test]
    fn import_document_depth_boundary() {
        on_2mb_thread(|| {
            // root 1 + `$defs` 2 + entry 3 + array levels 4..(3 + array_depth).
            fn boundary_doc(array_depth: usize) -> serde_json::Value {
                let mut entry = serde_json::Map::new();
                entry.insert("x-unknown".to_string(), deep_json_arrays(array_depth));
                let mut defs = serde_json::Map::new();
                defs.insert("X".to_string(), serde_json::Value::Object(entry));
                let mut root = serde_json::Map::new();
                root.insert("$defs".to_string(), serde_json::Value::Object(defs));
                serde_json::Value::Object(root)
            }

            // Exactly 400 JSON levels: the unused entry is never walked, so the
            // document still imports.
            let at_bound = boundary_doc(397);
            assert!(crate::from_json_schema(&at_bound).is_ok());
            std::mem::forget(at_bound);

            // One level more is refused before the import walk.
            let over = boundary_doc(398);
            let err = crate::from_json_schema(&over).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("document depth limit"),
                "{}",
                err.message
            );
            std::mem::forget(over);
        });
    }

    #[test]
    fn import_shallow_document_still_hits_walk_nesting_guard() {
        on_2mb_thread(|| {
            // A 100-level `properties` chain sits near 200 JSON levels, well
            // under the document bound, so the import walk's own nesting guard
            // must still be the one that refuses it.
            let doc = deep_schema_chain("properties", 100);
            let err = crate::from_json_schema(&doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("import depth limit of 100"),
                "{}",
                err.message
            );
        });
    }

    // --- bug sweep 3.2.3 ---

    #[test]
    fn unresolvable_lazy_exports_fail_closed() {
        // A thunk that exports its own lazy runs the nested export while the
        // lazy is still resolving, so that export cannot resolve it.
        let cell: Rc<RefCell<Option<Schema>>> = Rc::new(RefCell::new(None));
        let nested: Rc<RefCell<Option<serde_json::Value>>> = Rc::new(RefCell::new(None));
        let (c, n) = (cell.clone(), nested.clone());
        let lz: Schema = lazy(move || {
            if let Some(me) = c.borrow().as_ref() {
                *n.borrow_mut() = Some(me.to_json_schema());
            }
            string().into()
        })
        .into();
        *cell.borrow_mut() = Some(lz.clone());
        let _ = lz.to_json_schema();
        let inner = nested.borrow().clone().expect("nested export ran");
        let defs = inner["$defs"].as_object().expect("$defs present");
        assert_eq!(defs.len(), 1);
        assert_eq!(
            defs.values().next().unwrap(),
            &serde_json::Value::Bool(false)
        );
        cell.borrow_mut().take();
    }

    #[test]
    fn all_of_not_next_to_ref_keep_the_hint() {
        for (kw, hint) in [("allOf", "extend()"), ("not", "union()")] {
            let mut node = serde_json::json!({"$ref": "#/$defs/A"});
            node[kw] = if kw == "allOf" {
                serde_json::json!([{"type": "string"}])
            } else {
                serde_json::json!({"type": "string"})
            };
            let doc = serde_json::json!({
                "type": "object",
                "properties": {"a": node},
                "$defs": {"A": {"type": "string"}}
            });
            let err = from(doc).unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED, "{kw}");
            assert!(err.message.contains(hint), "{kw}: {}", err.message);
            assert_eq!(
                err.path,
                vec!["properties".to_string(), "a".to_string()],
                "{kw}"
            );
        }
    }
}
