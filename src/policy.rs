// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use crate::schema::Schema;
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
    /// `additionalProperties: true` or a schema-object value import as passthrough and are NOT
    /// affected — the flag reinterprets only the strict outcome.
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

// Applies mapper to every schema node, recursing into all positions that J2's
// core importer reads as child schemas.
fn deep_map_schema(
    value: &Value,
    mapper: &dyn Fn(&Value) -> Result<Value, ZerxError>,
) -> Result<Value, ZerxError> {
    let mapped = mapper(value)?;

    let obj = match &mapped {
        Value::Object(o) => o,
        _ => return Ok(mapped),
    };

    let mut out = obj.clone();

    // properties
    if let Some(Value::Object(props)) = out.get("properties").cloned() {
        let mut new_props = serde_json::Map::new();
        for (k, v) in &props {
            new_props.insert(k.clone(), deep_map_schema(v, mapper)?);
        }
        out.insert("properties".to_owned(), Value::Object(new_props));
    }

    // $defs
    if let Some(Value::Object(defs)) = out.get("$defs").cloned() {
        let mut new_defs = serde_json::Map::new();
        for (k, v) in &defs {
            new_defs.insert(k.clone(), deep_map_schema(v, mapper)?);
        }
        out.insert("$defs".to_owned(), Value::Object(new_defs));
    }

    // items: schema or array of schemas
    match out.get("items").cloned() {
        Some(Value::Array(arr)) => {
            let new_items: Result<Vec<Value>, ZerxError> =
                arr.iter().map(|v| deep_map_schema(v, mapper)).collect();
            out.insert("items".to_owned(), Value::Array(new_items?));
        }
        Some(v) => {
            out.insert("items".to_owned(), deep_map_schema(&v, mapper)?);
        }
        None => {}
    }

    // prefixItems (tuple members)
    if let Some(Value::Array(arr)) = out.get("prefixItems").cloned() {
        let new_arr: Result<Vec<Value>, ZerxError> =
            arr.iter().map(|v| deep_map_schema(v, mapper)).collect();
        out.insert("prefixItems".to_owned(), Value::Array(new_arr?));
    }

    // additionalProperties when it is a schema object (not a bool)
    if let Some(Value::Object(ap)) = out.get("additionalProperties").cloned() {
        out.insert(
            "additionalProperties".to_owned(),
            deep_map_schema(&Value::Object(ap), mapper)?,
        );
    }

    // anyOf / oneOf / allOf
    for key in &["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(arr)) = out.get(*key).cloned() {
            let new_arr: Result<Vec<Value>, ZerxError> =
                arr.iter().map(|v| deep_map_schema(v, mapper)).collect();
            out.insert(key.to_string(), Value::Array(new_arr?));
        }
    }

    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// Deref transform
// ---------------------------------------------------------------------------

fn make_deref_transform(resolver: RefResolver) -> SchemaTransform {
    Arc::new(move |value: &Value| {
        deep_map_schema(value, &|node: &Value| {
            let obj = match node.as_object() {
                Some(o) => o,
                None => return Ok(node.clone()),
            };
            let ref_str = match obj.get("$ref").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return Ok(node.clone()),
            };
            // Local #/$defs/… refs are left untouched for J2's core importer.
            if ref_str.starts_with("#/") {
                return Ok(node.clone());
            }
            // External ref: resolve, then merge sibling keywords (siblings win).
            let resolved = resolver(ref_str)?;
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
        })
    })
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

fn sql_nullable_normalise(value: &Value) -> Result<Value, ZerxError> {
    deep_map_schema(value, &|node: &Value| {
        let obj = match node.as_object() {
            Some(o) => o,
            None => return Ok(node.clone()),
        };

        // type: ["T", "null"] (exactly one non-null type) → anyOf
        if let Some(Value::Array(types)) = obj.get("type") {
            let type_strs: Vec<&str> = types.iter().filter_map(|v| v.as_str()).collect();
            let non_null: Vec<&str> = type_strs.iter().copied().filter(|&t| t != "null").collect();
            if type_strs.contains(&"null") && non_null.len() == 1 {
                let mut base = obj.clone();
                base.insert("type".to_owned(), Value::String(non_null[0].to_owned()));
                base.remove("oneOf");
                base.remove("allOf");
                return Ok(serde_json::json!({
                    "anyOf": [Value::Object(base), {"type": "null"}]
                }));
            }
        }

        // oneOf([X, {"type":"null"}]) (exactly 2 members, exactly one null) → anyOf
        if let Some(Value::Array(one_of)) = obj.get("oneOf") {
            if one_of.len() == 2 {
                let null_idx = one_of.iter().position(|v| {
                    v.as_object()
                        .and_then(|o| o.get("type"))
                        .and_then(|t| t.as_str())
                        == Some("null")
                        && v.as_object().map(|o| o.len() == 1).unwrap_or(false)
                });
                if let Some(null_i) = null_idx {
                    let other = one_of[1 - null_i].clone();
                    return Ok(serde_json::json!({
                        "anyOf": [other, {"type": "null"}]
                    }));
                }
            }
        }

        Ok(node.clone())
    })
}

