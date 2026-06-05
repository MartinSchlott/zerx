// ZerxError is intentionally structured for rich diagnostics; boxing it
// everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use crate::schema::{ParseContext, SchemaKind};
use crate::{ErrorCode, Schema, ZerxError, ZerxValue};

// ---------------------------------------------------------------------------
// Open error-code catalogue (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const INVALID_POINTER: ErrorCode = ErrorCode::new("invalid_pointer");
    pub const INVALID_PATH: ErrorCode = ErrorCode::new("invalid_path");
    pub const INDEX_OUT_OF_RANGE: ErrorCode = ErrorCode::new("index_out_of_range");
    pub const UNION_PATH_REQUIRES_INSTANCE: ErrorCode =
        ErrorCode::new("union_path_requires_instance");
    pub const MISSING_PARENT: ErrorCode = ErrorCode::new("missing_parent");
}

// ---------------------------------------------------------------------------
// JSON Pointer parser (RFC 6901)
// ---------------------------------------------------------------------------

fn parse_pointer(path: &str) -> Result<Vec<std::string::String>, ZerxError> {
    if path.is_empty() {
        return Ok(vec![]);
    }
    if !path.starts_with('/') {
        return Err(ZerxError::new(
            ErrorCode::INVALID_POINTER,
            format!("JSON Pointer must be empty or start with '/'; got {path:?}"),
        ));
    }
    let segments = path[1..]
        .split('/')
        .map(|seg| seg.replace("~1", "/").replace("~0", "~"))
        .collect();
    Ok(segments)
}

// Accepts only non-negative decimal integers (no sign, no leading-+, no whitespace).
fn parse_index(seg: &str, consumed: &[std::string::String]) -> Result<usize, ZerxError> {
    if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ZerxError::new(
            ErrorCode::INVALID_POINTER,
            format!("array/tuple index must be a non-negative decimal integer; got {seg:?}"),
        )
        .at(consumed.to_vec()));
    }
    seg.parse::<usize>().map_err(|_| {
        ZerxError::new(
            ErrorCode::INVALID_POINTER,
            format!("array/tuple index is out of representable range: {seg:?}"),
        )
        .at(consumed.to_vec())
    })
}

// ---------------------------------------------------------------------------
// Schema-only navigation (parse_delta)
// ---------------------------------------------------------------------------

// Loop through Lazy layers until a non-Lazy kind is reached.
fn resolve_lazy(schema: Schema) -> Result<Schema, ZerxError> {
    let mut current = schema;
    loop {
        let inner_rc = match &current.kind {
            SchemaKind::Lazy(lzy) => lzy.resolve()?,
            _ => return Ok(current),
        };
        current = (*inner_rc).clone();
    }
}

fn navigate_schema(root: &Schema, segments: &[std::string::String]) -> Result<Schema, ZerxError> {
    let mut consumed: Vec<std::string::String> = Vec::new();
    let mut current = root.clone();

    for seg in segments {
        let resolved = resolve_lazy(current)?;

        // Closed match — no `_` arm so a future SchemaKind variant is a compile error here.
        let next = match &resolved.kind {
            SchemaKind::Object(body) => {
                match body.shape.iter().find(|(k, _)| k == seg) {
                    Some((_, field)) => {
                        let field = field.clone();
                        consumed.push(seg.clone());
                        field
                    }
                    None => {
                        let mut path = consumed;
                        path.push(seg.clone());
                        return Err(ZerxError::new(
                            ErrorCode::UNKNOWN_PROPERTY,
                            format!("property {seg:?} is not defined in schema"),
                        )
                        .at(path)
                        .expected("property defined in schema")
                        .received(seg.clone()));
                    }
                }
            }
            SchemaKind::Record(vs) => {
                let item = *vs.clone();
                consumed.push(seg.clone());
                item
            }
            SchemaKind::Array(item) => {
                let _idx = parse_index(seg, &consumed)?;
                let item = *item.clone();
                consumed.push(seg.clone());
                item
            }
            SchemaKind::Tuple(items) => {
                let idx = parse_index(seg, &consumed)?;
                if idx >= items.len() {
                    let mut path = consumed;
                    path.push(seg.clone());
                    return Err(ZerxError::new(
                        ErrorCode::INDEX_OUT_OF_RANGE,
                        format!(
                            "tuple index {idx} is out of range (tuple length {})",
                            items.len()
                        ),
                    )
                    .at(path));
                }
                let field = items[idx].clone();
                consumed.push(seg.clone());
                field
            }
            SchemaKind::Union(_) | SchemaKind::DiscriminatedUnion(_) => {
                let mut path = consumed;
                path.push(seg.clone());
                return Err(ZerxError::new(
                    ErrorCode::UNION_PATH_REQUIRES_INSTANCE,
                    "cannot descend into a union without an instance; use replace() instead",
                )
                .at(path));
            }
            SchemaKind::Any
            | SchemaKind::String
            | SchemaKind::Number
            | SchemaKind::Boolean
            | SchemaKind::Enum(_)
            | SchemaKind::Null
            | SchemaKind::Literal(_)
            | SchemaKind::Buffer
            | SchemaKind::Uri
            | SchemaKind::Url
            | SchemaKind::Json
            | SchemaKind::JsonSchema => {
                let mut path = consumed;
                path.push(seg.clone());
                return Err(ZerxError::new(
                    ErrorCode::INVALID_PATH,
                    "cannot descend into a leaf/scalar schema kind",
                )
                .at(path));
            }
            SchemaKind::Lazy(_) => unreachable!("Lazy is resolved before dispatch"),
        };

        current = next;
    }

    Ok(current)
}

