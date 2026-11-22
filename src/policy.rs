// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use crate::json_schema::{
    check_import_document_depth, dispose_json_value, import_document_depth_error,
    json_depth_exceeds, MAX_IMPORT_DOCUMENT_DEPTH,
};
use crate::schema::{Schema, MAX_PARSE_DEPTH};
use crate::{ErrorCode, ZerxError};

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------

impl ErrorCode {
    /// `from_json_schema_with` was given a policy name with no registered policy.
    pub const POLICY_UNKNOWN: ErrorCode = ErrorCode::new("policy_unknown");
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Pre-parse transform: JSON Schema `Value` in, JSON Schema `Value` out.
pub type SchemaTransform = Arc<dyn Fn(&Value) -> Result<Value, ZerxError> + Send + Sync>;

/// Post-parse transform: `Schema` in, `Schema` out.
pub type TypeTransform = Arc<dyn Fn(Schema) -> Result<Schema, ZerxError> + Send + Sync>;

/// Given an external `$ref` string, return the resolved subschema.
pub type RefResolver = Arc<dyn Fn(&str) -> Result<Value, ZerxError> + Send + Sync>;

/// A named import policy: ordered schema transforms (pre-parse) and type transforms (post-parse).
#[derive(Default, Clone)]
pub struct Policy {
    pub schema_transforms: Vec<SchemaTransform>,
    pub type_transforms: Vec<TypeTransform>,
}

/// Options for `from_json_schema_with`.
#[derive(Default)]
pub struct ImportOptions {
    /// Named policy to look up in the registry.
    pub policy: Option<String>,
    /// Caller-supplied schema transforms, appended after policy transforms.
    pub schema_transforms: Vec<SchemaTransform>,
    /// Caller-supplied type transforms, appended after policy transforms.
    pub type_transforms: Vec<TypeTransform>,
    /// Deref hook for external `$ref` resolution (applied first, before policy schema transforms).
    pub deref: Option<RefResolver>,
    /// Disposition of unknown properties for every imported object node.
    ///
    /// `false` (default): a property the schema rejects yields `unknown_property` at validation
    /// time. `true`: it is silently dropped instead.
    ///
    /// Applies to every reconstructed object node, not only the root. Nodes with
    /// `additionalProperties: true` or `{}` import as passthrough and are NOT affected — the flag
    /// reinterprets only the strict outcome (`false` or absent). A non-empty schema-object
    /// `additionalProperties` on a plain object is refused with `import_unsupported` either way.
    ///
    /// This is caller policy and is not carried by the JSON Schema document: a schema imported
    /// with `strip_unknown: true` still exports `additionalProperties: false`, so the flag must be
    /// supplied again on every import.
    ///
    /// Note: `union` variants are matched first-match-wins. Under `strip_unknown: true` a variant
    /// that strict mode would have rejected for an extra key can match, and the extra keys are
    /// dropped.
    pub strip_unknown: bool,
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

fn registry() -> &'static Mutex<HashMap<String, Policy>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Policy>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert("sql".to_owned(), builtin_sql_policy());
        Mutex::new(map)
    })
}

/// Register a policy under `name`, overwriting any existing entry.
pub fn register_policy(name: impl Into<String>, policy: Policy) {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(name.into(), policy);
}

fn lookup_policy(name: &str) -> Option<Policy> {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    guard.get(name).cloned()
}

// ---------------------------------------------------------------------------
// deep_map_schema
// ---------------------------------------------------------------------------

// Applies mapper to every schema node, recursing into all positions that the core
// core importer reads as child schemas.
fn deep_map_schema(
    value: &Value,
    mapper: &dyn Fn(&Value) -> Result<Value, ZerxError>,
) -> Result<Value, ZerxError> {
    let mapped = mapper(value)?;
    map_child_schemas(mapped, &mut |child| deep_map_schema(child, mapper))
}

// Applies `f` to every direct child schema of `value` (the positions the core
// importer reads as child schemas); a non-object value is returned unchanged.
fn map_child_schemas(
    value: Value,
    f: &mut dyn FnMut(&Value) -> Result<Value, ZerxError>,
) -> Result<Value, ZerxError> {
    let mut out = match value {
        Value::Object(o) => o,
        other => return Ok(other),
    };

    // properties
    if let Some(Value::Object(props)) = out.get("properties").cloned() {
        let mut new_props = serde_json::Map::new();
        for (k, v) in &props {
            new_props.insert(k.clone(), f(v)?);
        }
        out.insert("properties".to_owned(), Value::Object(new_props));
    }

    // $defs
    if let Some(Value::Object(defs)) = out.get("$defs").cloned() {
        let mut new_defs = serde_json::Map::new();
        for (k, v) in &defs {
            new_defs.insert(k.clone(), f(v)?);
        }
        out.insert("$defs".to_owned(), Value::Object(new_defs));
    }

    // items: schema or array of schemas
    match out.get("items").cloned() {
        Some(Value::Array(arr)) => {
            let new_items: Result<Vec<Value>, ZerxError> = arr.iter().map(&mut *f).collect();
            out.insert("items".to_owned(), Value::Array(new_items?));
        }
        Some(v) => {
            out.insert("items".to_owned(), f(&v)?);
        }
        None => {}
    }

    // prefixItems (tuple members)
    if let Some(Value::Array(arr)) = out.get("prefixItems").cloned() {
        let new_arr: Result<Vec<Value>, ZerxError> = arr.iter().map(&mut *f).collect();
        out.insert("prefixItems".to_owned(), Value::Array(new_arr?));
    }

    // additionalProperties when it is a schema object (not a bool)
    if let Some(Value::Object(ap)) = out.get("additionalProperties").cloned() {
        out.insert("additionalProperties".to_owned(), f(&Value::Object(ap))?);
    }

    // anyOf / oneOf / allOf
    for key in &["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(arr)) = out.get(*key).cloned() {
            let new_arr: Result<Vec<Value>, ZerxError> = arr.iter().map(&mut *f).collect();
            out.insert(key.to_string(), Value::Array(new_arr?));
        }
    }

    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// Deref transform
// ---------------------------------------------------------------------------

fn make_deref_transform(resolver: RefResolver) -> SchemaTransform {
    Arc::new(move |value: &Value| deref_walk(value, &resolver, &mut Vec::new(), 0))
}

