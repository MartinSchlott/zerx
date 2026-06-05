// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::schema::{Schema, SchemaKind};
use crate::types::{DiscriminatorState, ObjectBody, ObjectMode};
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
    /// Export this schema to JSON Schema (Draft 2020-12). Infallible (C7).
    pub fn to_json_schema(&self) -> serde_json::Value {
        self.to_json_schema_with(&ExportOptions::default())
    }

    /// Export with options. Attaches `$schema` when `opts.dialect` is set.
    pub fn to_json_schema_with(&self, opts: &ExportOptions) -> serde_json::Value {
        let mut ctx = ExportContext::new();
        let mut root = export_node(self, &mut ctx);

        if let serde_json::Value::Object(ref mut map) = root {
            if !ctx.defs.is_empty() {
                map.insert(
                    "$defs".to_owned(),
                    serde_json::Value::Object(ctx.defs),
                );
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
    "type", "properties", "required", "additionalProperties",
    "items", "prefixItems", "minItems", "maxItems",
    "anyOf", "oneOf", "discriminator",
    "const", "enum",
    "$ref", "$defs", "$schema",
    "format", "contentMediaType",
    "minLength", "maxLength", "pattern",
    "minimum", "maximum",
    "description", "default", "examples",
    "deprecated", "readOnly", "writeOnly", "title",
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
    matches!(s.kind, SchemaKind::Any)
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

fn export_node(schema: &Schema, ctx: &mut ExportContext) -> serde_json::Value {
    // Step 1 — Lazy short-circuit
    if let SchemaKind::Lazy(lzy) = &schema.kind {
        let (id, is_new) = ctx.id_for(lzy.export_id());
        if is_new {
            // Placeholder prevents infinite recursion on cyclic schemas
            ctx.defs.insert(id.clone(), serde_json::Value::Object(serde_json::Map::new()));
            if let Ok(inner) = lzy.resolve() {
                let exported = export_node(&inner, ctx);
                ctx.defs.insert(id.clone(), exported);
            }
        }
        let mut core = serde_json::Map::new();
        core.insert("$ref".to_owned(), serde_json::Value::String(format!("#/$defs/{id}")));
        // Annotations/nullable still applied below (step 4+)
        apply_annotations(schema, ctx, &mut core);
        return wrap_nullable(schema, core);
    }

    // Step 2 — Base map
    let mut core = base_schema(&schema.kind, ctx);

    // Step 3 — Validator fragments (merge, last write wins)
    for v in &schema.validators {
        for (k, val) in v.json_schema() {
            core.insert(k, val);
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
        core.insert("description".to_owned(), serde_json::Value::String(d.clone()));
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
        core.insert("contentMediaType".to_owned(), serde_json::Value::String(mime.clone()));
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

fn wrap_nullable(schema: &Schema, core: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
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

fn base_schema(kind: &SchemaKind, ctx: &mut ExportContext) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    match kind {
        SchemaKind::Any => {}

        SchemaKind::String => {
            m.insert("type".to_owned(), serde_json::Value::String("string".to_owned()));
        }

        SchemaKind::Number => {
            m.insert("type".to_owned(), serde_json::Value::String("number".to_owned()));
        }

        SchemaKind::Boolean => {
            m.insert("type".to_owned(), serde_json::Value::String("boolean".to_owned()));
        }

        SchemaKind::Null => {
            m.insert("type".to_owned(), serde_json::Value::String("null".to_owned()));
        }

        SchemaKind::Enum(set) => {
            m.insert(
                "enum".to_owned(),
                serde_json::Value::Array(
                    set.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
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
            m.insert("type".to_owned(), serde_json::Value::String("array".to_owned()));
            m.insert("items".to_owned(), export_node(item, ctx));
        }

        SchemaKind::Record(value) => {
            m.insert("type".to_owned(), serde_json::Value::String("object".to_owned()));
            m.insert("format".to_owned(), serde_json::Value::String("record".to_owned()));
            m.insert("properties".to_owned(), serde_json::Value::Object(serde_json::Map::new()));
            let additional = if is_plain_any(value) {
                serde_json::Value::Bool(true)
            } else {
                export_node(value, ctx)
            };
            m.insert("additionalProperties".to_owned(), additional);
        }

        SchemaKind::Tuple(items) => {
            let n = items.len();
            m.insert("type".to_owned(), serde_json::Value::String("array".to_owned()));
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

        SchemaKind::DiscriminatedUnion(body) => {
            m.insert(
                "oneOf".to_owned(),
                serde_json::Value::Array(body.variants.iter().map(|s| export_node(s, ctx)).collect()),
            );
            let mut disc = serde_json::Map::new();
            disc.insert(
                "propertyName".to_owned(),
                serde_json::Value::String(body.key.clone()),
            );
            m.insert("discriminator".to_owned(), serde_json::Value::Object(disc));
        }

        SchemaKind::Buffer => {
            m.insert("type".to_owned(), serde_json::Value::String("string".to_owned()));
            m.insert("format".to_owned(), serde_json::Value::String("buffer".to_owned()));
        }

        SchemaKind::Uri => {
            m.insert("type".to_owned(), serde_json::Value::String("string".to_owned()));
            m.insert("format".to_owned(), serde_json::Value::String("uri".to_owned()));
        }

        SchemaKind::Url => {
            m.insert("type".to_owned(), serde_json::Value::String("string".to_owned()));
            m.insert("format".to_owned(), serde_json::Value::String("url".to_owned()));
        }

        SchemaKind::Json => {
            m.insert("format".to_owned(), serde_json::Value::String("json".to_owned()));
        }

        SchemaKind::JsonSchema => {
            m.insert("type".to_owned(), serde_json::Value::String("object".to_owned()));
            m.insert("format".to_owned(), serde_json::Value::String("jsonschema".to_owned()));
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
    m.insert("type".to_owned(), serde_json::Value::String("object".to_owned()));

    let mut properties = serde_json::Map::new();
    let mut required: Vec<serde_json::Value> = Vec::new();

    for (key, field) in &body.shape {
        properties.insert(key.clone(), export_node(field, ctx));
        if !body.all_optional
            && !field.modifiers.optional
            && field.modifiers.default.is_none()
        {
            required.push(serde_json::Value::String(key.clone()));
        }
    }

    m.insert("properties".to_owned(), serde_json::Value::Object(properties));

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
// Public import surface
// ---------------------------------------------------------------------------

/// Import a JSON Schema (Draft 2020-12) document back into a `Schema`. Fallible (C7).
pub fn from_json_schema(value: &serde_json::Value) -> Result<Schema, ZerxError> {
    let ctx = ImportContext::new(value);
    import_value(value, &ctx, &[])
}

// ---------------------------------------------------------------------------
// Import keyword classification
// ---------------------------------------------------------------------------

const IMPORT_HANDLED: &[&str] = &[
    "type", "properties", "required", "additionalProperties",
    "items", "prefixItems", "enum", "const",
    "anyOf", "oneOf", "$ref", "discriminator",
    "minLength", "maxLength", "pattern", "minimum", "maximum",
    "minItems", "maxItems", "format", "contentMediaType",
    "description", "title", "default", "examples",
    "deprecated", "readOnly", "writeOnly",
    "$defs", "$schema",
];

const IMPORT_UNSUPPORTED_STRUCTURAL: &[&str] = &[
    "allOf", "not", "if", "then", "else", "contains",
    "minContains", "maxContains", "patternProperties",
    "propertyNames", "dependentSchemas", "dependentRequired",
    "unevaluatedProperties", "unevaluatedItems", "$dynamicRef",
];

// ---------------------------------------------------------------------------
// ImportContext — $defs table + lazy memoisation + cycle guard
// ---------------------------------------------------------------------------

pub(crate) struct ImportContext {
    defs: serde_json::Map<String, serde_json::Value>,
    resolved: Rc<RefCell<HashMap<String, Schema>>>,
    placeholders: RefCell<HashMap<String, Schema>>,
    importing: RefCell<HashSet<String>>,
}

impl ImportContext {
    fn new(root: &serde_json::Value) -> Self {
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
        "properties", "required", "additionalProperties", "items", "prefixItems",
        "enum", "const", "anyOf", "oneOf", "$ref", "discriminator",
        "minLength", "maxLength", "pattern", "minimum", "maximum",
        "minItems", "maxItems", "allOf", "not", "format", "contentMediaType",
    ];
    !obj.keys().any(|k| k != "type" && STRUCTURAL.contains(&k.as_str()))
}

// ---------------------------------------------------------------------------
// Import walk — pub(crate) so P1 policy pipeline can wrap it
// ---------------------------------------------------------------------------

pub(crate) fn import_value(
    node: &serde_json::Value,
    ctx: &ImportContext,
    path: &[String],
) -> Result<Schema, ZerxError> {
    use crate::{
        any, array, boolean, buffer, discriminated_union, enumerate, json, jsonschema,
        literal, null, number, record, string, tuple, union, uri, url,
    };

    // Step 1: Boolean schema.
    match node {
        serde_json::Value::Bool(true) => return Ok(any().into()),
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

    let is_ref = obj.contains_key("$ref");
    let mut base: Option<Schema> = None;

    // Step 3: $ref — set base and skip to step 12.
    if is_ref {
        let ref_str = obj["$ref"].as_str().unwrap_or("");
        if !ref_str.starts_with("#/$defs/") {
            return Err(ZerxError::new(
                ErrorCode::IMPORT_MALFORMED,
                "only local `#/$defs/<id>` refs are supported",
            )
            .at(path.to_vec()));
        }
        let id = &ref_str["#/$defs/".len()..];
        base = Some(ctx.ref_schema(id, path)?);
    }

    if base.is_none() {
        // Step 4: allOf / not — raise clear error.
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

        // Step 5: Nullable collapse.
        if let Some(any_of_arr) = obj.get("anyOf").and_then(|v| v.as_array()) {
            if any_of_arr.len() == 2 {
                if let Some(null_i) = any_of_arr.iter().position(is_bare_null_schema) {
                    let other_i = 1 - null_i;
                    let mut child_path = path.to_vec();
                    child_path.push("anyOf".to_string());
                    let other = import_value(&any_of_arr[other_i], ctx, &child_path)?;
                    base = Some(other.nullable());
                    // Continue to step 12.
                }
            }
        }
    }

    // Step 6: oneOf / anyOf (not consumed by nullable collapse).
    if base.is_none() {
        let (arr_opt, is_one_of) = if let Some(a) = obj.get("oneOf").and_then(|v| v.as_array()) {
            (Some(a), true)
        } else if let Some(a) = obj.get("anyOf").and_then(|v| v.as_array()) {
            (Some(a), false)
        } else {
            (None, false)
        };

        if let Some(arr) = arr_opt {
            let kw = if is_one_of { "oneOf" } else { "anyOf" };
            let mut variants: Vec<Schema> = Vec::new();
            for (i, v) in arr.iter().enumerate() {
                let mut child_path = path.to_vec();
                child_path.push(kw.to_string());
                child_path.push(i.to_string());
                variants.push(import_value(v, ctx, &child_path)?);
            }

            let disc_key = obj
                .get("discriminator")
                .and_then(|d| d.as_object())
                .and_then(|d| d.get("propertyName"))
                .and_then(|p| p.as_str());

            let schema = if let Some(key) = disc_key {
                let du: Schema = discriminated_union(key, variants.clone()).into();
                let is_ok = matches!(
                    &du.kind,
                    SchemaKind::DiscriminatedUnion(b)
                        if matches!(b.state, DiscriminatorState::Ok { .. })
                );
                if is_ok {
                    du
                } else {
                    let u: Schema = union(variants).into();
                    if is_one_of { u.meta("x-oneOf", true) } else { u }
                }
            } else {
                let u: Schema = union(variants).into();
                if is_one_of { u.meta("x-oneOf", true) } else { u }
            };

            base = Some(schema);
            // Continue to step 12.
        }
    }

    // Step 7: enum.
    if base.is_none() {
        if let Some(enum_arr) = obj.get("enum").and_then(|v| v.as_array()) {
            let all_strings = enum_arr.iter().all(|v| v.is_string());
            if all_strings {
                let strings: Vec<String> = enum_arr
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                base = Some(enumerate(strings).into());
            } else {
                let mut lit_variants: Vec<Schema> = Vec::new();
                for v in enum_arr {
                    let zv = ZerxValue::from_serialize(v)
                        .map_err(|e| e.at(path.to_vec()))?;
                    lit_variants.push(literal(zv).into());
                }
                base = Some(union(lit_variants).into());
            }
            // Continue to step 12.
        }
    }

    // Step 8: const.
    if base.is_none() {
        if let Some(const_val) = obj.get("const") {
            let zv = ZerxValue::from_serialize(const_val)
                .map_err(|e| e.at(path.to_vec()))?;
            base = Some(literal(zv).into());
            // Continue to step 12.
        }
    }

    let fmt = obj.get("format").and_then(|v| v.as_str());
    let typ = obj.get("type").and_then(|v| v.as_str());

    // Step 9: Format-marker kinds (checked before generic type dispatch).
    if base.is_none() {
        if fmt == Some("json") {
            base = Some(json().into());
        } else if typ == Some("object") && fmt == Some("record") {
            match obj.get("additionalProperties") {
                Some(serde_json::Value::Bool(false)) => {
                    return Err(ZerxError::new(
                        ErrorCode::IMPORT_MALFORMED,
                        "record with `additionalProperties: false` is meaningless",
                    )
                    .at(path.to_vec()));
                }
                Some(serde_json::Value::Bool(true)) | None => {
                    base = Some(record(any()).into());
                }
                Some(ap_schema) => {
                    let mut ap_path = path.to_vec();
                    ap_path.push("additionalProperties".to_string());
                    let inner = import_value(ap_schema, ctx, &ap_path)?;
                    base = Some(record(inner).into());
                }
            }
        } else if typ == Some("object") && fmt == Some("jsonschema") {
            base = Some(jsonschema().into());
        } else if typ == Some("string") && fmt == Some("buffer") {
            base = Some(buffer().into());
        } else if typ == Some("string") && fmt == Some("uri") {
            base = Some(uri().into());
        } else if typ == Some("string") && fmt == Some("url") {
            base = Some(url().into());
        }
    }

    // Step 10: Type dispatch.
    if base.is_none() {
        base = match typ {
            Some("string") => {
                let mut sb = string();
                if let Some(n) = obj.get("minLength").and_then(|v| v.as_u64()) {
                    sb = sb.min(n as usize);
                }
                if let Some(n) = obj.get("maxLength").and_then(|v| v.as_u64()) {
                    sb = sb.max(n as usize);
                }
                if let Some(pat) = obj.get("pattern").and_then(|v| v.as_str()) {
                    sb = sb.regex(pat);
                }
                match fmt {
                    Some("email") => sb = sb.email(),
                    Some("uuid") => sb = sb.uuid(),
                    _ => {}
                }
                Some(sb.into())
            }
            Some("number") => {
                let mut nb = number();
                if let Some(n) = obj.get("minimum").and_then(|v| v.as_f64()) {
                    nb = nb.min(n);
                }
                if let Some(n) = obj.get("maximum").and_then(|v| v.as_f64()) {
                    nb = nb.max(n);
                }
                Some(nb.into())
            }
            Some("integer") => {
                let mut nb = number().int();
                if let Some(n) = obj.get("minimum").and_then(|v| v.as_f64()) {
                    nb = nb.min(n);
                }
                if let Some(n) = obj.get("maximum").and_then(|v| v.as_f64()) {
                    nb = nb.max(n);
                }
                Some(nb.into())
            }
            Some("boolean") => Some(boolean().into()),
            Some("null") => Some(null().into()),
            Some("array") => {
                if let Some(prefix) = obj.get("prefixItems").and_then(|v| v.as_array()) {
                    // Tuple: only when items is Bool(false).
                    match obj.get("items") {
                        Some(serde_json::Value::Bool(false)) => {
                            let mut items: Vec<Schema> = Vec::new();
                            for (i, pi) in prefix.iter().enumerate() {
                                let mut pi_path = path.to_vec();
                                pi_path.push("prefixItems".to_string());
                                pi_path.push(i.to_string());
                                items.push(import_value(pi, ctx, &pi_path)?);
                            }
                            Some(tuple(items).into())
                        }
                        _ => {
                            return Err(ZerxError::new(
                                ErrorCode::IMPORT_UNSUPPORTED,
                                "`prefixItems` with an open or typed tail is not representable as a zerx tuple",
                            )
                            .at(path.to_vec()));
                        }
                    }
                } else {
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
                    let item_schema =
                        if matches!(items_val, serde_json::Value::Bool(true))
                            || items_val
                                .as_object()
                                .map(|o| o.is_empty())
                                .unwrap_or(false)
                        {
                            any().into()
                        } else {
                            let mut items_path = path.to_vec();
                            items_path.push("items".to_string());
                            import_value(items_val, ctx, &items_path)?
                        };
                    let mut ab = array(item_schema);
                    if let Some(n) = obj.get("minItems").and_then(|v| v.as_u64()) {
                        ab = ab.min(n as usize);
                    }
                    if let Some(n) = obj.get("maxItems").and_then(|v| v.as_u64()) {
                        ab = ab.max(n as usize);
                    }
                    Some(ab.into())
                }
            }
            Some("object") => Some(import_object(obj, ctx, path)?),
            _ => None,
        };
    }

    // Step 14: Fallthrough → any().
    let mut schema = base.unwrap_or_else(|| any().into());

    // Step 12: Annotation / modifier reconstruction (inverse of apply_annotations).
    if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
        schema = schema.describe(desc);
    }
    if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
        schema = schema.title(title);
    }
    if let Some(def_val) = obj.get("default") {
        let zv = ZerxValue::from_serialize(def_val)
            .map_err(|e| e.at(path.to_vec()))?;
        schema = schema.default(zv);
    }
    if let Some(examples) = obj.get("examples").and_then(|v| v.as_array()) {
        if let Some(first) = examples.first() {
            let zv = ZerxValue::from_serialize(first)
                .map_err(|e| e.at(path.to_vec()))?;
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
        "buffer", "record", "json", "jsonschema", "uri", "url", "email", "uuid",
    ];
    if let Some(fmt_str) = fmt {
        if !FORMAT_SKIP.contains(&fmt_str) {
            schema = schema.format(fmt_str);
        }
    }

    // Step 13: Remaining keyword classification + extension meta.
    // For $ref nodes: skip IMPORT_UNSUPPORTED_STRUCTURAL errors (best-effort),
    // but still collect extension meta.
    for (key, val) in obj.iter() {
        if IMPORT_HANDLED.contains(&key.as_str()) {
            continue;
        }
        if IMPORT_UNSUPPORTED_STRUCTURAL.contains(&key.as_str()) {
            if is_ref {
                continue;
            }
            return Err(ZerxError::new(
                ErrorCode::IMPORT_UNSUPPORTED,
                format!("`{key}` is not representable in zerx"),
            )
            .at(path.to_vec()));
        }
        // Extension key → meta.
        let zv = ZerxValue::from_serialize(val)
            .map_err(|e| e.at(path.to_vec()))?;
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
) -> Result<Schema, ZerxError> {
    use crate::object;

    let required: HashSet<&str> = obj
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut fields: Vec<(String, Schema)> = Vec::new();

    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        for (key, prop_val) in props.iter() {
            let mut prop_path = path.to_vec();
            prop_path.push("properties".to_string());
            prop_path.push(key.clone());
            let mut field_schema = import_value(prop_val, ctx, &prop_path)?;

            if !required.contains(key.as_str()) && prop_val.get("default").is_none() {
                field_schema = field_schema.optional();
            }
            // Not in required but has default → non-optional (default supplies the value).
            fields.push((key.clone(), field_schema));
        }
    }

    let mut ob = object(fields);
    match obj.get("additionalProperties") {
        Some(serde_json::Value::Bool(true)) => {
            ob = ob.passthrough();
        }
        Some(serde_json::Value::Object(_)) => {
            ob = ob.passthrough();
        }
        _ => {}
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
    use std::rc::Rc;
    use std::cell::RefCell;

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
        assert_eq!(js(&boolean().into()), serde_json::json!({"type": "boolean"}));
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
        assert_eq!(get(&v, "minimum"), Some(&serde_json::json!(0.0)));
        assert_eq!(get(&v, "maximum"), Some(&serde_json::json!(9.0)));
    }

    // 6. Array
    #[test]
    fn array_export() {
        let s: Schema = array(string()).min(1).max(3).into();
        let v = js(&s);
        assert_eq!(get(&v, "type"), Some(&serde_json::json!("array")));
        assert_eq!(get(&v, "items"), Some(&serde_json::json!({"type": "string"})));
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
        assert_eq!(get(&v, "additionalProperties"), Some(&serde_json::json!({"format": "json"})));

        let sa: Schema = record(any()).into();
        let va = js(&sa);
        assert_eq!(get(&va, "additionalProperties"), Some(&serde_json::json!(true)));
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
        assert_eq!(get(&v, "additionalProperties"), Some(&serde_json::json!(false)));
        let req = get(&v, "required").unwrap().as_array().unwrap();
        assert_eq!(req, &[serde_json::json!("a")]);
        let props = get(&v, "properties").unwrap();
        assert_eq!(props["c"]["default"], serde_json::json!("x"));

        // passthrough
        let sp: Schema = object([("a", string().into())]).passthrough().into();
        let vp = js(&sp);
        assert_eq!(get(&vp, "additionalProperties"), Some(&serde_json::json!(true)));

        // strip
        let ss: Schema = object([("a", string().into())]).strip().into();
        let vs = js(&ss);
        assert_eq!(get(&vs, "additionalProperties"), Some(&serde_json::json!(false)));

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
        assert_eq!(v, serde_json::json!({"type": "string", "format": "buffer", "contentMediaType": "image/png"}));
    }

    // 11. uri / url / json / jsonschema markers
    #[test]
    fn special_type_markers() {
        assert_eq!(js(&uri().into()), serde_json::json!({"type": "string", "format": "uri"}));
        assert_eq!(js(&url().into()), serde_json::json!({"type": "string", "format": "url"}));
        assert_eq!(js(&json().into()), serde_json::json!({"format": "json"}));
        assert_eq!(js(&jsonschema().into()), serde_json::json!({"type": "object", "format": "jsonschema"}));
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
        assert_eq!(get(&js(&sp), "format"), Some(&serde_json::json!("date-time")));
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
        let s: Schema = lazy(|| {
            object([("id", string().into())]).into()
        })
        .into();
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
        let inner_lazy: Schema = lazy(move || {
            holder_clone.borrow().clone().unwrap()
        })
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
    // Import tests (J2)
    // ---------------------------------------------------------------------------

    fn from(v: serde_json::Value) -> Result<Schema, ZerxError> {
        from_json_schema(&v)
    }

    fn roundtrip_str(schema: &Schema) -> (String, String) {
        let v1 = schema.to_json_schema();
        let imported = from_json_schema(&v1).expect("import failed");
        let v2 = imported.to_json_schema();
        (serde_json::to_string(&v1).unwrap(), serde_json::to_string(&v2).unwrap())
    }

    // J2-1. Scalars and empty schema.
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

    // J2-2. Integer.
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

    // J2-3. String validators.
    #[test]
    fn import_string_validators() {
        let (v1, v2) = roundtrip_str(
            &string().min(2).max(8).regex("^x").email().into(),
        );
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

    // J2-4. UUID and user format.
    #[test]
    fn import_uuid_and_user_format() {
        let (v1, v2) = roundtrip_str(&string().uuid().into());
        assert_eq!(v1, v2);

        let input = serde_json::json!({"type": "string", "format": "date-time"});
        let exported = from(input).unwrap().to_json_schema();
        assert_eq!(exported["format"], serde_json::json!("date-time"));
    }

    // J2-5. Enum.
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

    // J2-6. Const / literal.
    #[test]
    fn import_const() {
        let (v1, v2) = roundtrip_str(&literal("x").into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&literal(5i64).into());
        assert_eq!(v1, v2);
        let (v1, v2) = roundtrip_str(&literal(ZerxValue::Null).into());
        assert_eq!(v1, v2);
    }

    // J2-7. Array and tuple.
    #[test]
    fn import_array_tuple() {
        let (v1, v2) = roundtrip_str(&array(string()).min(1).max(3).into());
        assert_eq!(v1, v2);

        let (v1, v2) =
            roundtrip_str(&tuple([number().into(), string().into()]).into());
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
        assert_eq!(
            from(bad).unwrap_err().code,
            ErrorCode::IMPORT_MALFORMED
        );

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

    // J2-8. Record.
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

    // J2-9. Object required / additionalProperties / default.
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
        assert_eq!(exported["properties"]["c"]["default"], serde_json::json!("x"));

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

        // schema object → passthrough
        let schema_ap = serde_json::json!({
            "type":"object","properties":{"a":{"type":"string"}},
            "additionalProperties":{"type":"string"}
        });
        assert_eq!(
            from(schema_ap).unwrap().to_json_schema()["additionalProperties"],
            serde_json::json!(true)
        );
    }

    // J2-10. Buffer + MIME / special markers.
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
    }

    // J2-11. Nullable collapse.
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

    // J2-12. Union vs discriminated union.
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
        assert!(from_json_schema(&serde_json::from_str::<serde_json::Value>(&v1).unwrap())
            .unwrap()
            .to_json_schema()
            .get("oneOf")
            .is_some());

        // oneOf without discriminator → union with x-oneOf:true
        let one_of_no_disc = serde_json::json!({
            "oneOf":[{"type":"string"},{"type":"number"}]
        });
        let exported = from(one_of_no_disc).unwrap().to_json_schema();
        assert!(exported.get("anyOf").is_some());
        assert_eq!(exported["x-oneOf"], serde_json::json!(true));

        // oneOf + discriminator but variants not all objects → fall back to union
        let bad_du = serde_json::json!({
            "oneOf":[{"type":"string"},{"type":"number"}],
            "discriminator":{"propertyName":"kind"}
        });
        let exported2 = from(bad_du).unwrap().to_json_schema();
        assert!(exported2.get("anyOf").is_some());

        // oneOf + discriminator + objects but discriminator field is plain string (not literal)
        let bad_du2 = serde_json::json!({
            "oneOf":[
                {"type":"object","properties":{"kind":{"type":"string"}},"required":["kind"],"additionalProperties":false},
                {"type":"object","properties":{"kind":{"type":"string"}},"required":["kind"],"additionalProperties":false}
            ],
            "discriminator":{"propertyName":"kind"}
        });
        let exported3 = from(bad_du2).unwrap().to_json_schema();
        assert!(exported3.get("anyOf").is_some());
    }

    // J2-13. Modifiers round-trip.
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

    // J2-14. allOf / not / false / unsupported applicators.
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
            from(serde_json::Value::Bool(false))
                .unwrap_err()
                .code,
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

    // J2-15. $ref / $defs.
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
        assert_eq!(
            from(bad).unwrap_err().code,
            ErrorCode::IMPORT_MALFORMED
        );

        // non-local $ref → IMPORT_MALFORMED
        let non_local = serde_json::json!({"$ref": "http://example.com/schema"});
        assert_eq!(
            from(non_local).unwrap_err().code,
            ErrorCode::IMPORT_MALFORMED
        );
    }

    // J2-16. Cyclic $ref terminates and re-exports to one $defs entry.
    #[test]
    fn import_cyclic_ref() {
        // Build a self-referential schema and export it first.
        let holder: Rc<RefCell<Option<Schema>>> = Rc::new(RefCell::new(None));
        let holder_clone = holder.clone();
        let inner_lazy: Schema = lazy(move || {
            holder_clone.borrow().clone().unwrap()
        })
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

    // J2-17. Full roundtrip — the vision `message` schema (mlua call field omitted).
    #[test]
    fn import_message_schema_roundtrip() {
        let address: Schema = object([
            ("geo", tuple([number().into(), number().into()]).optional().into()),
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
                    .default(
                        ZerxValue::from_serialize(&serde_json::json!({})).unwrap(),
                    )
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
                        object([
                            ("kind", literal("tool").into()),
                            ("tool", string().into()),
                        ])
                        .into(),
                    ],
                )
                .into(),
            ),
            (
                "tags",
                array(string().min(1)).max(10).nullable().into(),
            ),
        ])
        .into();

        let (v1, v2) = roundtrip_str(&message);
        assert_eq!(v1, v2, "message schema roundtrip not byte-identical");
    }

    // J2-18. Error path determinism.
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
        assert!(!err.path.is_empty(), "error path should point to offending node");
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
}