// ---------------------------------------------------------------------------
// Instance navigation + immutable rebuild (replace)
// ---------------------------------------------------------------------------

// Closed match — no `_` arm so a future ZerxValue variant is a compile error here.
fn set_at(
    value: &ZerxValue,
    segments: &[std::string::String],
    new: &ZerxValue,
    consumed: &[std::string::String],
) -> Result<ZerxValue, ZerxError> {
    if segments.is_empty() {
        return Ok(new.clone());
    }

    let seg = &segments[0];
    let rest = &segments[1..];

    let mut path_with_seg = consumed.to_vec();
    path_with_seg.push(seg.clone());

    match value {
        ZerxValue::Object(map) => match map.get(seg) {
            None => Err(ZerxError::new(
                ErrorCode::MISSING_PARENT,
                format!("object key {seg:?} is absent from the instance"),
            )
            .at(path_with_seg)
            .expected("existing parent key")
            .received(seg.clone())),
            Some(child) => {
                let child2 = set_at(child, rest, new, &path_with_seg)?;
                let mut new_map = map.clone();
                new_map.insert(seg.clone(), child2);
                Ok(ZerxValue::Object(new_map))
            }
        },
        ZerxValue::Array(items) => {
            let idx = parse_index(seg, consumed)?;
            if idx >= items.len() {
                return Err(ZerxError::new(
                    ErrorCode::INDEX_OUT_OF_RANGE,
                    format!("array index {idx} is out of range (length {})", items.len()),
                )
                .at(path_with_seg));
            }
            let child2 = set_at(&items[idx], rest, new, &path_with_seg)?;
            let mut new_vec = items.clone();
            new_vec[idx] = child2;
            Ok(ZerxValue::Array(new_vec))
        }
        ZerxValue::Null
        | ZerxValue::Bool(_)
        | ZerxValue::I64(_)
        | ZerxValue::U64(_)
        | ZerxValue::I128(_)
        | ZerxValue::U128(_)
        | ZerxValue::F64(_)
        | ZerxValue::String(_)
        | ZerxValue::Bytes(_) => Err(ZerxError::new(
            ErrorCode::INVALID_PATH,
            "cannot descend into a non-container value",
        )
        .at(path_with_seg)),
        // Uninhabited placeholder: statically unreachable in D1. When M1 realises
        // this variant with a real mlua type it MUST supply the descend-into-leaf error (C3).
        #[cfg(feature = "mlua")]
        ZerxValue::HostOpaque(h) => match *h {},
    }
}

// ---------------------------------------------------------------------------
// Public Schema methods
// ---------------------------------------------------------------------------

