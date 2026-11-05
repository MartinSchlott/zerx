# PLAN_T5: Literals compare by JSON Schema value, not by `ZerxValue` variant

## Context & Goal

A numeric `const` or non-string `enum` imported from JSON Schema rejects equal values with
`value does not match literal constant`. This affects Janus in production, which uses zerx through
endymion-validate.

Probe on zerx 2.0.1 (rev 23cd066), through `from_json_schema` and `Schema::validate`:

| Schema | Argument | Today | Required |
|---|---|---|---|
| `const: 1` | `I64(1)` | `invalid_literal` | accepted |
| `enum: [1, 2]` | `I64(2)` | `union_mismatch` | accepted |
| `const: 1.0` | `I64(1)` | `invalid_literal` | accepted |
| `const: 1` | `F64(1.0)` | `invalid_literal` | accepted |
| `const: -1` | `I64(-1)` | accepted | accepted |
| `const: 1.5` | `F64(1.5)` | accepted | accepted |
| `const: {"v": 1}` | `{"v": I64(1)}` | `invalid_literal` | accepted |
| `const: {"a": -1, "b": -2}` | `{"b": I64(-2), "a": I64(-1)}` | `invalid_literal` | accepted |
| `oneOf` + discriminator, `k: {const: 1}` | `{k: I64(1), ...}` | `invalid_literal` | accepted |
| `oneOf` + discriminator, `k: {const: 1}` | `{k: F64(1.0), ...}` | `invalid_discriminant` | accepted |
| same schema through `validate_lua` | `k = LuaValue::Float(1.0)` | `invalid_discriminant` | accepted |

Root causes:

1. `src/json_schema.rs` builds literals through `ZerxValue::from_serialize`, which maps a positive
   `serde_json::Number` to `U64`, a negative one to `I64` and a float to `F64`. endymion-validate's
   `json_to_zerx` maps every number that fits to `I64`.
2. `check_literal` in `src/types.rs` compares with `value == constant`, and `ZerxValue` derives
   `PartialEq`, so `I64(1) != U64(1)`. `Map` derives `PartialEq` over its entry vector, so key order
   also decides equality. A non-string `enum` is a union of literals and takes the same path.
3. `discriminant_key` in `src/types.rs` and `lua_discriminant_map` in `src/lua.rs` treat every float
   argument as non-keyable, so `F64(1.0)` fails before `check_literal` runs.

`src/delta.rs` compares no values itself. `parse_delta` and `replace` go through `parse_present` and
receive the fix without a change.

**Goal:** a literal accepts a value exactly when JSON Schema calls the two equal: numbers by exact
mathematical value across `I64`, `U64`, `I128`, `U128` and `F64`, recursively inside arrays and
objects, with object keys independent of order. Everything else stays strict.

### Decisions taken during discussion

Settled with the Product Owner; the implementation does not re-open them.

1. **`ZerxValue`/`Map` `PartialEq` stay derived.** `check_literal` is the only non-test site in zerx
   that compares `ZerxValue`s with `==`. Tests in zerx, endymion and Janus use `assert_eq!` to assert
   exact variants (for example `assert_eq!(json_to_zerx(&v), ZerxValue::U64(n))`); a value-based
   `PartialEq` would leave them green on the wrong variant. It would also silently change a public
   trait's meaning, and the test that `validate` keeps the input form could no longer detect a rewrite.
2. **Object key order is part of this fix.** Requested by the Product Owner. JSON Schema object
   equality ignores order, and `free_convert` in `src/lua.rs` keeps Lua table order.
3. **Float literals stay non-keyable on the schema side.** Only the *argument* side of the
   discriminator lookup accepts an integral float. Making `discriminant_key` accept floats for
   literals would turn `oneOf` with `const: 1.0` from a union fallback into a discriminated union and
   change its export, which is out of bounds. That fallback union accepts equal values through
   `literal_eq` anyway.

## Breaking Changes

No. Values that were wrongly rejected are now accepted; no API, error code or export changes, and
`check_literal`'s message is unchanged. A consumer relying on `literal(5i64)` rejecting `U64(5)` sees the
documented caveat lifted.

One message changes by intent: an integral `F64` discriminator argument that matches no variant, such as
`F64(3.0)`, now takes the existing miss branch (`no variant matched discriminator 'k'`) instead of the
non-keyable branch (`discriminator field 'k' has a non-keyable value`). The code stays
`invalid_discriminant`, and consumers branch on code per `D-english-only-errors`. Neither message carries
an argument value. Under `validate_lua`, `Float(3.0)` keeps the non-keyable message through the `Null`
placeholder.

