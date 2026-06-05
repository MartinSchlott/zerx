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

- `regex` (full crate, 1.x) — the `pattern`/`regex` validator compiles and matches user-supplied regular expressions; removing or replacing `regex` would require redesigning the `Pattern` validator.

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

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C1, C7, C8, C9
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
