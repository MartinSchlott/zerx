# Concern: Types

## Purpose

- This concern owns the type catalogue: basic (`string`, `number`, `boolean`, `enumerate`, `null`), complex (`object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, and the crate-internal `OneOf`), and special (`buffer`, `uri`, `url`, `json`, `jsonschema`).
- This concern owns the pluggable validators (`min`, `max`, `regex`, `int`, `email`, `uuid`, …) as concrete implementations of the `Validator` trait; each implementation exposes `validate` and a JSON Schema fragment contribution.
- This concern owns object utilities: the three modes (`strict`/`passthrough`/`strip`) and the shape/runtime helpers (`partial`, `extend`, `omit*`, `strip*`).
- `multiline` is a meta/modifier hint (`x-ui-multiline`), NOT a validator; it carries a UI line-count hint and has no validation effect.

## Non-Goals

- This concern does NOT define the core `Schema` representation or parse flow — see `schema-core`.
- This concern does NOT own JSON Schema export/import — see `json-schema`.

## Consumes from

- `schema-core`: `Schema` (core representation that type variants extend by adding `SchemaKind` variants)
- `schema-core`: `Validator` (trait contract that concrete type validators implement)
- `errors`: `ZerxError` (validator and union failures)
- `value-model`: `ZerxValue` (validated and constant values)

## Provides to

- `json-schema`: validator `json_schema()` fragment contributions (per-validator JSON Schema keyword maps merged during export)
- `json-schema`: type constructors (the builder/constructor surface import reconstructs)
- `schema-core`: `per-kind check and parse delegates` (dispatch targets of the kind arms)
- `lua`: `object and discriminant helpers` (prestrip, object mode and discriminant keying)
- `json-schema`: `kind payloads and literal equality` (read by export and import)

## External Contracts

None.

## External Services

None.

## Architecturally Significant Dependencies

- `regex` (full crate, 1.x) — the `pattern`/`regex` validator compiles the `regex` crate translation of its ECMA-262 source, and the `url` structural check matches with the `regex` crate directly; removing or replacing `regex` would require redesigning both.

## Constraints

### Basic types

- Each basic type MUST be a `SchemaKind` variant reached through one delegating arm per per-kind dispatch; no new parse-flow control logic MAY be introduced per type.
- `string()` MUST accept only `ZerxValue::String`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "string"` and `received = type_tag(value)`.
- `number()` MUST accept `ZerxValue::I64`, `U64`, `I128`, `U128`, and finite `F64`; a non-finite `F64` (NaN/±∞) and any non-numeric variant MUST yield `Err(TYPE_MISMATCH)` with `expected = "number"`.
- `boolean()` MUST accept only `ZerxValue::Bool`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "boolean"`.
- `enumerate(values)` MUST accept only a `ZerxValue::String` whose value is in the allowed set; a string not in the set MUST yield `Err(INVALID_ENUM_VALUE)` with `expected` listing the allowed values and `received = type_tag(value)`; a non-string MUST yield `Err(TYPE_MISMATCH)`.
- `null()` MUST accept only `ZerxValue::Null`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "null"`.
- The type tag used in `received` MUST be a compact static descriptor per `ZerxValue` variant (`"null"`, `"boolean"`, `"number"`, `"string"`, `"bytes"`, `"array"`, `"object"`).
- All `received` and `expected` fields on errors raised by this concern MUST be descriptor strings — never raw values.

### Negative compile-guarantee (`D-modifier-composition`)

- Type-specific validator methods MUST be inherent methods on the relevant typed builder only; they MUST NOT be available on builders where they make no sense.
- `BooleanSchema` and `NullSchema` MUST NOT expose any inherent validator methods.
- `EnumSchema` MUST NOT expose inherent validator methods; the value set is fixed at construction.

### String validators

- `StringSchema::min(n)` MUST push a minimum-length validator; a string whose Unicode scalar count is less than `n` MUST yield `Err(STRING_TOO_SHORT)`.
- `StringSchema::max(n)` MUST push a maximum-length validator; a string whose Unicode scalar count exceeds `n` MUST yield `Err(STRING_TOO_LONG)`.
- `StringSchema::regex(p)` and `StringSchema::pattern(p)` MUST install the same pattern validator; `pattern` is a forwarding alias for `regex` with identical storage and validation semantics.
- `string().regex(p)` MUST be infallible; a pattern that fails translation or compilation MUST be stored as invalid, and validating a string against it MUST yield `Err(PATTERN_INVALID)`.
- A valid pattern that does not match MUST yield `Err(PATTERN_MISMATCH)`.
- Pattern sources MUST be read as ECMA-262 with the `u` flag, restricted to the subset the translation represents.
- A construct whose meaning differs between ECMA-262 and the `regex` crate (`\d`, `\D`, `\w`, `\W`, `\s`, `\S`, `\b`, `\B`, `.`) MUST match only what both dialects accept, so the runtime is never wider than the exported `pattern`.
- A source that is invalid ECMA-262, or valid but outside the translated subset (lookaround, backreferences, `\p{…}`, constructs the dialects read differently such as `&&` in a class), MUST make the pattern invalid (`PATTERN_INVALID`).
- `StringSchema::email()` MUST push a hand-rolled email validator (no regex engine); a string failing the structural form check MUST yield `Err(INVALID_EMAIL)`; the JSON Schema fragment MUST be `{"format": "email"}`.
- `StringSchema::uuid()` MUST push a hand-rolled UUID validator; a string not matching the 8-4-4-4-12 hex-with-dashes form MUST yield `Err(INVALID_UUID)`; the JSON Schema fragment MUST be `{"format": "uuid"}`.

### `multiline` meta hint

- `StringSchema::multiline(lines)` MUST write the line count as the `x-ui-multiline` entry of the schema's `meta` map; it MUST NOT push any `Validator` and MUST have no validation effect.

### Number validators

- `NumberSchema::min(n)` (a float bound) and `NumberSchema::min_bound(n)` (a `NumberBound`, holding integer bounds exactly) MUST push a minimum validator; a numeric value less than `n`, or any numeric value when `n` is NaN, MUST yield `Err(NUMBER_TOO_SMALL)`.
- `NumberSchema::max(n)` (a float bound) and `NumberSchema::max_bound(n)` (a `NumberBound`) MUST push a maximum validator; a numeric value greater than `n`, or any numeric value when `n` is NaN, MUST yield `Err(NUMBER_TOO_LARGE)`.
- `NumberSchema::int()` MUST push an integer validator; `I64`/`U64`/`I128`/`U128` MUST always pass; a `F64` with a fractional part MUST yield `Err(NOT_INTEGER)`.
- An integer bound MUST be held and compared exactly against every numeric value (`I64`, `U64`, `I128`, `U128`, integral and fractional `F64`), with no rounding through `f64`; a float bound is compared exactly as a float.
- Integer bounds MUST NOT be compared through `f64`, because an `f64` comparison lets a bound of magnitude 2^53 or more accept a neighbouring integer beyond it.

### JSON Schema fragments

- The minimum- and maximum-length validators MUST contribute `{"minLength": n}` and `{"maxLength": n}`.
- The pattern validator MUST contribute `{"pattern": <source>}`, preserving the original source regardless of compilation success.
- The email and UUID validators MUST contribute `{"format": "email"}` and `{"format": "uuid"}`.
- The minimum validator MUST contribute `{"minimum": n}` for a finite `n`: an exact JSON integer for a bound held as an integer within the `i64`/`u64` range, the nearest float not below `n` for one beyond it, and a JSON float for a float bound; a NaN or `+∞` minimum accepts nothing and takes the fail-closed export of `json-schema`.
- The maximum validator MUST contribute `{"maximum": n}` for a finite `n` under the same rules, with the nearest float not above `n` beyond the integer range; a NaN or `-∞` maximum accepts nothing and takes the fail-closed export of `json-schema`.
- A non-finite bound that forbids nothing (`-∞` minimum, `+∞` maximum) MUST contribute no keyword.
- The integer validator MUST contribute `{"type": "integer"}`.

### Error codes — basic types

- Error codes introduced by this concern MUST be declared by this concern, not in the `errors` catalogue.
- Codes: `STRING_TOO_SHORT`, `STRING_TOO_LONG`, `PATTERN_MISMATCH`, `PATTERN_INVALID`, `INVALID_EMAIL`, `INVALID_UUID`, `NUMBER_TOO_SMALL`, `NUMBER_TOO_LARGE`, `NOT_INTEGER`, `INVALID_ENUM_VALUE`.

### Complex types

- Each complex type, including the crate-internal `OneOf`, MUST be a `SchemaKind` variant reached through one delegating arm per per-kind dispatch; no new parse-flow control logic MAY be introduced per variant.
- Container `parse_*` delegates MUST prepend their path segment to every child error as the stack unwinds; object/record delegates MUST prepend the field key; array/tuple delegates MUST prepend the decimal-string element index.

#### `object` (`D-strict-by-default`)

- `check_object` MUST accept `ZerxValue::Object`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "object"`.
- The `object(fields)` constructor MUST canonicalize duplicate keys using last-write-wins / first-occurrence-order semantics: a repeated key overwrites the schema in place without moving its position.
- Object validation operates in one of three modes: `Strict` (default), `Passthrough`, or `Strip`.
- In `Strict` mode, the first input key absent from `shape` MUST yield `Err(UNKNOWN_PROPERTY)` with that key prepended to `path`; field validation MUST NOT proceed after the first unknown-key error.
- In `Passthrough` mode, unknown keys MUST be appended verbatim to the output after all shape-defined fields; output key order MUST be shape order first, then passthrough keys in input order.
- In `Strip` mode, unknown keys MUST be silently dropped.
- Per-field delegation MUST invoke `Schema::parse_field` for each `(key, field_schema)` pair in `shape` order; a missing required field MUST yield `Err(REQUIRED)` with the key prepended to `path`; any field error MUST have the field key prepended to `path`.

#### `array`

- `check_array` MUST accept `ZerxValue::Array`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "array"`.
- Each element MUST be validated with `item.parse_present`; an element error MUST have its decimal-string index prepended to `path`.
- `ArraySchema::min(n)` MUST push a minimum-items validator; an array whose length is less than `n` MUST yield `Err(ARRAY_TOO_SHORT)`; the JSON Schema fragment MUST be `{"minItems": n}`.
- `ArraySchema::max(n)` MUST push a maximum-items validator; an array whose length exceeds `n` MUST yield `Err(ARRAY_TOO_LONG)`; the JSON Schema fragment MUST be `{"maxItems": n}`.
- Array-length validators run in the `validators` step, before element descent in `parse_inner`.
- `ArraySchema::unique_items()` MUST push a uniqueness validator; an array holding two equal elements MUST yield `Err(ARRAY_NOT_UNIQUE)` with the decimal-string index of the later duplicate as `path`; the JSON Schema fragment MUST be `{"uniqueItems": true}`.
- Element equality for `unique_items()` MUST be literal equality; two `Bytes` elements compare byte-wise and a `Bytes` element MUST NOT equal any non-`Bytes` element. Bytes uniqueness is a zerx extension, because JSON Schema has no bytes.
- `unique_items()` MUST evaluate the input instance in the `validators` step, before element descent and before refinement predicates, as JSON Schema applies `uniqueItems` to the instance; elements that become equal only through stripping or defaults are not duplicates.
- Literal equality and the uniqueness check MUST NOT recurse without bound on the nesting depth of the compared values, because validators run before the parse-depth guard sees the elements.

#### `record`

- `check_record` MUST accept `ZerxValue::Object`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "object"`.
- Every value in the input map MUST be validated against the single `value_schema` via `parse_present`; a value error MUST have its key prepended to `path`; the output map MUST preserve input key order.

#### `tuple`

- `check_tuple` MUST accept `ZerxValue::Array`; any other kind MUST yield `Err(TYPE_MISMATCH)` with `expected = "array"`.
- If the input array length differs from `items.len()`, `parse_tuple` MUST yield `Err(TUPLE_LENGTH_MISMATCH)` with `expected = "array of length <n>"` and `received = "array of length <m>"`.
- Each position `i` MUST be validated with `items[i].parse_present`; a position error MUST have the decimal-string position prepended to `path`.

#### `union`

- `check_union` MUST always return `Ok(())`; variant matching is deferred to `parse_union`.
- `parse_union` MUST attempt each variant via `parse_present` in declaration order and return the first `Ok`.
- If all variants fail, `parse_union` MUST return the union-failure aggregate of `errors` with per-variant errors as `inner_errors`.

#### `OneOf` (crate-internal)

- The `OneOf` kind MUST be constructible only inside the crate; it carries the exactly-one semantics of an imported JSON Schema `oneOf` and has no public builder.
- `parse_one_of` MUST attempt every variant and accept the value only when exactly one variant accepts it, returning that variant's output.
- When no variant accepts the value, `parse_one_of` MUST return the union-failure aggregate of `errors`.
- When two or more variants accept the value, `parse_one_of` MUST return `Err(ONE_OF_AMBIGUOUS)` without embedding the value.

#### `discriminated_union`

- A `discriminated_union` variant MUST be an `object` whose shape contains the discriminator key.
- The discriminator field MUST be a `literal` that is required, carries no default and is not nullable.
- The literal constant MUST be keyable (`Bool`, `i128`-range integer, or `String`), and its key MUST be unique across all variants.
- `discriminated_union(key, variants)` MUST be infallible; any structural defect MUST be recorded on the schema and raised as `Err(INVALID_DISCRIMINATED_UNION)` in the type-check step, so the schema-defect error wins over `TYPE_MISMATCH` for every input that reaches type checking.
- Discriminant keys MUST cover `Bool`, `i128`-range integers (every integer `ZerxValue` variant via lossless widening) and `String`; any other literal kind (float, null, array, object, bytes) MUST be non-keyable and make the union invalid.
- An input object whose discriminator field is absent or non-keyable MUST yield `Err(INVALID_DISCRIMINANT)` with `expected` listing allowed discriminant values.
- A discriminator value present but not in the lookup map MUST yield `Err(INVALID_DISCRIMINANT)` with `expected` listing allowed values and `received = type_tag(discriminator_value)`.
- An integral `F64` discriminator argument in `i128` range MUST key as its integer, while float literals MUST stay non-keyable, so import classification and export do not depend on the argument-side rule.
- When a variant is matched, `parse_present` MUST be called on the whole input object; no path segment is added at the discriminated-union level.

#### `literal`

- `check_literal` MUST accept input only when the value equals the constant under the literal equality rules below; any non-matching value MUST yield `Err(INVALID_LITERAL)` with `expected = literal_descriptor(constant)` and `received = type_tag(value)`.
- `literal_descriptor` MUST NOT embed a raw `ZerxValue` in any error field; it MUST return a compact string form of the constant.
- Literal equality MUST compare numbers by exact mathematical value across `I64`, `U64`, `I128`, `U128` and `F64`, and MUST NOT compare through a lossy `f64` conversion.
- Literal equality MUST compare arrays recursively by position and objects recursively independent of key order.
- Literal equality MUST keep non-numeric values variant-strict: a number MUST NOT equal a string, boolean or null, and `Bytes` MUST NOT equal `String` or `Array`.
- `ZerxValue` `PartialEq` MUST stay variant-strict, because tests and consumers use it to assert exact variants; literal equality MUST NOT change it.
- A matched literal MUST return the input value in its own form, not in the form of the constant.

### Error codes — complex types

- Codes: `UNKNOWN_PROPERTY`, `ARRAY_TOO_SHORT`, `ARRAY_TOO_LONG`, `ARRAY_NOT_UNIQUE`, `TUPLE_LENGTH_MISMATCH`, `INVALID_LITERAL`, `INVALID_DISCRIMINANT`, `INVALID_DISCRIMINATED_UNION`, `ONE_OF_AMBIGUOUS`.

### Special types

- Each special type MUST be a `SchemaKind` variant reached through one delegating arm per per-kind dispatch, with no new parse-flow control logic; special types are leaves whose type-specific parse returns the input unchanged.

#### `buffer` (`D-buffer-fidelity`)

- `buffer()` MUST accept only `ZerxValue::Bytes`; any other variant MUST yield `Err(TYPE_MISMATCH)` with `expected = "buffer"` and `received = type_tag(value)`.
- `buffer()` MUST NOT coerce a number array, an object shape, or any other representation into bytes; silent coercion of any other shape is prohibited (`D-buffer-fidelity`).
- `BufferSchema::mime(mime_type)` MUST write the same MIME modifier as the blanket `mime_format` method; JSON Schema export reads it to emit `contentMediaType`.

#### `uri`

- `uri()` with a non-string value MUST yield `Err(TYPE_MISMATCH)` with `expected = "uri"` and `received = type_tag(value)`.
- `uri()` with a string failing the structural check MUST yield `Err(INVALID_URI)` with `expected = "uri"`.
- Structural URI rule: the string MUST contain `':'`; the part before `':'` (scheme) MUST be non-empty and start with an ASCII alphabetic character with every subsequent character ASCII alphanumeric or one of `+`, `-`, `.`; the part after `':'` (rest) MUST be non-empty.

#### `url`

- `url()` with a non-string value MUST yield `Err(TYPE_MISMATCH)` with `expected = "url"` and `received = type_tag(value)`.
- `url()` with a string failing the structural check MUST yield `Err(INVALID_URL)` with `expected = "url"`.
- The structural URL check requires the string to match a case-insensitive HTTP/HTTPS pattern; the scheme MUST be `http` or `https`.
- After the regex match, the extracted hostname MUST NOT be empty, MUST NOT contain `".."`, MUST NOT start or end with `'.'`, and MUST either contain a `'.'` or equal `"localhost"`.
- If a port is present (`:` + decimal digits before the path), it MUST parse to a value in `1..=65535`.

#### `json` / `jsonschema`

- `json()` MUST accept any serde-bridgeable `ZerxValue` variant with identity parse.
- `jsonschema()` MUST accept only `ZerxValue::Object`, failing with `TYPE_MISMATCH` otherwise, with identity parse; its runtime check MUST agree with its exported form `type:"object"`.
- No structural JSON Schema document validation is performed by `jsonschema()`.
- Neither `json()` nor `jsonschema()` exposes any inherent validator methods.

#### Negative compile-guarantee — special types

- `BufferSchema`, `UriSchema`, `UrlSchema`, `JsonSchema`, and `JsonschemaSchema` MUST NOT expose any inherent string or number validator methods.
- A `uri()` or `url()` reconstructed by JSON Schema import MAY carry string-length and pattern validators attached by the import; they run after the structural check.

### Error codes — special types

- Codes: `INVALID_URI`, `INVALID_URL`.

### Object utilities

- Object-utility logic MUST reside within the `object` delegate and MUST add no `SchemaKind` variant, dispatch arm, parse-flow control logic or error code.
- The object body records partial mode, an explicit prestrip key list, and prestrip-by-read-only / prestrip-by-write-only flags.
- The effective prestrip set MUST be the union of the explicit prestrip keys, the shape keys whose field carries `read_only` (when prestrip-by-read-only is set), and the shape keys whose field carries `write_only` (when prestrip-by-write-only is set).
- Prestrip filtering MUST be applied before the unknown-key check; a prestripped key is not an unknown key and MUST NOT yield `UNKNOWN_PROPERTY`.
- A prestripped key is treated as absent for field validation: if the field carries a `default`, that default MUST be applied; if the field is required with no default, `Err(REQUIRED)` with the key prepended to `path` MUST be returned.
- `strip_read_only` and `strip_write_only` derive their effective strip sets exclusively from fields currently present in `shape`; a field already removed from `shape` by `omit_read_only` or `omit_write_only` is NOT auto-stripped by a subsequent `strip_read_only` or `strip_write_only` call; `strip_only` is the explicit escape hatch for naming such keys directly.
- Under partial mode, a missing required field's `Err(REQUIRED)` MUST be reinterpreted as omit, while the missing-value order `default → optional → REQUIRED` still applies (a missing defaulted field is still defaulted under `partial`).
- Under partial mode, a present field MUST validate normally.
- `extend` MUST merge incoming fields with last-write-wins / first-occurrence-order semantics; mode and all other object-body state MUST be preserved.
- `omit`, `omit_read_only`, and `omit_write_only` remove fields from `shape` (by name, by `read_only` modifier, and by `write_only` modifier respectively); an omitted field becomes an unknown key under the unchanged strict mode (`D-strict-by-default`); omitting a key absent from `shape` MUST be a no-op.
- Every object utility method MUST be infallible and MUST consume and return the builder, leaving every other schema value unchanged; no utility returns a `Result`.
- Partial mode is the source of truth for the exported `required` set, consumed by the JSON Schema export.

## Splitting Rationale

- The type catalogue (basic/complex/special), its pluggable validators, and the object utilities form a single responsibility boundary: validators are members of the catalogue (inherent methods on the type builders), and object utilities are operations on the `object` type — neither is an independent cross-cutting capability that would warrant its own concern file.
- The length reflects the full type catalogue across three groups, each with its validators and utility operations, not multiple concerns; the special types would be extracted first, once the catalogue can no longer be held as one rule per bullet within this file's ceiling.

## Related Decisions

- `D-modifier-composition`
- `D-buffer-fidelity`
- `D-strict-by-default`
- `D-result-only-api`
- `D-serde-foundation`
