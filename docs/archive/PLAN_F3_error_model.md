# PLAN_F3_error_model

## Context & Goal

This is the first plan of the `CONCEPT_zerx_foundation` execution (Phase 1 —
Foundation) and therefore also bootstraps the Cargo crate. It implements the
`errors` concern: the structured, machine-readable `ZerxError` that every
fallible zerx operation returns (per candidate decision C7 — single
`Result`-returning API).

`ZerxError` is the vehicle for validation failures. It carries `path`, `code`,
`message`, `received`, `expected`, and `inner_errors`, is `Serialize` /
`to_json`-capable so it crosses process and tool boundaries, and offers the
union-failure aggregation that collects per-variant errors into one combined
error.

Per candidate decision C8, `received` and `expected` are **plain serialisable
descriptor strings** — `expected` describes the schema's requirement, `received`
is a compact type tag (not the raw value). They do **not** embed a `ZerxValue`.
This is the deliberate design that keeps F3 self-contained with **no dependency
on `PLAN_F1_value_model`**: a `ZerxValue` may hold a host-opaque value that is
not `Serialize` (C3), which would break `ZerxError`'s own serialisability and
couple the error model to `value-model`.

The concern boundary is fixed in `docs/architecture/errors.md`:

- This concern owns `ZerxError` and its fields.
- This concern owns error serialisation (`to_json` / `Serialize`).
- This concern owns union-failure aggregation.
- This concern does **NOT** own *when* errors are raised — that is the parse
  flow in `schema-core` (`PLAN_F2_schema_core` and later). Therefore this plan
  delivers the error *type and mechanism*, not a closed catalogue of every code
  the system will ever raise.

