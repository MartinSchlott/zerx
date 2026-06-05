# PLAN_L1_lua_validate

Phase 5 of `docs/CONCEPT_zerx_foundation.md` (the final concept plan). Implements
candidate decision **C3** (`D-lua-schema-directed`) and reconciles the obsolete
mlua/host-opaque design dropped mid-concept.

## Context & Goal

zerx must validate Lua data produced by the sibling runtime **endymion**. endymion's
host boundary exposes only data — `nil`/`bool`/`int`/`float`/`bytes`/`table`; Lua
functions, coroutines, and userdata are *not representable* there (reading one
returns an error), so there is nothing "host-opaque" to validate. The earlier
mlua/host-opaque design (C3 old, `PLAN_M1_host_opaque`) is therefore obsolete and is
removed by this plan.

The replacement is **schema-directed Lua validation** (C3 revised): given a `Schema`
and a raw Lua value, the schema resolves every Lua ambiguity in one disambiguating
pass, producing a normal serde-bridgeable `ZerxValue` which the existing parse flow
then validates. The two ambiguities the schema resolves:

- **byte string → `string` vs `buffer`** — Lua strings are byte vectors; the schema
  node decides whether they decode to UTF-8 text or stay raw bytes.
- **table → `array` vs `object`/`record`** — a Lua table carries an array part and a
  hash part; the schema node decides which is meant, and non-string hash keys are
  rejected at the exact path.

zerx **owns** the Lua input type (`zerx::lua::LuaValue` + `LuaTable`) behind a `lua`
Cargo feature, mirroring endymion's shape so consumers of both crates share one type
with no conversion. The `lua` feature adds **no external dependency** (no `mlua`).

**Goal:** ship `Schema::validate_lua(&lua::LuaValue) -> Result<ZerxValue, ZerxError>`
behind the `lua` feature, remove the obsolete `mlua` feature and `HostOpaque`
placeholder, and reconcile the permanent docs that still reference host-opaque/mlua.
This plan completes the concept; the Architect runs Concept Closeout afterwards.

## Breaking Changes

**Yes** — but only to the experimental, never-realised `mlua` feature surface:

- The Cargo feature `mlua` is **removed** and replaced by `lua`. Any build enabling
  `--features mlua` breaks; nothing in the default build changes.
- `ZerxValue::HostOpaque` (the uninhabited placeholder, present only under the old
  `mlua` feature) is **removed**. It was uninhabited and never constructible, so no
  runtime value or default-build code depended on it.

The **default build is unaffected**: `ZerxValue`, the parse flow, JSON Schema
roundtrip, delta/replace, and the policy pipeline keep their exact current behaviour
and public API. No Product-Owner action (DB reset, env change) is required.

## Reference Patterns

- `src/types.rs:1520-1634` — T4 special types (`buffer`, `uri`, `url`, `json`,
  `jsonschema`): builder structs, `BuilderInner`/`From<…> for Schema`, constructors.
  The `lua` module's new public types and the validate path follow this module style.
- `src/schema.rs:317-394` — `Schema::validate` / `parse_present` / `parse_present_body`
  / `parse_field`: the parse flow `validate_lua` feeds into and whose ordering it
  mirrors during the transform pass.
- `src/schema.rs:230-255` — `ParseContext` depth guard (`enter`/`exit`,
  `MAX_PARSE_DEPTH`); the transform pass reuses it.
- `src/types.rs:599-630` — `ObjectBody { shape, mode, … }`,
  `DiscriminatedUnionBody { key, variants, state }`, `DiscriminatorState::Ok { map,
  allowed }`, `DiscriminantKey`: the bodies the transform reads to direct per-field
  and per-variant conversion.
- `src/delta.rs:64-74` — `resolve_lazy` loop: the lazy-resolution pattern the
  transform reuses to reach a concrete `SchemaKind`.
- Error-code open-catalogue pattern (`impl ErrorCode { … }` in the owning module,
  no edit to `src/error.rs`) — `src/types.rs:570-586`, `src/delta.rs:12-19`.
- Reference behaviour to port (read, do not import):
  - endymion `src/value.rs:18-36` — the `LuaValue`/`LuaTable` shape to mirror.
  - Zex `src/zex/**/transformLua` — the schema-directed disambiguation Zex performs;
    its Lua-interop tests are the behavioural oracle (see Verification).

## Dependencies