fn sql_array_items_fallback(value: &Value) -> Result<Value, ZerxError> {
    deep_map_schema(value, &|node: &Value| {
        let obj = match node.as_object() {
            Some(o) => o,
            None => return Ok(node.clone()),
        };
        if obj.get("type").and_then(|v| v.as_str()) == Some("array")
            && !obj.contains_key("items")
        {
            let mut new_obj = obj.clone();
            new_obj.insert("items".to_owned(), Value::Object(serde_json::Map::new()));
            return Ok(Value::Object(new_obj));
        }
        Ok(node.clone())
    })
}

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
            if fmt.map(|s| s.eq_ignore_ascii_case("bytea")).unwrap_or(false) {
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

/// Import a JSON Schema with policy and transform options.
pub fn from_json_schema_with(
    value: &Value,
    opts: &ImportOptions,
) -> Result<Schema, ZerxError> {
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
    }
    for t in &policy.schema_transforms {
        effective = t(&effective)?;
    }
    for t in &opts.schema_transforms {
        effective = t(&effective)?;
    }

    // Step 3: J2 core import.
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
        assert_eq!(exported.get("type").and_then(|v| v.as_str()), Some("string"));
    }

    // 3. apply_type_transforms fold + ordering
    #[test]
    fn test_apply_type_transforms_ordering() {
        // T1 converts any schema to string; T2 verifies it sees a string schema.
        let t1: TypeTransform = Arc::new(|_s: Schema| Ok(crate::string().into()));
        let t2: TypeTransform = Arc::new(|s: Schema| {
            let exported = s.to_json_schema();
            if exported.get("type").and_then(|v| v.as_str()) == Some("string") {
                Ok(s)
            } else {
                Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "t1 did not run first"))
            }
        });
        assert!(apply_type_transforms(crate::any().into(), &[t1, t2]).is_ok());
    }

    #[test]
    fn test_apply_type_transforms_error_propagates() {
        let t_err: TypeTransform = Arc::new(|_s: Schema| {
            Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "transform error"))
        });
        assert!(apply_type_transforms(crate::any().into(), &[t_err]).is_err());
    }

    // 4. deref hook + sibling merge
    #[test]
    fn test_deref_external_ref_resolves_to_string() {
        let resolver: RefResolver =
            Arc::new(|_r: &str| Ok(json!({"type": "string"})));
        let result = from_json_schema_with(
            &json!({"$ref": "https://x/y"}),
            &ImportOptions {
                deref: Some(resolver),
                ..Default::default()
            },
        );
        let exported = re_export(&result.unwrap());
        assert_eq!(exported.get("type").and_then(|v| v.as_str()), Some("string"));
    }

    #[test]
    fn test_deref_sibling_description_survives() {
        let resolver: RefResolver =
            Arc::new(|_r: &str| Ok(json!({"type": "string"})));
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
            Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "should not be called"))
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
    fn test_deref_resolver_error_propagates() {
        let resolver: RefResolver = Arc::new(|_r: &str| {
            Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "resolver error"))
        });
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
    fn test_sql_nullable_one_of() {
        let result = import_sql(json!({"oneOf": [{"type": "number"}, {"type": "null"}]}));
        let exported = re_export(&result.unwrap());
        assert!(exported.get("anyOf").is_some());
    }

    // 6. sql array items fallback
    #[test]
    fn test_sql_array_items_fallback_bare_fails() {
        let v = json!({"type": "array"});
        // Bare importer errors on an array missing items.
        assert!(crate::json_schema::from_json_schema(&v).is_err());
        // sql policy adds items:{} so it succeeds.
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
        let props = exported
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();
        // data: buffer (was bytea)
        assert_eq!(
            props.get("data").unwrap().get("format").and_then(|v| v.as_str()),
            Some("buffer")
        );
        // count: string (was int64 number), optional
        assert_eq!(
            props.get("count").unwrap().get("type").and_then(|v| v.as_str()),
            Some("string")
        );
        // strict mode preserved
        assert_eq!(
            exported.get("additionalProperties").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    // 12. sql leaves discriminated union intact
    #[test]
    fn test_sql_discriminated_union_with_int64() {
        // Discriminator field must be {"const":"..."} so J2 imports it as literal and
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
            Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "schema transform error"))
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
            Err(ZerxError::new(ErrorCode::UNKNOWN_ERROR, "type transform error"))
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
        let member_fmt = exported
            .get("prefixItems")
            .unwrap()
            .as_array()
            .unwrap()[0]
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
        assert_eq!(exported.get("format").and_then(|v| v.as_str()), Some("buffer"));
        assert!(exported.get("x-pg-type").is_none(), "x-pg-type leaked into export");
    }

    #[test]
    fn test_sql_dual_signal_int64_no_leaked_pg_type() {
        // format:"int64" and x-pg-type:"int8" both present; x-pg-type must be consumed.
        let result = import_sql(json!({"type": "number", "format": "int64", "x-pg-type": "int8"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(exported.get("type").and_then(|v| v.as_str()), Some("string"));
        assert!(exported.get("x-pg-type").is_none(), "x-pg-type leaked into export");
    }

    #[test]
    fn test_sql_dual_signal_numeric_no_leaked_pg_type() {
        // format:"numeric" and x-pg-type:"numeric" both present; x-pg-type must be consumed.
        let result = import_sql(json!({"type": "number", "format": "numeric", "x-pg-type": "numeric"}));
        let exported = re_export(&result.unwrap());
        assert_eq!(exported.get("type").and_then(|v| v.as_str()), Some("string"));
        assert!(exported.get("x-pg-type").is_none(), "x-pg-type leaked into export");
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
}
