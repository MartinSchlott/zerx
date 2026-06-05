# Concern: Lua

## Purpose

- This concern owns the zerx-owned Lua input value type (`lua::LuaValue` + `LuaTable`) and the schema-directed `validate_lua` path that disambiguates Lua data against a `Schema` and produces a serde-bridgeable `ZerxValue`.

## Non-Goals

- This concern does NOT own the Lua runtime (VM, host commands, CBOR) — that is the consumer's (endymion).
- This concern does NOT add an external Lua dependency.
- This concern does NOT exist in the default build — it is gated behind the `lua` Cargo feature.

## Consumes from

- `schema-core`: `Schema` + parse flow (`validate_lua` feeds `parse_present` after the transform pass)
- `value-model`: `ZerxValue` (the output of `validate_lua`)
- `errors`: `ZerxError` (structured validation failure returned by `validate_lua`)

## Provides to

(None — `validate_lua` is an external contract, not an internal concern-to-concern edge.)

## External Contracts

- `Schema::validate_lua(&lua::LuaValue) -> Result<ZerxValue, ZerxError>` — schema-directed Lua validation entry point.
- `lua::LuaValue` and `lua::LuaTable` — the public Lua input value types; re-exported from the crate root under the `lua` feature.

## External Services

(None.)

## Architecturally Significant Dependencies

(None — the `lua` feature adds no external crate dependency; `LuaValue`/`LuaTable` are zerx-owned types.)

## Constraints

- The `lua` feature MUST add no external crate dependency; `LuaValue` and `LuaTable` MUST be zerx-owned types mirroring endymion's value shape exactly.
- `validate_lua` MUST execute two independent passes: a schema-directed transform pass (`lua_transform`) that resolves Lua ambiguity into a `ZerxValue`, followed by an unchanged `parse_present` pass that performs full schema validation.
- The transform pass MUST resolve exactly two Lua ambiguities: `Bytes` → `String` or `Bytes(raw)` depending on the directing schema kind; `Table` → `Array` or `Object` depending on the directing schema kind.
- A `Bytes` value under a string-family schema (`string`, `enumerate`, `uri`, `url`, `literal(<String>)`) MUST be decoded as UTF-8 to produce `ZerxValue::String`; non-UTF-8 bytes MUST produce `ZerxValue::Bytes`, which the parse pass rejects as `TYPE_MISMATCH` (no Lua-specific error code).
- A `Bytes` value under `buffer` MUST be kept raw as `ZerxValue::Bytes` without any decode attempt.
- The transform pass MUST be depth-guarded using the same `ParseContext` depth counter as the parse pass (`MAX_PARSE_DEPTH = 100`); a table tree that would exceed the limit MUST return `PARSE_DEPTH_EXCEEDED`.
- No data-value cycle guard is required; the producer (endymion) guarantees acyclic, finite trees.
- The transform pass MUST be gate-aware: container gates (unknown-key gate for strict object, length gate for tuple, discriminator gate for discriminated union) MUST be applied before any field/element value is converted, to preserve the error-precedence contract.
- Error codes: `LUA_INVALID_KEY` — a hash key is not a UTF-8 byte string where the directing schema requires string keys; `LUA_SHAPE_MISMATCH` — a table's populated parts do not match the directing schema kind (non-empty hash under `array`/`tuple`; non-empty array part under `object`/`record`; mixed table under a free conversion).
- Representability errors (`LUA_INVALID_KEY`, `LUA_SHAPE_MISMATCH`) MUST be raised only by the transform pass, never by the parse pass; schema-semantic errors (`TYPE_MISMATCH`, `UNKNOWN_PROPERTY`, `REQUIRED`, `TUPLE_LENGTH_MISMATCH`, `INVALID_DISCRIMINANT`, validator errors, `REFINEMENT_FAILED`) MUST be raised only by the parse pass.
- Non-string hash keys and mixed tables MUST be rejected with representability errors even under free-conversion schemas (`any`, `json`, `jsonschema`) — no silent data loss.
- An empty Lua table under a free-conversion schema MUST produce an empty `ZerxValue::Array` (the documented disambiguation bias).
- The discriminator value in a `discriminated_union` MUST be mapped to an infallible scalar placeholder (`Bool`/`I64`/`String`/`Null`) before the Miss/non-keyable path builds its output object; a `Table`-valued discriminator MUST produce the `Null` placeholder, never a recursive table conversion.

## Related Decisions

- `D-lua-schema-directed`