// Inlines every external `$ref`, then walks the result. `active` holds the
// external refs whose resolved content is being walked: meeting one of them
// again is a recursive external schema, which inlining cannot represent, and
// an unbounded chain of distinct refs is cut at MAX_PARSE_DEPTH. `depth` is how
// many JSON containers sit above `node`; inlining grows the document, so the
// walk stops at the document bound rather than only at the ref-chain cut. The
// walk is split so each nesting level keeps only two small frames on the stack:
// an importable document reaches the bound in debug builds on a 2 MB thread.
fn deref_walk(
    node: &Value,
    resolver: &RefResolver,
    active: &mut Vec<String>,
    depth: usize,
) -> Result<Value, ZerxError> {
    if depth >= MAX_IMPORT_DOCUMENT_DEPTH {
        return Err(import_document_depth_error());
    }
    let obj = match node.as_object() {
        Some(o) => o,
        None => return Ok(node.clone()),
    };
    let ref_str = match obj.get("$ref").and_then(|v| v.as_str()) {
        // Every `#…` ref is document-local: `#/$defs/…` is resolved by the
        // core importer, and any other local form (`#`, `#anchor`,
        // `#/properties/…`) is left for it to refuse. Handing one to the
        // caller's resolver could import content the document never names.
        Some(s) if !s.starts_with('#') => s,
        _ => return deref_children(obj, resolver, active, depth),
    };
    // The resolved document runs before `has_document_local_refs`, which
    // recurses over the whole tree, and is inlined into a document already
    // `depth` deep.
    let merged = deref_resolve(obj, ref_str, resolver, active)?;
    active.push(ref_str.to_owned());
    let result = deref_walk(&merged, resolver, active, depth);
    active.pop();
    result
}

// Resolves one external ref and merges its sibling keywords (siblings win).
fn deref_resolve(
    obj: &serde_json::Map<String, Value>,
    ref_str: &str,
    resolver: &RefResolver,
    active: &[String],
) -> Result<Value, ZerxError> {
    if active.iter().any(|r| r == ref_str) {
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("deref: external ref '{ref_str}' refers to itself; recursive external schemas are not supported"),
        ));
    }
    if active.len() >= MAX_PARSE_DEPTH {
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("deref: external ref nesting exceeded maximum depth of {MAX_PARSE_DEPTH}"),
        ));
    }
    let resolved = resolver(ref_str)?;
    if json_depth_exceeds(&resolved, MAX_IMPORT_DOCUMENT_DEPTH) {
        // Dispose iteratively: this value is ours now, and its own drop would
        // recurse once per level.
        dispose_json_value(resolved);
        return Err(import_document_depth_error());
    }
    if has_document_local_refs(&resolved) {
        return Err(ZerxError::new(
            ErrorCode::IMPORT_UNSUPPORTED,
            format!("deref: content resolved for '{ref_str}' carries its own $defs or a local '#' ref; such refs would bind to the importing document"),
        ));
    }
    let mut merged = match resolved.into_object() {
        Some(m) => m,
        None => {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                format!("deref: resolver did not return an object for ref '{ref_str}'"),
            ));
        }
    };
    for (k, v) in obj {
        if k != "$ref" {
            merged.insert(k.clone(), v.clone());
        }
    }
    Ok(Value::Object(merged))
}

// Walks every child schema position of an object node, one JSON container level
// per keyword container (`properties`/`$defs`, an `items`/`prefixItems`/
// `anyOf`/`oneOf`/`allOf` array) and one per directly held child (`items` as a
// single schema, `additionalProperties`).
fn deref_children(
    obj: &serde_json::Map<String, Value>,
    resolver: &RefResolver,
    active: &mut Vec<String>,
    depth: usize,
) -> Result<Value, ZerxError> {
    let mut out = obj.clone();

    if let Some(Value::Object(props)) = out.get("properties").cloned() {
        let mut new_props = serde_json::Map::new();
        for (k, v) in &props {
            new_props.insert(k.clone(), deref_walk(v, resolver, active, depth + 2)?);
        }
        out.insert("properties".to_owned(), Value::Object(new_props));
    }

    if let Some(Value::Object(defs)) = out.get("$defs").cloned() {
        let mut new_defs = serde_json::Map::new();
        for (k, v) in &defs {
            new_defs.insert(k.clone(), deref_walk(v, resolver, active, depth + 2)?);
        }
        out.insert("$defs".to_owned(), Value::Object(new_defs));
    }

    match out.get("items").cloned() {
        Some(Value::Array(arr)) => {
            let mut new_items = Vec::with_capacity(arr.len());
            for v in &arr {
                new_items.push(deref_walk(v, resolver, active, depth + 2)?);
            }
            out.insert("items".to_owned(), Value::Array(new_items));
        }
        Some(v) => {
            out.insert(
                "items".to_owned(),
                deref_walk(&v, resolver, active, depth + 1)?,
            );
        }
        None => {}
    }

    if let Some(Value::Array(arr)) = out.get("prefixItems").cloned() {
        let mut new_arr = Vec::with_capacity(arr.len());
        for v in &arr {
            new_arr.push(deref_walk(v, resolver, active, depth + 2)?);
        }
        out.insert("prefixItems".to_owned(), Value::Array(new_arr));
    }

    if let Some(Value::Object(ap)) = out.get("additionalProperties").cloned() {
        out.insert(
            "additionalProperties".to_owned(),
            deref_walk(&Value::Object(ap), resolver, active, depth + 1)?,
        );
    }

    for key in &["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(arr)) = out.get(*key).cloned() {
            let mut new_arr = Vec::with_capacity(arr.len());
            for v in &arr {
                new_arr.push(deref_walk(v, resolver, active, depth + 2)?);
            }
            out.insert(key.to_string(), Value::Array(new_arr));
        }
    }

    Ok(Value::Object(out))
}

// Local refs and `$defs` in resolved content are relative to the external
// document, which is inlined without its root; the core importer would bind
// them to the importing document's `$defs` instead.
fn has_document_local_refs(value: &Value) -> bool {
    match value {
        Value::Object(o) => o.iter().any(|(k, v)| {
            k == "$defs"
                || (k == "$ref" && v.as_str().is_some_and(|r| r.starts_with('#')))
                || has_document_local_refs(v)
        }),
        Value::Array(a) => a.iter().any(has_document_local_refs),
        _ => false,
    }
}

trait IntoObject {
    fn into_object(self) -> Option<serde_json::Map<String, Value>>;
}