**No new crate dependencies.** This is the central point of C3/C9: the `lua` feature
adds only zerx-owned types and the `validate_lua` path.

- `Cargo.toml`: remove `[features] mlua = []`; add `[features] lua = []` (empty
  feature, no `dep:` entries).

## Assumptions & Risks

- **A1 — endymion's value shape is the contract.** `zerx::lua::LuaValue` mirrors
  endymion `src/value.rs:18-36` exactly:
  - `LuaValue`: `Nil`, `Boolean(bool)`, `Integer(i64)`, `Float(f64)`, `Bytes(Vec<u8>)`,
    `Table(LuaTable)`.
  - `LuaTable { array: Vec<LuaValue>, hash: Vec<(LuaValue, LuaValue)> }` — `array` holds
    the contiguous integer-keyed `1..=n` part; `hash` holds arbitrary key/value pairs
    in insertion order. endymion guarantees the tree is **acyclic**, hash keys/values
    are **non-nil**, and the array part contains no nil.
- **A2 — no data-value cycle guard needed.** Because the producer delivers acyclic
  finite trees (A1), `validate_lua` needs no visited-set cycle guard; the existing
  `MAX_PARSE_DEPTH` depth guard is sufficient and is reused. The `schema-core.md` note
  "data-value cycle detection deferred to `PLAN_M1`" is now false and is removed.
- **R1 — divergence from Zex on strictness (deliberate, plan-local).** Zex silently
  ignores non-string keys and the unused part of a mixed table. zerx instead **errors**
  at the exact path, consistent with strict-by-default (C4) and the Architect's
  mapping mandate. This is a plan-local decision (it does not bind future plans) and is
  archived with this plan. Concretely: non-string/non-UTF-8 hash keys under
  `object`/`record`/free → `LUA_INVALID_KEY`; a non-empty hash under `array`/`tuple`, a
  non-empty array part under `object`/`record`, or a mixed table under a free conversion
  (`any`/`json`/`jsonschema`) → `LUA_SHAPE_MISMATCH`. The strictness is applied uniformly,
  including under free conversion (Finding 1 — no silent data loss). The one inherent
  ambiguity with no schema to resolve it is an **empty** Lua table under a free
  conversion; it is defined to produce an empty `ZerxValue::Array`.
- **R2 — union/discriminated-union entanglement.** Disambiguation at a `union` cannot
  be decided without attempting each variant (one variant may want `string`, another
  `buffer`). The transform therefore *tries* variants (Step C), accepting one extra
  validation pass per union for clarity. Mitigation: keep all validation semantics in
  the existing parse flow; the transform only selects and disambiguates.
- **R3 — minimal schema-core seam.** The transform must depth-guard its own recursion
  (a pathologically deep table would overflow the stack before `parse_present` runs).
  This requires `ParseContext::enter`/`exit` to be `pub(crate)` (currently private).
  This is the only edit to `src/schema.rs`; it is a visibility widening, not a
  behaviour change, and falls within this plan's schema-core reconciliation scope.

## Steps

### A. Remove the obsolete mlua/host-opaque surface

1. `Cargo.toml`: replace `mlua = []` with `lua = []` under `[features]`.
2. `src/value.rs`:
   - Delete the `#[cfg(feature = "mlua")] pub enum HostOpaque {}` block
     (`src/value.rs:82-90`, including its doc comment).
   - Delete the `#[cfg(feature = "mlua")] HostOpaque(HostOpaque)` variant from
     `ZerxValue` (`src/value.rs:110-112`).
   - Delete the `#[cfg(feature = "mlua")] ZerxValue::HostOpaque(h) => match *h {}` arm
     and its comment from the `Serialize` impl (`src/value.rs:281-285`). The match over
     the remaining variants stays exhaustive.
3. `src/delta.rs`: delete the `#[cfg(feature = "mlua")] ZerxValue::HostOpaque(h) =>
   match *h {}` arm and its comment in `set_at` (`src/delta.rs:236-239`). The closed
   `match value` over the remaining `ZerxValue` variants stays exhaustive.
4. `src/types.rs`:
   - In `type_tag` (`src/types.rs:31-46`) delete the `#[cfg(feature = "mlua")]
     ZerxValue::HostOpaque(_) => "host_opaque",` arm.
   - In `check_json` (`src/types.rs:1496-1505`) delete the `#[cfg(feature = "mlua")]`
     `HostOpaque` guard and the `let _ = value;`; the body becomes `Ok(())` with the
     parameter renamed to `_value`.
   - In `check_jsonschema` (`src/types.rs:1507-…`) apply the identical deletion.
