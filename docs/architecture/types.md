# Concern: Types

## Purpose

- This concern owns the type catalogue: basic (`string`, `number`, `boolean`, `enumerate`, `null`, `any`), complex (`object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `lazy`), and special (`buffer`, `uri`, `url`, `json`, `jsonschema`).
- This concern owns the pluggable validators (`min`, `max`, `regex`, `int`, `email`, `uuid`, …) as concrete implementations of the `Validator` trait; each implementation exposes `validate` and a JSON Schema fragment contribution.
- This concern owns object utilities: the three modes (`strict`/`passthrough`/`strip`) and the shape/runtime helpers (`partial`, `extend`, `omit*`, `strip*`).
- `multiline` is a meta/modifier hint (`x-ui-multiline`), NOT a validator; it carries a UI line-count hint and has no validation effect.

## Non-Goals

- This concern does NOT define the core `Schema` representation or parse flow — see `schema-core`.
- This concern does NOT own the host-opaque `function`/`tvalue` types — see `mlua`.
- This concern does NOT own JSON Schema export/import — see `json-schema`.

## Consumes from

- `schema-core`: `Schema` (core representation that type variants extend by adding `SchemaKind` variants)
- `schema-core`: `Validator` (trait contract that concrete type validators implement)

## Provides to

- `json-schema`: validator `json_schema()` fragment contributions (per-validator JSON Schema keyword map; established at `PLAN_J1_export`)

## External Contracts

None.

## External Services

None.

## Architecturally Significant Dependencies

- `regex` (full crate, 1.x) — the `pattern`/`regex` validator and the `url` structural check both compile and match regular expressions using the `regex` crate; removing or replacing `regex` would require redesigning both.

## Constraints

### Basic types (PLAN_T1)

- The five basic-type variants (`String`, `Number`, `Boolean`, `Enum(Vec<String>)`, `Null`) MUST be members of `SchemaKind`; each MUST add exactly one delegating arm to `check_type`, `parse_inner`, and the `Debug` match — no new parse-flow control logic MAY be introduced per type.
- `string()` MUST accept only `ZerxValue::String`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "string"` and `received = type_tag(value)`.
- `number()` MUST accept `ZerxValue::I64`, `U64`, `I128`, `U128`, and finite `F64`; a non-finite `F64` (NaN/±∞) and any non-numeric variant MUST yield `Err(TYPE_MISMATCH)` with `expected = "number"`.
- `boolean()` MUST accept only `ZerxValue::Bool`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "boolean"`.
- `enumerate(values)` MUST accept only a `ZerxValue::String` whose value is in the allowed set; a string not in the set MUST yield `Err(INVALID_ENUM_VALUE)` with `expected` listing the allowed values and `received = type_tag(value)`; a non-string MUST yield `Err(TYPE_MISMATCH)`.
- `null()` MUST accept only `ZerxValue::Null`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "null"`.
- `type_tag(value)` MUST return a compact static descriptor for each `ZerxValue` variant (`"null"`, `"boolean"`, `"number"`, `"string"`, `"bytes"`, `"array"`, `"object"`; `"host_opaque"` under the `mlua` feature).
- All `received` and `expected` fields on errors raised by this concern MUST be descriptor strings — never raw values.

### Negative compile-guarantee (PLAN_T1 — candidate C1)

- Type-specific validator methods MUST be inherent methods on the relevant typed builder only; they MUST NOT be available on builders where they make no sense.
- `BooleanSchema` and `NullSchema` MUST NOT expose any inherent validator methods.
- `EnumSchema` MUST NOT expose inherent validator methods; the value set is fixed at construction.

### String validators (PLAN_T1)

- `StringSchema::min(n: usize)` MUST push a `MinLength(n)` validator; a string whose Unicode scalar count is less than `n` MUST yield `Err(STRING_TOO_SHORT)`.
- `StringSchema::max(n: usize)` MUST push a `MaxLength(n)` validator; a string whose Unicode scalar count exceeds `n` MUST yield `Err(STRING_TOO_LONG)`.
- `StringSchema::regex(p)` and `StringSchema::pattern(p)` MUST install the same `Pattern` validator; `pattern` is a forwarding alias for `regex` with identical storage and validation semantics.
- Pattern compilation MUST be deferred to `validate` time; `string().regex(invalid)` MUST be infallible; a value validated against an invalid pattern MUST yield `Err(PATTERN_INVALID)`.
- A valid pattern that does not match MUST yield `Err(PATTERN_MISMATCH)`.
- `StringSchema::email()` MUST push a hand-rolled `Email` validator (no regex engine); a string failing the structural form check MUST yield `Err(INVALID_EMAIL)`; the JSON Schema fragment MUST be `{"format": "email"}`.
- `StringSchema::uuid()` MUST push a hand-rolled `Uuid` validator; a string not matching the 8-4-4-4-12 hex-with-dashes form MUST yield `Err(INVALID_UUID)`; the JSON Schema fragment MUST be `{"format": "uuid"}`.

### `multiline` meta hint (PLAN_T1)

- `StringSchema::multiline(lines: u32)` MUST write `x-ui-multiline → ZerxValue::U64(lines)` into the schema's `meta` map; it MUST NOT push any `Validator` and MUST have no validation effect.

### Number validators (PLAN_T1)

- `NumberSchema::min(n: impl Into<f64>)` MUST push a `MinValue(n)` validator; a numeric value whose `f64` representation is less than `n` MUST yield `Err(NUMBER_TOO_SMALL)`.
- `NumberSchema::max(n: impl Into<f64>)` MUST push a `MaxValue(n)` validator; a numeric value whose `f64` representation exceeds `n` MUST yield `Err(NUMBER_TOO_LARGE)`.
- `NumberSchema::int()` MUST push an `IntValidator`; `I64`/`U64`/`I128`/`U128` MUST always pass; a `F64` with `fract() != 0.0` MUST yield `Err(NOT_INTEGER)`.
- Bound comparison is performed in `f64`; precision loss at `i128`/`u128` magnitudes beyond 2^53 is accepted for v1.

### JSON Schema fragments (PLAN_T1)

- `MinLength(n).json_schema()` MUST return `{"minLength": n}`.
- `MaxLength(n).json_schema()` MUST return `{"maxLength": n}`.
- `Pattern.json_schema()` MUST return `{"pattern": <source>}` preserving the original pattern string regardless of compilation success.
- `Email.json_schema()` MUST return `{"format": "email"}`.
- `Uuid.json_schema()` MUST return `{"format": "uuid"}`.
- `MinValue(n).json_schema()` MUST return `{"minimum": n}`.
- `MaxValue(n).json_schema()` MUST return `{"maximum": n}`.
- `IntValidator.json_schema()` MUST return `{"type": "integer"}`.

### Error codes (PLAN_T1)

- Error codes introduced by this concern MUST be declared as associated constants in a separate `impl ErrorCode` block in `src/types.rs`; no edit to `src/error.rs` is permitted.
- Codes introduced by PLAN_T1: `STRING_TOO_SHORT`, `STRING_TOO_LONG`, `PATTERN_MISMATCH`, `PATTERN_INVALID`, `INVALID_EMAIL`, `INVALID_UUID`, `NUMBER_TOO_SMALL`, `NUMBER_TOO_LARGE`, `NOT_INTEGER`, `INVALID_ENUM_VALUE`.

### Complex types (PLAN_T2)

- The seven complex-type variants (`Object`, `Array`, `Record`, `Tuple`, `Union`, `DiscriminatedUnion`, `Literal`) MUST each add exactly one delegating arm to `check_type`, `parse_inner`, and the `Debug` match; no new parse-flow control logic MAY be introduced per variant.
- Container `parse_*` delegates MUST prepend their path segment to every child error via `e.path.insert(0, segment)` as the stack unwinds; object/record delegates MUST prepend the field key; array/tuple delegates MUST prepend the decimal-string element index.

#### `object` (PLAN_T2 — candidate C4)

- `check_object` MUST accept `ZerxValue::Object`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "object"`.
- The `object(fields)` constructor MUST canonicalize duplicate keys using last-write-wins / first-occurrence-order semantics: a repeated key overwrites the schema in place without moving its position.
- Object validation operates in one of three modes: `Strict` (default), `Passthrough`, or `Strip`.
- In `Strict` mode, the first input key absent from `shape` MUST yield `Err(UNKNOWN_PROPERTY)` with that key prepended to `path`; field validation MUST NOT proceed after the first unknown-key error.
- In `Passthrough` mode, unknown keys MUST be appended verbatim to the output after all shape-defined fields; output key order MUST be shape order first, then passthrough keys in input order.
- In `Strip` mode, unknown keys MUST be silently dropped.
- Per-field delegation MUST invoke `Schema::parse_field` for each `(key, field_schema)` pair in `shape` order; a missing required field MUST yield `Err(REQUIRED)` with the key prepended to `path`; any field error MUST have the field key prepended to `path`.

#### `array` (PLAN_T2)

- `check_array` MUST accept `ZerxValue::Array`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "array"`.
- Each element MUST be validated with `item.parse_present`; an element error MUST have its decimal-string index prepended to `path`.
- `ArraySchema::min(n)` MUST push an `ArrayMinLength(n)` validator; an array whose length is less than `n` MUST yield `Err(ARRAY_TOO_SHORT)`; the JSON Schema fragment MUST be `{"minItems": n}`.
- `ArraySchema::max(n)` MUST push an `ArrayMaxLength(n)` validator; an array whose length exceeds `n` MUST yield `Err(ARRAY_TOO_LONG)`; the JSON Schema fragment MUST be `{"maxItems": n}`.
- Array-length validators run in the `validators` step, before element descent in `parse_inner`.

#### `record` (PLAN_T2)

- `check_record` MUST accept `ZerxValue::Object`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "object"`.
- Every value in the input map MUST be validated against the single `value_schema` via `parse_present`; a value error MUST have its key prepended to `path`; the output map MUST preserve input key order.

#### `tuple` (PLAN_T2)

- `check_tuple` MUST accept `ZerxValue::Array`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "array"`.
- If the input array length differs from `items.len()`, `parse_tuple` MUST yield `Err(TUPLE_LENGTH_MISMATCH)` with `expected = "array of length <n>"` and `received = "array of length <m>"`.
- Each position `i` MUST be validated with `items[i].parse_present`; a position error MUST have `i.to_string()` prepended to `path`.

#### `union` (PLAN_T2)

- `check_union` MUST always return `Ok(())`; variant matching is deferred to `parse_union`.
- `parse_union` MUST attempt each variant via `parse_present` in declaration order and return the first `Ok`.
- If all variants fail, `parse_union` MUST return `Err(ZerxError::union([], inner_errors))` with per-variant errors as `inner_errors`.

#### `discriminated_union` (PLAN_T2)

- A variant is well-formed for `discriminated_union` only if: its kind is `Object`; the `shape` contains the discriminator key; the discriminator field's kind is `Literal`; the discriminator field is required, non-defaulted, and non-nullable (`optional == false`, `default.is_none()`, `nullable == false`); the literal const is keyable (`Bool`, `i128`-range integer, or `String`); and the discriminant key is unique across all variants.
- `discriminated_union(key, variants)` MUST be infallible; any structural defect MUST be recorded as `DiscriminatorState::Invalid(message)` and raised as `Err(INVALID_DISCRIMINATED_UNION)` in `check_discriminated_union` — the first step of the parse flow — so the schema-defect error wins over `TYPE_MISMATCH` for any input.
- `DiscriminantKey` MUST represent `Bool`, `i128`-range integers (all integer `ZerxValue` variants via lossless widening), and `String`; any other literal kind (float, null, array, object, bytes) MUST be treated as non-keyable and yield `DiscriminatorState::Invalid`.
- An input object whose discriminator field is absent or non-keyable MUST yield `Err(INVALID_DISCRIMINANT)` with `expected` listing allowed discriminant values.
- A discriminator value present but not in the lookup map MUST yield `Err(INVALID_DISCRIMINANT)` with `expected` listing allowed values and `received = type_tag(discriminator_value)`.
- When a variant is matched, `parse_present` MUST be called on the whole input object; no path segment is added at the discriminated-union level.

#### `literal` (PLAN_T2)

- `check_literal` MUST accept input only when `value == constant` using `ZerxValue` `PartialEq`; any non-matching value MUST yield `Err(INVALID_LITERAL)` with `expected = literal_descriptor(constant)` and `received = type_tag(value)`.
- `literal_descriptor` MUST NOT embed a raw `ZerxValue` in any error field; it MUST return a compact string form of the constant.
- Numeric literals use exact variant equality: `literal(5i64)` stores `I64(5)` and MUST NOT match `U64(5)`. This cross-variant caveat is accepted for v1.

### Error codes (PLAN_T2)

- Codes introduced by PLAN_T2: `UNKNOWN_PROPERTY`, `ARRAY_TOO_SHORT`, `ARRAY_TOO_LONG`, `TUPLE_LENGTH_MISMATCH`, `INVALID_LITERAL`, `INVALID_DISCRIMINANT`, `INVALID_DISCRIMINATED_UNION`.

### Special types (PLAN_T4)

- The five special-type variants (`Buffer`, `Uri`, `Url`, `Json`, `JsonSchema`) MUST each add exactly one delegating arm to `check_type`, `parse_inner`, and the `Debug` match; no new parse-flow control logic MAY be introduced per variant; all five are leaves with identity `parse_inner`.

#### `buffer` (PLAN_T4 — candidate C2)

- `buffer()` MUST accept only `ZerxValue::Bytes`; any other variant MUST yield `Err(TYPE_MISMATCH)` with `expected = "buffer"` and `received = type_tag(value)`.
- `buffer()` MUST NOT coerce a number array, an object shape, or any other representation into bytes; silent coercion of any other shape is prohibited (candidate C2).
- `BufferSchema::mime(mime_type)` MUST write the MIME string to `modifiers.mime`; this is the same field written by the blanket `mime_format` method; `J1` reads `modifiers.mime` to emit `contentMediaType`.

#### `uri` (PLAN_T4)

- `uri()` with a non-string value MUST yield `Err(TYPE_MISMATCH)` with `expected = "uri"` and `received = type_tag(value)`.
- `uri()` with a string failing the structural check MUST yield `Err(INVALID_URI)` with `expected = "uri"`.
- Structural URI rule: the string MUST contain `':'`; the part before `':'` (scheme) MUST be non-empty and start with an ASCII alphabetic character with every subsequent character ASCII alphanumeric or one of `+`, `-`, `.`; the part after `':'` (rest) MUST be non-empty.

#### `url` (PLAN_T4)

- `url()` with a non-string value MUST yield `Err(TYPE_MISMATCH)` with `expected = "url"` and `received = type_tag(value)`.
- `url()` with a string failing the structural check MUST yield `Err(INVALID_URL)` with `expected = "url"`.
- The structural URL check requires the string to match a `OnceLock`-cached HTTP/HTTPS regex (case-insensitive, ported from Zex); the scheme MUST be `http` or `https`.
- After the regex match, the extracted hostname MUST NOT be empty, MUST NOT contain `".."`, MUST NOT start or end with `'.'`, and MUST either contain a `'.'` or equal `"localhost"`.
- If a port is present (`:` + decimal digits before the path), it MUST parse to a value in `1..=65535`.

#### `json` / `jsonschema` (PLAN_T4)

- `json()` and `jsonschema()` MUST accept any serde-bridgeable `ZerxValue` variant with identity parse (`parse_inner` returns `Ok(value.clone())`).
- Under the `mlua` feature both MUST reject `ZerxValue::HostOpaque` with `Err(TYPE_MISMATCH)` (candidate C5); in the default build the arm is forward-correct but non-exercisable until `PLAN_M1_host_opaque`.
- `json()` and `jsonschema()` differ only in their `SchemaKind` variant; that variant is the export marker read by `J1`; no structural JSON Schema document validation is performed (v1 accept-all).
- Neither `json()` nor `jsonschema()` exposes any inherent validator methods.

#### Negative compile-guarantee (PLAN_T4)

- `BufferSchema`, `UriSchema`, `UrlSchema`, `JsonSchema`, and `JsonschemaSchema` MUST NOT expose any inherent string or number validator methods.

### Error codes (PLAN_T4)

- Codes introduced by PLAN_T4: `INVALID_URI`, `INVALID_URL`.

### Object utilities (PLAN_T3)

- All T3 logic MUST reside within the `object` delegate; T3 introduces no new `SchemaKind` variant, no new dispatch arm, no new parse-flow control logic, and no new error code.
- `ObjectBody` carries four utility-governing fields: `all_optional` (partial mode), `prestrip_keys` (explicit prestrip list), `prestrip_read_only` (prestrip by read-only modifier), and `prestrip_write_only` (prestrip by write-only modifier).
- The effective prestrip set MUST be computed from the union of `prestrip_keys`, the `shape` keys whose field carries `read_only` (when `prestrip_read_only` is set), and the `shape` keys whose field carries `write_only` (when `prestrip_write_only` is set).
- Prestrip filtering MUST be applied before the unknown-key check; a prestipped key is not an unknown key and MUST NOT yield `UNKNOWN_PROPERTY`.
- A prestripped key is treated as absent for field validation: if the field carries a `default`, that default MUST be applied; if the field is required with no default, `Err(REQUIRED)` with the key prepended to `path` MUST be returned.
- `strip_read_only` and `strip_write_only` derive their effective strip sets exclusively from fields currently present in `shape`; a field already removed from `shape` by `omit_read_only` or `omit_write_only` is NOT auto-stripped by a subsequent `strip_read_only` or `strip_write_only` call; `strip_only` is the explicit escape hatch for naming such keys directly.
- Under `all_optional`, a missing required field's `Err(REQUIRED)` MUST be reinterpreted as omit; field iteration MUST still delegate through `Schema::parse_field`, preserving `default → optional → REQUIRED` precedence (a missing defaulted field is still re-defaulted under `partial`); a present field MUST validate normally regardless of `all_optional`.
- `extend` MUST merge incoming fields with last-write-wins / first-occurrence-order semantics; mode and all other `ObjectBody` state MUST be preserved.
- `omit`, `omit_read_only`, and `omit_write_only` remove fields from `shape` (by name, by `read_only` modifier, and by `write_only` modifier respectively); an omitted field becomes an unknown key under the unchanged strict mode (per C4); omitting a key absent from `shape` MUST be a no-op.
- All eight utility methods MUST be infallible and MUST clone-and-return per C1 immutability; no utility returns a `Result`.
- `all_optional` is the source of truth for the exported `required` set, consumed by `PLAN_J1_export`.

## Splitting Rationale

- The type catalogue (basic/complex/special), its pluggable validators, and the object utilities form a single responsibility boundary: validators are members of the catalogue (inherent methods on the type builders), and object utilities are operations on the `object` type — neither is an independent cross-cutting capability that would warrant its own concern file.
- The length reflects the vision's full type catalogue (≈19 types across three groups, each with its validators and utility operations), not multiple concerns; the file stays under the 300-line hard ceiling and is therefore not split.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C1, C2, C4, C7, C8, C9
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
