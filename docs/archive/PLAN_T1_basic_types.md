# PLAN_T1_basic_types

## Context & Goal

First plan of **Phase 2 — Type catalogue** in the `CONCEPT_zerx_foundation`
execution. Phase 1 (`PLAN_F3_error_model`, `PLAN_F1_value_model`,
`PLAN_F2_schema_core`) is complete and archived. F2 delivered the `schema-core`
spine — the erased `Schema` carrier, the `SchemaKind` container with its
per-kind dispatch seam, the blanket `Modify` trait, the parse flow, the
`Validator` **storage contract**, and the `any`/`lazy` proof types.

T1 delivers the first **typed builders** and the first **concrete validators** on
top of that spine:

- The basic types `string`, `number`, `boolean`, `enumerate`, `null`
  (`any` is already seeded by F2; `lazy` too).
- Their type-specific inherent methods and the concrete `Validator`
  implementations behind them: `min`, `max`, `regex`/`pattern`, `int`, `email`,
  `uuid` (and `multiline` as a meta hint, not a validator — see Design).
- The **negative compile-guarantee** (`.min()` is unrepresentable on a boolean)
  and the **type-specific-method composition** (`string().min(3).optional()`),
  both of which F2 deliberately deferred to this plan.

T1 extends `SchemaKind` with its five variants and one delegating dispatch arm
per variant in each dispatch method, introducing **no new parse-flow control
logic** (concept C1 / `schema-core.md` constraints). It fills the `types`
concern's Constraints at Doc Update.

### Boundary (`docs/architecture/types.md`, confirmed by the Architect)

- `types` owns the type catalogue and the pluggable validators (the concrete
  `Validator` implementations and their JSON-Schema-fragment contributions).
- `types` does **NOT** define the core `Schema` representation, the parse flow,
  the `Validator` **trait** itself, or the `validators` storage field — those are
  `schema-core` (F2).
- `types` does **NOT** own JSON Schema export/import. T1 fills each validator's
  `json_schema()` fragment, but the **merge/export** of those fragments is
  `PLAN_J1_export`. T1 asserts the per-validator fragment shape only.
- `types` does **NOT** own the host-opaque `function`/`tvalue` types — see
  `mlua`.

### Governing candidate decisions (`CONCEPT_zerx_foundation.md`)

- **C1 — hybrid enum-core + typed builders.** Type-specific validators are
  inherent methods on the relevant builder only, hence unrepresentable where they
  make no sense. T1 lands this guarantee.
- **C7 — single `Result`-returning API.** No throwing variant. Builder
  construction is infallible (returns the builder); validation failures — *including
  an un-compilable user regex* — surface as `Result<_, ZerxError>` at `validate`
  time, never as a panic (see Design → `regex`/`pattern`).
- **C8 — English-only, machine-readable errors.** Each validator failure carries
  a distinct `code` so consumers branch on `code`, not message text.
- **C9 — built on serde, lean beyond it.** The Architect added the `regex` crate
  to the concept's core dependency set, **scoped to the `regex`/`pattern`
  validator** (Product Owner decided full `regex` over `regex-lite` for
  performance and full feature set). T1 is the plan that introduces it.

### Architect rulings encoded in this plan (consultation, this session)

- **`regex` is approved** as a core (non-optional) dependency, used solely by the
  `pattern` validator; now part of the active concept (C9). Permitted in T1's
  Dependencies under Hard Rule 11.
- **`multiline` is not a `Validator`.** In Zex it is a UI hint stored in `meta`
  (`x-ui-multiline`). T1 ports it as a meta-setter on the string builder, with no
  `Validator` impl and no validation effect. The concept/`types.md` wording that
  lists `multiline` under "validators" is a carry-over imprecision from the vision
  list; the T1 Doc Update corrects it (multiline → meta/modifier hint).
- **`email`/`uuid` are hand-rolled** with fixed patterns (no regex engine), each
  contributing `format: "email"`/`"uuid"` for the later exporter.
