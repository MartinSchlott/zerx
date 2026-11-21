# PLAN_VM1: Bound value nesting at the serde input boundary

## Context & Goal

Bug card "Deep `ZerxValue` clone and drop recurse per nesting level" (`docs/bug.kanban.md`),
commissioned as a stability fix for zerx 3.2.1 (DevLead, 2026-11-21, under the Product Owner's
mandate): an accepted deep value can abort the process with a stack overflow.

### Reproduction (zerx 3.2.0, debug build, 2 MB thread)

A scratch probe outside the repository built nested single-element arrays iteratively and
`mem::forget`-ed every value it did not mean to drop. Depth = number of nested containers.

| Path | Survives | Aborts |
|---|---|---|
| `any()`/`array(any())`/`lazy` `.validate(&serde_json::Value)` | 600 | 700 |
| `json().validate(&ZerxValue)` | 500 | 600 |
| `parse_delta("", &serde_json::Value)` | 600 | 700 |
| `replace(&serde_json::Value, "/0", &1)` | 600 | 700 |
| `validate_lua(&LuaValue)` under `any()` | refused with `PARSE_DEPTH_EXCEEDED` at every depth tested (500 – 100 000) | — |
| `ZerxValue` derived `Clone` | 2 000 | 5 000 |
| `ZerxValue` derived `PartialEq`, `Debug` | 2 000 | 5 000 |
| `ZerxValue` `Serialize` into `serde_json::to_value` | 1 000 | 2 000 |
| drop of a `ZerxValue` (and, for comparison, `serde_json::Value`, `LuaValue`) | 5 000 | 20 000 |

The first overflow on every serde input path is `ZerxValue::from_serialize`: `ValueSerializer`
recurses through the caller's `Serialize` once per level with large debug frames, and nothing
bounds it. The `Clone`/`Drop` recursion named on the card is real but sits an order of magnitude
deeper; it bites only because nothing stops a deep value from entering. The Lua boundary is
already safe: its table conversion counts every table level against `MAX_PARSE_DEPTH`
(`docs/architecture/lua.md`, `free_convert` in `src/lua.rs`).

### Why not an iterative `Drop`

A hand-written `Drop` on `ZerxValue` would make every by-value pattern that moves a field out of a
variant (`match v { ZerxValue::Array(items) => items, … }`, error E0509) stop compiling — in
consumer code too. That is a public API break and not admissible in a patch release. A
non-recursive `Clone` alone would not help: `Drop`, `PartialEq`, `Debug` and `Serialize` recurse
at similar depths. The commission's fallback applies: refuse at a documented depth.

**Goal:** zerx never builds a `ZerxValue` from caller input whose counted nesting exceeds
`MAX_PARSE_DEPTH` (100). Struct and tuple enum variants count one input level but emit two
container levels (`{variant: {…}}` / `{variant: […]}`), so the built value can reach about twice
the counted depth (≈200) — still an order of magnitude below the measured abort depths. `from_serialize` — and with it `validate`, `parse_delta`, `replace` and every import path
that converts a JSON value — refuses deeper input with `PARSE_DEPTH_EXCEEDED`, the same limit and
code the Lua boundary already uses. All later recursion over zerx-built values (identity clone in
`any()`/`json()`/`literal`, drop, `PartialEq`, `Debug`, `Serialize` out) then stays shallow.
`replace` additionally refuses a result deeper than the limit.

### Values that enter through builders

`default`, `meta`, `example` (the `Modify` trait, `src/schema.rs`) and `literal` (`src/types.rs`)
take `impl Into<ZerxValue>` and store it in the schema without passing `from_serialize`. A
hand-built deep value stored there is cloned recursively by every `Schema` clone, cloned again by
`parse_field` before a default is parsed, serialised recursively by export, and dropped
recursively with the schema — a plain `validate` or `to_json_schema` can overflow. The builders
are infallible (public API), so they cannot return an error. Instead, each such builder measures
the value's nesting iteratively; an over-deep value is **not stored**: zerx tears it down with an
internal iterative disposal helper (moving children onto an explicit work stack — possible because
`ZerxValue` has no `Drop`) and marks the schema node defective. A defective node fails closed: it
rejects every value with `PARSE_DEPTH_EXCEEDED` (after the `nullable` null short-circuit), a
missing field under it yields the same error instead of a default, and it exports via the existing
fail-closed export (`false`, or `{"type":"null"}` when nullable). This follows the
`discriminated_union` precedent of an infallible builder whose defect surfaces at validation.

### What stays outside the 3.2.1 guarantee