## Reference Patterns

- `src/types.rs` — `check_literal`, `discriminant_key`, `parse_discriminated_union`, in-module tests
  using the `val` helper (`literal_matching`, `discriminated_union_happy_path`).
- `src/json_schema.rs` — in-module import tests (`import_union_discriminated`).
- `src/lua.rs` — in-module discriminator tests (`discriminated_union_integer_discriminator`).

## Assumptions & Risks

- **Argument values in errors.** Janus relies on error fields not carrying argument values. No error
  construction changes; `expected` carries the schema's constant and `received` a type tag only.
- **Recursion.** `literal_eq` recurses only while both sides are containers, so its depth is bounded
  by the constant's depth, which is schema-defined.

## Steps

### 1. Add `literal_eq` in `src/types.rs`

Add a private `fn literal_eq(value: &ZerxValue, constant: &ZerxValue) -> bool` next to `check_literal`
and make `check_literal` use it instead of `==`. Keep the error construction byte-identical.

Rules, in this order:

- `(Array, Array)`: equal length and `literal_eq` pairwise by position.
- `(Object, Object)`: equal `len()` and, for every key of one side, the other side has that key with a
  `literal_eq` value. `Map` holds no duplicate keys, so this is symmetric.
- `(F64(a), F64(b))`: `a == b`. Thus `-0.0 == 0.0`, `NaN` equals nothing, infinities equal themselves.
- Both sides have an exact integer form (below): the forms are equal.
- Anything else: derived `value == constant`. This keeps `String`, `Bytes`, `Bool` and `Null` strict,
  keeps `Bytes` apart from `String` and `Array`, and yields `false` for a non-integral or out-of-range
  float against an integer, and for a number against a non-number.

Exact integer form, a private helper returning `Option<(bool, u128)>` as (is negative, magnitude):

- `I64`, `I128`: `(n < 0, n.unsigned_abs())`; `U64`, `U128`: `(false, n)`.
- `F64(f)`: only when `f` is finite, `f.fract() == 0.0` and `f.abs() < 2^128`:
  `(f < 0.0, f.abs() as u128)`. The cast is exact because `f` is integral and in range. `-0.0` gives
  `(false, 0)`.
- Every other variant: `None`.

No integer is ever converted to `f64`. This is what keeps `U64(9007199254740993)` unequal to
`F64(9007199254740992.0)`, and it covers `U128` values above `i128::MAX`.

### 2. Accept integral float arguments in the discriminator lookup

- In `src/types.rs` add a `pub(crate)` helper taking an `f64` and returning
  `Option<DiscriminantKey>`: `Some(DiscriminantKey::Int(f as i128))` when `f` is finite, integral and
  in `[-2^127, 2^127)`, else `None`. Same range as the existing `i128` key space.
- In `parse_discriminated_union`, derive the key of the **argument** with this helper when it is `F64`,
  and with `discriminant_key` otherwise. Do not change `discriminant_key` or
  `build_discriminator_state`: float literals remain non-keyable and `literal(1.0)` as a discriminator
  still yields `DiscriminatorState::Invalid`.
- In `lua_discriminant_map` in `src/lua.rs`, add an arm for `LuaValue::Float(f)` returning this key and
  the `Null` placeholder, and adjust the comment on the fallback arm. On a hit the whole table is
  transformed against the variant; on a miss the `Null` placeholder keeps today's
  `invalid_discriminant` result, and the placeholder set stays as `docs/architecture/lua.md` states it.

### 3. Tests

Extend the existing `mod tests` blocks; `docs/tests.md` needs no new suite. Boundary: unit. Perspectives:
conformance, failure mode, round-trip.

`src/json_schema.rs`, through `from_json_schema` and `Schema::validate` with hand-built `ZerxValue`s:

- Every row of the Context table, with the listed required outcome.
- `const: 1` and `enum: [1, 2]` accept `I64`, `U64` and `F64` of equal value; `const: 1.0` likewise.
- Unequal rejected: `const: 1` against `I64(2)`, `U64(2)`, `F64(1.5)`, `F64(NaN)`; `enum: [1, 2]` against
  `I64(3)` with `union_mismatch`.