- **Doc-Update obligations (Step 6):** record `regex` as an *Architecturally
  Significant Dependency* in `types.md` (the `pattern` validator is unbuildable
  without a general regex engine; replacing it would redesign the validator).
  `serde`/`serde_json` ASD coverage already lives in `schema-core.md`. The
  `types ↔ schema-core` cross-concern edges already exist on both sides; no new
  edge is opened by this plan.

## Breaking Changes

**No.** T1 is additive: a new module `src/types.rs`, new re-exports from
`src/lib.rs`, and new `SchemaKind` variants plus delegating dispatch arms in
`src/schema.rs`. It does not change any existing public signature and does not
edit `src/error.rs` or `src/value.rs`. `Cargo.toml` gains one new core
dependency (`regex`), which is purely additive.

## Dependencies

One new core dependency, approved by the Architect/Product Owner and recorded in
concept C9:

- **`regex = "1"`** (latest 1.x, full crate) — used **only** by the
  `regex`/`pattern` validator. Not feature-gated.

No other crate is added. `email`, `uuid`, `min`, `max`, `int`, `multiline`, the
type checks, and the enum membership check are all pure logic over the existing
`serde`/`serde_json`/`std` set.

## Reference Patterns

- `src/schema.rs` (F2) — the authoritative pattern for everything T1 does:
  - the `SchemaKind` dispatch seam (`check_type`, `parse_inner` as single
    `match self`, each arm delegating to type-owned logic),
  - the typed-builder newtype + `BuilderInner` + blanket `Modify` + `From<…> for
    Schema` + constructor quartet (`AnySchema`/`LazySchema`, `any()`/`lazy()`),
  - the hand-written `Debug` match on `SchemaKind`,
  - the open `ErrorCode` catalogue (`impl ErrorCode { pub const … }` in the
    consuming module; no edit to `src/error.rs`).
  T1 mirrors all of these in `src/types.rs` for the five new builders and adds
  the five variants + arms to `src/schema.rs`.
- `src/value.rs` (F1) — `ZerxValue` variants and accessors
  (`as_str`, `as_bool`, `as_i64`/`as_u64`/`as_i128`/`as_u128`/`as_f64`,
  `is_null`), and the module/test layout style.
- `src/error.rs` (F3) — `ZerxError::new(code, msg).expected(…).received(…)`
  builder; the seeded `ErrorCode::TYPE_MISMATCH`.
- **Zex semantics** (`/Users/martinschlott/Documents/MyProjects/zex`):
  - `src/zex/validators.ts` — `MinLength`/`MaxLength` (`value.length`,
    `minLength`/`maxLength`), `Pattern` (`new RegExp(pattern).test`),
    `Min`/`Max` (`minimum`/`maximum`), `Int` (`Number.isInteger`), `Email`
    (`/^[^\s@]+@[^\s@]+\.[^\s@]+$/`, `format: email`), `Uuid`
    (`/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i`,
    `format: uuid`).
  - `src/zex/basic-types.ts` — `ZexString.min/max/pattern/email/uuid/multiline`,
    `ZexNumber.min/max/int`. `multiline` writes the `x-ui-multiline` meta hint.

## Assumptions & Risks

- **`min`/`max` overload the method *name* across builders, with different
  signatures, by design.** `StringSchema::min(self, n: usize)` constrains length;
  `NumberSchema::min(self, n: impl Into<f64>)` constrains value. They are inherent
  methods on distinct newtypes, so there is no conflict, and their coexistence is
  precisely the type-specific-method demonstration C1/F2 ask T1 to land. A boolean
  builder exposes neither — the negative guarantee.
- **Number bound comparison is performed in `f64`.** `min`/`max` store an `f64`
  bound and compare the numeric value as `f64`. For `i128`/`u128` magnitudes
  beyond 2^53 this loses precision at the bound; acceptable for v1 (the concept
  defers micro-precision concerns), and called out for the Reviewer. Number
  *equality* is never used by min/max — only ordering against the bound.
- **`number().min(0)` / `.max(9)` compile with bare integer literals.** Because an
  unconstrained integer literal defaults to `i32` and `i32: Into<f64>` holds, the
  `impl Into<f64>` parameter accepts `min(0)` without an explicit float suffix,
  matching the vision's illustration.
