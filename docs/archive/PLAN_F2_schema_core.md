# PLAN_F2_schema_core

## Context & Goal

Third plan of the `CONCEPT_zerx_foundation` execution (Phase 1 — Foundation).
It implements the `schema-core` concern: the erased, cloneable `Schema` value, the
typed-builder front, the universal-modifier composition, the pluggable
`Validator` storage contract, and the **parse flow** (depth guard, default /
optional / nullable ordering, validators, refine) — plus `lazy` with its
reentrance guard.

`schema-core` is the spine of the library (concept Architecture: "The spine is
C1"). This plan establishes the *framework* against which the type catalogue is
built; it does **not** deliver the type catalogue itself. F2 seeds two kinds it
legitimately owns — `any` (the degenerate carrier-only type) and `lazy` (the
recursion indirection) — and proves the whole mechanism end-to-end with them, per
the concept's Risk mitigation ("`PLAN_F2_schema_core` prototypes the chaining
end-to-end before Phase 2 builds on it"). Every other type variant
(`string`, `number`, `object`, `array`, …) is added by `PLAN_T1`–`PLAN_T4`, each
of which appends its `SchemaKind` variant and one delegating dispatch arm.

`PLAN_F3_error_model` and `PLAN_F1_value_model` are complete and archived. F2
builds directly on both:

- From `errors` (F3): `ZerxError` and the **open `ErrorCode` catalogue** (each
  module declares its own codes via an `impl ErrorCode` block — no edit to
  `src/error.rs`).
- From `value-model` (F1): `ZerxValue` (the parse flow produces it), its
  accessors (`is_null`, `as_*`, `get`, `get_index`), and
  `ZerxValue::from_serialize` (the serde-in bridge that `validate<T: Serialize>`
  is built on).

### Boundary (fixed in `docs/architecture/schema-core.md` and confirmed by the Architect)

- This concern owns the `Schema` representation (enum-kind core + shared
  modifier/validator carrier + typed builders + the blanket `Modify` trait), the
  parse flow, and lazy/cyclic schemas with their reentrance guard.
- This concern owns immutable chaining: deriving a modified schema is
  clone-and-return; the original is never mutated.
- This concern does **NOT** define the individual types or concrete validators —
  see `types` (`PLAN_T1`–`PLAN_T4`).
- This concern does **NOT** own JSON Schema export/import — see `json-schema`.
- This concern does **NOT** define the dynamic value it produces — see
  `value-model`.

### Governing candidate decisions (concept `CONCEPT_zerx_foundation.md`)

- **C1 — Modifier composition: hybrid enum-core + typed builders.** A schema is a
  single concrete `Schema` value (`enum SchemaKind` over type variants + a shared
  carrier for universal modifiers and validators) fronted by typed builder structs.
  Universal modifiers live on the carrier and are exposed on every builder via a
  blanket trait, each returning the builder type so chaining is preserved.
  Type-specific validators are inherent methods on the relevant builder only.
  `Schema` is `Clone`; immutability is clone-and-return. Whether the blanket trait
  is hand-written or macro-generated is **this plan's** call.
- **C7 — Single `Result`-returning API.** Every fallible operation returns
  `Result<_, ZerxError>`; no throwing variant, no `parse`/`safeParse` split.
- **C4 — Strict by default** governs object validation (`unknown_property`).
  Object modes are introduced by `PLAN_T2`; F2 carries no object type and so does
  not implement modes — it only provides the carrier and parse flow they build on.

### Architect rulings encoded in this plan (consultation, this session)

- **Closed-enum growth.** `SchemaKind` belongs to `schema-core` as a *container*;
  each *variant* (tag + payload) belongs to the type-plan that introduces it, even
  though it lives syntactically in the `schema-core` file. F2 defines a single
  per-kind **dispatch seam** so that adding a type means adding its variant plus
  exactly one delegating arm per dispatch match — and writes **no** new control
  logic into the parse flow. The concrete seam mechanism is F2's to choose.
- **`Validator` trait location.** F2 defines the bare `Validator` trait
  (`validate(&ZerxValue) -> Result<(), ZerxError>` + a JSON-Schema-contribution
  method) **and** the `validators` field on the carrier. `schema-core` owns the
  *storage contract* (trait signature + field); `types` owns the *validator system
  and all concretes* (`PLAN_T1`). The one-line concern-doc reconciliation between
  `schema-core.md` and `types.md` happens when F2 / T1 fill their skeletons (F2
  fills `schema-core.md` only).
- **Proof vehicle.** F2 proves the carrier / `Modify` / `Into<Schema>` / `Clone` /
  guard mechanism with `any` + `lazy`. F2 MUST NOT invent an artificial
  type-specific inherent method on `any`/`lazy` to demonstrate composition; the
  proof that "a type-specific inherent method composes with universal modifiers"
  **and** the negative compile guarantee (`.min()` not callable on a bool) both
  belong to `PLAN_T1`, where `string().min().optional()` exists naturally. T1's
  basic-types list therefore excludes `any` (seeded here) and adds the rest.

## Breaking Changes

**No.** This plan adds a new module (`src/schema.rs`) and re-exports from
`src/lib.rs`; it does not change the existing public API and does not modify
`src/error.rs` or `src/value.rs`. No `Cargo.toml` change (the `mlua` feature
already exists from F1).

## Dependencies

No new crates. Per C9 (*Built on serde, lean beyond it*), this plan uses only the
dependencies already declared in `Cargo.toml`:

- `serde` — `Serialize` bound on `Schema::validate`.
- `serde_json` — the return type of the `Validator` JSON-Schema-contribution
  method (a `serde_json::Map`); used in tests.
- `std` only beyond that (`Rc`, `RefCell`, `Cell`). No `mlua` interaction (the
  host-opaque variant is realised by `PLAN_M1`; F2 must merely compile under
  `--features mlua`, where the `HostOpaque` variant remains the uninhabited F1
  placeholder).

## Reference Patterns

- `src/value.rs` — the established module style: a single concern file with the
  type, its inherent methods, trait impls, the open-`ErrorCode` block, and a
  `#[cfg(test)] mod tests`. F2 mirrors this layout in `src/schema.rs`.
- `src/error.rs` — the open-`ErrorCode`-catalogue pattern (`impl ErrorCode { pub
  const X: ErrorCode = ErrorCode::new("x"); }` in the consuming module) and the
  `ZerxError` builder style (`ZerxError::new(code, msg).at(path)…`). F2 adds its
  parse-flow codes the same way, in `src/schema.rs`, with no edit to
  `src/error.rs`.
- `serde_json::value::Serializer` parity is **not** needed here; the serde-in path
  is already done in F1 (`ZerxValue::from_serialize`), which F2 simply calls.
- `docs/vision.md` lines 112–217 (type catalogue, modifier catalogue, the
  "schema with all the trimmings" example) and lines 160–167 (the parse-flow
  ordering) for the modifier set and the faithful ordering. `docs/vision.md`
  lines 268–274 for `lazy` and the reentrance guard.

## Assumptions & Risks

- **`Schema` is single-threaded (`Rc`-based), not `Send`/`Sync`.** Shared,
  immutable pieces (validators, refine predicates, the `lazy` thunk and its
  resolution cache) are held behind `Rc` (and `Rc<RefCell<…>>` for the cache).
  This is the minimal mechanism F2 needs and follows YAGNI: nothing in the concept
  requires thread-safe schema sharing. If a later plan requires `Send`/`Sync`
  schemas (e.g. a global policy registry shared across request handlers), swapping
  `Rc`→`Arc` and adding `Send + Sync` bounds is a breaking internal change — and
  breaking changes are this project's default (Hard Rule 2). This is flagged so
  Review can object if single-threaded `Schema` is judged too narrow now.
- **`Schema` is `Clone` but neither `PartialEq` nor (derived) `Debug`.** It holds
  closures (refine) and trait objects (validators), which are not `PartialEq`;
  tests assert *behaviour* (`validate` outcomes, modifier storage via in-module
  access), never `Schema` equality. A **hand-written** `Debug` for `Schema`
  renders the kind, the modifier flags, and the validator *count* (not validator
  contents, avoiding a `Debug` bound on `Validator`), with closures/thunks shown as
  placeholders — useful for test assertions and users, low-cost.
- **The parse-flow "circular-reference check" is realised as the `lazy` reentrance
  guard + the depth limit; data-value cycle detection is deferred to `PLAN_M1`.**
  An owned `ZerxValue` produced by `from_serialize` is always an acyclic tree, so a
  data-value cycle cannot be constructed in the default build; building a
  visited-set for impossible-to-construct cycles now would be dead code (YAGNI).
  Cyclic *data* first exists with `mlua` (Lua tables), so the data-cycle visited-set
  attaches in `PLAN_M1` at the `ParseContext` seam this plan establishes. F2
  delivers the two mechanisms that are real and testable now: the **depth limit**
  (`MAX_PARSE_DEPTH = 100`, guarding deep parse recursion / DoS) and the **`lazy`
  reentrance guard** (guarding schema self-resolution). This is a deliberate
  interpretation of the concept's "circular-reference check → depth limit" step and
  is called out for the Reviewer to sign off.
- **`MAX_PARSE_DEPTH` bounds recursion frames, and `lazy` indirections count
  against it — by design.** Every `parse_present` invocation is one real stack
  frame and consumes one depth level, including the frame for a `lazy` wrapper that
  then delegates to its resolved inner schema (so a lazy-wrapped node costs two
  levels: the wrapper frame + the resolved-kind frame). This is the faithful
  realisation of the vision's stated purpose — "malicious or accidental deep/cyclic
  input cannot exhaust the stack" (`docs/vision.md:162-167`): the limit tracks
  actual stack depth, which is exactly what prevents overflow. The rejected
  alternative — making `lazy` delegate *without* consuming a level so that "100"
  equals "100 concrete-node levels" — reopens a stack-overflow hole: a base-less
  lazy cycle (a `lazy` whose thunk resolves, directly or transitively, back to a
  `lazy` with **no** concrete node between them) recurses with no depth increment,
  and the memoisation-based reentrance guard does not catch it (resolution
  *succeeds* and is cached; the non-termination is in the parse delegation, not the
  resolution). Counting the lazy frame closes that hole: such a cycle hits
  `MAX_PARSE_DEPTH` and errors instead of overflowing. Consequence to state
  plainly: for a recursive schema built as `lazy(|| <concrete>)`, each input
  recursion level costs two depth levels, so the effective input-nesting bound is
  ≈ `MAX_PARSE_DEPTH / 2` (≈ 50 levels) — far deeper than any realistic non-adversarial
  data, and a safety limit, not a feature limit. Verification item 8 asserts the
  bound in terms of recursion frames.
- **`default` accepts `impl Into<ZerxValue>`, covering the F1 `From` impls**
  (`bool`, integers, `f64`, `String`, `&str`). Complex/serde-shaped defaults (e.g.
  `serde_json::json!({})` from the vision example) are not ergonomically supported
  in F2 because the complex types they pair with (`record`, `json`, `object`)
  arrive in Phase 2; revisiting default ergonomics for complex values is left to
  the plan that introduces those types, not pre-built here.
- **Terminal-call ergonomics are deferred to `PLAN_T1`.** `validate` lives on
  `Schema`; builders convert via `Into<Schema>`. Whether builders should also expose
  a terminal `.validate` (so `string().min(3).validate(&x)` reads without an
  explicit `.into()`) is an ergonomics question best settled once real typed
  builders exist; F2 does not pre-decide it. F2 tests convert explicitly
  (`let s: Schema = any().optional().into();`), which also exercises the `Into<Schema>`
  requirement of C1.

## Design

### Module layout

- New file `src/schema.rs` declaring: `Schema`, `SchemaKind`, `Modifiers`,
  `Refinement`, the `Validator` trait, `ParseContext`, `MAX_PARSE_DEPTH`, the
  `Modify` trait and its private `BuilderInner` driver, the builder newtypes
  `AnySchema` / `LazySchema`, the `Lazy` body + its `LazyState`, the constructors
  `any()` / `lazy()`, the parse-flow error codes (open catalogue), and
  `#[cfg(test)] mod tests`.
- `src/lib.rs` gains `mod schema;` and
  `pub use schema::{Schema, Modify, Validator, AnySchema, LazySchema, any, lazy, MAX_PARSE_DEPTH};`.
  No other change; `src/error.rs` and `src/value.rs` are untouched.

### `Schema` — the erased, cloneable value (C1)

```rust
pub struct Schema {
    pub(crate) kind: SchemaKind,
    pub(crate) modifiers: Modifiers,
    pub(crate) validators: Vec<std::rc::Rc<dyn Validator>>,
}
```

- Derives `Clone` (validators are `Rc`, so the `Vec` clones cheaply; `kind` and
  `modifiers` are `Clone`). Hand-written `Debug` (see Assumptions). **Not**
  `PartialEq`, **not** `Eq`.
- Fields are `pub(crate)`: the public surface is the constructors, the `Modify`
  trait, `validate`, and (later) the `Into<Schema>` conversions. `Schema` itself is
  `pub` — it is the central entity returned by builders, cloned/sliced by
  `omit`/`partial`/`extend` (T3), and reconstructed by `from_json_schema` (J2).

### `SchemaKind` — the closed enum container (C1; Architect Q1)

```rust
pub(crate) enum SchemaKind {
    Any,            // owned by this plan
    Lazy(Lazy),     // owned by this plan
    // PLAN_T1/T2/T4 each add their variant here, plus one arm in each dispatch match.
}
```

- `pub(crate)` (visible to the type modules that extend it; not part of the user
  API — users never match on it). Derives `Clone`.
- **Dispatch seam.** Two `pub(crate)` inherent methods on `SchemaKind`, each a
  single `match self`:
  - `fn check_type(&self, value: &ZerxValue, ctx: &mut ParseContext) -> Result<(), ZerxError>`
    — confirm the value's kind matches the schema kind (leaf type check). `Any`
    accepts anything (`Ok(())`). The `Lazy` arm is `unreachable!` — `lazy` is
    intercepted in `parse_present` before this method is reached.
  - `fn parse_inner(&self, value: &ZerxValue, ctx: &mut ParseContext) -> Result<ZerxValue, ZerxError>`
    — the type-specific structural step (element walk / child parsing), producing
    the output `ZerxValue`. `Any` returns `value.clone()`. The `Lazy` arm is
    `unreachable!`.
  Adding a type = one new enum variant + one new arm in each of these two matches,
  the arm delegating to a function defined in the *type's own module*. The
  ordering/control logic (depth, null, default/optional, validators, refine) stays
  in `Schema::parse_present` / `parse_field` and is never duplicated per type.

### `Modifiers` — the universal carrier (C1, vision modifier catalogue)

```rust
#[derive(Clone, Default)]
pub(crate) struct Modifiers {
    pub optional: bool,
    pub nullable: bool,
    pub default: Option<ZerxValue>,
    pub description: Option<String>,
    pub refine: Vec<Refinement>,
    pub format: Option<String>,
    pub mime: Option<String>,
    pub deprecated: bool,
    pub read_only: bool,
    pub write_only: bool,
    pub meta: Map,                 // reuses value-model's ordered Map
    pub example: Option<ZerxValue>,
    pub title: Option<String>,
}
```

```rust
#[derive(Clone)]
pub(crate) struct Refinement {
    pub check: std::rc::Rc<dyn Fn(&ZerxValue) -> bool>,
    pub message: String,
}
```

`Modifiers` hand-writes `Debug` (skips the `refine` closures, rendering a count).

### `Modify` — the blanket universal-modifier trait (C1)

```rust
pub trait Modify: Sized {
    fn optional(self) -> Self;
    fn nullable(self) -> Self;
    fn default(self, value: impl Into<ZerxValue>) -> Self;
    fn describe(self, text: impl Into<String>) -> Self;
    fn refine<F: Fn(&ZerxValue) -> bool + 'static>(self, f: F, message: impl Into<String>) -> Self;
    fn format(self, fmt: impl Into<String>) -> Self;
    fn mime_format(self, mime: impl Into<String>) -> Self;
    fn deprecated(self) -> Self;
    fn read_only(self) -> Self;
    fn write_only(self) -> Self;
    fn meta(self, key: impl Into<String>, value: impl Into<ZerxValue>) -> Self;
    fn example(self, value: impl Into<ZerxValue>) -> Self;
    fn title(self, text: impl Into<String>) -> Self;
}
```

- Driven by a **private** trait `BuilderInner { fn schema_mut(&mut self) -> &mut Schema; }`
  with a blanket `impl<T: BuilderInner> Modify for T` that mutates
  `self.schema_mut().modifiers` and returns `self` (consume-and-return chaining;
  this is the idiomatic Rust form of clone-and-return immutability — clone first if
  the original is needed). Two builders (`AnySchema`, `LazySchema`) plus `Schema`
  itself implement `BuilderInner`, so `Modify` is available on all three. A macro
  is **not** introduced for two builders; the macro-vs-hand-written choice is
  revisited only if forwarding boilerplate grows in T1+ (concept Risk).
- `refine` pushes a `Refinement { check: Rc::new(f), message: message.into() }`.
  Multiple `refine` calls accumulate and all run.

### Typed builders + constructors (C1)

```rust
#[derive(Clone)] pub struct AnySchema(Schema);
#[derive(Clone)] pub struct LazySchema(Schema);

pub fn any() -> AnySchema;                               // SchemaKind::Any, empty carrier
pub fn lazy<F: Fn() -> Schema + 'static>(f: F) -> LazySchema;   // SchemaKind::Lazy

impl From<AnySchema> for Schema { /* unwrap */ }
impl From<LazySchema> for Schema { /* unwrap */ }
impl BuilderInner for AnySchema { /* &mut self.0 */ }
impl BuilderInner for LazySchema { /* &mut self.0 */ }
impl BuilderInner for Schema { /* self */ }
```

Builders are `pub` (they are return types of public constructors). They prove the
C1 surface: typed front, `Into<Schema>`, blanket `Modify`, `Clone`. No
type-specific inherent method is added (Architect Q3).

### `Validator` — the storage contract (Architect Q2)

```rust
pub trait Validator {
    fn validate(&self, value: &ZerxValue) -> Result<(), ZerxError>;
    fn json_schema(&self) -> serde_json::Map<String, serde_json::Value>;
}
```

- `schema-core` owns this trait signature and the `validators: Vec<Rc<dyn Validator>>`
  field; it provides **no** concrete validators (all concretes are `PLAN_T1`).
  `json_schema` returns the JSON-Schema fragment a validator contributes (e.g.
  `{"minLength": 3}`); the merge semantics are `PLAN_J1`'s — F2 only fixes the
  signature so the carrier and exporter can rely on it.
- There is no public method to *attach* a validator in F2 (typed builders gain
  `.min()`/`.max()`/… in T1, which push into `validators`). F2 exercises storage
  and execution white-box via a test-only `Validator` impl.

### Parse flow (C7; vision lines 160–167; faithful ordering)

`ParseContext` carries the depth counter and is the `&mut`-threaded seam for the
future path stack and the M1 data-cycle visited-set (added without signature
changes):

```rust
pub const MAX_PARSE_DEPTH: usize = 100;
pub(crate) struct ParseContext { depth: usize }
// enter(): depth += 1; if depth > MAX_PARSE_DEPTH -> Err(PARSE_DEPTH_EXCEEDED).
// exit():  depth -= 1.
```

`Schema::validate` is the public entry; `parse_present` and `parse_field` are the
`pub(crate)` recursion used by container types (T2):

```rust
impl Schema {
    pub fn validate<T: serde::Serialize + ?Sized>(&self, value: &T)
        -> Result<ZerxValue, ZerxError>
    {
        let v = ZerxValue::from_serialize(value)?;        // F1 serde-in bridge
        self.parse_present(&v, &mut ParseContext::new())
    }
}
```

- **`parse_present(value, ctx)`** — parses a *present* value:
  1. `ctx.enter()?` (depth guard).
  2. If `kind` is `Lazy`: resolve the inner schema (memoised, reentrance-guarded)
     and delegate — `inner.parse_present(value, ctx)` (the inner schema carries its
     own constraints; `lazy` is a thin recursion indirection). The outer `lazy`
     frame's `ctx.enter()` (step 1) counts against `MAX_PARSE_DEPTH` **by design** —
     it is a real stack frame, and counting it closes the base-less-lazy-cycle
     overflow hole (see the `MAX_PARSE_DEPTH` Assumption). The outer `lazy` frame's
     own `modifiers.refine` still runs in step 4 on the delegated result.
  3. Else: if `value.is_null()` and `modifiers.nullable` → output `Null`
     (a `nullable` schema accepts an explicit null). Otherwise, in this fixed
     order (vision): **type check** (`kind.check_type`) → **validators** (run each
     stored `Validator::validate` on the value) → **type-specific logic**
     (`kind.parse_inner`, producing the output value).
  4. **refine** (after the type-specific step succeeds, for *all* kinds incl.
     `lazy`): run each `Refinement::check` on the produced output; the first that
     returns `false` → `Err(REFINEMENT_FAILED)` carrying its message.
  5. `ctx.exit()`; return the output.
  `ctx.exit()` runs on every return path (the Coder structures it so depth is
  always balanced).
- **`parse_field(value: Option<&ZerxValue>, ctx)`** — the missing/default/optional
  wrapper that container property iteration (T2) calls per property; returns
  `Option<ZerxValue>` (`None` = omit from output):
  - `value == None` (property absent): **default precedes null/optional on a
    missing value** — if `modifiers.default` is set, parse the default via
    `parse_present` → `Some(parsed)`; else if `modifiers.optional` → `None` (the
    missing optional is omitted from output, never emitted as `Null`); else →
    `Err(REQUIRED)`.
  - `value == Some(v)` (present, incl. explicit `Null`): `Some(self.parse_present(v, ctx)?)`.
    Note an explicit `Null` does **not** trigger `default` (default is
    missing-only); it is handled by the `nullable` branch in `parse_present`.

### `lazy` — memoisation + reentrance guard (vision lines 268–274)

```rust
#[derive(Clone)]
pub(crate) struct Lazy {
    thunk: std::rc::Rc<dyn Fn() -> Schema>,
    state: std::rc::Rc<std::cell::RefCell<LazyState>>,   // shared across clones
}
enum LazyState { Unresolved, Resolving, Resolved(std::rc::Rc<Schema>) }
```

`Lazy::resolve(&self) -> Result<Rc<Schema>, ZerxError>`:

- `Resolved(s)` → return `s.clone()` (memoised: the thunk runs once, even across
  `Schema` clones, because `state` is shared via `Rc`).
- `Resolving` → `Err(LAZY_REENTRANCE)` (the thunk re-entered its own resolution
  synchronously — an unresolvable self-reference; the guard breaks it instead of
  overflowing the stack).
- `Unresolved` → set `Resolving`, call the thunk, store `Resolved(Rc::new(schema))`,
  return it.

Normal recursive schemas defer through the *parse* recursion (a `lazy` whose thunk
returns a structure that itself contains another `lazy`), so `resolve` is never
re-entered synchronously; the `Resolving` reentrance error is the defensive guard
the vision mandates against a thunk that resolves *itself* synchronously. It does
**not** catch a base-less lazy *parse* cycle (a chain of `lazy`→`lazy`→…→self with
no concrete node between hops), because each `resolve` there succeeds and memoises;
that case is bounded instead by `MAX_PARSE_DEPTH`, which every `lazy` frame
consumes (see the `MAX_PARSE_DEPTH` Assumption). The two guards are complementary:
the reentrance error covers synchronous self-resolution at construction, the depth
limit covers runaway parse recursion (whether from adversarial data depth or a
base-less lazy cycle).

### Error codes (open catalogue, in `src/schema.rs`)

```rust
impl ErrorCode {
    pub const PARSE_DEPTH_EXCEEDED: ErrorCode = ErrorCode::new("parse_depth_exceeded");
    pub const REQUIRED: ErrorCode            = ErrorCode::new("required");
    pub const REFINEMENT_FAILED: ErrorCode   = ErrorCode::new("refinement_failed");
    pub const LAZY_REENTRANCE: ErrorCode     = ErrorCode::new("lazy_reentrance");
}
```

`ErrorCode::TYPE_MISMATCH` (seeded in `errors`) is reserved for typed kinds (T1+);
`any` never raises it. No edit to `src/error.rs`.

## Steps

1. `src/lib.rs`: add `mod schema;` and the `pub use schema::{…}` re-exports listed
   under Module layout.
2. `src/schema.rs`: declare the parse-flow `ErrorCode` constants (open catalogue)
   and `pub const MAX_PARSE_DEPTH: usize = 100;`.
3. Implement `Validator` (trait), `Refinement`, and `Modifiers` (with `Default`
   derive and hand-written `Debug`).
4. Implement `SchemaKind` (`Any`, `Lazy(Lazy)`), the `Lazy` body + `LazyState` +
   `Lazy::resolve`, and the two dispatch-seam methods (`check_type`, `parse_inner`)
   with the `Any` arms implemented and the `Lazy` arms `unreachable!`.
5. Implement `Schema` (fields, `Clone` derive, hand-written `Debug`), `ParseContext`
   (`new`/`enter`/`exit`), and the parse flow (`validate`, `parse_present`,
   `parse_field`).
6. Implement the `Modify` trait, the private `BuilderInner` driver + blanket impl,
   the builder newtypes `AnySchema`/`LazySchema`, the `From<…> for Schema` and
   `BuilderInner` impls (incl. `BuilderInner for Schema`), and the `any()`/`lazy()`
   constructors.
7. Add `#[cfg(test)] mod tests` per Verification (including a test-only `Validator`
   impl and a white-box reentrance test).
8. Run the full build/lint/test matrix below; resolve all warnings.

**Doc Update (workflow step 6, by the Coder after Validation — not now):** fill
`docs/architecture/schema-core.md` Constraints with the review-hardened normative
contract delivered here: the `Schema` carrier (kind + modifiers + validators) and
its `Clone` clone-and-return immutability; the `SchemaKind` container-vs-variant
ownership and the dispatch seam (a new type adds one variant + one arm per dispatch
match, no parse-flow control logic); the universal modifier set exposed via the
blanket `Modify` trait returning the builder type; the `Validator` **storage
contract** (trait signature + `validators` field owned here, all concretes owned by
`types`) — the one-line reconciliation noting this boundary; the parse-flow ordering
(depth guard `MAX_PARSE_DEPTH = 100` → for a missing value, default precedes
optional/nullable; missing optional omitted from output → type check → validators →
type-specific logic → refine; explicit null does not trigger default); the depth
guard counts recursion frames (every `parse_present` frame, `lazy` hops included),
not concrete-node levels; and `lazy` memoisation + reentrance guard, with the
data-value cycle check deferred to `PLAN_M1`. Per this concept's Decision-Reference Discipline, this Doc Update fills
**Constraints only** and MUST NOT add any `D-<slug>` reference to `schema-core.md`;
the C1/C7/C4 migration and all slug references are routed to Concept Closeout. The
binding candidate decisions for this plan during execution are C1, C7, C4.

## Verification

Build & lint — all must be warning-free (the `mlua` variants prove F2 compiles with
the gated `HostOpaque` placeholder present):

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets`
- `cargo clippy --all-targets --features mlua`

Unit tests (`#[cfg(test)] mod tests` in `src/schema.rs`) — each asserts behaviour,
not mere existence. A test-only `RejectAll` `Validator` (its `validate` always
returns a chosen `ZerxError`; `json_schema` returns an empty map) and white-box,
same-module access to internals stand in for what `PLAN_T1` later exposes publicly.

1. **`any` accepts every scalar/container.** `any().into()` validated against
   `true`, `7i32`, `2.5f64`, `"hi"`, `()`, `vec![1i64, 2, 3]`, and a small serde
   struct each returns `Ok` with the `ZerxValue` equal to `from_serialize` of the
   same input (proves `validate` = `from_serialize` + identity `parse_inner`).
2. **Blanket `Modify` chains and returns the builder type.**
   `any().optional().nullable().describe("d").deprecated().read_only()
   .meta("x", true).title("t")` type-checks as `AnySchema`, and (white-box) the
   resulting `Schema.modifiers` carries every set field
   (`optional`/`nullable`/`deprecated`/`read_only == true`,
   `description == Some("d")`, `meta.get("x") == Some(Bool(true))`,
   `title == Some("t")`). Same chain on a `lazy(...)` builder type-checks as
   `LazySchema` (proves the blanket trait is available on both builders).
3. **`Into<Schema>` and `Clone` immutability.** From `let base = any().describe("x");`
   produce `let opt: Schema = base.clone().optional().into();` — `opt.modifiers`
   is `optional == true` with `description == Some("x")`, while a `Schema::from(base)`
   has `optional == false` (the original is untouched: clone-and-return).
4. **Default precedes optional on a missing value.** With
   `parse_field(None, ctx)`: `any().default(7i64)` → `Ok(Some(I64(7)))`;
   `any().optional()` → `Ok(None)`; `any()` (required) →
   `Err` with `code == ErrorCode::REQUIRED`.
5. **Nullable and explicit null.** `parse_present(&Null, ctx)` on `any().nullable()`
   → `Ok(Null)`. `parse_field(Some(&Null), ctx)` on `any().default(7i64).nullable()`
   → `Ok(Some(Null))` (explicit null does **not** trigger the default).
6. **Validators are stored, executed in order, and run before refine.** A `Schema`
   whose `validators` hold a single `RejectAll` (white-box push of
   `Rc::new(RejectAll{code})`) validated against any value → `Err` with that
   validator's `code` (proves the carrier stores and the parse flow executes
   validators). With **two** `RejectAll` validators carrying distinct codes, the
   error carries the **first** validator's code (proves stored execution order:
   first failure wins). A schema with a `RejectAll` validator **and** a `refine`
   that records into a shared `Rc<Cell<bool>>` → on validation the `Err` carries the
   validator's code and the refine flag stays `false` (proves validators run, and
   short-circuit, **before** refine). Note: the *`type-check → validators`* leg of
   the ordering is **not** observable in F2 — `any`'s `check_type` never fails, so
   no value can distinguish `check_type → validators` from the reverse. That leg
   becomes observable and is verified in `PLAN_T1`, where a typed kind (e.g.
   `string`) rejects a mismatching value in `check_type` and the test can assert the
   validator is never reached on a type mismatch.