impl Schema {
    /// Validate `value` against the sub-schema at the JSON Pointer `path`.
    ///
    /// An empty `path` validates against the whole schema (equivalent to
    /// `validate`). Navigation is schema-only; descending into a
    /// `union`/`discriminated_union` requires an instance — use `replace`.
    pub fn parse_delta<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        value: &T,
    ) -> Result<ZerxValue, ZerxError> {
        let segments = parse_pointer(path)?;
        let target = if segments.is_empty() {
            self.clone()
        } else {
            navigate_schema(self, &segments)?
        };
        let v = ZerxValue::from_serialize(value)?;
        target.parse_present(&v, &mut ParseContext::new())
    }

    /// Produce a new instance with the value at `path` replaced, then run full
    /// root revalidation (incl. `refine` and `default`) via `parse_present`.
    ///
    /// An empty `path` replaces the whole root. Does not mutate the supplied
    /// instance (clone-and-return, C1). Does not insert absent object keys.
    pub fn replace<I, T>(
        &self,
        instance: &I,
        path: &str,
        value: &T,
    ) -> Result<ZerxValue, ZerxError>
    where
        I: serde::Serialize + ?Sized,
        T: serde::Serialize + ?Sized,
    {
        let segments = parse_pointer(path)?;
        let new_value = ZerxValue::from_serialize(value)?;

        if segments.is_empty() {
            return self.parse_present(&new_value, &mut ParseContext::new());
        }

        let current = ZerxValue::from_serialize(instance)?;
        let rebuilt = set_at(&current, &segments, &new_value, &[])?;
        self.parse_present(&rebuilt, &mut ParseContext::new())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        any as any_schema, array, discriminated_union, lazy, literal, number, object, record,
        string, tuple, union, Map, Modify, Schema,
    };

    fn make_obj(pairs: &[(&str, ZerxValue)]) -> ZerxValue {
        let mut map = Map::new();
        for (k, v) in pairs {
            map.insert(*k, v.clone());
        }
        ZerxValue::Object(map)
    }

    fn make_arr(items: Vec<ZerxValue>) -> ZerxValue {
        ZerxValue::Array(items)
    }

    // ==========================================================================
    // Pointer parser
    // ==========================================================================

    #[test]
    fn pointer_root() {
        assert_eq!(parse_pointer("").unwrap(), Vec::<std::string::String>::new());
    }

    #[test]
    fn pointer_single_segment() {
        assert_eq!(parse_pointer("/a").unwrap(), vec!["a"]);
    }

    #[test]
    fn pointer_multi_segment() {
        assert_eq!(parse_pointer("/a/b/0").unwrap(), vec!["a", "b", "0"]);
    }

    #[test]
    fn pointer_slash_alone() {
        // "/" → single empty-string key (not root)
        assert_eq!(parse_pointer("/").unwrap(), vec![""]);
    }

    #[test]
    fn pointer_trailing_slash() {
        assert_eq!(parse_pointer("/a/").unwrap(), vec!["a", ""]);
    }

    #[test]
    fn pointer_unescape_slash() {
        assert_eq!(parse_pointer("/a~1b").unwrap(), vec!["a/b"]);
    }

    #[test]
    fn pointer_unescape_tilde() {
        assert_eq!(parse_pointer("/m~0n").unwrap(), vec!["m~n"]);
    }

    #[test]
    fn pointer_unescape_order() {
        // "~01": ~1 replacement finds no match (chars are ~,0,1 — not ~,1);
        // then ~0→~ gives "~1", NOT "/"  (order check per RFC 6901)
        assert_eq!(parse_pointer("/~01").unwrap(), vec!["~1"]);
    }

    #[test]
    fn pointer_no_leading_slash() {
        let err = parse_pointer("a/b").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_POINTER);
    }

    // ==========================================================================
    // parse_delta
    // ==========================================================================

    #[test]
    fn parse_delta_object_field_ok() {
        let schema: Schema = object([("age", number().into())]).into();
        let result = schema.parse_delta("/age", &30i32);
        assert_eq!(result.unwrap(), ZerxValue::I64(30));
    }

    #[test]
    fn parse_delta_object_field_type_mismatch() {
        let schema: Schema = object([("age", number().into())]).into();
        let err = schema.parse_delta("/age", &"x").unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
    }

    #[test]
    fn parse_delta_unknown_key() {
        let schema: Schema = object([("age", number().into())]).into();
        let err = schema.parse_delta("/bogus", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["bogus"]);
    }

    #[test]
    fn parse_delta_nested_object() {
        let schema: Schema = object([("a", object([("b", number().into())]).into())]).into();
        let result = schema.parse_delta("/a/b", &42i32);
        assert_eq!(result.unwrap(), ZerxValue::I64(42));
    }

    #[test]
    fn parse_delta_array_item() {
        let schema: Schema = array(number()).into();
        let result = schema.parse_delta("/0", &99i32);
        assert_eq!(result.unwrap(), ZerxValue::I64(99));
    }

    #[test]
    fn parse_delta_array_non_numeric_index() {
        let schema: Schema = array(number()).into();
        let err = schema.parse_delta("/x", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_POINTER);
    }

    #[test]
    fn parse_delta_tuple_ok() {
        let schema: Schema = tuple([string().into(), number().into()]).into();
        let result = schema.parse_delta("/1", &7i32);
        assert_eq!(result.unwrap(), ZerxValue::I64(7));
    }

    #[test]
    fn parse_delta_tuple_out_of_range() {
        let schema: Schema = tuple([string().into(), number().into()]).into();
        let err = schema.parse_delta("/5", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INDEX_OUT_OF_RANGE);
    }

    #[test]
    fn parse_delta_record_value() {
        let schema: Schema = record(number()).into();
        let result = schema.parse_delta("/anything", &3i32);
        assert_eq!(result.unwrap(), ZerxValue::I64(3));
    }

    #[test]
    fn parse_delta_descend_into_scalar() {
        let schema: Schema = object([("name", string().into())]).into();
        let err = schema.parse_delta("/name/0", &"x").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PATH);
    }

    #[test]
    fn parse_delta_empty_pointer_root_ok() {
        let schema: Schema = string().into();
        assert_eq!(
            schema.parse_delta("", &"hello").unwrap(),
            ZerxValue::String("hello".to_string())
        );
    }

    #[test]
    fn parse_delta_empty_pointer_root_fail() {
        let schema: Schema = string().into();
        let err = schema.parse_delta("", &42i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
    }

    #[test]
    fn parse_delta_union_descent_fails() {
        let schema: Schema = union([string().into(), number().into()]).into();
        let err = schema.parse_delta("/0", &"x").unwrap_err();
        assert_eq!(err.code, ErrorCode::UNION_PATH_REQUIRES_INSTANCE);
    }

    #[test]
    fn parse_delta_union_target_itself_ok() {
        // Pointer stops on the union node (navigates TO it, not INTO it)
        let outer: Schema =
            object([("u", union([string().into(), number().into()]).into())]).into();
        assert!(outer.parse_delta("/u", &"hello").is_ok());
        assert!(outer.parse_delta("/u", &99i32).is_ok());
    }

    #[test]
    fn parse_delta_discriminated_union_descent_fails() {
        // Two variants with different discriminator literals
        let schema: Schema = discriminated_union(
            "type",
            [
                object([
                    ("type", literal("cat").into()),
                    ("name", string().into()),
                ])
                .into(),
                object([
                    ("type", literal("dog").into()),
                    ("breed", string().into()),
                ])
                .into(),
            ],
        )
        .into();
        // Descending into the discriminator key fails — no single well-defined sub-schema
        let err = schema.parse_delta("/type", &"cat").unwrap_err();
        assert_eq!(err.code, ErrorCode::UNION_PATH_REQUIRES_INSTANCE);
        // Descending into another key also fails
        let err2 = schema.parse_delta("/name", &"Whiskers").unwrap_err();
        assert_eq!(err2.code, ErrorCode::UNION_PATH_REQUIRES_INSTANCE);
    }

    #[test]
    fn parse_delta_lazy_schema() {
        // Nested lazy wrapping an object — navigated to finite depth
        let schema: Schema = lazy(|| {
            object([
                ("x", number().into()),
                (
                    "child",
                    lazy(|| object([("leaf", string().into())]).into()).into(),
                ),
            ])
            .into()
        })
        .into();
        let result = schema.parse_delta("/child/leaf", &"hello");
        assert_eq!(result.unwrap(), ZerxValue::String("hello".to_string()));
    }

    #[test]
    fn parse_delta_any_schema() {
        // any() accepts any value; navigating INTO it is INVALID_PATH
        let schema: Schema = object([("data", any_schema().into())]).into();
        // Reaching the any node itself is fine
        assert!(schema.parse_delta("/data", &42i32).is_ok());
        // Descending into it is INVALID_PATH
        let err = schema.parse_delta("/data/key", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PATH);
    }

    // ==========================================================================
    // replace
    // ==========================================================================

    #[test]
    fn replace_object_leaf() {
        let schema: Schema = object([("name", string().into()), ("age", number().into())]).into();
        let instance = make_obj(&[
            ("name", ZerxValue::String("Alice".to_string())),
            ("age", ZerxValue::I64(30)),
        ]);
        let result = schema.replace(&instance, "/age", &31i32).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("age"), Some(&ZerxValue::I64(31)));
        assert_eq!(obj.get("name"), Some(&ZerxValue::String("Alice".to_string())));
        // order preserved: name first, then age
        let keys: Vec<&str> = obj.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["name", "age"]);
    }

    #[test]
    fn replace_nested_object() {
        let schema: Schema = object([(
            "profile",
            object([("city", string().into()), ("zip", string().into())]).into(),
        )])
        .into();
        let instance = make_obj(&[(
            "profile",
            make_obj(&[
                ("city", ZerxValue::String("Paris".to_string())),
                ("zip", ZerxValue::String("75000".to_string())),
            ]),
        )]);
        let result = schema.replace(&instance, "/profile/city", &"Berlin").unwrap();
        let city = result.get("profile").and_then(|v| v.get("city")).unwrap();
        assert_eq!(city, &ZerxValue::String("Berlin".to_string()));
        let zip = result.get("profile").and_then(|v| v.get("zip")).unwrap();
        assert_eq!(zip, &ZerxValue::String("75000".to_string()));
    }

    #[test]
    fn replace_array_element() {
        let schema: Schema = array(number()).into();
        let instance = make_arr(vec![ZerxValue::I64(1), ZerxValue::I64(2), ZerxValue::I64(3)]);
        let result = schema.replace(&instance, "/1", &99i32).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0], ZerxValue::I64(1));
        assert_eq!(arr[1], ZerxValue::I64(99));
        assert_eq!(arr[2], ZerxValue::I64(3));
    }

    #[test]
    fn replace_tuple_element() {
        let schema: Schema = tuple([string().into(), number().into()]).into();
        let instance = make_arr(vec![
            ZerxValue::String("hello".to_string()),
            ZerxValue::I64(10),
        ]);
        let result = schema.replace(&instance, "/0", &"world").unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0], ZerxValue::String("world".to_string()));
        assert_eq!(arr[1], ZerxValue::I64(10));
    }

    #[test]
    fn replace_record_value() {
        let schema: Schema = record(number()).into();
        let instance = make_obj(&[("a", ZerxValue::I64(1)), ("b", ZerxValue::I64(2))]);
        let result = schema.replace(&instance, "/a", &99i32).unwrap();
        assert_eq!(result.get("a"), Some(&ZerxValue::I64(99)));
        assert_eq!(result.get("b"), Some(&ZerxValue::I64(2)));
    }

    #[test]
    fn replace_instance_immutable() {
        let schema: Schema = object([("x", number().into())]).into();
        let instance = make_obj(&[("x", ZerxValue::I64(1))]);
        let _result = schema.replace(&instance, "/x", &2i32).unwrap();
        // Original instance is unchanged — reuse it for a second replace
        let result2 = schema.replace(&instance, "/x", &3i32).unwrap();
        assert_eq!(result2.get("x"), Some(&ZerxValue::I64(3)));
        assert_eq!(instance.get("x"), Some(&ZerxValue::I64(1)));
    }

    #[test]
    fn replace_missing_parent_intermediate() {
        let schema: Schema =
            object([("a", object([("b", number().into())]).into())]).into();
        // "a" is present but has no "b"
        let instance = make_obj(&[("a", make_obj(&[]))]);
        let err = schema.replace(&instance, "/a/b", &42i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::MISSING_PARENT);
    }

    #[test]
    fn replace_missing_parent_final_key() {
        let schema: Schema = object([("c", number().optional().into())]).into();
        // Instance has no "c" even though the schema declares it optional
        let instance = make_obj(&[]);
        let err = schema.replace(&instance, "/c", &10i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::MISSING_PARENT);
        // No silent insertion
        assert!(instance.get("c").is_none());
    }

    #[test]
    fn replace_index_out_of_range() {
        let schema: Schema = array(number()).into();
        let instance = make_arr(vec![ZerxValue::I64(1), ZerxValue::I64(2)]);
        let err = schema.replace(&instance, "/5", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INDEX_OUT_OF_RANGE);
    }

    #[test]
    fn replace_invalid_pointer_non_numeric_index() {
        let schema: Schema = array(number()).into();
        let instance = make_arr(vec![ZerxValue::I64(1)]);
        let err = schema.replace(&instance, "/abc", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_POINTER);
    }

    #[test]
    fn replace_invalid_path_scalar() {
        let schema: Schema = object([("x", number().into())]).into();
        let instance = make_obj(&[("x", ZerxValue::I64(1))]);
        let err = schema.replace(&instance, "/x/0", &0i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PATH);
    }

    #[test]
    fn replace_empty_pointer_root_ok() {
        let schema: Schema = number().into();
        let instance = ZerxValue::I64(1);
        let result = schema.replace(&instance, "", &42i32).unwrap();
        assert_eq!(result, ZerxValue::I64(42));
    }

    #[test]
    fn replace_empty_pointer_root_type_mismatch() {
        let schema: Schema = number().into();
        let instance = ZerxValue::I64(1);
        let err = schema.replace(&instance, "", &"not_a_number").unwrap_err();
        assert_eq!(err.code, ErrorCode::TYPE_MISMATCH);
    }

    #[test]
    fn replace_refine_cross_field() {
        // priority < 9 unless role == "system"
        let schema: Schema = object([("priority", number().into()), ("role", string().into())])
            .refine(
                |v| {
                    let Some(obj) = v.as_object() else {
                        return true;
                    };
                    let priority = obj.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
                    let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    priority < 9 || role == "system"
                },
                "priority must be < 9 unless role is system",
            )
            .into();

        let instance = make_obj(&[
            ("priority", ZerxValue::I64(5)),
            ("role", ZerxValue::String("user".to_string())),
        ]);

        // Compliant: priority stays < 9
        let ok = schema.replace(&instance, "/priority", &7i32).unwrap();
        assert_eq!(ok.get("priority"), Some(&ZerxValue::I64(7)));

        // Violating: priority → 10, role == "user" → refine fails
        let err = schema.replace(&instance, "/priority", &10i32).unwrap_err();
        assert_eq!(err.code, ErrorCode::REFINEMENT_FAILED);

        // Compliant via role == "system"
        let instance2 = make_obj(&[
            ("priority", ZerxValue::I64(10)),
            ("role", ZerxValue::String("system".to_string())),
        ]);
        assert!(schema.replace(&instance2, "/priority", &10i32).is_ok());
    }

    #[test]
    fn replace_default_applied_after_replace() {
        // count has a default of 0; missing from instance → applied during revalidation
        let schema: Schema = object([
            ("name", string().into()),
            ("count", number().default(ZerxValue::I64(0)).into()),
        ])
        .into();

        let instance = make_obj(&[("name", ZerxValue::String("Alice".to_string()))]);
        let result = schema.replace(&instance, "/name", &"Bob").unwrap();
        assert_eq!(
            result.get("name"),
            Some(&ZerxValue::String("Bob".to_string()))
        );
        // Default for count is applied by full root revalidation
        assert_eq!(result.get("count"), Some(&ZerxValue::I64(0)));
    }

    #[test]
    fn replace_strict_mode_enforced_after_replace() {
        // Outer object (strict by default) with a nested strict profile object
        let schema: Schema = object([
            ("name", string().into()),
            ("profile", object([("city", string().into())]).into()),
        ])
        .into();

        let instance = make_obj(&[
            ("name", ZerxValue::String("Alice".to_string())),
            (
                "profile",
                make_obj(&[("city", ZerxValue::String("Paris".to_string()))]),
            ),
        ]);

        // Replace profile with extra unknown key → strict rejects it
        let extra_profile = make_obj(&[
            ("city", ZerxValue::String("Berlin".to_string())),
            ("extra", ZerxValue::String("oops".to_string())),
        ]);
        let err = schema
            .replace(&instance, "/profile", &extra_profile)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::UNKNOWN_PROPERTY);
        assert_eq!(err.path, vec!["profile", "extra"]);

        // Compliant replacement (no extra key) succeeds
        let good_profile = make_obj(&[("city", ZerxValue::String("Berlin".to_string()))]);
        let ok = schema.replace(&instance, "/profile", &good_profile).unwrap();
        assert_eq!(
            ok.get("profile").and_then(|p| p.get("city")),
            Some(&ZerxValue::String("Berlin".to_string()))
        );

        // Replacing a scalar leaf with a value of the wrong type → TYPE_MISMATCH at the leaf path
        let err2 = schema.replace(&instance, "/profile/city", &42i32).unwrap_err();
        assert_eq!(err2.code, ErrorCode::TYPE_MISMATCH);
        assert_eq!(err2.path, vec!["profile", "city"]);
    }
}