5. Confirm no other `mlua`/`HostOpaque`/`host_opaque` references remain in `src/`
   (grep must be clean except the unrelated comment at `src/json_schema.rs:1737`,
   which is a test label and MAY be left or reworded; it references the vision example,
   not the feature).

### B. The `lua` module: value types and feature wiring

6. Create `src/lua.rs`, the whole file gated `#![cfg(feature = "lua")]` in spirit
   (the module is declared under `#[cfg(feature = "lua")]` in `lib.rs`). Define the
   public input types mirroring A1 exactly:
   - `pub enum LuaValue { Nil, Boolean(bool), Integer(i64), Float(f64), Bytes(Vec<u8>),
     Table(LuaTable) }` — `#[derive(Debug, Clone, PartialEq)]`.
   - `pub struct LuaTable { pub array: Vec<LuaValue>, pub hash: Vec<(LuaValue,
     LuaValue)> }` — `#[derive(Debug, Clone, PartialEq, Default)]`.
7. Declare the plan's error codes in an `impl ErrorCode { … }` block in `src/lua.rs`
   (open-catalogue pattern; no edit to `src/error.rs`):
   - `LUA_INVALID_KEY` (`"lua_invalid_key"`) — a hash key is not a UTF-8 byte string
     where the directing schema (`object`/`record`/free) requires string keys.
   - `LUA_SHAPE_MISMATCH` (`"lua_shape_mismatch"`) — a table's populated parts do not
     match the directing schema kind (non-empty `hash` under `array`/`tuple`, or
     non-empty `array` part under `object`/`record`, or a mixed table under a free
     conversion).
   - There is **no** `LUA_INVALID_UTF8` code. A byte string that the directing schema
     wants as a `string` but which is not valid UTF-8 is converted to
     `ZerxValue::Bytes` and rejected by the existing parse flow as `TYPE_MISMATCH`
     (`expected = "string"`, `received = "bytes"`). This reuses existing machinery and
     keeps the byte-string mismatch on the same precedence footing as any other type
     mismatch (see the precedence rules in Step C).
8. `src/lib.rs`:
   - Add `#[cfg(feature = "lua")] pub mod lua;`.
   - Add `#[cfg(feature = "lua")] pub use lua::{LuaValue, LuaTable};`.
9. `src/schema.rs`: widen `ParseContext::enter` and `ParseContext::exit` from private
   to `pub(crate)` (R3). No other change to this file's logic.

### C. `validate_lua` and the schema-directed transform

All of the following lives in `src/lua.rs`, in an `impl Schema { … }` block (methods
may be split across files within the crate). The transform is the only genuinely new
logic; all *validation* (type checks, validators, modes, defaults, optional/nullable,
refine, union selection semantics, discriminated-union checks) is delegated to the
existing, tested parse flow.

**Precedence contract (the spine of this section).** `validate_lua` MUST preserve the
existing parse-flow error precedence. Two error classes exist:

- *Schema-semantic* errors — `TYPE_MISMATCH`, `UNKNOWN_PROPERTY`, `REQUIRED`,
  `TUPLE_LENGTH_MISMATCH`, `INVALID_DISCRIMINANT`, validator errors, `REFINEMENT_FAILED`
  — are raised **only** by `parse_present`, never by the transform.
- *Representability* errors — `LUA_INVALID_KEY`, `LUA_SHAPE_MISMATCH` — are raised
  **only** by the transform, and only at the node whose own structure cannot be
  represented as a `ZerxValue` at all.

The transform achieves correct precedence by being **gate-aware**: at every container
it mirrors the ordering of the corresponding parse delegate
(`parse_object`/`parse_tuple`/`parse_discriminated_union`) and MUST NOT convert any
child subtree that the parse delegate would reject before ever reaching it. Concretely:
in strict mode the unknown-key gate precedes field-value conversion; the tuple
length gate precedes element conversion; the discriminator gate precedes variant-body
conversion. This is what makes the two-pass split sound.