- **An un-compilable user regex surfaces at `validate` time, not at build time
  (C7).** `string().regex("(")` returns a `StringSchema` (builder construction is
  infallible — it is not one of C7's fallible operations); the invalid pattern is
  remembered and the *validation* of any value against it returns
  `Err(PATTERN_INVALID)`. This mirrors Zex (which compiles the pattern at validate
  time) and keeps the fluent API panic-free. The alternative — panicking in
  `.regex()` — is rejected: it would introduce a panic path into infallible schema
  assembly.
- **`regex::Regex` is cheaply `Clone` (internally reference-counted), so storing a
  compiled regex inside a validator keeps `Schema: Clone` intact** (validators are
  held as `Rc<dyn Validator>` regardless, so the `Vec` clone is a refcount bump).
- **Type checks reject before validators run, and T1 makes that ordering
  observable for the first time.** F2 could not test the `check_type → validators`
  leg (`any` never fails its type check). With a typed kind, validating a wrong
  type against e.g. `number().min(5)` yields `TYPE_MISMATCH`, proving the validator
  is not reached on a type mismatch (Verification item 14).
- **Leaf errors carry an empty `path`.** Path segments are attached by container
  types (T2) as they descend; the `Validator::validate(&self, value)` signature is
  F2-fixed and intentionally path-unaware. T1 leaf/validator errors therefore have
  `path == []`; this is correct for the leaf layer and is not a defect.

## Design

### Module layout

- New file `src/types.rs` declaring: the five builder newtypes
  (`StringSchema`, `NumberSchema`, `BooleanSchema`, `EnumSchema`, `NullSchema`),
  their `BuilderInner` + `From<…> for Schema` impls, their inherent methods, the
  constructors (`string`, `number`, `boolean`, `enumerate`, `null`), the concrete
  `Validator` implementations, the per-kind `check_*` delegate functions, the
  `type_tag` helper, the T1 error-code catalogue block, and `#[cfg(test)] mod
  tests`.
- `src/lib.rs`: add `mod types;` and
  `pub use types::{string, number, boolean, enumerate, null, StringSchema,
  NumberSchema, BooleanSchema, EnumSchema, NullSchema};`. Concrete validators and
  `check_*` delegates stay `pub(crate)` / private (not public API).
- `src/schema.rs`: two changes only — (a) extend `SchemaKind` with the five
  variants and add one delegating arm per variant in `check_type`, `parse_inner`,
  and the `Debug` match; (b) **change the `BuilderInner` trait's visibility from
  private to `pub(crate)`** (trait header only — `pub(crate) trait BuilderInner`).
  This is required: the blanket `impl<T: BuilderInner> Modify for T` lives in
  `schema.rs` and gives a builder its universal modifiers *only if* the builder
  implements `BuilderInner`; since the T1 builders live in the sibling module
  `src/types.rs`, they cannot implement a private trait. No other change to
  `schema.rs` — `Schema::new`, the `Schema` fields (`kind`/`modifiers`/
  `validators`), and `SchemaKind` are already `pub(crate)` and reachable from
  `src/types.rs`. The blanket impl, `BuilderInner for Schema`, and `Modify` itself
  are untouched.
- `Cargo.toml`: add `regex = "1"` to `[dependencies]`.

### `SchemaKind` extension (in `src/schema.rs`)

Add variants (T1-owned, living syntactically in the container per C1):

```rust
pub(crate) enum SchemaKind {
    Any,                 // F2
    Lazy(Lazy),          // F2
    String,              // T1
    Number,              // T1
    Boolean,             // T1
    Enum(Vec<String>),   // T1 — the allowed value set is the type's payload
    Null,                // T1
}
```

Dispatch arms (delegating to type-owned logic; no control logic added):

- `check_type`:
  `SchemaKind::String => crate::types::check_string(value)`,
  `Number => crate::types::check_number(value)`,
  `Boolean => crate::types::check_boolean(value)`,
  `Enum(set) => crate::types::check_enum(value, set)`,
  `Null => crate::types::check_null(value)`.
- `parse_inner`: basic kinds are leaves with no structural transformation —
  `SchemaKind::String | Number | Boolean | Enum(_) | Null => Ok(value.clone())`
  (identical to `Any`'s behaviour; may share one arm).
- `Debug` match: render `"String"`, `"Number"`, `"Boolean"`, `"Enum"`, `"Null"`.

`Enum` carries `Vec<String>`, so `SchemaKind` remains `Clone`.

### Type checks (`check_*` in `src/types.rs`)

Each returns `Result<(), ZerxError>`; on mismatch, `ErrorCode::TYPE_MISMATCH`
(seeded by `errors`) with `.expected(<requirement>).received(type_tag(value))`.

- `check_string` — accept `ZerxValue::String`; else `TYPE_MISMATCH`, expected
  `"string"`.
- `check_number` — accept `I64`/`U64`/`I128`/`U128`/`F64`; reject a non-finite
  `F64` (NaN/±∞) and all non-numeric variants → `TYPE_MISMATCH`, expected
  `"number"` (the "finite number" requirement matches Zex's `Number.isFinite`).
- `check_boolean` — accept `ZerxValue::Bool`; else expected `"boolean"`.
- `check_enum(value, set)` — accept a `String` whose value is in `set`; a `String`
  not in `set` → `ErrorCode::INVALID_ENUM_VALUE` with `expected` listing the
  allowed values; a non-string → `TYPE_MISMATCH`, expected the enum requirement.
- `check_null` — accept `ZerxValue::Null`; else expected `"null"`. (Distinct from
  the `nullable` modifier: `null()` is a type whose only valid value is `Null`.)

`type_tag(value: &ZerxValue) -> &'static str` — `pub(crate)` helper mapping a
value to a compact tag (`"null"`, `"boolean"`, `"number"`, `"string"`,
`"bytes"`, `"array"`, `"object"`; under `--features mlua`, `"host_opaque"`) for
the `received` descriptor.

### Concrete validators (`src/types.rs`, implementing `schema-core`'s `Validator`)

Each implements `validate(&self, &ZerxValue) -> Result<(), ZerxError>` and
`json_schema(&self) -> serde_json::Map<String, serde_json::Value>`. Validators run
*after* `check_type`, so each may assume the value is already of the right kind;
a defensive non-matching value is treated as a pass-through (the type check owns
type errors) to keep responsibilities clean — validators only enforce their own
constraint.

String validators:

- `MinLength(usize)` — fail if `s.chars().count() < n` →
  `STRING_TOO_SHORT`. Fragment `{"minLength": n}`. (Length is counted in
  Unicode scalar values, matching JS `String.length`'s intent closely enough for
  v1; noted for the Reviewer.)
- `MaxLength(usize)` — fail if `> n` → `STRING_TOO_LONG`. Fragment
  `{"maxLength": n}`.
- `Pattern { source: String, compiled: PatternState }` where
  `enum PatternState { Ok(regex::Regex), Invalid(String) }`. Built by compiling
  the pattern once: `Ok` on success, `Invalid(err)` on failure. `validate`:
  `Invalid(err) → Err(PATTERN_INVALID)` carrying the compile error; `Ok(re) →`
  fail if `!re.is_match(s)` → `PATTERN_MISMATCH`. Fragment `{"pattern": source}`
  in both cases (the source is preserved for export).
- `Email` — fail if the value does not match the fixed structural form
  (non-empty run, `@`, non-empty run, `.`, non-empty run; no whitespace/`@` in the
  runs) → `INVALID_EMAIL`. Hand-rolled (no regex engine). Fragment
  `{"format": "email"}`.
- `Uuid` — fail unless the value is `8-4-4-4-12` lowercase/uppercase hex with
  dashes at positions 8/13/18/23 → `INVALID_UUID`. Hand-rolled. Fragment
  `{"format": "uuid"}`.

Number validators:

- `MinValue(f64)` — fail if `numeric_as_f64(value) < n` → `NUMBER_TOO_SMALL`.
  Fragment `{"minimum": n}`.
- `MaxValue(f64)` — fail if `> n` → `NUMBER_TOO_LARGE`. Fragment
  `{"maximum": n}`.
- `IntValidator` — pass for `I64`/`U64`/`I128`/`U128`; for `F64`, fail unless
  `v.fract() == 0.0` → `NOT_INTEGER`. Fragment `{"type": "integer"}`.

`numeric_as_f64(value: &ZerxValue) -> Option<f64>` — private helper folding the
five numeric variants to `f64` for bound comparison.

### Builders, inherent methods, constructors

Pattern per builder (mirrors F2's `AnySchema`/`LazySchema`; relies on the
`BuilderInner` trait being `pub(crate)` — see Module layout):

```rust
#[derive(Clone)] pub struct StringSchema(Schema);
impl BuilderInner for StringSchema { fn schema_mut(&mut self) -> &mut Schema { &mut self.0 } }
impl From<StringSchema> for Schema { fn from(b: StringSchema) -> Schema { b.0 } }
pub fn string() -> StringSchema { StringSchema(Schema::new(SchemaKind::String)) }
```

The `impl BuilderInner for StringSchema` line is what gives `StringSchema` the
blanket `Modify` (`.optional()`, `.describe()`, …); it compiles only because
`BuilderInner` is `pub(crate)`. The same holds for all five builders.

Inherent methods push a validator (or set meta) and return `Self`:

- `StringSchema`: `min(usize)`, `max(usize)`, `regex(impl Into<String>)`,
  `pattern(impl Into<String>)`, `email()`, `uuid()`, `multiline(lines: u32)`.
  `regex` and `pattern` are two names for the same operation: `pattern` forwards
  to `regex` (`fn pattern(self, p: impl Into<String>) -> Self { self.regex(p) }`),
  installing the same `Pattern` validator with identical storage and validation
  semantics. Both names are provided because the vision advertises `regex`/`pattern`
  (`docs/vision.md:128-130`) and `pattern` is the JSON-Schema keyword already used
  project-wide; the alias is a one-line forward and avoids API drift.
- `NumberSchema`: `min(impl Into<f64>)`, `max(impl Into<f64>)`, `int()`.
- `BooleanSchema`, `NullSchema`: no inherent validators.
- `EnumSchema`: no inherent validators; the value set is fixed at construction.

`multiline(lines)` sets the meta hint via the existing carrier:
`self.0.modifiers.meta.insert("x-ui-multiline", ZerxValue::U64(lines as u64)); self`.
It pushes **no** validator and has **no** validation effect — purely a UI hint
carried for later export. (Zex's `0-removes-the-key` nicety is dropped under
YAGNI; the `Map` has no remove and adding one is out of T1 scope.)

Constructors:

- `string()`, `number()`, `boolean()`, `null()` — wrap `Schema::new(<kind>)`.
- `enumerate<I, S>(values: I) -> EnumSchema where I: IntoIterator<Item = S>,
  S: Into<String>` — collect into `Vec<String>`, store in `SchemaKind::Enum`.

**Decision — `regex` and `pattern` are both exposed, `pattern` forwarding to
`regex`.** The vision advertises `regex`/`pattern` as the built-in surface
(`docs/vision.md:128-130`) and `pattern` is the JSON-Schema keyword used elsewhere
in the project. T1 provides both names for the single `Pattern` validator;
`pattern` is a one-line forward to `regex` with identical storage and validation
semantics, so the compatibility win costs nothing and avoids API drift.

**Decision — no terminal `.validate` on builders in T1 (KISS / YAGNI).** F2 left
this ergonomics question to T1. The primary composition path is the container
constructors (T2) that accept `Into<Schema>`; for standalone validation a user
writes `let s: Schema = string().min(3).into(); s.validate(&x)`. Adding a
per-builder or blanket terminal `.validate` is deferred until real friction
appears in T2+. Flagged for Reviewer objection.

### Error-code catalogue (open, in `src/types.rs`, no edit to `error.rs`)

```rust
impl ErrorCode {
    pub const STRING_TOO_SHORT: ErrorCode   = ErrorCode::new("string_too_short");
    pub const STRING_TOO_LONG: ErrorCode    = ErrorCode::new("string_too_long");
    pub const PATTERN_MISMATCH: ErrorCode   = ErrorCode::new("pattern_mismatch");
    pub const PATTERN_INVALID: ErrorCode    = ErrorCode::new("pattern_invalid");
    pub const INVALID_EMAIL: ErrorCode      = ErrorCode::new("invalid_email");
    pub const INVALID_UUID: ErrorCode       = ErrorCode::new("invalid_uuid");
    pub const NUMBER_TOO_SMALL: ErrorCode   = ErrorCode::new("number_too_small");
    pub const NUMBER_TOO_LARGE: ErrorCode   = ErrorCode::new("number_too_large");
    pub const NOT_INTEGER: ErrorCode        = ErrorCode::new("not_integer");
    pub const INVALID_ENUM_VALUE: ErrorCode = ErrorCode::new("invalid_enum_value");
}
```

`TYPE_MISMATCH` (seeded in `errors`) is reused for kind mismatches; T1 adds no
edit to `src/error.rs`.

## Steps

1. `Cargo.toml`: add `regex = "1"` under `[dependencies]`.
2. `src/schema.rs`: (a) add the five `SchemaKind` variants (`String`, `Number`,
   `Boolean`, `Enum(Vec<String>)`, `Null`) and one delegating arm per variant in
   `check_type`, `parse_inner`, and the `Debug` match (per Design); (b) change the
   `BuilderInner` trait header to `pub(crate) trait BuilderInner` so the
   `src/types.rs` builders can implement it and receive the blanket `Modify`. No
   other change.
3. `src/lib.rs`: add `mod types;` and the `pub use types::{…}` re-exports.
4. `src/types.rs`: declare the T1 `ErrorCode` block and the `type_tag` /
   `numeric_as_f64` helpers.
5. `src/types.rs`: implement the `check_string`/`check_number`/`check_boolean`/
   `check_enum`/`check_null` delegate functions.
6. `src/types.rs`: implement the concrete validators (`MinLength`, `MaxLength`,
   `Pattern` + `PatternState`, `Email`, `Uuid`, `MinValue`, `MaxValue`,
   `IntValidator`) with `validate` + `json_schema`.
7. `src/types.rs`: implement the five builder newtypes (`BuilderInner`,
   `From<…> for Schema`, inherent methods — including `pattern` forwarding to
   `regex` on `StringSchema`) and the constructors (`string`, `number`,
   `boolean`, `enumerate`, `null`).
8. Add `#[cfg(test)] mod tests` per Verification, plus the `compile_fail` doctest
   for the negative guarantee (on the `boolean` constructor's doc comment).
9. Run the full build/lint/test matrix below; resolve every warning.

**Doc Update (workflow Step 6, by the Coder after Validation — not now):** fill
`docs/architecture/types.md` Constraints with the review-hardened normative
contract: the five basic types and their kind variants; each type's accepted
`ZerxValue` kinds and `TYPE_MISMATCH` behaviour; the inherent-validator set per
builder and the negative compile-guarantee (type-specific methods unrepresentable
on builders where they make no sense); the concrete `Validator` implementations
and their `json_schema()` fragments (`minLength`/`maxLength`/`pattern`/`minimum`/
`maximum`/`type:integer`/`format:email`/`format:uuid`); `multiline` reclassified
as a **meta/modifier hint** (`x-ui-multiline`), explicitly **not** part of the
`Validator` system (correcting the vision-list imprecision per the Architect);
the C7-consistent deferral of an invalid-regex error to validate-time
(`PATTERN_INVALID`); and add **`regex`** as an *Architecturally Significant
Dependency* in `types.md`.

Beyond Constraints/ASD, the same Doc Update MUST repair the two existing internal
inconsistencies in `types.md` that T1 substantiates, so the concern file is
coherent after the step (not merely its Constraints):

- **Purpose** (`docs/architecture/types.md:8-10`) currently lists `multiline`
  among the "pluggable validators". Remove `multiline` from that validator list —
  the reclassification is incomplete while the Purpose line still contradicts it.
- **Related Decisions** (`docs/architecture/types.md:22-25`) currently carries a
  candidate note pointing at C4/C2 (which belong to the not-yet-executed `T2`/`T4`
  object-mode and buffer work, whose constraints are not yet in the file). Update
  the candidate note to the candidates T1 actually substantiates here — **C1, C7,
  C8, C9** — matching the now-filled content. `T2`/`T4` re-add their candidates
  (C4, C2) when they fill their own constraints, per the concept's incremental
  "filled per plan, reconciled at closeout" model.

Per the concept's Decision-Reference Discipline the note stays a **non-slug
candidate reference** (e.g. "candidate C1"); this Doc Update MUST NOT add any
`D-<slug>` reference to `types.md` — the C1/C7/C8/C9 migration and all slug
references are routed to Concept Closeout. The `types`↔`schema-core` `Consumes
from`/`Provides to` edges already exist on both sides; no new edge is opened.

## Verification

Build & lint — all warning-free (the `mlua` runs prove T1 compiles with the gated
`HostOpaque` placeholder present; `type_tag` covers it):

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets`
- `cargo clippy --all-targets --features mlua`

Unit tests (`#[cfg(test)] mod tests` in `src/types.rs`) — each asserts behaviour
(validate outcomes, error `code`, `expected`/`received`, and `json_schema()`
fragments), not mere existence. Builders are converted via `let s: Schema =
<builder>.into();` then `s.validate(&x)`, exercising the `Into<Schema>` path.

1. **`string` type check.** `string()` accepts `"hi"` (→ `Ok(String("hi"))`);
   rejects `7i32`, `true`, `()` → `Err` with `code == TYPE_MISMATCH`,
   `expected == Some("string")`, `received == Some("number"|"boolean"|"null")`.
2. **String `min`/`max`.** `string().min(2)` accepts `"hi"`, rejects `"h"` →
   `STRING_TOO_SHORT`; `string().max(3)` accepts `"hey"`, rejects `"heyy"` →
   `STRING_TOO_LONG`. Boundary lengths (`== n`) pass. `MinLength(2).json_schema()
   == {"minLength":2}`, `MaxLength(3).json_schema() == {"maxLength":3}`.
3. **String `regex` / `pattern`.** `string().regex(r"^\d{5}$")` accepts
   `"12345"`, rejects `"abc"` → `PATTERN_MISMATCH`. `string().pattern(r"^\d{5}$")`
   behaves identically (the alias forwards to `regex`): same accept/reject and
   same `PATTERN_MISMATCH` code, proving `pattern` installs the same validator.
   `Pattern` over an invalid source (e.g. `"("`) validated against any string →
   `Err` with `code == PATTERN_INVALID`. `json_schema() == {"pattern": "^\\d{5}$"}`
   (source preserved, including for the invalid case).
4. **String `email`/`uuid`.** `string().email()` accepts `"a@b.co"`, rejects
   `"a@b"`, `"a b@c.d"` → `INVALID_EMAIL`; fragment `{"format":"email"}`.
   `string().uuid()` accepts a canonical UUID (both cases), rejects a too-short or
   mis-dashed string → `INVALID_UUID`; fragment `{"format":"uuid"}`.
5. **`multiline` is a meta hint, not a validator.** `string().multiline(3)`
   carries `modifiers.meta.get("x-ui-multiline") == Some(U64(3))` (white-box) and
   adds **no** validator (`validators.len()` unchanged); validating `"x"` against
   it still succeeds.
6. **`number` type check.** `number()` accepts `7i32`, `7u64`, `i128::MAX`,
   `u128::MAX`, `2.5f64`; rejects `"hi"`, `true` → `TYPE_MISMATCH`,
   `expected == Some("number")`. A non-finite `F64` (`f64::NAN`, `f64::INFINITY`,
   constructed as a `ZerxValue::F64` and parsed via `parse_present`) →
   `TYPE_MISMATCH`.
7. **Number `min`/`max` with integer-literal ergonomics.** `number().min(0)`
   accepts `0`, `5`; rejects `-1` → `NUMBER_TOO_SMALL`. `number().max(9)` accepts
   `9`; rejects `10` → `NUMBER_TOO_LARGE`. (The bare-literal calls also prove the
   `impl Into<f64>` ergonomics compile.) `MinValue(0.0).json_schema() ==
   {"minimum":0.0}`, `MaxValue(9.0).json_schema() == {"maximum":9.0}`.
8. **Number `int`.** `number().int()` accepts `7i64` and the integral float
   `4.0f64`; rejects `2.5f64` → `NOT_INTEGER`. Fragment `{"type":"integer"}`.
9. **`boolean` type check.** `boolean()` accepts `true`/`false`; rejects `1i32`,
   `"true"` → `TYPE_MISMATCH`, `expected == Some("boolean")`.
10. **`enumerate`.** `enumerate(["user","assistant","system"])` accepts
    `"assistant"`; rejects `"other"` → `INVALID_ENUM_VALUE` (with `expected`
    listing the values); rejects `7i32` → `TYPE_MISMATCH`.
11. **`null` type.** `null()` accepts `()` (→ `ZerxValue::Null`); rejects `0i32`,
    `false` → `TYPE_MISMATCH`, `expected == Some("null")`. (Independent of the
    `nullable` modifier.)
12. **Type-specific-method composition (both orders).** `string().min(3).optional()`
    and `string().optional().min(3)` both type-check as `StringSchema` and produce
    an equivalent schema: validating `"hi"` → `STRING_TOO_SHORT`, validating
    `"hello"` → `Ok`. Via `parse_field(None, ctx)` the optional schema yields
    `Ok(None)` (missing optional omitted). `number().int().min(0).max(9)` chains
    inherent methods and universal modifiers together and validates `5` → `Ok`,
    `-1` → `NUMBER_TOO_SMALL`, `2.5` → `NOT_INTEGER`.
13. **Negative compile-guarantee.** A `compile_fail` doctest on `boolean()` proves
    `.min()` is unrepresentable on a boolean builder:
    ````text
    ```compile_fail
    use zerx::boolean;
    let _ = boolean().min(3);   // ERROR: no method `min` on BooleanSchema
    ```
    ````
    Runs under `cargo test` (doctests); no extra dependency.
14. **`check_type → validators` ordering is now observable.** `number().min(5)`
    validated against `"hi"` → `Err` with `code == TYPE_MISMATCH` (the `min`
    validator is never reached on a type mismatch), closing the F2 test gap where
    `any` could not distinguish this ordering.
15. **Clone immutability with validators.** From `let base = string().min(1);`,
    `let s2: Schema = base.clone().max(5).into();` has **two** validators while
    `Schema::from(base)` has **one** (the original chain is untouched —
    clone-and-return holds for the validator vector too).
16. **`SchemaKind` dispatch + Debug.** `Schema`'s hand-written `Debug` renders the
    correct kind name for each new builder (`"String"`/`"Number"`/`"Boolean"`/
    `"Enum"`/`"Null"`), proving the Debug arms and confirming a new type added
    exactly one variant + one arm per dispatch method.

Expected outcome: a `zerx` crate exposing the `string`/`number`/`boolean`/
`enumerate`/`null` constructors and their typed builders, the inherent validators
(`min`, `max`, `regex`, `int`, `email`, `uuid`) as concrete `Validator`
implementations with correct JSON-Schema fragments, the `multiline` meta hint, the
negative compile-guarantee, and the type-specific-method composition — all built
on F2's dispatch seam with no new parse-flow control logic — ready for
`PLAN_T2_complex_types` to add `object`/`array`/`record`/`tuple`/`union`/
`discriminated_union`/`literal` and the object modes on top.
```

<plan_ready>docs/PLAN_T1_basic_types.md</plan_ready>
