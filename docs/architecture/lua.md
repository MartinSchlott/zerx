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
- `types`: `object and discriminant helpers` (prestrip, object mode and discriminant keying)

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
- A `Bytes` value under a string-family schema (`string`, `enumerate`, `uri`, `url`, `literal(<String>)`) MUST be decoded as UTF-8 to produce `ZerxValue::String`; non-UTF-8 bytes MUST produce `ZerxValue::Bytes`, which the parse pass rejects with that schema's own error (`TYPE_MISMATCH`, or `INVALID_LITERAL` under `literal`), with no Lua-specific error code.
- A `Bytes` value under `buffer` MUST be kept raw as `ZerxValue::Bytes` without any decode attempt.
- Under a free conversion (`any`, `json`, `jsonschema`, an unknown passthrough key), a `Bytes` value MUST decode to `ZerxValue::String` when it is valid UTF-8 and stay `ZerxValue::Bytes` otherwise.
- The transform pass MUST be depth-guarded with the parse pass's limit (`MAX_PARSE_DEPTH = 100`), counting each schema step and each freely converted table; a table tree that would exceed the limit MUST return `PARSE_DEPTH_EXCEEDED`.
- Unwrapping `lazy` layers before a transform step MUST be bounded by the same limit and MUST return `PARSE_DEPTH_EXCEEDED` beyond it, because a lazy may resolve only to further lazies.
- No data-value cycle guard is required; the producer (endymion) guarantees acyclic, finite trees.
- The transform pass MUST be gate-aware: container gates (unknown-key gate for strict object, length gate for tuple, discriminator gate for discriminated union) MUST be applied before any field/element value is converted, to preserve the error-precedence contract.
- `LUA_INVALID_KEY` MUST be raised when a hash key that becomes an object key (under `object`, `record` or a free conversion) is not a UTF-8 byte string.
- `LUA_SHAPE_MISMATCH` MUST be raised when a table's populated parts do not match the directing schema kind: a non-empty hash part under `array`/`tuple`, a non-empty array part under `object`/`record`, or a mixed table under a free conversion.
- Representability errors (`LUA_INVALID_KEY`, `LUA_SHAPE_MISMATCH`) MUST be raised only by the transform pass, never by the parse pass.
- Schema-semantic errors (`TYPE_MISMATCH`, `UNKNOWN_PROPERTY`, `REQUIRED`, `TUPLE_LENGTH_MISMATCH`, `INVALID_DISCRIMINANT`, validator errors, `REFINEMENT_FAILED`) MUST be raised only by the parse pass.
- `union` and `OneOf` variant selection validates each transformed candidate inside the transform pass and reports `UNION_MISMATCH` / `ONE_OF_AMBIGUOUS` from there.
- Non-string hash keys and mixed tables MUST be rejected with representability errors even under free-conversion schemas (`any`, `json`, `jsonschema`) — no silent data loss.
- An empty Lua table under a free-conversion schema MUST produce an empty `ZerxValue::Array` (the documented disambiguation bias).
- The discriminator value in a `discriminated_union` MUST be mapped to an infallible scalar placeholder (`Bool`/`I64`/`String`/`Null`) before the Miss/non-keyable path builds its output object; a `Table`-valued discriminator MUST produce the `Null` placeholder, never a recursive table conversion.
- An integral `Float` discriminator in `i128` range MUST key as its integer, and its placeholder MUST be `Null`.
- The crate-internal `OneOf` kind MUST transform and parse a Lua value against every variant and return the transformed value only when exactly one variant succeeds; zero successes MUST yield the union error and several MUST yield `ONE_OF_AMBIGUOUS`.

## Related Decisions

- `D-lua-schema-directed`