impl IntoObject for Value {
    fn into_object(self) -> Option<serde_json::Map<String, Value>> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in sql schema transforms
// ---------------------------------------------------------------------------

// Keywords that can constrain `null` itself. `[T, "null"]` → `(T and K) or null`
// is exact only when none of these appears in the node.
const NULL_CONSTRAINING: &[&str] = &[
    "enum",
    "const",
    "anyOf",
    "oneOf",
    "allOf",
    "not",
    "$ref",
    "$dynamicRef",
    "if",
    "then",
    "else",
];

fn sql_nullable_normalise(value: &Value) -> Result<Value, ZerxError> {
    // The root is rewritten on its own so only it may lift `$id`; children
    // go through the ordinary deep map.
    let root = sql_nullable_rewrite(value, true)?;
    map_child_schemas(root, &mut |child| {
        deep_map_schema(child, &|node| sql_nullable_rewrite(node, false))
    })
}

// type: ["T", "null"] (exactly two strings, one null) → anyOf, but only when
// the rewrite is exact: no keyword in the node constrains null. Document
// metadata stays on the outer node: `$defs` and `$schema` always, and `$id`
// at the root, where the importer accepts it and `$ref`s resolve against it.
// A nested `$id` stays in the core member and is refused as before.
fn sql_nullable_rewrite(node: &Value, is_root: bool) -> Result<Value, ZerxError> {
    let obj = match node.as_object() {
        Some(o) => o,
        None => return Ok(node.clone()),
    };

    if let Some(Value::Array(types)) = obj.get("type") {
        if types.len() == 2 {
            let strs: Option<Vec<&str>> = types.iter().map(|v| v.as_str()).collect();
            if let Some(strs) = strs {
                let null_count = strs.iter().filter(|&&t| t == "null").count();
                if null_count == 1 && !NULL_CONSTRAINING.iter().any(|k| obj.contains_key(*k)) {
                    let other = *strs.iter().find(|&&t| t != "null").unwrap();
                    let mut core = obj.clone();
                    core.insert("type".to_owned(), Value::String(other.to_owned()));
                    let mut outer = serde_json::Map::new();
                    let lifted: &[&str] = if is_root {
                        &["$id", "$schema", "$defs"]
                    } else {
                        &["$schema", "$defs"]
                    };
                    for key in lifted {
                        if let Some(v) = core.remove(*key) {
                            outer.insert((*key).to_owned(), v);
                        }
                    }
                    outer.insert(
                        "anyOf".to_owned(),
                        Value::Array(vec![
                            Value::Object(core),
                            serde_json::json!({"type": "null"}),
                        ]),
                    );
                    return Ok(Value::Object(outer));
                }
            }
        }
    }

    // `oneOf` is left to the core importer: with real exactly-one semantics
    // the old `anyOf` rewrite is not exact (it accepts `null` even when the
    // non-null member also admits it).

    Ok(node.clone())
}

fn sql_array_items_fallback(value: &Value) -> Result<Value, ZerxError> {
    deep_map_schema(value, &|node: &Value| {
        let obj = match node.as_object() {
            Some(o) => o,
            None => return Ok(node.clone()),
        };
        if obj.get("type").and_then(|v| v.as_str()) == Some("array") && !obj.contains_key("items") {
            let mut new_obj = obj.clone();
            new_obj.insert("items".to_owned(), Value::Object(serde_json::Map::new()));
            return Ok(Value::Object(new_obj));
        }
        Ok(node.clone())
    })
}

const NUMERIC_ASSERTIONS: &[&str] = &[
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
];

fn sql_pg_type_substitute(value: &Value) -> Result<Value, ZerxError> {
    deep_map_schema(value, &|node: &Value| {
        let obj = match node.as_object() {
            Some(o) => o,
            None => return Ok(node.clone()),
        };

        let typ = obj.get("type").and_then(|v| v.as_str());
        let fmt = obj.get("format").and_then(|v| v.as_str());
        let pg = obj.get("x-pg-type").and_then(|v| v.as_str());

        if typ == Some("string") {
            // bytea → buffer  (format check is case-insensitive; also remove x-pg-type)
            if fmt
                .map(|s| s.eq_ignore_ascii_case("bytea"))
                .unwrap_or(false)
            {
                let mut new_obj = obj.clone();
                new_obj.remove("x-pg-type");
                new_obj.insert("format".to_owned(), Value::String("buffer".to_owned()));
                return Ok(Value::Object(new_obj));
            }
            if pg == Some("bytea") {
                let mut new_obj = obj.clone();
                new_obj.remove("x-pg-type");
                new_obj.insert("format".to_owned(), Value::String("buffer".to_owned()));
                return Ok(Value::Object(new_obj));
            }
            // timestamp variants → date-time
            if matches!(
                fmt,
                Some("timestamp without time zone")
                    | Some("timestamp with time zone")
                    | Some("timestamptz")
            ) {
                let mut new_obj = obj.clone();
                new_obj.insert("format".to_owned(), Value::String("date-time".to_owned()));
                return Ok(Value::Object(new_obj));
            }
        }

        // json/jsonb → {"format":"json"} regardless of type
        if pg == Some("json") || pg == Some("jsonb") {
            let mut new_obj = obj.clone();
            new_obj.remove("x-pg-type");
            new_obj.remove("type");
            new_obj.insert("format".to_owned(), Value::String("json".to_owned()));
            return Ok(Value::Object(new_obj));
        }

        if typ == Some("number") {
            let string_rewrite = matches!(fmt, Some("int64") | Some("numeric") | Some("decimal"))
                || matches!(pg, Some("int8") | Some("numeric"));
            // A string cannot carry numeric bounds: the core importer would
            // read them as vacuous for the rewritten type and drop them.
            if string_rewrite {
                if let Some(kw) = NUMERIC_ASSERTIONS.iter().find(|k| obj.contains_key(**k)) {
                    return Err(ZerxError::new(
                        ErrorCode::IMPORT_UNSUPPORTED,
                        format!("sql: '{kw}' cannot be enforced on a number imported as a string"),
                    ));
                }
            }
            // int64 / int8 → string  (also remove x-pg-type)
            if fmt == Some("int64") {
                let mut new_obj = obj.clone();
                new_obj.remove("format");
                new_obj.remove("x-pg-type");
                new_obj.insert("type".to_owned(), Value::String("string".to_owned()));
                return Ok(Value::Object(new_obj));
            }
            if pg == Some("int8") {
                let mut new_obj = obj.clone();
                new_obj.remove("x-pg-type");
                new_obj.insert("type".to_owned(), Value::String("string".to_owned()));
                return Ok(Value::Object(new_obj));
            }
            // numeric / decimal → string  (also remove x-pg-type)
            if matches!(fmt, Some("numeric") | Some("decimal")) {
                let mut new_obj = obj.clone();
                new_obj.remove("format");
                new_obj.remove("x-pg-type");
                new_obj.insert("type".to_owned(), Value::String("string".to_owned()));
                return Ok(Value::Object(new_obj));
            }
            if pg == Some("numeric") {
                let mut new_obj = obj.clone();
                new_obj.remove("x-pg-type");
                new_obj.insert("type".to_owned(), Value::String("string".to_owned()));
                return Ok(Value::Object(new_obj));
            }
        }

        Ok(node.clone())
    })
}

// ---------------------------------------------------------------------------
// Built-in sql policy
// ---------------------------------------------------------------------------

fn builtin_sql_policy() -> Policy {
    Policy {
        schema_transforms: vec![
            Arc::new(|v: &Value| sql_nullable_normalise(v)),
            Arc::new(|v: &Value| sql_array_items_fallback(v)),
            Arc::new(|v: &Value| sql_pg_type_substitute(v)),
        ],
        type_transforms: vec![],
    }
}

// ---------------------------------------------------------------------------
// Composer + apply_type_transforms
// ---------------------------------------------------------------------------

/// Apply a sequence of type transforms to a root schema (non-recursive root-only fold).
pub fn apply_type_transforms(
    schema: Schema,
    transforms: &[TypeTransform],
) -> Result<Schema, ZerxError> {
    let mut current = schema;
    for t in transforms {
        current = t(current)?;
    }
    Ok(current)
}

/// Bound a transformed document this function now owns. An over-bound value is
/// disposed iteratively rather than dropped, since its own drop would recurse
/// once per nesting level.
fn guard_owned_document(value: &mut Value) -> Result<(), ZerxError> {
    if json_depth_exceeds(value, MAX_IMPORT_DOCUMENT_DEPTH) {
        dispose_json_value(std::mem::replace(value, Value::Null));
        return Err(import_document_depth_error());
    }
    Ok(())
}

/// Import a JSON Schema with policy and transform options.
pub fn from_json_schema_with(value: &Value, opts: &ImportOptions) -> Result<Schema, ZerxError> {
    // Bound the caller's document before the first clone, and every transform
    // output before it is handed on: no raw `Value` on this path may be cloned,
    // compared or walked beyond the import document bound.
    check_import_document_depth(value)?;

    // Step 1: Resolve named policy.
    let policy = if let Some(name) = &opts.policy {
        lookup_policy(name).ok_or_else(|| {
            ZerxError::new(
                ErrorCode::POLICY_UNKNOWN,
                format!("unknown policy: '{name}'"),
            )
        })?
    } else {
        Policy::default()
    };

    // Step 2: Apply ordered schema transforms: [deref?, policy.schema_transforms…, opts.schema_transforms…].
    let mut effective = value.clone();
    if let Some(resolver) = opts.deref.clone() {
        effective = make_deref_transform(resolver)(&effective)?;
        guard_owned_document(&mut effective)?;
    }
    for t in &policy.schema_transforms {
        effective = t(&effective)?;
        guard_owned_document(&mut effective)?;
    }
    for t in &opts.schema_transforms {
        effective = t(&effective)?;
        guard_owned_document(&mut effective)?;
    }

    // Step 3: core import.
    let schema = crate::json_schema::from_json_schema_inner(&effective, opts.strip_unknown)?;

    // Step 4: Apply ordered type transforms: [policy.type_transforms…, opts.type_transforms…].
    let mut type_transforms: Vec<TypeTransform> = Vec::new();
    type_transforms.extend(policy.type_transforms.iter().cloned());
    type_transforms.extend(opts.type_transforms.iter().cloned());

    apply_type_transforms(schema, &type_transforms)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn import_sql(v: Value) -> Result<Schema, ZerxError> {
        from_json_schema_with(
            &v,
            &ImportOptions {
                policy: Some("sql".into()),
                ..Default::default()
            },
        )
    }

    fn re_export(s: &Schema) -> Value {
        s.to_json_schema()
    }

    // 1. Registry + unknown policy
    #[test]
    fn test_unknown_policy_error() {
        let v = json!({"type": "string"});
        let result = from_json_schema_with(
            &v,
            &ImportOptions {
                policy: Some("nope".into()),
                ..Default::default()
            },
        );
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::POLICY_UNKNOWN);
        assert!(err.message.contains("nope"));
    }

