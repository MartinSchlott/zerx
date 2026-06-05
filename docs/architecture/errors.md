# Concern: Errors

## Purpose

- This concern owns `ZerxError`: the structured, machine-readable validation failure that every fallible zerx operation returns.
- This concern owns error serialisation: `ZerxError` implements `Serialize` and exposes `to_json`.
- This concern owns union-failure aggregation: collecting per-variant errors into a single combined `ZerxError`.

## Non-Goals

- This concern does NOT own when errors are raised — that is the parse flow in `schema-core`.
- This concern does NOT localise error messages; messages are English-only.

## Consumes from

(None. This concern is foundational and has no zerx-internal dependencies.)

## Provides to

- `schema-core`: `ZerxError` (structured validation failure returned by the parse flow)
- `value-model`: `ZerxError` (serialisation failure surfaced by the serde-in bridge)
- `json-schema`: `ZerxError` (structured import failure)
- `policy`: `ZerxError` (structured pipeline failure)

## External Contracts

(None. This concern is a library; it exposes no network, hook, or event-stream interface.)

## External Services

(None.)

## Architecturally Significant Dependencies

- `serde` — `ZerxError` derives `Serialize`; all fields MUST be `Serialize` to guarantee `to_json` is infallible; removing `serde` would require redesigning the serialisation contract.
- `serde_json` — `to_json` returns `serde_json::Value`; the return type of the public API is bound to this crate.

## Constraints

- `ZerxError` MUST carry exactly these fields: `path` (list of path segments), `code` (`ErrorCode`), `message` (English string), `received` (optional descriptor), `expected` (optional descriptor), `inner_errors` (list of nested errors).
- `received` and `expected` MUST be plain serialisable descriptor strings, not embedded `ZerxValue`; `received` is a compact type tag of what arrived; `expected` describes the schema's requirement.
- `to_json` MUST be infallible and MUST return `serde_json::Value` directly, not `Result`.
- `received`, `expected`, and `inner_errors` MUST be omitted from serialised output when `None` or empty.
- `ZerxError` MUST implement `Serialize` and MUST NOT implement `Deserialize`; errors flow outward only.
- `ZerxError` MUST implement `std::error::Error`.
- The `ErrorCode` catalogue is open: each module MUST declare its own codes as associated constants in a separate `impl ErrorCode` block next to the code that raises them; no central exhaustive enum of all codes exists.
- The union-failure aggregation constructor is owned by this concern; which errors to pass and any variant-selection heuristic belong to the union type in `types`.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C7 and C8 in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at Concept Closeout, once promoted to `docs/decisions.md`.