10. Public entry:
    ```text
    pub fn validate_lua(&self, lua: &LuaValue) -> Result<ZerxValue, ZerxError> {
        let zv = self.lua_transform(lua, &mut ParseContext::new())?;
        self.parse_present(&zv, &mut ParseContext::new())
    }
    ```
    Two passes are intentional: the transform resolves Lua ambiguity and produces a
    serde-bridgeable `ZerxValue` (or fails on unrepresentable structure); `parse_present`
    then performs the full, unchanged validation. Both return
    `Result<ZerxValue, ZerxError>` (C7).

11. `lua_transform(&self, lua, ctx) -> Result<ZerxValue, ZerxError>` — schema-directed,
    depth-guarded, recursive:
    - **Lazy + depth:** resolve `Lazy` layers to a concrete kind (reuse the
      `delta.rs:resolve_lazy` loop pattern); wrap the body in `ctx.enter()? … ctx.exit()`
      so the transform recursion is depth-bounded exactly like the parse flow (R3,
      `MAX_PARSE_DEPTH`). On the depth error, do not call `exit` (mirror
      `ParseContext::enter`'s balanced contract).
    - **Leaf / representation choice** (closed `match` over the resolved `SchemaKind`;
      no catch-all, so a future variant is a compile error):

      | Directing kind | Lua input | Produces |
      |---|---|---|
      | `String`, `Enum`, `Uri`, `Url` | `Bytes(b)` | `String` from UTF-8 decode of `b`; **non-UTF-8 → `Bytes(b)`** (parse → `TYPE_MISMATCH`, no Lua error) |
      | `Literal(c)` where `c` is `String` | `Bytes(b)` | as above (so `literal("cat")` matches a Lua byte string; non-UTF-8 → `Bytes`) |
      | `Buffer` | `Bytes(b)` | `Bytes(b.clone())` (no decode) |
      | `Number`, `Boolean`, `Null`, and every leaf kind facing a non-container Lua value | any non-`Table` | base-convert (Step 12) |
      | `Any`, `Json`, `JsonSchema` | any | free-convert (Step 13) |
      | any scalar/leaf kind | `Table(t)` | free-convert(`Table`) (Step 13) → parse yields `TYPE_MISMATCH` |

    - **Container handling** (gate-aware, preserving the precedence contract):
      - `Array(item)` facing `Table(t)`: if `t.hash` is non-empty → `Err(LUA_SHAPE_MISMATCH)`
        at the container node (a table with a hash part is not a pure sequence). Else
        produce `Array` of `item.lua_transform(e, ctx)` for each `e` in `t.array`, in
        index order (matches `parse_array`'s element order; element errors fire at the
        same index parse would). A non-`Table` input → base-convert → parse `TYPE_MISMATCH`.
      - `Tuple(items)` facing `Table(t)`: if `t.hash` is non-empty → `Err(LUA_SHAPE_MISMATCH)`.
        Then the **length gate first**: if `t.array.len() != items.len()`, produce a
        `ZerxValue::Array` of `t.array.len()` placeholder `Null`s (do **not** convert any
        element) so `parse_tuple` raises `TUPLE_LENGTH_MISMATCH`. Only when the lengths
        match, convert each position `i` with `items[i].lua_transform`, in index order.
      - `Record(value)` facing `Table(t)`: if `t.array` is non-empty → `Err(LUA_SHAPE_MISMATCH)`.
        Else build `Object` from `t.hash` in input order: decode each key (below); convert
        each value with `value.lua_transform`. (`parse_record` validates every value, so
        eager conversion matches its semantics; value errors fire in input order.)
      - `Object(body)` facing `Table(t)`: if `t.array` is non-empty → `Err(LUA_SHAPE_MISMATCH)`.
        Decode every `t.hash` key (below). Then branch on `body.mode`, mirroring
        `parse_object`:
        - **Strict:** if any decoded key (not in `effective_prestrip(body)`) is absent
          from `body.shape`, build an `Object` containing every decoded key mapped to a
          placeholder `Null` and return it — `parse_object` then raises `UNKNOWN_PROPERTY`
          on the first unknown key *in input order* without any field value being
          converted. If all keys are known, convert each shape field's value **in shape
          order** (look the value up by key in `t.hash`), producing the `Object`; absent
          fields are simply omitted (parse applies default/optional/REQUIRED).
        - **Strip:** build an `Object` of the known shape fields converted **in shape
          order**; unknown (and prestripped) keys are dropped (not converted). Parse
          strips per its rules.
        - **Passthrough:** convert known shape fields **in shape order**, then append
          unknown keys converted via free-convert **in input order** (matches
          `parse_object`'s "shape first, then passthrough in input order" output rule).
      - `Union(variants)` facing any input: for each variant `v` in declaration order,
        attempt `v.lua_transform(lua, ctx)` and, if that succeeds, `v.parse_present(&that,
        …)`; the first variant for which **both** succeed → return that variant's
        transform result. A variant whose transform or parse fails contributes its error
        to `inner_errors` and the next variant is tried. If none succeed →
        `Err(ZerxError::union(vec![], inner_errors))`. (Transform errors are caught
        per-variant here and therefore never preempt — this is the one place transform
        errors are swallowed by design.)
      - `DiscriminatedUnion(body)` facing `Table(t)` (the **discriminator gate first**):
        read the value of key `body.key` from `t.hash` and map it to a `DiscriminantKey`
        (Step 11a). Look it up in `body.state` (`DiscriminatorState::Ok { map }`):
        - **Hit** → transform the whole table against `body.variants[idx]` (an object
          schema, handled by the `Object` rule above).
        - **Miss / absent / non-keyable discriminator** → build an `Object` containing
          **only** the discriminator field, set to the *infallible discriminator
          placeholder* of Step 11a (or omit the field entirely if absent), and return it
          — `parse_present` raises `INVALID_DISCRIMINANT` without converting any other
          field. The whole-table free/base conversion MUST NOT be invoked on this path,
          so no representability error (`LUA_SHAPE_MISMATCH`/`LUA_INVALID_KEY`) can fire
          here — in particular a `Table`-valued discriminator yields the non-keyable
          `Null` placeholder, never a recursive table conversion. A non-`Table` input to
          the discriminated-union node → base-convert → parse `TYPE_MISMATCH`.

    - **(11a) Discriminator mapping** (Finding 2 — `DiscriminantKey` admits `Bool`,
      `Int`, `Str`): map the Lua discriminator value to **both** a keyability verdict and
      an *infallible discriminator placeholder* `ZerxValue` (a scalar conversion that can
      never fail — no table recursion, no key decoding):
      - `Boolean(b)` → keyable `DiscriminantKey::Bool(b)`; placeholder `ZerxValue::Bool(b)`.
      - `Integer(n)` → keyable `DiscriminantKey::Int(n as i128)`; placeholder
        `ZerxValue::I64(n)`.
      - `Bytes(b)` valid UTF-8 → keyable `DiscriminantKey::Str(decoded)`; placeholder
        `ZerxValue::String(decoded)`.
      - `Float`, `Table`, `Nil`, and non-UTF-8 `Bytes` → **non-keyable**; placeholder
        `ZerxValue::Null` (a fixed sentinel; `Table` is **not** routed through
        free-convert).

      The placeholder is what the Miss path puts in the discriminator field. Because every
      placeholder is `Bool`/`I64`/`String`/`Null`, `parse_discriminated_union`'s own
      `discriminant_key` (`src/types.rs:636`) agrees on keyability — `Null` →
      `None` → `INVALID_DISCRIMINANT` — and no placeholder construction can raise a
      representability error. On the **Hit** path the placeholder is unused: the whole
      table is transformed against the matched variant (whose discriminator field is a
      `Literal`, decoding the byte string itself).
    - **(11b) Key decoding** (object/record/free): a hash key MUST be `LuaValue::Bytes(b)`
      with `b` valid UTF-8 → `String`; any other key kind, or non-UTF-8 bytes →
      `Err(LUA_INVALID_KEY)` with a best-effort key descriptor in the message and the key
      (when decodable) or its position prepended to `path`. Duplicate decoded keys follow
      `Map::insert` last-write-wins. Key decoding for all of a table's keys precedes the
      mode branch, so a non-representable key is reported before unknown-key
      classification (a key that cannot be represented cannot be classified).
    - **Error paths:** child errors MUST carry the path to the failing node — prepend the
      object key or the decimal element index as the transform unwinds (same discipline
      as the parse-flow container delegates, `types.md` PLAN_T2).

12. `base-convert(lua) -> ZerxValue` — schema-independent leaf mapping, no table
    recursion: `Nil`→`Null`, `Boolean(b)`→`Bool(b)`, `Integer(n)`→`I64(n)`,
    `Float(f)`→`F64(f)`, `Bytes(b)`→`Bytes(b)`. A `Table` never reaches base-convert
    from the leaf path; callers route a `Table` facing a scalar kind through
    free-convert (Step 13) so the resulting container `TYPE_MISMATCH` comes from parse.

13. `free-convert(lua) -> Result<ZerxValue, ZerxError>` — schema-independent recursive
    mapping, used by `Any`/`Json`/`JsonSchema` and by passthrough unknown-key values.
    Scalars map as in base-convert, except `Bytes(b)` → `String` if `b` is valid UTF-8
    else `Bytes(b)` (matches Zex's `any` byte handling). For `Table(t)` it is **strict
    and consistent with the directed rules (Finding 1 — no silent data loss)**:
    - `t.array` non-empty **and** `t.hash` non-empty → `Err(LUA_SHAPE_MISMATCH)` (a mixed
      table is unrepresentable as a single JSON-shaped value; it is never silently split).
    - `t.hash` non-empty (array empty) → `Object`: decode each key (Step 11b),
      free-convert each value, input order.
    - otherwise (`t.hash` empty) → `Array` of free-convert over `t.array` (an empty table
      → empty `Array`; this is the documented disambiguation bias for an empty Lua table
      under a free conversion, where no schema is available to direct it).

### D. Tests (in `src/lua.rs`, gated by the `lua` feature)

14. Cover every mapping branch, every new error code, and every precedence rule. At
    minimum:
    - **string vs buffer:** `string()` accepts a UTF-8 `Bytes`; `string()` on non-UTF-8
      `Bytes` → `TYPE_MISMATCH` with `received = "bytes"` (no `LUA_*` code); `buffer()`
      accepts the same non-UTF-8 `Bytes` unchanged as `ZerxValue::Bytes`.
    - **table → array:** `array(number())` over a `Table { array:[Integer,…], hash:[] }`
      → `ZerxValue::Array`; the same with non-empty `hash` → `LUA_SHAPE_MISMATCH`.
    - **table → object/record:** `object([...])` / `record(...)` over a hash-only table
      → `ZerxValue::Object`; non-empty `array` part → `LUA_SHAPE_MISMATCH`; a non-string
      (e.g. `Integer`) hash key → `LUA_INVALID_KEY` at that path; a non-UTF-8 `Bytes`
      key → `LUA_INVALID_KEY`.
    - **object semantics via parse flow:** unknown key under default strict → reuses
      `UNKNOWN_PROPERTY`; missing optional field omitted; missing defaulted field gets
      its default — assert these still come from `parse_present`, not the transform.
    - **precedence — unknown key beats nested value error (Finding 3-i):** a strict
      `object([("a", object([...]))])` fed a table that has BOTH an unknown key `"x"`
      AND a known key `"a"` whose value is an unrepresentable (mixed) table MUST fail
      with `UNKNOWN_PROPERTY` on `"x"`, NOT `LUA_SHAPE_MISMATCH` on `"a"` — proving
      unknown-key values are not converted in strict mode.
    - **precedence — tuple length beats element error (Finding 3-ii):** a
      `tuple([string(), number()])` fed a 3-element array table MUST fail with
      `TUPLE_LENGTH_MISMATCH`, even when one of the extra/elements is itself an
      unrepresentable table — proving elements are not converted on a length mismatch.
    - **precedence — discriminator beats body error (Finding 3-iii):** a
      `discriminated_union("kind", […])` fed a table with an absent or unmatched
      discriminator MUST fail with `INVALID_DISCRIMINANT`, even when another field's
      value is an unrepresentable table — proving the body is not converted on a
      discriminator miss.
    - **scalars:** `Integer`→ `number()`/`number().int()` ok; `Float` with fract under
      `number().int()` → `NOT_INTEGER` (existing validator); `boolean()`/`null()`;
      `Nil` under `string().nullable()` → `Null`, under non-nullable `string()` →
      `TYPE_MISMATCH`.
    - **nested:** an object containing a buffer field and a string field, both fed
      `Bytes`, disambiguated correctly per field.
    - **union:** `union([string(), buffer()])` over a UTF-8 `Bytes` resolves to the
      first succeeding variant (`string()`); over a non-UTF-8 `Bytes`, `string()` fails
      and `buffer()` wins.
    - **discriminated_union — all key kinds (Finding 2):** discriminator delivered as a
      `Bytes` byte string decodes and selects a string-literal variant (port Zex's
      `lua-union-literal-discriminant-bytes` behaviour); a `Boolean` discriminator
      selects a `literal(true/false)` variant; an `Integer` discriminator selects a
      `literal(<int>)` variant; a wrong/absent discriminator → `INVALID_DISCRIMINANT`.
    - **discriminator Miss path cannot fail with a representability error (Finding 3
      follow-up):** a `discriminated_union` fed a table whose discriminator field is
      itself a **mixed `Table`** (or a `Float`/`Nil`) MUST fail with
      `INVALID_DISCRIMINANT`, NOT `LUA_SHAPE_MISMATCH` — proving the non-keyable `Null`
      placeholder is used and the discriminator value is never recursively converted.
    - **lazy + depth:** a `lazy`-wrapped object validates via `validate_lua`; a table
      nested past `MAX_PARSE_DEPTH` → `PARSE_DEPTH_EXCEEDED` (proves the transform is
      depth-guarded, R3).
    - **`any`/`json` free-convert:** a hash-only nested table under `any()` free-converts
      (nested `Bytes` decode to strings); a `Bytes`-key table → `LUA_INVALID_KEY`; a
      **mixed** table under `any()`/`json()` → `LUA_SHAPE_MISMATCH` (Finding 1 — no
      silent data loss); an empty table under `any()` → empty `Array` (documented bias).
15. Port the relevant Zex Lua-interop test *cases* (string/buffer disambiguation,
    0-/1-based array tables, strict-vs-strip under Lua, discriminator-as-bytes) as zerx
    tests, adapting expectations to R1 (zerx errors where Zex silently ignores).

### E. Doc reconciliation (Doc Update step — after build + tests pass, Hard Rule 12)

This plan removes obsolete code, so the archived permanent docs that referenced
host-opaque/mlua are reconciled **in the same plan** (concept "Affected docs" §). All
edits are normative-style (RFC 2119, no code, no narrative).

16. Create `docs/architecture/lua.md` (new concern, `architecture-concern` skeleton):
    - **Purpose:** owns the zerx-owned Lua input value type (`lua::LuaValue` +
      `LuaTable`) and the schema-directed `validate_lua` path that disambiguates Lua
      data against a `Schema` and produces a serde-bridgeable `ZerxValue`.
    - **Non-Goals:** does NOT own the Lua *runtime* (VM, host commands, CBOR) — that is
      the consumer's (endymion); does NOT add an external Lua dependency; does NOT
      exist in the default build (feature-gated `lua`).
    - **Consumes from:** `schema-core`: `Schema` + parse flow (`validate_lua` feeds
      `parse_present`); `value-model`: `ZerxValue` (the output); `errors`: `ZerxError`.
    - **Provides to:** (none internal — `validate_lua` is an external contract).
    - **External Contracts:** `Schema::validate_lua(&lua::LuaValue) ->
      Result<ZerxValue, ZerxError>`; the public `lua::LuaValue`/`LuaTable` types.
    - **Constraints:** the disambiguation rules (Step 11) as MUST-rules; the two error
      codes (`LUA_INVALID_KEY`, `LUA_SHAPE_MISMATCH`); the precedence contract
      (representability errors from the transform vs schema-semantic errors from the
      parse flow, Step C); non-UTF-8 bytes under a string-family schema → `TYPE_MISMATCH`
      (no Lua code); depth-guarded; no cycle guard (acyclic input, A2); strict treatment
      of non-string keys and mixed tables, including under free conversion (R1, Finding 1).
    - **Related Decisions:** candidate-note "candidate C3" only (no `D-` slug until
      Closeout, per the concept's decision-reference discipline).
17. Delete `docs/architecture/mlua.md`.
18. `docs/architecture/_overview.md`: change the concern list entry and the
    `mlua → value-model` relationship and the "Host boundary (`mlua` feature)" seam to
    `lua` (the Lua data boundary owned by `lua`). Add the `lua → schema-core`
    relationship (validate_lua feeds the parse flow).
19. `docs/architecture/value-model.md`: remove the `HostOpaque` variant from the
    variants constraint (`value-model.md:38`), the two host-opaque constraints
    (`value-model.md:44-45`), and the `Provides to … mlua: ZerxValue::HostOpaque` edge
    (`value-model.md:22`). Update the candidate-decisions note (drop C3 from this
    concern's governing set, since the host-opaque layer is gone; C5 wording already
    revised in the concept). The `Purpose` line "plus the host-opaque layer" → single
    serde-bridgeable layer.
20. `docs/architecture/schema-core.md`: remove the Non-Goal "does NOT implement
    data-value cycle detection — see `mlua`" (`schema-core.md:15`) and the constraint
    "Data-value cycle detection is deferred to `PLAN_M1` …" (`schema-core.md:61`).
    Update the delta Non-Goal "Host-opaque descent errors … deferred to
    `PLAN_M1_host_opaque`" (`schema-core.md:74`) — the `replace` `Serialize` bound
    already excludes Lua input (Lua values are not `Serialize`); restate as: host-opaque
    descent no longer exists; `replace` operates only on serde-bridgeable trees.
21. `docs/architecture/types.md`: remove the Non-Goal "does NOT own the host-opaque
    `function`/`tvalue` types — see `mlua`" (`types.md:13`); drop `"host_opaque"` from
    the `type_tag` constraint (`types.md:48`); remove the `json`/`jsonschema` constraint
    about rejecting `ZerxValue::HostOpaque` under the `mlua` feature (`types.md:180`).
22. `docs/architecture/json-schema.md`: remove the Non-Goal line "The host-opaque format
    markers (`function`, `tvalue`) belong to `PLAN_M1_host_opaque` …"
    (`json-schema.md:13`).
23. `docs/definition.md`: revise host-opaque/mlua wording to schema-directed Lua
    validation — Scope bullet (`definition.md:13`: "host-opaque Lua values via a
    dedicated path under an optional feature" → "Lua values via schema-directed
    validation under an optional feature"); the `ZerxValue` entity line
    (`definition.md:36`: drop "plus host-opaque values"); the `Non-JSON-clean
    validation` feature (`definition.md:27`); Success Criteria mentioning `mlua`
    functions/userdata (`definition.md:44`, `definition.md:49`: Lua *data* validates,
    not functions/userdata).
24. Run the `permanent-doc-consistency-check` skill; it MUST pass green (every concern
    listed in `_overview.md`, every `Consumes from`/`Provides to` dual present, no
    dangling `mlua` references, no slug references to non-Active decisions).

### F. Archive

25. Move `docs/PLAN_L1_lua_validate.md` to `docs/archive/` and commit all changes
    (code + docs) in one session commit. (Concept Closeout is a separate, Architect-led
    step that follows.)

## Verification

- **Default build unchanged:**
  - `cargo build` and `cargo test` (no features) pass; the full existing suite is
    green. Grep confirms zero `mlua`/`HostOpaque`/`host_opaque` references remain in
    `src/` (modulo the unrelated test label at `src/json_schema.rs:1737`).
  - `cargo build --no-default-features` (if applicable) and `cargo build` produce an
    identical public API for the non-`lua` surface (spot-check `cargo doc` or
    `cargo public-api` is not required; the diff to `src/lib.rs` re-exports is limited
    to the new `#[cfg(feature = "lua")]` lines).
- **Feature build:**
  - `cargo build --features lua` and `cargo test --features lua` pass, including all
    Step D tests.
  - `cargo clippy --features lua --all-targets` is clean (no new warnings;
    `result_large_err` already allowed crate-wide where needed).
- **Behavioural assertions** (the Step D tests are the executable spec). Each of the
  three core ambiguities has a positive and a negative case; each new error code
  (`LUA_INVALID_KEY`, `LUA_SHAPE_MISMATCH`) is asserted with its exact `code` and `path`;
  non-UTF-8 bytes under a string schema assert `TYPE_MISMATCH`/`received="bytes"`; the
  three precedence regressions (unknown-key ≻ nested value, tuple-length ≻ element,
  discriminator ≻ body) are asserted; depth-guard and `Bool`/`Int`/`Str` discriminator
  behaviour are covered.
- **Regression guard:** the ported Zex Lua-interop cases (Step 15) pass with zerx's
  stricter expectations (R1), demonstrating the disambiguation matches the battle-tested
  original except where strict-by-default deliberately diverges.
- **Docs:** `permanent-doc-consistency-check` passes; a manual grep over `docs/` shows
  no remaining `mlua`/`host-opaque`/`PLAN_M1` references except inside this archived
  plan and the concept's historical-context paragraphs.