Reference: the type is a faithful port of Zex's `ZexError`
(`/Users/martinschlott/Documents/MyProjects/zex/src/zex/types.ts:71`), adapted
to Rust idioms (`std::error::Error`, `Serialize`, owned descriptor strings per
C8 instead of Zex's raw `received: unknown`).

## Breaking Changes

**No.** This is the first plan; there is no prior code, crate, or published
artifact to break. It creates `Cargo.toml` and `src/`.

## Dependencies

Declared at concept level by candidate decision C9 (*Built on serde, lean beyond
it*). These are the only dependencies this plan introduces:

- `serde` (with the `derive` feature) — for `#[derive(Serialize)]` on
  `ZerxError`.
- `serde_json` — for the `to_json` helper returning `serde_json::Value`.

No other crates. In particular **no `serde_bytes`** (C9) and **no `mlua`** (that
is Phase 5, feature-gated). The exact patch versions are the Coder's choice at
`cargo add` time; the crate set above is fixed by this plan.

## Reference Patterns

- Zex error class & `toJSON`:
  `/Users/martinschlott/Documents/MyProjects/zex/src/zex/types.ts:71-105`
  (fields `path`, `code`, `message`, `received`, `expected`, `innerErrors`;
  `toJSON` nesting).
- Zex union aggregation:
  `/Users/martinschlott/Documents/MyProjects/zex/src/zex/unions.ts:60-90`
  (collect per-variant errors, attach as inner errors, combined message). Note:
  zerx keeps the *mechanism* (attach inner errors, produce one combined error);
  the variant-selection *strategy* (Zex's shortest-path heuristic) belongs to
  the union type in `PLAN_T2_complex_types`, not here.

## Assumptions & Risks

- **Assumption (C8):** `received`/`expected` as `Option<String>` descriptors are
  sufficient for consumers; consumers branch on `code` and `path`, not on the
  raw value. This is fixed by C8 and is the basis for self-containment.
- **Assumption:** an open code catalogue (no central exhaustive enum) is the
  right shape, because the full set of codes is only known once the parse flow,
  types, json-schema, delta, and policy plans land. Sealing the catalogue now
  would couple F3 to every later plan and force F3 edits on each new code. See
  **Proposed Decisions** below — Review must confirm this binds later plans.
- **Risk:** later plans must be able to introduce their own codes without
  editing `src/error.rs`. Mitigation: `ErrorCode` has a `const fn new`, and Rust
  permits additional inherent `impl ErrorCode { … }` blocks in any module of the
  same crate. Each later plan therefore adds its own codes as **associated
  constants** in an `impl ErrorCode` block next to the code that raises them
  (e.g. `impl ErrorCode { pub const UNKNOWN_PROPERTY: ErrorCode = ErrorCode::new("unknown_property"); }`
  in the types module), with no edit to `src/error.rs`.

## Design

### Crate bootstrap

- `Cargo.toml`: package `zerx`, edition `2021`, the two dependencies above.
- `src/lib.rs`: crate root. Declares `mod error;` and re-exports the public
  surface (`pub use error::{ZerxError, ErrorCode};`). A short crate-level doc
  comment is acceptable; no other modules are created by this plan (later plans
  add `value_model`, `schema_core`, etc.).

### `ErrorCode`

- A lightweight newtype wrapping `Cow<'static, str>`, serialising **transparently
  as the bare string** (`#[serde(transparent)]`), so the JSON `code` field is a
  plain string and stays a stable machine-readable identifier (C8).
- `const fn new(&'static str) -> ErrorCode` — the single constructor every
  named code is built from. Owned codes (rare, runtime-derived) via
  `From<String>`; `From<&'static str>` for convenience.
- `as_str(&self) -> &str`, `Display`, `PartialEq`/`Eq`, `Clone`, `Debug`.
- **Public access form is fixed: associated constants on `ErrorCode`**
  (`ErrorCode::TYPE_MISMATCH`), not free module constants. This is the API
  later plans and tests depend on, and it composes with the open-catalogue story
  because Rust allows additional `impl ErrorCode` blocks in any module of the
  same crate (see the Risk note above).
- **Seed codes** — defined in `error.rs` as associated constants in an
  `impl ErrorCode` block; only the foundational/cross-cutting ones this concern
  itself owns or uses in tests. The catalogue is explicitly open:
  - `ErrorCode::UNKNOWN_ERROR` (`"unknown_error"`) — generic catch-all.
  - `ErrorCode::TYPE_MISMATCH` (`"type_mismatch"`) — the canonical/universal
    failure, used as the worked example in tests.
  - `ErrorCode::UNION_MISMATCH` (`"union_mismatch"`) — produced by the union
    aggregation helper this concern owns.
  - A doc comment on the `impl` block states later plans add their own codes as
    associated constants in their own `impl ErrorCode` blocks, near the code
    that raises them.

### `ZerxError`

Fields (exact Rust types are the Coder's, but the shape is fixed here):

- `path: Vec<String>` — path segments from root to the failure site (object
  keys and array indices rendered as strings), matching Zex's `string[]`. Empty
  `Vec` means the root.
- `code: ErrorCode`.
- `message: String` — English-only (C8).
- `received: Option<String>` — compact descriptor / type tag of what arrived
  (C8); **not** the raw value, **not** a `ZerxValue`.
- `expected: Option<String>` — descriptor of the schema's requirement (C8).
- `inner_errors: Vec<ZerxError>` — per-variant / nested errors for aggregation;
  empty when there are none.

Derives & impls:

- `#[derive(Debug, Clone, Serialize)]`. **No `Deserialize`** — errors flow
  outward only; deserialisation is YAGNI for v1 (a later plan may add it if a
  real consumer needs it).
- Serde attributes so the JSON stays clean:
  - `received` and `expected`: `#[serde(skip_serializing_if = "Option::is_none")]`.
  - `inner_errors`: `#[serde(skip_serializing_if = "Vec::is_empty")]`.
  - No `name` field (a TypeScript artifact in Zex; dropped).
- `impl std::fmt::Display` — human-oriented one-line rendering: the path
  segments joined by `/` (e.g. `a/b/0`; empty path rendered as `(root)`),
  followed by `: {message}`. This is a **plain readable join, NOT an RFC 6901
  JSON Pointer**: no escaping of `/` or `~` inside segments, no leading `/`, and
  it is not intended to be parsed back into segments. This rendering is cosmetic;
  consumers branch on the structured `code` and `path` fields, not on the
  `Display` text. (The machine-readable path stays the `Vec<String>` segments;
  any JSON-Pointer addressing for delta/replace is the concern of
  `PLAN_D1_delta_replace`, not this Display.)
- `impl std::error::Error for ZerxError` — so `ZerxError` is a first-class Rust
  error usable with `?` and `Box<dyn Error>`.

Constructors / builder:

- `new(code: ErrorCode, message: impl Into<String>) -> ZerxError` — minimal
  constructor; `path` empty, `received`/`expected` `None`, `inner_errors` empty.
- Chainable builder setters (consume & return `self`):
  `at(path: Vec<String>)`, `received(impl Into<String>)`,
  `expected(impl Into<String>)`, `with_inner(Vec<ZerxError>)`. These keep
  construction ergonomic for the many call sites later plans will add.
- `to_json(&self) -> serde_json::Value` — infallible; implemented via
  `serde_json::to_value(self)`. Because every field is a plain serialisable type
  (the C8 guarantee), this conversion cannot fail; the implementation may
  `expect` on the (unreachable) error with a clear message, or use a form that
  is statically infallible. The public signature returns `serde_json::Value`
  directly, not a `Result`.

### Union aggregation (owned by this concern)

- `union(path: Vec<String>, inner: Vec<ZerxError>) -> ZerxError` — produces one
  combined error with `code = UNION_MISMATCH`, the supplied `path`, the supplied
  errors attached as `inner_errors`, and a generated `message` summarising that
  no variant matched and how many alternatives were tried (e.g.
  `"no union variant matched (N alternatives)"`). `expected` set to a generic
  descriptor (e.g. `"one of the union variants"`); `received` left `None` (the
  variant errors carry the specifics). This is the **mechanism** only; which
  errors to pass and any best-variant heuristic is the union type's job
  (`PLAN_T2_complex_types`).

## Steps

1. Create `Cargo.toml` for package `zerx`, edition 2021; add `serde` (with
   `derive`) and `serde_json` via `cargo add`.
2. Create `src/lib.rs` with `mod error;` and the public re-exports.
3. Implement `ErrorCode` in `src/error.rs`: the `Cow`-backed transparent
   newtype, `const fn new`, `From` impls, `as_str`, `Display`, and the three
   seed associated constants (in an `impl ErrorCode` block), with the
   "catalogue is open" doc comment.
4. Implement `ZerxError` in `src/error.rs`: the six fields, derives + serde
   skip attributes, `Display`, `std::error::Error`, the constructor + builder
   setters, `to_json`, and the `union` aggregation constructor.
5. Add unit tests (`#[cfg(test)] mod tests` in `src/error.rs`) per the
   Verification section.
6. Run `cargo build`, `cargo test`, and `cargo clippy --all-targets` and
   resolve any issues. (`--all-targets` is the acceptance bar — it lints the
   `#[cfg(test)]` code this plan adds.)

**Doc Update (step 6 of the workflow, performed by the Coder after Validation —
not now):** fill `docs/architecture/errors.md` Constraints with the
review-hardened, normative contract delivered here (field set; `received`/
`expected` are serialisable descriptor strings, not `ZerxValue`; `to_json` is
infallible; `inner_errors` carries aggregated variant errors; the code catalogue
is open). Per this concept's Decision-Reference-Discipline, this Doc Update fills
**Constraints only** and MUST NOT add any `D-<slug>` reference to `errors.md`:
for the plans under `CONCEPT_zerx_foundation`, the concept routes all
decision-slug references and the C1–C9 migration into `decisions.md` to Concept
Closeout. This is a concept-local choice, consistent with — not an override of —
`decisions.md`'s general allowance that a decision *may* enter via a Plan's Doc
Update; that allowance is simply not exercised by this concept's plans. The
binding candidate decisions for this plan during execution live in
`CONCEPT_zerx_foundation.md` (C7, C8).

## Proposed Decisions

The following design choice binds **every later plan that raises a zerx error**,
so it is surfaced for Review to confirm whether it migrates to `decisions.md`
(it is not covered by any existing candidate C1–C9):

- **Open `ErrorCode` catalogue, no central enum.** zerx represents an error
  `code` as a string-backed `ErrorCode` newtype with `const`-declarable values,
  **not** a single exhaustive `enum` of all codes. Each plan declares its own
  codes next to the code that raises them. Rationale: the full code set is only
  knowable once all plans land; a central enum would force F3 edits on every new
  code and couple the error model to all later concerns. Consequence: there is
  no compile-time exhaustiveness check over codes; correctness of the
  machine-readable `code` contract (C8) rests on the `const` declarations and
  tests, not the type system.

Under this concept's Decision-Reference-Discipline, promotion of its candidate
decisions is bundled at Concept Closeout rather than done per-plan. So if Review
accepts this as future-binding, the route is: the Architect adopts it as a new
candidate in `CONCEPT_zerx_foundation.md` (e.g. `D-open-error-code-catalogue`),
and it migrates with C1–C9 at Concept Closeout. If Review judges it plan-local,
it stays in this section and is archived with the plan. Either way, this plan
adds no decision-slug reference to any permanent doc.

## Verification

Build & lint:

- `cargo build` — succeeds, no warnings.
- `cargo clippy --all-targets` — no warnings.
- `cargo test` — all tests pass.

Unit tests (each asserts behaviour, not mere existence):

1. **Minimal construction:** `ZerxError::new(ErrorCode::TYPE_MISMATCH, "...")`
   has empty `path`, `None` received/expected, empty `inner_errors`.
2. **Builder chaining:** `new(...).at(vec!["a".into(),"0".into()]).expected("string").received("number")`
   sets each field exactly.
3. **JSON shape (clean):** `to_json` / `serde_json::to_value` of an error with
   `received`/`expected` set and inner errors yields an object with keys exactly
   `{path, code, message, received, expected, inner_errors}`; `code` is the bare
   string (e.g. `"type_mismatch"`), `path` is a JSON array of strings.
4. **JSON shape (omission):** an error with `None` received/expected and empty
   `inner_errors` serialises **without** the `received`, `expected`, and
   `inner_errors` keys.
5. **Nested aggregation:** `ZerxError::union(path, vec![e1, e2])` has
   `code == ErrorCode::UNION_MISMATCH`, two entries in `inner_errors`, and its
   `to_json` contains an `inner_errors` array of length 2 whose elements are
   themselves the serialised inner errors.
6. **Display:** an error with empty `path` renders `(root): {message}`; an error
   with `path == ["a","b","0"]` renders `a/b/0: {message}` (segments joined by
   `/`, no leading slash). No escaping contract is asserted — Display is a
   readable hint, not a parseable pointer.
7. **`std::error::Error`:** a function returning `Result<(), Box<dyn std::error::Error>>`
   compiles when `?`-propagating a `ZerxError` (compile-level assertion that the
   trait is implemented).
8. **`ErrorCode` equality & open-catalogue extension:** an additional
   `impl ErrorCode { pub const CUSTOM: ErrorCode = ErrorCode::new("custom_code"); }`
   block (proving later plans can extend the catalogue via associated constants
   in their own module) compiles, and `ErrorCode::CUSTOM.as_str() == "custom_code"`;
   `ErrorCode::TYPE_MISMATCH == ErrorCode::new("type_mismatch")`.

Expected outcome: a compiling `zerx` crate exposing `ZerxError` and `ErrorCode`,
fully serialisable, with union aggregation, ready for `PLAN_F1_value_model` and
`PLAN_F2_schema_core` to build on.
