use std::collections::HashMap;

use crate::schema::{Schema, SchemaKind};
use crate::types::{ObjectMode, ObjectBody};

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