7. **Refine runs and aggregates.** `any().refine(|v| v.as_i64() == Some(5), "must be 5")`
   validated against `5i64` → `Ok`; against `6i64` →
   `Err` with `code == ErrorCode::REFINEMENT_FAILED` and `message == "must be 5"`.
   A second chained `refine` that also fails is reached only when the first passes
   (assert a two-refine schema reports the first failing predicate's message).
8. **Depth limit (counted in recursion frames).** A `lazy` chain whose total
   `parse_present` frames exceed `MAX_PARSE_DEPTH` (e.g. build `lazy(|| prev.into())`
   for `> MAX_PARSE_DEPTH` levels via a loop) parsed against any value → `Err` with
   `code == ErrorCode::PARSE_DEPTH_EXCEEDED`; a chain whose frame count is
   ≤ `MAX_PARSE_DEPTH` succeeds. The assertion is in terms of recursion frames, not
   "concrete-node levels": each `lazy` hop is one frame and counts (per the
   `MAX_PARSE_DEPTH` Assumption), so the test builds the chain by frame count.
9. **`lazy` memoisation.** A thunk that increments a shared `Rc<Cell<usize>>` and
   returns `any().into()` is wrapped in `lazy(...)`; validating the resulting schema
   twice leaves the counter at `1` (the thunk resolves once; the result is cached and
   reused, including across a `Clone` of the schema).
10. **`lazy` reentrance guard.** White-box: take the `Lazy` inside a `lazy(...)`,
    set its `state` to `Resolving`, then call `resolve()` → `Err` with
    `code == ErrorCode::LAZY_REENTRANCE` (proves the guard breaks self-resolution
    instead of recursing).

Expected outcome: a `zerx` crate exposing `Schema`, the `Modify` and `Validator`
traits, the `any`/`lazy` constructors and their builders, and the parse flow
(`validate` plus the `pub(crate)` `parse_present`/`parse_field` used by Phase 2),
with the dispatch seam, the modifier carrier, `Clone` immutability, the depth guard,
and `lazy` memoisation + reentrance guard all proven — ready for `PLAN_T1_basic_types`
to add the first typed builders (`string`, `number`, `boolean`, `enumerate`, `null`)
and their concrete validators on top.