- A caller's own clone, drop, `==`, `Debug` or `Serialize` of a deep `ZerxValue` or `LuaValue` it
  built itself, in its own code (same as `serde_json::Value`). Only an `impl Drop` / manual
  `Clone` on the public enum would change that, and `impl Drop` is a source-compatibility break
  (E0509); it is an open decision for the supervisor, not part of this plan.
- Deep values the caller captures in its own `refine` closures or custom `Validator`
  implementations; zerx never sees them.

The card is narrowed to that residue.

## Breaking Changes

Behaviour narrowing, no API change. Two kinds of input that previously validated when the stack
happened to be large enough are now refused with `PARSE_DEPTH_EXCEEDED`:

- input nested more than 100 containers deep;
- input whose serialisation needs more than `3 * MAX_PARSE_DEPTH` (300) nested serializer calls of
  any kind — containers, `Option` (`serialize_some`), newtype structs, and newtype-struct map keys.
  Wrapper-dense shapes can therefore be refused below 100 container levels (e.g. a struct chain
  whose every link is `Option<Option<Option<…>>>` around the next struct), and a pure wrapper chain
  with no containers is refused beyond 300 wrappers.

Input within both bounds converts exactly as before; in particular a 100-level chain whose every
link is an `Option<Option<…>>` field (3 calls per level) still passes. No new public item, no new error code;
patch release 3.2.0 → 3.2.1 as commissioned. Recovery for a caller who needs deeper data: none in
this release; that would be a new decision (higher limit or a major-version `Drop`).

## Reference Patterns

- `free_convert` / `free_convert_inner` in `src/lua.rs` — the existing per-container depth guard
  and its `PARSE_DEPTH_EXCEEDED` error.
- `free_convert_is_depth_guarded` test in `src/lua.rs`.
- `ValueSerializer`, `SeqBuilder`, `MapBuilder`, `TupleVariantBuilder` and the other builders in
  `src/value.rs`.

## Dependencies

None.

## Steps

1. **Failing tests first** (see Verification); confirm each aborts or fails on the current code.
   Run deep tests on a spawned thread with a 2 MB stack and `mem::forget` every deep input and
   output, so the test exercises zerx and not the drop of the fixture. Build fixtures iteratively.
   A test that aborts the binary must be confirmed failing in isolation (`scripts/verify test
   <name>`), then pass after the fix.

2. **Depth-carrying serializer** (`src/value.rs`). Give `ValueSerializer` a depth field
   (container levels already open) and a frame field (nested serializer calls of any kind); every
   builder (`SeqBuilder`, `MapBuilder`, the tuple/struct variant builders, `MapKeySerializer`
   where it re-enters) carries the depth of the container it builds and hands `depth + 1` to the
   `ValueSerializer` it creates for children.
   - Opening a container (`serialize_seq`, `serialize_tuple`, `serialize_tuple_struct`,
     `serialize_tuple_variant`, `serialize_map`, `serialize_struct`, `serialize_struct_variant`,
     and `serialize_newtype_variant`, which produces a one-key object) that would make the depth
     exceed `MAX_PARSE_DEPTH` MUST return
     `ZerxError::new(ErrorCode::PARSE_DEPTH_EXCEEDED, "input value nesting exceeds maximum depth of 100")`
     (format the constant), before recursing.
   - Transparent wrappers (`serialize_some`, `serialize_newtype_struct`) add no value level but do
     add a stack frame: count every nested `ValueSerializer` call in the frame field and refuse
     with the same code once it exceeds `3 * MAX_PARSE_DEPTH`, so a chain of newtypes/options
     cannot recurse unboundedly while fields of up to two nested `Option`s at every level of
     100-deep data still pass (exactly 300 calls). Add an explanatory comment stating that
     invariant on the frame field.
   - `MapKeySerializer::serialize_newtype_struct` re-enters itself for a newtype key; it MUST carry
     the frame count, seeded from the `MapBuilder`'s frame state in `serialize_key` (and the struct
     variant / map builders likewise), and refuse with the same error once the same `3 *
     MAX_PARSE_DEPTH` budget is exceeded. Every other key path is unchanged.
   - Struct/tuple variants count as one level (do not count their two output containers twice:
     that would refuse a single variant inside 99-deep ordinary data).
   - Count exactly as `free_convert` does: a value with `MAX_PARSE_DEPTH` nested containers is
     accepted, one more is refused. Scalars add no level.
   - `ZerxValue::from_serialize`'s signature and every other behaviour stay unchanged.

3. **`replace` result bound** (`src/delta.rs`). After the immutable rebuild and before root
   revalidation, refuse with `PARSE_DEPTH_EXCEEDED` when the rebuilt tree is nested deeper than
   `MAX_PARSE_DEPTH` (instance and new value are each bounded, their combination is not). Compute
   the depth with an explicit work stack, not recursion.

