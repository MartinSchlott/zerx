# Concern: Value Model

## Purpose

- This concern owns `ZerxValue`: the validated dynamic value spanning serde's full data model (including bytes) plus the host-opaque layer.
- This concern owns the serde bridge — accepting any `T: Serialize` and serialising any `ZerxValue` to any serde format.

## Non-Goals

- This concern does NOT own validation rules or schema shape — see `schema-core`.
- This concern does NOT own JSON Schema representation — see `json-schema`.
- This concern does NOT own the realisation of the host-opaque variant — see `mlua`.

## Consumes from

- `errors`: `ZerxError` (serialisation failure surfaced by the serde-in bridge)

## Provides to

- `schema-core`: `ZerxValue` (the validated dynamic value produced by the parse flow)
- `schema-core`: `Map` (the ordered object map carried inside `ZerxValue::Object`)
- `mlua`: `ZerxValue::HostOpaque` variant (the gated slot that `mlua` realises with a concrete handle type)

## External Contracts

None.

## External Services

None.

## Architecturally Significant Dependencies

- `serde` — the `Serialize`/`Serializer` traits define the architecture of the bridge; replacing serde would require redesigning this entire concern.

## Constraints

- `ZerxValue` MUST carry these variants: `Null`, `Bool(bool)`, `I64(i64)`, `U64(u64)`, `I128(i128)`, `U128(u128)`, `F64(f64)`, `String(String)`, `Bytes(Vec<u8>)`, `Array(Vec<ZerxValue>)`, `Object(Map)`, and a feature-gated `HostOpaque` variant.
- The `Bytes` variant MUST be populated only via serde's native `serialize_bytes` path; zerx MUST NOT coerce number arrays into `Bytes`.
- zerx MUST NOT depend on `serde_bytes`; bytes fidelity via `serialize_bytes` is a documented caller-side contract.
- Signed integers `i8..i64` MUST map to `I64`; `i128` MUST map to `I128`; unsigned `u8..u64` MUST map to `U64`; `u128` MUST map to `U128`; floats MUST map to `F64`; `char` MUST map to a one-character `String`.
- `ZerxValue::from_serialize` MUST accept any `T: Serialize + ?Sized` and return `Result<ZerxValue, ZerxError>`; the `Bytes` variant MUST be produced when the source calls `serialize_bytes`.
- `ZerxValue` MUST implement `Serialize`; the `Bytes` variant MUST serialise via `serialize_bytes` to preserve byte fidelity on byte-aware formats and MUST degrade to a number array on JSON.
- The `HostOpaque` variant MUST be absent from the default build and present only under the `mlua` Cargo feature.
- `HostOpaque` values MUST NOT enter the serde roundtrip; serialising a `HostOpaque` value MUST produce a serialisation error.
- Object entries MUST be stored and serialised in insertion order; the position of the first occurrence of a key MUST be preserved when the key is overwritten.
- `Map::insert` MUST overwrite the existing value in place for a duplicate key and MUST preserve the insertion position of the first occurrence.
- `From<Vec<u8>>` MUST NOT be implemented for `ZerxValue`; `Bytes` MUST be constructed explicitly to prevent the array/bytes ambiguity.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C5, C2, C3
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