- `const: 9007199254740993` rejects `F64(9007199254740992.0)` and accepts `U64(9007199254740993)`.
- Nested: `const: {"v": 1}` and `const: [1, {"v": 2.0}]` accept every integer/float form; unequal nested
  number, missing key, extra key and different array length rejected; array order still matters.
- `const: 1` rejects `String("1")`, `Bool(true)`, `Null`, `Bytes(vec![1])` and `Array([I64(1)])`.
- `const: "x"` rejects `Bytes(b"x")`.
- Rejection keeps code, message and fields: `invalid_literal`,
  `value does not match literal constant`, `expected` = `1`, `received` = `number`. For a nested literal
  with a distinctive string argument, the serialised error does not contain that string.
- The result keeps the input form, asserted with strict `assert_eq!`: `I64(1)` stays `I64(1)` and
  `F64(1.0)` stays `F64(1.0)` under `const: 1`; an object argument under a literal keeps its own key order.
- Discriminated union with `k: {const: 1}` / `{const: 2}`: accepts `I64(1)`, `U64(1)`, `F64(1.0)` and
  returns the input form; `F64(3.0)` and `F64(1.5)` yield `invalid_discriminant`.
- Export unchanged: `const: 1` exports `{"const": 1}`, `const: 1.0` exports `{"const": 1.0}`, an
  `I64`-built `literal(5i64)` exports `{"const": 5}`, and each export/import/export is stable.
- `oneOf` + discriminator with `const: 1.0`/`const: 2.0` still imports as a union and exports as
  `anyOf` with `x-oneOf: true` and no `discriminator`; it now accepts `I64(1)` and `F64(1.0)`.

`src/types.rs`:

- `literal_eq` boundaries on hand-built literals: `i128::MIN` against `F64(-2^127)` equal; `U128(u128::MAX)`
  against `F64(2^128)` unequal; `U128(2^127)` against `F64(2^127)` equal; `F64(0.0)` against `F64(-0.0)` and
  `I64(0)` equal; `F64(NaN)` against itself unequal; `F64(INFINITY)` against itself equal.
- `ZerxValue` `PartialEq` stays strict: `I64(1) != U64(1)`, `I64(1) != F64(1.0)`, and maps with different
  key order are unequal.
- Hand-built `discriminated_union` with `literal(1.0)` still yields `invalid_discriminated_union`.
- The float-discriminator helper: `-2^127` hits `i128::MIN`; `2^127` returns `None` rather than a saturated
  `i128::MAX`; the next float below `-2^127`, `NaN`, `INFINITY`, `NEG_INFINITY` and `1.5` return `None`.
- Update the stale comment in `literal_matching` and assert that `literal(5i64)` accepts `U64(5)`.

`src/lua.rs`:

- A discriminated union with integer discriminators under `validate_lua`: `Float(1.0)` selects the variant
  and the result holds `F64(1.0)`; `Float(3.0)` and `Float(1.5)` yield `invalid_discriminant`.
- A multi-key object `literal` under `validate_lua` accepts a table with the keys in a different order.

## Documentation

At finalization, after verification:

- `docs/architecture/types.md`, `literal`: replace the `value == constant` rule and the
  "exact variant equality … accepted for v1" caveat with the value-based rule: numbers by exact
  mathematical value across variants, never through a lossy `f64` conversion; recursive through arrays
  and objects; object keys independent of order; non-numbers strict; `ZerxValue` `PartialEq` stays
  variant-strict; the returned value keeps its input form.
- `docs/architecture/types.md`, `discriminated_union`: state that an integral `F64` argument in `i128` range
  keys as its integer, while float *literals* stay non-keyable.
- `docs/architecture/lua.md`: state that an integral `Float` discriminator keys as its integer; the placeholder
  rule stays.

No register entry is proposed or superseded; the choice is visible at `literal_eq`. `docs/tests.md` is
unchanged. `docs/architecture/json-schema.md` is unaffected.

**Version:** patch, 2.0.1 → 2.0.2 in `Cargo.toml` and `Cargo.lock`, applied by the Planner at finalization.

## Verification

Required gate: `scripts/verify all` (lint, `test`, `test core`, `api`) passes on the final functional state.
`test core` matters because this plan touches `#[cfg(feature = "lua")]` code.

Acceptance check, run and reported by the Coder: `git diff` shows no change to `ZerxValue`'s or `Map`'s
`derive`, none to `discriminant_key` or `build_discriminator_state`, and no changed error construction in
`check_literal`.