    #[test]
    fn test_sql_policy_resolves_without_prior_register() {
        let v = json!({"type": "string"});
        assert!(from_json_schema_with(
            &v,
            &ImportOptions {
                policy: Some("sql".into()),
                ..Default::default()
            },
        )
        .is_ok());
    }

    // 2. register_policy round-trip
    #[test]
    fn test_register_policy_round_trip() {
        let policy_name = "test_register_policy_round_trip";
        register_policy(
            policy_name,
            Policy {
                schema_transforms: vec![],
                type_transforms: vec![Arc::new(|_s: Schema| Ok(crate::string().into()))],
            },
        );
        let result = from_json_schema_with(
            &json!({"type": "number"}),
            &ImportOptions {
                policy: Some(policy_name.into()),
                ..Default::default()
            },
        );
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    // 3. apply_type_transforms fold + ordering
    #[test]
    fn test_apply_type_transforms_ordering() {
        // The first transform converts any schema to string; the second verifies it sees a string schema.
        let t1: TypeTransform = Arc::new(|_s: Schema| Ok(crate::string().into()));
        let t2: TypeTransform = Arc::new(|s: Schema| {
            let exported = s.to_json_schema();
            if exported.get("type").and_then(|v| v.as_str()) == Some("string") {
                Ok(s)
            } else {
                Err(ZerxError::new(
                    ErrorCode::UNKNOWN_ERROR,
                    "t1 did not run first",
                ))
            }
        });
        assert!(apply_type_transforms(crate::any().into(), &[t1, t2]).is_ok());
    }

    #[test]
    fn test_apply_type_transforms_error_propagates() {
        let t_err: TypeTransform =
            Arc::new(|_s: Schema| Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "transform error")));
        assert!(apply_type_transforms(crate::any().into(), &[t_err]).is_err());
    }

    // 4. deref hook + sibling merge
    #[test]
    fn test_deref_external_ref_resolves_to_string() {
        let resolver: RefResolver = Arc::new(|_r: &str| Ok(json!({"type": "string"})));
        let result = from_json_schema_with(
            &json!({"$ref": "https://x/y"}),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        );
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_deref_sibling_description_survives() {
        let resolver: RefResolver = Arc::new(|_r: &str| Ok(json!({"type": "string"})));
        let result = from_json_schema_with(
            &json!({"$ref": "https://x/y", "description": "d"}),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        );
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("description").and_then(|v| v.as_str()),
            Some("d")
        );
    }

    #[test]
    fn test_deref_local_ref_left_to_core_importer() {
        // Deref must NOT be called for local #/$defs/… refs.
        let resolver: RefResolver = Arc::new(|_r: &str| {
            Err(ZerxError::new(
                ErrorCode::UNKNOWN_ERROR,
                "should not be called",
            ))
        });
        let result = from_json_schema_with(
            &json!({
                "$ref": "#/$defs/S1",
                "$defs": {"S1": {"type": "string"}}
            }),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_deref_never_sends_document_local_refs_to_resolver() {
        // Every `#…` ref is document-local; the resolver must never see it,
        // and the result must equal the plain core import of the document.
        let calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = calls.clone();
        let resolver: RefResolver = Arc::new(move |r: &str| {
            seen.lock().unwrap().push(r.to_owned());
            Ok(json!({"type": "string"}))
        });
        for doc in [
            json!({"$ref": "#"}),
            json!({"$ref": "#a"}),
            json!({"type": "object", "properties": {"x": {"$ref": "#/properties/y"}}}),
        ] {
            let with = from_json_schema_with(
                &doc,
                &ImportOptions {
                    deref: Some(resolver.clone()),
                    ..Default::default()
                },
            );
            let plain = crate::from_json_schema(&doc);
            assert_eq!(
                with.as_ref().err().map(|e| e.code.clone()),
                plain.as_ref().err().map(|e| e.code.clone()),
                "deref result differs from core import for {doc}"
            );
        }
        assert!(
            calls.lock().unwrap().is_empty(),
            "resolver called: {:?}",
            calls.lock().unwrap()
        );
    }

    #[test]
    fn test_deref_resolver_error_propagates() {
        let resolver: RefResolver =
            Arc::new(|_r: &str| Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "resolver error")));
        let result = from_json_schema_with(
            &json!({"$ref": "https://x/y"}),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    // 5. sql nullable normalisation
    #[test]
    fn test_sql_nullable_array_type() {
        let result = import_sql(json!({"type": ["string", "null"]}));
        let exported = re_export(&result.unwrap());
        // Collapsed to nullable anyOf
        assert!(exported.get("anyOf").is_some());
    }

    #[test]
    fn test_sql_nullable_root_keeps_root_id() {
        let schema = import_sql(json!({"$id": "x", "type": ["string", "null"]})).unwrap();
        assert!(schema.validate(&"a").is_ok());
        assert!(schema.validate(&serde_json::Value::Null).is_ok());
        assert!(schema.validate(&1).is_err());
    }

    #[test]
    fn test_sql_nullable_root_metadata_resolves_like_plain_import() {
        let doc = json!({
            "$id": "https://example.test/s",
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {"Name": {"type": "string", "minLength": 2}},
            "type": ["object", "null"],
            "properties": {"n": {"$ref": "#/$defs/Name"}},
            "required": ["n"]
        });
        let sql = import_sql(doc.clone()).unwrap();
        let plain = crate::from_json_schema(&doc).unwrap();
        for v in [
            json!({"n": "ab"}),
            json!({"n": "a"}),
            json!({"n": 1}),
            json!({}),
            serde_json::Value::Null,
            json!("x"),
        ] {
            assert_eq!(
                sql.validate(&v).is_ok(),
                plain.validate(&v).is_ok(),
                "sql and plain import disagree on {v}"
            );
        }
    }

    #[test]
    fn test_sql_nullable_nested_id_still_refused() {
        let err = import_sql(json!({
            "type": "object",
            "properties": {"a": {"$id": "y", "type": ["string", "null"]}}
        }))
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
    }

    #[test]
    fn test_sql_nullable_one_of() {
        let result = import_sql(json!({"oneOf": [{"type": "number"}, {"type": "null"}]}));
        let exported = re_export(&result.unwrap());
        assert!(exported.get("oneOf").is_some());
        assert!(exported.get("anyOf").is_none());
    }

    #[test]
    fn test_sql_one_of_keeps_exactly_one_semantics() {
        let s = import_sql(json!({"oneOf": [{"type": "number"}, {"type": "null"}]})).unwrap();
        assert!(s.validate(&json!(3)).is_ok());
        assert!(s.validate(&json!(null)).is_ok());
        assert!(s.validate(&json!("x")).is_err());

        // `{}` already admits null, so `null` matches both variants.
        let ambiguous = import_sql(json!({"oneOf": [{}, {"type": "null"}]})).unwrap();
        assert_eq!(
            ambiguous.validate(&json!(null)).unwrap_err().code,
            ErrorCode::ONE_OF_AMBIGUOUS
        );
    }

    // 6. sql array items fallback
    #[test]
    fn test_sql_array_items_fallback() {
        let v = json!({"type": "array"});
        // The bare importer reads a missing `items` as any element.
        assert!(crate::json_schema::from_json_schema(&v).is_ok());
        // The sql policy adds items:{}, with the same result.
        let result = import_sql(v);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            re_export(&result.unwrap())
                .get("type")
                .and_then(|v| v.as_str()),
            Some("array")
        );
    }

    // 7. sql type substitution — bytea → buffer
    #[test]
    fn test_sql_bytea_format() {
        let result = import_sql(json!({"type": "string", "format": "bytea"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("format").and_then(|v| v.as_str()),
            Some("buffer")
        );
    }

    #[test]
    fn test_sql_bytea_pg_type() {
        let result = import_sql(json!({"type": "string", "x-pg-type": "bytea"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("format").and_then(|v| v.as_str()),
            Some("buffer")
        );
    }

    // 8. sql type substitution — json/jsonb → json()
    #[test]
    fn test_sql_jsonb_pg_type() {
        let result = import_sql(json!({"type": "string", "x-pg-type": "jsonb"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("format").and_then(|v| v.as_str()),
            Some("json")
        );
    }

    // 9. sql type substitution — int64 / numeric → string
    #[test]
    fn test_sql_int64_format() {
        let result = import_sql(json!({"type": "number", "format": "int64"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_sql_int8_pg_type() {
        let result = import_sql(json!({"type": "number", "x-pg-type": "int8"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_sql_numeric_format() {
        let result = import_sql(json!({"type": "number", "format": "numeric"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    // 10. sql type substitution — timestamp → date-time
    #[test]
    fn test_sql_timestamptz_format() {
        let result = import_sql(json!({"type": "string", "format": "timestamptz"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("format").and_then(|v| v.as_str()),
            Some("date-time")
        );
    }

    // 11. sql nested recursion + modifier preservation
    #[test]
    fn test_sql_nested_object_with_modifiers() {
        let result = import_sql(json!({
            "type": "object",
            "properties": {
                "data": {"type": "string", "format": "bytea"},
                "count": {"type": "number", "format": "int64"}
            },
            "required": ["data"],
            "additionalProperties": false
        }));
        let exported = re_export(&result.unwrap());
        let props = exported.get("properties").unwrap().as_object().unwrap();
        // data: buffer (was bytea)
        assert_eq!(
            props
                .get("data")
                .unwrap()
                .get("format")
                .and_then(|v| v.as_str()),
            Some("buffer")
        );
        // count: string (was int64 number), optional
        assert_eq!(
            props
                .get("count")
                .unwrap()
                .get("type")
                .and_then(|v| v.as_str()),
            Some("string")
        );
        // strict mode preserved
        assert_eq!(
            exported
                .get("additionalProperties")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    // 12. sql leaves discriminated union intact
    #[test]
    fn test_sql_discriminated_union_with_int64() {
        // Discriminator field must be {"const":"..."} so the core import reads it as a literal and
        // the discriminated-union check passes.
        let result = import_sql(json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "kind": {"const": "a"},
                        "val": {"type": "number", "format": "int64"}
                    },
                    "required": ["kind", "val"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "kind": {"const": "b"},
                        "count": {"type": "number", "format": "int64"}
                    },
                    "required": ["kind", "count"],
                    "additionalProperties": false
                }
            ],
            "discriminator": {"propertyName": "kind"}
        }));
        assert!(result.is_ok(), "{result:?}");
        let exported = re_export(&result.unwrap());
        assert!(exported.get("oneOf").is_some());
        assert!(exported.get("discriminator").is_some());
    }

    // 13. End-to-end error propagation
    #[test]
    fn test_schema_transform_error_propagates() {
        let failing: SchemaTransform = Arc::new(|_v: &Value| {
            Err(ZerxError::new(
                ErrorCode::UNKNOWN_ERROR,
                "schema transform error",
            ))
        });
        let result = from_json_schema_with(
            &json!({"type": "string"}),
            &ImportOptions {
                schema_transforms: vec![failing],
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_type_transform_error_propagates() {
        let failing: TypeTransform = Arc::new(|_s: Schema| {
            Err(ZerxError::new(
                ErrorCode::UNKNOWN_ERROR,
                "type transform error",
            ))
        });
        let result = from_json_schema_with(
            &json!({"type": "string"}),
            &ImportOptions {
                type_transforms: vec![failing],
                ..Default::default()
            },
        );
        assert!(result.is_err());
    }

    // 14. sql substitution reaches $defs (lazy/reusable-def case)
    #[test]
    fn test_sql_substitution_reaches_defs() {
        let result = import_sql(json!({
            "$ref": "#/$defs/Row",
            "$defs": {
                "Row": {
                    "type": "object",
                    "properties": {
                        "data": {"type": "string", "format": "bytea"}
                    },
                    "required": ["data"],
                    "additionalProperties": false
                }
            }
        }));
        assert!(result.is_ok(), "{result:?}");
        let exported = re_export(&result.unwrap());
        // The exporter assigns auto-generated $defs keys (e.g. "S1"), not the
        // original "Row" name.  Follow the $ref from the root to find the key.
        let ref_str = exported.get("$ref").unwrap().as_str().unwrap();
        let def_key = ref_str.strip_prefix("#/$defs/").unwrap();
        let defs = exported.get("$defs").unwrap().as_object().unwrap();
        let data_fmt = defs
            .get(def_key)
            .unwrap()
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap()
            .get("data")
            .unwrap()
            .get("format")
            .and_then(|v| v.as_str());
        assert_eq!(data_fmt, Some("buffer"));
    }

    // 15. deep_map_schema reaches tuple and record positions
    #[test]
    fn test_deep_map_reaches_prefix_items() {
        let result = import_sql(json!({
            "type": "array",
            "prefixItems": [{"type": "string", "format": "bytea"}],
            "items": false
        }));
        assert!(result.is_ok(), "{result:?}");
        let exported = re_export(&result.unwrap());
        let member_fmt = exported.get("prefixItems").unwrap().as_array().unwrap()[0]
            .get("format")
            .and_then(|v| v.as_str());
        assert_eq!(member_fmt, Some("buffer"));
    }

    #[test]
    fn test_deep_map_reaches_additional_properties() {
        let result = import_sql(json!({
            "type": "object",
            "format": "record",
            "additionalProperties": {"type": "string", "format": "bytea"}
        }));
        assert!(result.is_ok(), "{result:?}");
        let exported = re_export(&result.unwrap());
        let ap_fmt = exported
            .get("additionalProperties")
            .unwrap()
            .get("format")
            .and_then(|v| v.as_str());
        assert_eq!(ap_fmt, Some("buffer"));
    }

    // Regression: dual-signal nodes (format + x-pg-type both present) must not
    // leak x-pg-type into the output as extension meta.
    #[test]
    fn test_sql_dual_signal_bytea_no_leaked_pg_type() {
        // Both format:"bytea" and x-pg-type:"bytea" present; x-pg-type must be consumed.
        let result = import_sql(json!({"type": "string", "format": "bytea", "x-pg-type": "bytea"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("format").and_then(|v| v.as_str()),
            Some("buffer")
        );
        assert!(
            exported.get("x-pg-type").is_none(),
            "x-pg-type leaked into export"
        );
    }

    #[test]
    fn test_sql_dual_signal_int64_no_leaked_pg_type() {
        // format:"int64" and x-pg-type:"int8" both present; x-pg-type must be consumed.
        let result = import_sql(json!({"type": "number", "format": "int64", "x-pg-type": "int8"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
        assert!(
            exported.get("x-pg-type").is_none(),
            "x-pg-type leaked into export"
        );
    }

    #[test]
    fn test_sql_dual_signal_numeric_no_leaked_pg_type() {
        // format:"numeric" and x-pg-type:"numeric" both present; x-pg-type must be consumed.
        let result =
            import_sql(json!({"type": "number", "format": "numeric", "x-pg-type": "numeric"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(
            exported.get("type").and_then(|v| v.as_str()),
            Some("string")
        );
        assert!(
            exported.get("x-pg-type").is_none(),
            "x-pg-type leaked into export"
        );
    }

    // 16. strip_unknown
    #[test]
    fn test_strip_unknown_default_is_reject() {
        let schema = from_json_schema_with(
            &json!({
                "type":"object",
                "properties":{"a":{"type":"string"}},
                "required":["a"],
                "additionalProperties":false
            }),
            &ImportOptions::default(),
        )
        .unwrap();
        let err = schema.validate(&json!({"a":"x","stale":1})).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
    }

    #[test]
    fn test_strip_unknown_via_import_options() {
        let schema = from_json_schema_with(
            &json!({
                "type":"object",
                "properties":{"a":{"type":"string"}},
                "required":["a"],
                "additionalProperties":false
            }),
            &ImportOptions {
                strip_unknown: true,
                ..Default::default()
            },
        )
        .unwrap();
        let ok = schema.validate(&json!({"a":"x","stale":1})).unwrap();
        assert!(ok.as_object().unwrap().get("stale").is_none());
    }

    // Regression guard for decision 1: strip_unknown is orthogonal to `policy` and
    // composes with it.
    #[test]
    fn test_strip_unknown_composes_with_sql_policy() {
        let schema = from_json_schema_with(
            &json!({
                "type":"object",
                "properties":{"amount":{"type":"number","format":"int64"}},
                "required":["amount"],
                "additionalProperties":false
            }),
            &ImportOptions {
                policy: Some("sql".into()),
                strip_unknown: true,
                ..Default::default()
            },
        )
        .unwrap();
        let exported = re_export(&schema);
        assert_eq!(
            exported["properties"]["amount"]["type"].as_str(),
            Some("string")
        );

        let ok = schema.validate(&json!({"amount":"42","stale":1})).unwrap();
        let obj = ok.as_object().unwrap();
        assert!(obj.get("amount").is_some());
        assert!(obj.get("stale").is_none());
    }

    // 17. Exact sql nullable normalisation — type array split.

    #[test]
    fn test_sql_nullable_array_accepts_base_and_null() {
        let s = import_sql(json!({"type": ["string", "null"]})).unwrap();
        assert!(s.validate(&json!("a")).is_ok());
        assert!(s.validate(&json!(null)).is_ok());
        assert!(s.validate(&json!(5)).is_err());

        let i = import_sql(json!({"type": ["integer", "null"]})).unwrap();
        assert!(i.validate(&json!(3)).is_ok());
        assert!(i.validate(&json!(null)).is_ok());
        assert!(i.validate(&json!("x")).is_err());
    }

    #[test]
    fn test_sql_nullable_bytea_becomes_nullable_buffer() {
        let s = import_sql(json!({"type": ["string", "null"], "format": "bytea"})).unwrap();
        assert!(s.validate(&json!(null)).is_ok());
        assert_eq!(re_export(&s)["anyOf"][0]["format"], json!("buffer"));
    }

    #[test]
    fn test_sql_nullable_array_items_fallback() {
        let s = import_sql(json!({"type": ["array", "null"]})).unwrap();
        assert!(s.validate(&json!(null)).is_ok());
        assert!(s.validate(&json!([])).is_ok());
    }

    #[test]
    fn test_sql_nullable_enum_rejects_null_unless_member() {
        let s = import_sql(json!({"type": ["string", "null"], "enum": ["a"]})).unwrap();
        assert!(s.validate(&json!("a")).is_ok());
        assert!(s.validate(&json!(null)).is_err());

        let with_null =
            import_sql(json!({"type": ["string", "null"], "enum": ["a", null]})).unwrap();
        assert!(with_null.validate(&json!(null)).is_ok());
    }

    #[test]
    fn test_sql_nullable_all_of_not_silently_dropped() {
        assert_eq!(
            import_sql(json!({"type": ["object", "null"], "allOf": [{"type": "object"}]}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    #[test]
    fn test_sql_malformed_nullable_type_array() {
        assert_eq!(
            import_sql(json!({"type": ["string", 5, "null"]}))
                .unwrap_err()
                .code,
            ErrorCode::IMPORT_MALFORMED
        );
    }

    #[test]
    fn test_sql_nullable_keeps_defs_reachable() {
        let s = import_sql(json!({
            "type": ["object", "null"],
            "properties": {"a": {"$ref": "#/$defs/A"}},
            "additionalProperties": false,
            "$defs": {"A": {"type": "string"}}
        }))
        .unwrap();
        assert!(s.validate(&json!(null)).is_ok());
        assert!(s.validate(&json!({"a": "x"})).is_ok());
        assert!(s.validate(&json!({"a": 1})).is_err());
    }

    // Exact sql nullable normalisation — oneOf branch.

    #[test]
    fn test_sql_nullable_one_of_with_type_sibling_refused() {
        assert_eq!(
            import_sql(json!({
                "type": ["string", "null"],
                "oneOf": [{"type": "number"}, {"type": "null"}]
            }))
            .unwrap_err()
            .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
        assert_eq!(
            import_sql(json!({
                "type": "string",
                "oneOf": [{"type": "number"}, {"type": "null"}]
            }))
            .unwrap_err()
            .code,
            ErrorCode::IMPORT_UNSUPPORTED
        );
    }

    #[test]
    fn test_sql_nullable_one_of_within_outer_type_imports() {
        let s = import_sql(json!({
            "type": ["string", "null"],
            "oneOf": [{"type": "string"}, {"type": "null"}]
        }))
        .unwrap();
        assert!(s.validate(&json!("a")).is_ok());
        assert!(s.validate(&json!(null)).is_ok());
        assert!(s.validate(&json!(5)).is_err());
    }

    #[test]
    fn test_sql_nullable_one_of_keeps_annotations_and_default() {
        let s = import_sql(json!({
            "oneOf": [{"type": "number"}, {"type": "null"}],
            "description": "d",
            "default": null
        }))
        .unwrap();
        let exported = re_export(&s);
        assert_eq!(exported["description"], json!("d"));
        assert_eq!(exported["default"], json!(null));

        let obj_schema = import_sql(json!({
            "type": "object",
            "properties": {
                "n": {
                    "oneOf": [{"type": "number"}, {"type": "null"}],
                    "description": "d",
                    "default": null
                }
            },
            "additionalProperties": false
        }))
        .unwrap();
        let out = obj_schema.validate(&json!({})).unwrap();
        assert_eq!(
            out.as_object().unwrap().get("n"),
            Some(&crate::ZerxValue::Null)
        );
    }

    #[test]
    fn test_sql_nullable_one_of_keeps_defs_reachable() {
        let s = import_sql(json!({
            "oneOf": [{"type": "number"}, {"type": "null"}],
            "$defs": {"A": {"type": "string"}}
        }))
        .unwrap();
        assert!(s.validate(&json!(null)).is_ok());
        assert_eq!(re_export(&s)["oneOf"][0]["type"], json!("number"));
    }

    // The driving use case, end to end, as a permanent test: a root-level and a
    // nested stale key both disappear in the same pass through the public entry point.
    #[test]
    fn test_strip_unknown_heals_nested_config() {
        let schema = from_json_schema_with(
            &json!({
                "type":"object",
                "properties":{
                    "host":{"type":"string"},
                    "tls":{
                        "type":"object",
                        "properties":{"enabled":{"type":"boolean"}},
                        "required":["enabled"],
                        "additionalProperties":false
                    }
                },
                "required":["host","tls"],
                "additionalProperties":false
            }),
            &ImportOptions {
                strip_unknown: true,
                ..Default::default()
            },
        )
        .unwrap();

        let ok = schema
            .validate(&json!({
                "host":"a",
                "legacy_port":1,
                "tls":{"enabled":true,"legacy_ca":"x"}
            }))
            .unwrap();
        let obj = ok.as_object().unwrap();
        assert!(obj.get("host").is_some());
        assert!(obj.get("legacy_port").is_none());
        let tls = obj.get("tls").unwrap().as_object().unwrap();
        assert!(tls.get("enabled").is_some());
        assert!(tls.get("legacy_ca").is_none());
    }

    #[test]
    fn test_deref_self_referential_external_ref_is_refused() {
        // A linked-list schema cannot be inlined; resolving it again inside
        // its own resolution must fail instead of recursing without end.
        let resolver: RefResolver = Arc::new(|_r: &str| {
            Ok(json!({"type": "object", "properties": {"next": {"$ref": "node.json"}}}))
        });
        let err = from_json_schema_with(
            &json!({"$ref": "node.json"}),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
    }

    #[test]
    fn test_deref_same_external_ref_twice_side_by_side_resolves() {
        // Repetition without nesting is not a cycle.
        let resolver: RefResolver = Arc::new(|_r: &str| Ok(json!({"type": "string"})));
        let schema = from_json_schema_with(
            &json!({
                "type": "object",
                "properties": {"a": {"$ref": "s.json"}, "b": {"$ref": "s.json"}},
                "required": ["a", "b"]
            }),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(schema.validate(&json!({"a": "x", "b": "y"})).is_ok());
        assert!(schema.validate(&json!({"a": "x", "b": 1})).is_err());
    }

    #[test]
    fn test_deref_resolved_content_with_local_refs_is_refused() {
        // A local ref inside resolved content belongs to the external
        // document; resolving it against the root's $defs would bind it to
        // the wrong schema.
        let resolver: RefResolver = Arc::new(|_r: &str| {
            Ok(json!({"$defs": {"Id": {"type": "integer", "minimum": 0}}, "$ref": "#/$defs/Id"}))
        });
        let err = from_json_schema_with(
            &json!({
                "$defs": {"Id": {"type": "string"}},
                "type": "object",
                "properties": {"x": {"$ref": "ext.json"}},
                "required": ["x"]
            }),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);

        let nested: RefResolver = Arc::new(|_r: &str| {
            Ok(json!({"type": "object", "properties": {"y": {"$ref": "#/$defs/Id"}}}))
        });
        let err = from_json_schema_with(
            &json!({"$defs": {"Id": {"type": "string"}}, "$ref": "ext.json"}),
            &ImportOptions {
                deref: Some(nested),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
    }

    #[test]
    fn test_sql_numeric_rewrite_refuses_numeric_assertions() {
        // The string rewrite cannot carry numeric bounds; the core importer
        // would treat them as vacuous for a string and drop them.
        for signal in [
            json!({"format": "int64"}),
            json!({"x-pg-type": "int8"}),
            json!({"format": "numeric"}),
            json!({"format": "decimal"}),
            json!({"x-pg-type": "numeric"}),
        ] {
            for (kw, val) in [
                ("minimum", json!(0)),
                ("maximum", json!(10)),
                ("exclusiveMinimum", json!(0)),
                ("exclusiveMaximum", json!(10)),
                ("multipleOf", json!(2)),
            ] {
                let mut node = json!({"type": "number"});
                for (k, v) in signal.as_object().unwrap() {
                    node[k] = v.clone();
                }
                node[kw] = val;
                let err = import_sql(node.clone()).unwrap_err();
                assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED, "{node}");
            }
        }
        // Without numeric assertions the documented rewrite still applies.
        assert!(import_sql(json!({"type": "number", "format": "int64"})).is_ok());
    }

    // ------------------------------------------------------------------
    // Import document depth bound
    // ------------------------------------------------------------------

    fn on_2mb_thread<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(f)
            .expect("spawn deep-test thread")
            .join()
            .expect("deep-test thread panicked");
    }

    fn deep_json_arrays(depth: usize) -> Value {
        let mut v = json!(0);
        for _ in 0..depth {
            v = Value::Array(vec![v]);
        }
        v
    }

    fn deep_schema_chain(kind: &str, levels: usize) -> Value {
        let mut v = Value::Object({
            let mut m = serde_json::Map::new();
            m.insert("type".to_string(), Value::String("string".to_string()));
            m
        });
        for _ in 0..levels {
            let mut node = serde_json::Map::new();
            match kind {
                "items" => {
                    node.insert("type".to_string(), Value::String("array".to_string()));
                    node.insert("items".to_string(), v);
                }
                "properties" => {
                    node.insert("type".to_string(), Value::String("object".to_string()));
                    let mut props = serde_json::Map::new();
                    props.insert("a".to_string(), v);
                    node.insert("properties".to_string(), Value::Object(props));
                    node.insert(
                        "required".to_string(),
                        Value::Array(vec![Value::String("a".to_string())]),
                    );
                }
                _ => unreachable!("unsupported chain kind: {kind}"),
            }
            v = Value::Object(node);
        }
        v
    }

    fn unused_defs_document(member: Value) -> Value {
        let mut entry = serde_json::Map::new();
        entry.insert("const".to_string(), member);
        let mut defs = serde_json::Map::new();
        defs.insert("X".to_string(), Value::Object(entry));
        let mut root = serde_json::Map::new();
        root.insert("$defs".to_string(), Value::Object(defs));
        Value::Object(root)
    }

    fn assert_document_depth_refused(doc: &Value, opts: &ImportOptions) {
        let err = from_json_schema_with(doc, opts).unwrap_err();
        assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
        assert!(
            err.message.contains("document depth limit"),
            "{}",
            err.message
        );
    }

    #[test]
    fn test_all_entries_refuse_over_deep_documents() {
        on_2mb_thread(|| {
            let defs_doc = unused_defs_document(deep_json_arrays(10_000));
            let items_doc = deep_schema_chain("items", 100_000);
            let properties_doc = deep_schema_chain("properties", 100_000);

            let default_opts = ImportOptions::default();
            let sql_opts = ImportOptions {
                policy: Some("sql".into()),
                ..Default::default()
            };
            let resolver: RefResolver = Arc::new(|_r: &str| Ok(json!({"type": "string"})));
            let deref_opts = ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            };

            for doc in [&defs_doc, &items_doc, &properties_doc] {
                for opts in [&default_opts, &sql_opts, &deref_opts] {
                    assert_document_depth_refused(doc, opts);
                }
            }

            std::mem::forget(defs_doc);
            std::mem::forget(items_doc);
            std::mem::forget(properties_doc);
        });
    }

    #[test]
    fn test_over_deep_schema_transform_output_refused() {
        on_2mb_thread(|| {
            let transform: SchemaTransform =
                Arc::new(|_v: &Value| Ok(unused_defs_document(deep_json_arrays(10_000))));
            let err = from_json_schema_with(
                &json!({"type": "string"}),
                &ImportOptions {
                    schema_transforms: vec![transform],
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("document depth limit"),
                "{}",
                err.message
            );
        });
    }

    #[test]
    fn test_over_deep_deref_resolver_result_refused() {
        on_2mb_thread(|| {
            let resolver: RefResolver = Arc::new(|_r: &str| {
                let mut v = json!({"type": "string"});
                for _ in 0..10_000 {
                    let mut m = serde_json::Map::new();
                    m.insert("const".to_string(), v);
                    v = Value::Object(m);
                }
                Ok(v)
            });
            let err = from_json_schema_with(
                &json!({"$ref": "deep.json"}),
                &ImportOptions {
                    deref: Some(resolver),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("document depth limit"),
                "{}",
                err.message
            );
        });
    }

    #[test]
    fn test_deref_combined_inline_depth_refused() {
        on_2mb_thread(|| {
            // Every resolved document adds six JSON levels around the next ref,
            // so the inlined result outgrows the bound while each single
            // resolution stays shallow. The ref-chain cut (100) must not be
            // what stops it.
            let resolver: RefResolver = Arc::new(|r: &str| {
                let n: u32 = r.trim_start_matches('r').parse().unwrap();
                if n == 0 {
                    return Ok(json!({"type": "string"}));
                }
                let mut node = json!({"$ref": format!("r{}", n - 1)});
                for _ in 0..3 {
                    node = json!({
                        "type": "object",
                        "properties": {"a": node},
                        "required": ["a"]
                    });
                }
                Ok(node)
            });
            let err = from_json_schema_with(
                &json!({"$ref": "r80"}),
                &ImportOptions {
                    deref: Some(resolver),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("document depth limit"),
                "{}",
                err.message
            );
        });
    }

    #[test]
    fn test_properties_chain_under_bound_reaches_import_walk() {
        on_2mb_thread(|| {
            // 199 nested object levels sit near 400 JSON levels: under the
            // document bound, so the import walk's own nesting guard reports it
            // after deref and the `sql` policy have run on the 2 MB thread.
            let doc = deep_schema_chain("properties", 199);
            let resolver: RefResolver = Arc::new(|_r: &str| Ok(json!({"type": "string"})));
            let err = from_json_schema_with(
                &doc,
                &ImportOptions {
                    policy: Some("sql".into()),
                    deref: Some(resolver),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::IMPORT_UNSUPPORTED);
            assert!(
                err.message.contains("import depth limit of 100"),
                "{}",
                err.message
            );
        });
    }
}