4. **Builder value bound** (`src/schema.rs`, `src/types.rs`).
   - A crate-private `fn value_depth(&ZerxValue) -> usize` (iterative, explicit stack; same
     counting as Step 2: containers count, scalars do not) and a crate-private
     `fn dispose(ZerxValue)` that tears a value down iteratively (pop a value, move the children
     of `Array`/`Object` out onto the stack, let the now-shallow node drop). Put both in
     `src/value.rs`; `Map` exposes whatever crate-private owned-entry access `dispose` needs.
   - `Modify::default`, `Modify::meta`, `Modify::example` and `literal(…)`: when
     `value_depth(value) > MAX_PARSE_DEPTH`, do not store the value; `dispose` it and set a
     crate-private defect marker on the schema node (field shape the Coder's choice, e.g. a flag in
     `Modifiers`). `literal` stores a shallow placeholder constant (`Null`) beside the marker.
   - Parse flow: in `parse_present_body`, directly after the `nullable` null short-circuit and
     before lazy resolution, a defective node returns
     `ZerxError::new(ErrorCode::PARSE_DEPTH_EXCEEDED, "schema value nesting exceeds maximum depth of 100")`
     (format the constant); `parse_field` on a missing value returns the same error for a
     defective node before consulting `default`/`optional`.
   - Export: `accepts_nothing` in `src/json_schema.rs` returns `true` for a defective node, so the
     existing `never_export` applies.
   - The `Record` export arm decides `additionalProperties` through `is_plain_any(value)` and
     bypasses `export_node` for a plain `any()`; `is_plain_any` MUST return `false` for a defective
     node, so the value schema routes through `export_node` → `accepts_nothing` → `never_export`
     and the record exports `additionalProperties: false` (or `{"type":"null"}` when the defective
     value schema is nullable). Check every other arm that short-circuits on `is_plain_any` or a
     similar shape test the same way.
   - The defect marker survives clone-and-return chaining (later modifier calls keep it).
   - The `schema-core` constraint "An explicit `Null` value MUST NOT trigger `default`" and the
     missing-value order stay as they are for non-defective nodes.

5. **No other code change.** The identity `value.clone()` in `parse_inner`, the derived `Clone`,
   `Drop`, `PartialEq`, `Debug` and `Serialize` of `ZerxValue` stay as they are; the bound makes
   them safe for zerx-built values. Do not add `Drop` to `ZerxValue`, `Map` or `LuaValue`.

6. **Version.** Not the Coder's: the Planner bumps to 3.2.1 at finalization.

## Documentation

Updated by the Planner at finalization, after verification:

- `docs/architecture/value-model.md`, Constraints: `from_serialize` MUST refuse input nested more
  than `MAX_PARSE_DEPTH` containers deep with `PARSE_DEPTH_EXCEEDED`, before recursing further;
  transparent wrapper chains MUST be bounded as well. Basis: the load-bearing stack-safety
  invariant the parse flow and the Lua boundary already state (`MAX_PARSE_DEPTH`,
  `docs/architecture/lua.md`), and the Product-Owner-authorized stability goal of this fix.
- `docs/architecture/schema-core.md`, Constraints: a value stored by `default`, `meta`, `example`
  or `literal` that is nested deeper than `MAX_PARSE_DEPTH` MUST NOT be kept; the node MUST fail
  closed with `PARSE_DEPTH_EXCEEDED` (after the `nullable` short-circuit, and for a missing field)
  and MUST take the fail-closed export. Basis: the same stack-safety invariant, and the existing
  infallible-builder rule (`D-result-only-api` keeps builders infallible; a defect surfaces at
  validation, as for `discriminated_union`). Also update the two enumerations it touches: the
  fixed present-value order (defect gate between the `nullable` short-circuit and the type check)
  and the missing-value order (defect before `default`/`optional`).
- `docs/architecture/schema-core.md`, Delta / Replace: `replace` MUST refuse a rebuilt tree nested
  deeper than `MAX_PARSE_DEPTH` with `PARSE_DEPTH_EXCEEDED`. Basis: the same invariant.
- `docs/bug.kanban.md`: narrow the card "Deep `ZerxValue` clone and drop recurse per nesting level"
  to the residue (hand-built deep values cloned or dropped by the caller; an iterative `Drop`
  would break by-value variant destructuring) and keep it `Open`, `minor`; the zerx input paths
  are fixed by this plan.
- `README.md`: confirm or adjust the existing sentences on `parse_depth_exceeded` at depth 100 and
  `MAX_PARSE_DEPTH = 100` so they describe the input bound, the wrapper budget and the builder
  values accurately.
