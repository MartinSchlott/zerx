# Concern: Value Model

## Purpose

- This concern owns `ZerxValue`: the validated dynamic value spanning serde's full data model (including bytes).
- This concern owns the serde bridge — accepting any `T: Serialize` and serialising any `ZerxValue` to any serde format.

## Non-Goals

- This concern does NOT own validation rules or schema shape — see `schema-core`.
- This concern does NOT own JSON Schema representation — see `json-schema`.

## Consumes from

- `errors`: `ZerxError` (serialisation failure surfaced by the serde-in bridge)
- `schema-core`: `MAX_PARSE_DEPTH` (nesting bound enforced by the serde-in bridge)

## Provides to

- `schema-core`: `ZerxValue` (the validated dynamic value produced by the parse flow)
- `schema-core`: `Map` (the ordered object map carried inside `ZerxValue::Object`)
- `lua`: `ZerxValue` (the output of `validate_lua`)
- `types`: `ZerxValue` (validated and constant values)
- `json-schema`: `ZerxValue` (literal, default and annotation values)

## External Contracts

None.

## External Services

None.

## Architecturally Significant Dependencies

- `serde` — the `Serialize`/`Serializer` traits define the architecture of the bridge; replacing serde would require redesigning this entire concern.

## Constraints

- `ZerxValue` MUST carry these variants: `Null`, `Bool(bool)`, `I64(i64)`, `U64(u64)`, `I128(i128)`, `U128(u128)`, `F64(f64)`, `String(String)`, `Bytes(Vec<u8>)`, `Array(Vec<ZerxValue>)`, `Object(Map)`.
- The serde-in bridge MUST produce `Bytes` only from `serialize_bytes`; zerx MUST NOT coerce number arrays into `Bytes`.
- zerx MUST NOT depend on `serde_bytes`; bytes fidelity via `serialize_bytes` is a documented caller-side contract.
- Signed integers `i8..i64` MUST map to `I64`; `i128` MUST map to `I128`; unsigned `u8..u64` MUST map to `U64`; `u128` MUST map to `U128`; floats MUST map to `F64`; `char` MUST map to a one-character `String`.
- `ZerxValue::from_serialize` MUST accept any `T: Serialize + ?Sized` and return `Result<ZerxValue, ZerxError>`; the `Bytes` variant MUST be produced when the source calls `serialize_bytes`.
- `ZerxValue::from_serialize` MUST refuse input nested more than `MAX_PARSE_DEPTH` containers deep with `PARSE_DEPTH_EXCEEDED` before recursing further, counting each container; an enum variant's two output containers count as one level.
- `ZerxValue::from_serialize` MUST bound the total of nested serializer calls, transparent wrappers (`Option`, newtype structs, newtype map keys) included, at three times `MAX_PARSE_DEPTH`, refusing beyond it with `PARSE_DEPTH_EXCEEDED`, because wrappers recurse without adding a value level.
- `ZerxValue` MUST implement `Serialize`; the `Bytes` variant MUST serialise via `serialize_bytes` to preserve byte fidelity on byte-aware formats and MUST degrade to a number array on JSON.
- Object entries MUST be stored and serialised in insertion order.
- `Map::insert` MUST overwrite the existing value in place for a duplicate key and MUST preserve the insertion position of the first occurrence.
- `From<Vec<u8>>` MUST NOT be implemented for `ZerxValue`; `Bytes` MUST be constructed explicitly to prevent the array/bytes ambiguity.

## Related Decisions

- `D-serde-value-model`
- `D-buffer-fidelity`
