# PLAN_T3_object_utilities

## Context & Goal

Third plan of **Phase 2 — Type catalogue** in the `CONCEPT_zerx_foundation`
execution. Phase 1 (`PLAN_F3_error_model`, `PLAN_F1_value_model`,
`PLAN_F2_schema_core`) and the first two Phase-2 plans (`PLAN_T1_basic_types`,
`PLAN_T2_complex_types`) are complete and archived. T2 delivered the `object`
type with the three unknown-key modes (`strict`/`passthrough`/`strip`), the
ordered `ObjectBody { shape, mode }`, the recursive `parse_object` delegate, and
the path-prefix construction.

T3 delivers the **object utilities** named in the vision
(`docs/vision.md:131-136`) and the `types` concern
(`docs/architecture/types.md:7`):

- **Shape changes** (alter the schema's `shape`, derive a new schema):
  - `partial` — make every top-level field optional (all-optional variant).
  - `extend` — add (or overwrite) top-level fields.
  - `omit` — remove named top-level fields.
  - `omit_read_only` / `omit_write_only` — remove fields carrying the
    `read_only` / `write_only` modifier.
- **Runtime-only filters** (shape unchanged; input is filtered *before* the
  unknown-key / mode check):
  - `strip_only` — drop named keys from the input.
  - `strip_read_only` / `strip_write_only` — drop input keys whose shape field
    carries the `read_only` / `write_only` modifier.

T3 extends `ObjectBody` with the `all_optional` flag and the prestrip fields
(anticipated by T2: `PLAN_T2_complex_types.md:56-58`, `:204-207`) and threads
those fields through `parse_object`. It adds **no** new `SchemaKind` variant, no
new dispatch arm, no new parse-flow control logic in `schema-core`, and **no**
new error code. Every utility is an inherent method on the already-exported
`ObjectSchema` builder; each clones-and-returns per C1 immutability.

### Boundary (`docs/architecture/types.md`, `docs/architecture/schema-core.md`)

- `types` owns the object utilities (`docs/architecture/types.md:7`). T3 adds
  the eight inherent methods on `ObjectSchema`, the `ObjectBody` fields they set,
  and the prestrip/all-optional handling inside `parse_object`.
- T3 does **NOT** touch `schema-core`: no change to `src/schema.rs`, the parse
  flow, `parse_field`, `parse_present`, the depth/cycle guard, the `SchemaKind`
  container, or any dispatch method. All T3 logic lives in `src/types.rs` inside
  the `object` type's own delegate and builder — the same containment T2
  reserved for it.
- T3 does **NOT** own JSON Schema export/import. The `all_optional` flag becomes
  the source of truth for the exported `required` set, but reading it into a
  JSON Schema is `PLAN_J1_export`. T3 implements no `to_json_schema`.
- T3 adds **no** type, **no** validator, and **no** error code.

### Governing candidate decisions (`CONCEPT_zerx_foundation.md`)

- **C1 — hybrid enum-core + typed builders / clone-and-return immutability.**
  Each utility is an inherent method on `ObjectSchema` that clones the receiver
  (the builder is taken by value) and mutates only the relevant `ObjectBody`
  field, returning `Self`. The original schema is never mutated. This mirrors
  the existing `strict`/`passthrough`/`strip` methods (`src/types.rs:1041-1062`).
- **C4 — strict by default.** Utilities do **not** change the mode. `omit`,
  `extend`, `partial`, and the strip family all preserve the receiver's mode.
  A field removed by `omit*` becomes an unknown key under the (unchanged) strict
  mode — this is the intended interaction (`docs/vision.md:250`).
- **C7 — single `Result`-returning API.** Every utility method is infallible and
  returns `ObjectSchema` (used as `.into()` into a `Schema`). No utility returns
  a `Result`; structural impossibilities (e.g. omitting an absent key) are
  no-ops, never errors.

### Architect rulings to encode

None pending. T3 introduces **no new dependency** (no entry under Hard Rule 11);
all logic is pure over the existing `std` + `serde_json` set already present.

### Scope decision — omit∕strip composition (committed v1 contract)

Zex (`/Users/martinschlott/Documents/MyProjects/zex/src/zex/complex-types/object.ts:63-110,197-218`)
threads two extra sets — `removedReadOnlyKeys` / `removedWriteOnlyKeys` — so that
`omit_read_only().strip_read_only()` still strips, from the input, the keys that
`omit_read_only` already removed from the shape.

**T3 commits to the lean v1 contract: no removed-key tracking.**
`strip_read_only` / `strip_write_only` operate **only on the current shape** —
they strip input keys whose shape field carries the `read_only` / `write_only`
modifier. A field already removed from the shape by `omit_read_only` /
`omit_write_only` is therefore **not** auto-stripped by a subsequent
`strip_read_only` / `strip_write_only`; under the unchanged strict mode it
surfaces as `UNKNOWN_PROPERTY`. The explicit escape hatch for that composition is
`strip_only([...])`, which names the keys directly.

Rationale: the concept restricts scope to the named utilities
(`docs/vision.md:131-136`) and does **not** require the `omit*` → `strip*`
composition; `strip_only` fully covers the use case, so the two removed-key sets
are speculative scope (YAGNI / Hard Rule 8). This order-dependent behaviour is
**locked by Verification test 13** so it cannot drift silently. Reversing the
decision later is a strictly additive change (two `Vec<String>` fields on
`ObjectBody`, populated by the `omit_*` methods and unioned into the effective
strip set) and would not break any T3 behaviour.

## Breaking Changes

**No.** T3 is additive at the public boundary: eight new inherent methods on the
already-exported `ObjectSchema`. The non-additive edits are all internal
(`pub(crate)`): four new fields on `ObjectBody`, an updated `ObjectBody`
literal in the `object()` constructor, and a rewritten `parse_object` body. No
existing public signature changes; `src/lib.rs` is **unchanged** (no new symbol
to export — the methods ride on `ObjectSchema`). `src/schema.rs`,
`src/error.rs`, and `src/value.rs` are untouched.

## Dependencies

**None added.** No new crate. `Cargo.toml` is untouched.

## Reference Patterns

- `src/types.rs` (T2) — the authoritative pattern for everything T3 does:
  - `ObjectSchema::strict/passthrough/strip` (`:1041-1062`) — the
    match-`self.0.kind`-as-`Object(body)`, mutate, return-`Self` idiom every T3
    method follows.
  - `object()` constructor (`:1080-1095`) — the duplicate-key merge
    (last-write-wins, first-occurrence order) reused verbatim by `extend`; the
    `ObjectBody { shape, mode }` literal to be extended with the new fields.
  - `parse_object` (`:735-780`) — the strict unknown-key check, the
    `shape`-order field loop over `parse_field`, and the passthrough tail that
    T3 rewrites to honour prestrip and `all_optional`.
  - `prefix_path` (`:657-660`) — unchanged; reused on field errors.
  - the `#[cfg(test)] mod tests` layout and the `val(builder, &value)` helper
    (`:1255-1258`).
- `src/schema.rs` (F2) — read-only references:
  - `Modifiers { read_only, write_only, default, optional, … }`
    (`:50-65`) — `omit_read_only` / `strip_read_only` read `read_only`;
    `omit_write_only` / `strip_write_only` read `write_only`. All are
    `pub(crate)`, reachable from `types.rs` (already read by
    `build_discriminator_state`, `src/types.rs:1166-1183`).
  - `Schema::parse_field` (`:347-367`) — the fixed missing/present precedence
    (default → optional → `REQUIRED`) that T3 reuses unchanged for the
    non-`all_optional` path.
- **Zex semantics** (`/Users/martinschlott/Documents/MyProjects/zex/src/zex/complex-types/object.ts`):
  - `partial` (`:43-45`), `omit` (`:48-60`), `omitReadOnly`/`omitWriteOnly`
    (`:63-90`), `stripOnly` (`:93-101`), `stripReadOnly`/`stripWriteOnly`
    (`:104-110`), `extend` (`:180-185`), and the prestrip step in `_parse`
    (`:187-219`) that runs **before** the strict unknown-property check.

## Assumptions & Risks

- **All logic stays in the `object` delegate.** Adding fields to `ObjectBody`
  and prestrip/all-optional handling to `parse_object` is exactly the
  containment T2 reserved (`PLAN_T2_complex_types.md:204-207`): "Adding a field
  to `ObjectBody` in T3 is additive and stays within the object type's own
  delegate — it does not reopen the shared parse flow." No `schema-core` edit.
- **`partial` preserves `parse_field`'s default precedence (contract-aligned).**
  Under `all_optional`, every field is still iterated through
  `Schema::parse_field` (honouring `docs/architecture/types.md:106`), and that
  call follows the fixed `default → optional → REQUIRED` precedence
  (`docs/architecture/schema-core.md:53`) unchanged. `partial`'s only effect is
  that the object delegate reinterprets a **missing required top-level field's**
  `Err(REQUIRED)` as "omit" instead of propagating it. Consequently, for a
  missing field: a `default` is still applied; an `optional` field is omitted; a
  plain required field is omitted (the partial effect). A **present** field
  validates normally. This **diverges from Zex** (`object.ts:287-307`, which
  suppresses defaults under `partial`); the divergence is deliberate — the zerx
  normative core contract (docs are truth for what *should be*) outranks
  Zex-faithfulness, and aligning with it keeps the "no `schema-core` touch /
  `parse_field` reused unchanged" boundary honest. The reinterpretation is scoped
  to `raw.is_none()` (a missing top-level field), so a nested `REQUIRED` raised
  by a *present* field's sub-validation can never be swallowed.
- **Prestrip can resurrect a default and can starve a required field.** Prestrip
  removes a key from the *working input* before the field loop, so the field is
  then seen as **missing** and runs the normal `parse_field` precedence:
  - a stripped key whose field has a `default` → the default is re-applied (the
    field reappears with its default value) — faithful to Zex
    (`object.ts:188-219` deletes, then the field loop re-defaults);
  - a stripped key whose field is **required** (no default, not optional) →
    `Err(REQUIRED)`. `read_only`/`write_only` fields used with the strip family
    SHOULD therefore be `optional` or `default`ed. Both behaviours are
    documented and locked by tests.
- **Prestrip precedes the unknown-key check.** A `strip_only` key that is *not*
  in the shape is dropped before the strict unknown-property scan, so it does
  **not** raise `UNKNOWN_PROPERTY` — the primary use of `strip_only` (silently
  drop selected foreign keys while staying strict on the rest).
- **`omit*` interacts with strict mode by design (C4).** `omit(["id"])` keeps
  the mode strict, so a subsequent input still carrying `id` raises
  `UNKNOWN_PROPERTY`. This is the documented, intended composition
  (`docs/vision.md:250`), not a defect.
- **No removed-key tracking (see Flagged scope decision).** `omit_read_only`
  followed by `strip_read_only` does **not** auto-strip the already-omitted keys
  in v1; use `strip_only` for that.

## Design

### Module layout

`src/types.rs` only. Three change sites:

1. **`ObjectBody`** (`:599-603`) — add four `pub(crate)` fields (below).
2. **`object()` constructor** (`:1080-1095`) — initialise the four new fields to
   their empty/false defaults in the `ObjectBody` literal.
3. **`parse_object`** (`:735-780`) — insert the prestrip step before the
   unknown-key check, and honour `all_optional` in the field loop.
4. **`impl ObjectSchema`** (`:1041-1062`) — add the eight utility methods after
   the existing `strict`/`passthrough`/`strip`.
5. **`#[cfg(test)] mod tests`** — append T3 cases.

`src/schema.rs`, `src/lib.rs`, `src/error.rs`, `src/value.rs`, `Cargo.toml`:
**unchanged**.

### `ObjectBody` extension

```rust
#[derive(Clone)]
pub(crate) struct ObjectBody {
    pub(crate) shape: Vec<(String, Schema)>,   // T2
    pub(crate) mode: ObjectMode,                // T2
    pub(crate) all_optional: bool,              // T3 — partial()
    pub(crate) prestrip_keys: Vec<String>,      // T3 — strip_only()
    pub(crate) prestrip_read_only: bool,        // T3 — strip_read_only()
    pub(crate) prestrip_write_only: bool,       // T3 — strip_write_only()
}
```

All four new fields are `Clone`; `ObjectBody` stays `Clone`. The `object()`
constructor initialises them to `false` / `Vec::new()`.

### Effective prestrip set (private helper)

```rust
fn effective_prestrip(body: &ObjectBody) -> Vec<String> {
    let mut keys = body.prestrip_keys.clone();
    if body.prestrip_read_only {
        for (k, fs) in &body.shape {
            if fs.modifiers.read_only { keys.push(k.clone()); }
        }
    }
    if body.prestrip_write_only {
        for (k, fs) in &body.shape {
            if fs.modifiers.write_only { keys.push(k.clone()); }
        }
    }
    keys
}
```

Membership is checked with `keys.iter().any(|k| k == key)`; duplicates are
harmless (no dedup required). The set is `Vec<String>` to match `shape`'s style;
it is small and consulted by membership only.

### `parse_object` rewrite

The structure stays T2's (read input → unknown check → field loop → passthrough
tail → return); prestrip and `all_optional` are woven in. `value`,
`ctx`, and the signature are unchanged.