- No register entry proposed or superseded.

## Verification

Required gate: `scripts/verify all`. `lua` tests are feature-gated; `test core` covers the
feature-off build.

Tests (owning modules' existing `mod tests` blocks; deep ones on a 2 MB thread, fixtures
iteratively built and forgotten):

`src/value.rs`:
- boundary: `from_serialize` of a `serde_json::Value` with exactly `MAX_PARSE_DEPTH` nested
  arrays succeeds; with `MAX_PARSE_DEPTH + 1` it returns `PARSE_DEPTH_EXCEEDED`; the same for
  nested objects and for a deep `ZerxValue` passed as the source;
- 100 000 levels: `from_serialize` of a `serde_json::Value`, of a `ZerxValue`, and of a recursive
  user struct (`struct Node { next: Option<Box<Node>> }`, serialised as nested structs) returns
  `PARSE_DEPTH_EXCEEDED` without overflow;
- a 100 000-deep newtype chain (`struct W(Option<Box<W>>)`) returns `PARSE_DEPTH_EXCEEDED`
  without overflow; a 100-level struct chain whose every link is an `Option<Option<…>>` field
  succeeds (300 calls), and the same chain with a third `Option` per link is refused;
- a map whose key is a 100 000-level newtype-struct chain returns `PARSE_DEPTH_EXCEEDED` without
  overflow; a key wrapped in a few newtypes still converts to its string;
- a 60-deep struct-variant chain (`enum Node { B { child: Box<Node> }, L }`) converts (built value
  about 120 levels), and a 101-deep one is refused;
- bytes, maps, enums (all four variant shapes) and existing conversion tests pass unchanged;
- move-semantics guard (required by the supervisor, DevLead 2026-11-21): a test function that
  takes `ZerxValue`s by value and moves the payload out of every variant by destructuring
  (`ZerxValue::Array(items) => items`, `ZerxValue::Object(map) => map`, `String(s)`,
  `Bytes(b)`, and the scalar variants); it compiles only while `ZerxValue` does not implement
  `Drop` (E0509), so a future `Drop` impl on the enum fails the `test` gate. `Map` (crate-private
  fields, moved whole) and `LuaValue` are not guarded by this test; Step 5 forbids `Drop` on them.

`src/schema.rs`: 100 000 levels against `any()`, `json()`, `array(any())`, a self-referential
`lazy` array schema and `literal(<shallow constant>)` via `validate` → `PARSE_DEPTH_EXCEEDED`, no
overflow; a 100-level value under `any()` is accepted and returned intact.

`src/schema.rs` / `src/types.rs`, builder values (100 000 levels, hand-built `ZerxValue`, on a
2 MB thread; nothing deep is forgotten here — the builder must dispose it itself):
- `object([("a", any().default(deep))])`: building, cloning the schema, `validate(&{})` →
  `PARSE_DEPTH_EXCEEDED` at path `["a"]`, `validate(&{"a": 1})` → `PARSE_DEPTH_EXCEEDED`,
  `to_json_schema` (the node exports as `false`), and dropping the schema all complete without
  overflow;
- the same for `literal(deep)`, `any().meta("k", deep)` and `any().example(deep)`;
- `any().default(deep).nullable()` accepts `null` and exports `{"type":"null"}`;
- `record(any().default(deep))` exports `additionalProperties: false`, and with `.nullable()` on
  the value schema `additionalProperties: {"type":"null"}`; `{}` still validates under that record
  (no value is parsed, so the defect never fires — existing record semantics, pinned as known
  behaviour), `{"k": 1}` is refused with `PARSE_DEPTH_EXCEEDED` at path `["k"]`;
- a 100-level value in each builder is kept and behaves as before (default applied, literal
  matched, meta/example exported).

`src/delta.rs`: `parse_delta("", deep)` and `replace(deep, "/0", &1)` at 100 000 →
`PARSE_DEPTH_EXCEEDED`; `replace(shallow_instance_80_deep, <pointer 80 deep>, value_40_deep)` →
`PARSE_DEPTH_EXCEEDED` (combined result over the limit); a combination within the limit succeeds.

`src/json_schema.rs`: `from_json_schema` of a schema whose `const` (and one whose `default`) is
100 000 levels deep returns an error (`PARSE_DEPTH_EXCEEDED` or the existing import error the
conversion surfaces), without overflow.

`src/lua.rs`: `validate_lua` of a 100 000-deep table under `any()` → `PARSE_DEPTH_EXCEEDED`
(regression guard for the already-safe path).

Final: `scripts/verify all` exits 0. No acceptance check needs agent judgment.