1. `let input = value.as_object()` — unchanged; defensive non-object →
   `Ok(value.clone())`.
2. `let strip = effective_prestrip(body);` and a local
   `let is_stripped = |key: &str| strip.iter().any(|k| k == key);`.
3. **Strict unknown-key check** (only when `mode == Strict`): iterate
   `input.iter()`; **skip** any key where `is_stripped(key)`; for a remaining key
   not in `shape`, return the `UNKNOWN_PROPERTY` error exactly as T2
   (`:745-759`).
4. **Field loop**, in `shape` order, into `output`. Every field — including under
   `all_optional` — is delegated through `Schema::parse_field`
   (`docs/architecture/types.md:106`), which keeps its fixed
   `default → optional → REQUIRED` precedence (`docs/architecture/schema-core.md:53`):
   ```text
   for (key, field_schema) in &body.shape:
       let raw = if is_stripped(key) { None } else { input.get(key) };
       match field_schema.parse_field(raw, ctx):
           Ok(Some(v)) => output.insert(key.clone(), v),
           Ok(None)    => {}                                   // optional missing → omit
           Err(e) =>
               // partial: a MISSING required top-level field is treated as optional
               if body.all_optional && raw.is_none() && e.code == ErrorCode::REQUIRED { /* omit */ }
               else { return Err(prefix_path(e, key.clone())); }
   ```
   - A **present** field (`raw == Some`) always validates via
     `parse_field(Some(v))` → `parse_present`, regardless of `all_optional`.
   - Under `all_optional`, the only override is that a **missing** field's
     `Err(REQUIRED)` is reinterpreted as omit. A missing field with a `default`
     still returns `Ok(Some(default))` (default precedence preserved); a missing
     `optional` field still returns `Ok(None)` (omit). The `raw.is_none()` guard
     ensures a nested `REQUIRED` from a *present* field is never swallowed.
   - Without `all_optional`, the path is exactly T2's behaviour
     (default → optional-omit → `REQUIRED`), with prestripped keys treated as
     missing.
5. **Passthrough tail** (only when `mode == Passthrough`): iterate
   `input.iter()`; **skip** `is_stripped(key)`; append every non-shape key
   verbatim, as T2 (`:771-777`).
6. Return `Ok(ZerxValue::Object(output))`. Output ordering unchanged: shape
   order, then passthrough keys in input order.

### Builder methods (`impl ObjectSchema`)

All follow the existing `strict`/`passthrough`/`strip` idiom: take `mut self` by
value (the receiver is already a clone at the call site via `.clone()` per the
vision's `row.clone().omit(...)`), pattern-match `self.0.kind` as
`SchemaKind::Object(ref mut body)`, mutate, return `self`. The `if let` guard
makes each a no-op on a non-object kind (unreachable — these are inherent to
`ObjectSchema`).

- `partial(mut self) -> Self` — `body.all_optional = true;`.
- `extend<I, K>(mut self, fields: I) -> Self where I: IntoIterator<Item = (K, Schema)>, K: Into<String>`
  — merge each `(key, schema)` into `body.shape` with the **same** last-write-wins
  / first-occurrence-order rule as `object()` (`:1086-1092`): overwrite in place
  if the key exists, else push. Mode and all other `ObjectBody` state preserved.
- `omit<I, K>(mut self, keys: I) -> Self where I: IntoIterator<Item = K>, K: Into<String>`
  — collect `keys` into a `Vec<String>`; `body.shape.retain(|(k, _)| !drop.contains(k))`.
  Omitting an absent key is a no-op.
- `omit_read_only(mut self) -> Self` —
  `body.shape.retain(|(_, fs)| !fs.modifiers.read_only)`.
- `omit_write_only(mut self) -> Self` —
  `body.shape.retain(|(_, fs)| !fs.modifiers.write_only)`.
- `strip_only<I, K>(mut self, keys: I) -> Self where I: IntoIterator<Item = K>, K: Into<String>`
  — for each key, push into `body.prestrip_keys` if not already present (union).
- `strip_read_only(mut self) -> Self` — `body.prestrip_read_only = true;`.
- `strip_write_only(mut self) -> Self` — `body.prestrip_write_only = true;`.

`extend` takes erased `Schema` children (each composed at the call site via
`.into()`), consistent with the multi-child construction rule established for
`object` (`PLAN_T2_complex_types.md:253-276`):
`row.clone().extend([("created_at", string().into())])`.

### No new error codes

`omit*`/`partial`/`extend`/`strip*` introduce no new failure mode. Outcomes are
covered by the existing `UNKNOWN_PROPERTY`, `REQUIRED`, and `TYPE_MISMATCH`
codes. No `impl ErrorCode` block is added; `src/error.rs` is untouched.

## Steps

1. `src/types.rs`: extend `ObjectBody` with `all_optional: bool`,
   `prestrip_keys: Vec<String>`, `prestrip_read_only: bool`,
   `prestrip_write_only: bool`.
2. `src/types.rs`: update the `ObjectBody` literal in `object()` to initialise
   the four new fields (`all_optional: false`, `prestrip_keys: Vec::new()`,
   `prestrip_read_only: false`, `prestrip_write_only: false`).
3. `src/types.rs`: add the private `effective_prestrip(body)` helper.
4. `src/types.rs`: rewrite `parse_object` to compute the prestrip set, skip
   stripped keys in the unknown-key check and passthrough tail, and honour
   `all_optional` in the field loop (per Design).
5. `src/types.rs`: add the eight inherent methods to `impl ObjectSchema`
   (`partial`, `extend`, `omit`, `omit_read_only`, `omit_write_only`,
   `strip_only`, `strip_read_only`, `strip_write_only`).
6. Add the `#[cfg(test)] mod tests` cases per Verification (appended to the
   existing T2 test module).
7. Run the full build/lint/test matrix below; resolve every warning.

**Doc Update (workflow Step 6, by the Coder after Validation — not now):** add a
`### Object utilities (PLAN_T3)` subsection to the `### Complex types (PLAN_T2)`
block in `docs/architecture/types.md`, recording the review-hardened contract:

- the four `ObjectBody` fields (`all_optional`, `prestrip_keys`,
  `prestrip_read_only`, `prestrip_write_only`) and the rule that all T3 logic
  lives in the `object` delegate — no new `SchemaKind` variant, dispatch arm,
  parse-flow control logic, or error code;
- `partial`: `all_optional` makes every field optional by reinterpreting a
  missing required field's `REQUIRED` as omit; field iteration still delegates
  through `Schema::parse_field`, so `default → optional → REQUIRED` precedence is
  preserved (a missing defaulted field is still defaulted); a present field still
  validates;
- `extend`: merges fields with last-write-wins / first-occurrence order;
  preserves mode and all other object state;
- `omit` / `omit_read_only` / `omit_write_only`: shape removal (by name / by the
  field's `read_only` / `write_only` modifier); an omitted field becomes an
  unknown key under the unchanged strict mode (C4);
- `strip_only` / `strip_read_only` / `strip_write_only`: runtime-only input
  filtering applied **before** the unknown-key check; a stripped key is treated
  as missing for field validation (re-defaulted if defaulted, `REQUIRED` if
  required); `strip_read_only` / `strip_write_only` act **only on shape fields**
  carrying the modifier — no removed-key tracking, so a field already removed by
  `omit_read_only` / `omit_write_only` is not auto-stripped (committed v1
  contract; use `strip_only` to name such keys explicitly);
- the source-of-truth note: `all_optional` governs the exported `required` set,
  consumed by `PLAN_J1_export`.

The same Doc Update keeps `types.md` **Related Decisions** as the existing
non-slug candidate note (C1, C4, C7, C8, C9); **no `D-<slug>` reference** is
added (migration is at Concept Closeout, per the concept's
Decision-Reference Discipline). T3 opens **no new cross-concern edge** (it
consumes only the `Schema` / `Validator` already recorded), so no dual-endpoint
mirroring is required.

## Verification

Build & lint — all warning-free (the `mlua` runs prove T3 compiles with the
gated `HostOpaque` placeholder present):

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets`
- `cargo clippy --all-targets --features mlua`

Unit tests (`#[cfg(test)] mod tests` in `src/types.rs`) — each asserts behaviour
(validate outcomes, output shape, error `code` and `path`), not mere existence.
Builders convert via `let s: Schema = <builder>.into();` then `s.validate(&v)`
(the `val(...)` helper may be reused). Serialise inputs with `serde_json::json!`
maps where convenient.

1. **`partial` makes required fields optional.**
   `object([("a", number().into()), ("b", string().into())]).partial()`
   validating `{}` → `Ok` with empty output; validating `{a:1}` → `Ok` with
   output `{a:1}` (b omitted).
2. **`partial` still validates present fields.** The same partial schema
   validating `{a:"x"}` → `Err(TYPE_MISMATCH)` with `path == ["a"]`.
3. **`partial` preserves default precedence (contract-aligned).**
   `object([("a", number().default(5i64).into()), ("b", number().into())]).partial()`
   validating `{}` → `Ok` with output `{a:5}` (defaulted field `a` IS applied;
   plain required field `b` omitted). This confirms field iteration still
   delegates through `parse_field` and `default → optional → REQUIRED` holds
   under `partial` (`docs/architecture/schema-core.md:53`).
4. **`extend` adds a required field.**
   `object([("a", number().into())]).extend([("b", string().into())])`
   validating `{a:1}` → `Err(REQUIRED)` with `path == ["b"]`; validating
   `{a:1, b:"x"}` → `Ok` with `{a:1, b:"x"}`.
5. **`extend` overwrites a duplicate key in place.**
   `object([("a", number().into())]).extend([("a", string().into())])`
   validating `{a:"x"}` → `Ok`; validating `{a:1}` → `Err(TYPE_MISMATCH)` at
   `path == ["a"]` (the new `string` schema replaced the old `number`).
6. **`extend` preserves mode.**
   `object([("a", number().into())]).strip().extend([("b", string().into())])`
   validating `{a:1, b:"x", extra:9}` → `Ok` with `{a:1, b:"x"}` (extra dropped
   — still strip mode).
7. **`omit` removes a field; strict makes it unknown.**
   `object([("a", number().into()), ("id", string().into())]).omit(["id"])`
   validating `{a:1}` → `Ok` with `{a:1}`; validating `{a:1, id:"x"}` →
   `Err(UNKNOWN_PROPERTY)` with `id` in `path`. Omitting an absent key
   (`.omit(["nope"])`) is a no-op (schema unchanged).
8. **`omit_read_only` / `omit_write_only` remove by modifier.**
   `object([("a", number().into()), ("ro", string().read_only().into())]).omit_read_only()`
   validating `{a:1}` → `Ok`; validating `{a:1, ro:"x"}` →
   `Err(UNKNOWN_PROPERTY)` (`ro` removed from shape, now unknown in strict).
   Symmetric case with `.write_only()` + `.omit_write_only()`.
9. **`strip_only` drops an unknown key silently in strict.**
   `object([("a", number().into())]).strip_only(["junk"])` validating
   `{a:1, junk:true}` → `Ok` with `{a:1}` (no `UNKNOWN_PROPERTY`; `junk`
   dropped). A non-stripped unknown key still errors:
   `{a:1, other:1}` → `Err(UNKNOWN_PROPERTY)` with `other` in `path`.
10. **`strip_only` re-defaults a stripped defaulted field (documented nuance).**
    `object([("a", number().default(5i64).into())]).strip_only(["a"])`
    validating `{a:99}` → `Ok` with `{a:5}` (input `a` stripped, then the
    field's default re-applied).
11. **`strip_only` starves a stripped required field.**
    `object([("a", number().into())]).strip_only(["a"])` validating `{a:1}` →
    `Err(REQUIRED)` with `path == ["a"]` (stripped → missing → required).
12. **`strip_read_only` drops a read-only field from input.**
    `object([("a", number().into()), ("ro", string().read_only().optional().into())]).strip_read_only()`
    validating `{a:1, ro:"x"}` → `Ok` with `{a:1}` (`ro` stripped, omitted as
    optional-missing). Symmetric case with `.write_only().optional()` +
    `.strip_write_only()`.
13. **`omit_read_only` then `strip_read_only` does NOT auto-strip (locks the
    committed v1 contract — no removed-key tracking).**
    `object([("a", number().into()), ("ro", string().read_only().into())]).omit_read_only().strip_read_only()`
    validating `{a:1, ro:"x"}` → `Err(UNKNOWN_PROPERTY)` with `ro` in `path`
    (`ro` was removed from the shape by `omit_read_only`, so `strip_read_only`'s
    shape scan no longer sees it; under unchanged strict mode it is an unknown
    key). The explicit escape hatch `…​.omit_read_only().strip_only(["ro"])`
    validating the same input → `Ok` with `{a:1}`. This is the order-dependent
    behaviour the Scope decision commits to.
14. **Clone immutability.** From
    `let base = object([("a", number().into())]);`,
    `base.clone().partial()` validates `{}` → `Ok`, while
    `Schema::from(base)` validating `{}` → `Err(REQUIRED)` (original untouched —
    still strict + required).
15. **Composition.**
    `object([("a", number().into()), ("id", string().into())]).omit(["id"]).extend([("b", string().into())]).strip()`
    validating `{a:1, b:"x", id:"ignored", extra:7}` → `Ok` with `{a:1, b:"x"}`
    (`id` no longer in shape and `extra` unknown — both dropped by strip; `b`
    added by extend). A deep field error still carries its path:
    `object([("inner", object([("n", number().into())]).into())]).partial()`
    validating `{inner:{n:"x"}}` → `Err(TYPE_MISMATCH)` with
    `path == ["inner","n"]` (partial does not relax a *present* nested field's
    validation).

Expected outcome: a `zerx` crate whose `ObjectSchema` exposes `partial`,
`extend`, `omit`, `omit_read_only`, `omit_write_only`, `strip_only`,
`strip_read_only`, and `strip_write_only` — all clone-and-return, all contained
in the `object` delegate with no `schema-core` change and no new error code —
ready for `PLAN_T4_special_types` to add `buffer`/`uri`/`url`/`json`/`jsonschema`
and for `PLAN_J1_export` to consume the `all_optional` flag.

<plan_ready>docs/PLAN_T3_object_utilities.md</plan_ready>
