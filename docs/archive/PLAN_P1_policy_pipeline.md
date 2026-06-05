# PLAN_P1_policy_pipeline

## Context & Goal

First plan of **Phase 4 — Delta/Replace + Policy** still open in the
`CONCEPT_zerx_foundation` execution (`PLAN_D1_delta_replace`, the other Phase 4
plan, is complete and archived). All of Phase 1, Phase 2, and Phase 3
(`PLAN_J1_export`, `PLAN_J2_import`) are complete and archived. The bare core
importer `from_json_schema(&serde_json::Value) -> Result<Schema, ZerxError>` is
implemented in `src/json_schema.rs` and its contract is fixed in
`docs/architecture/json-schema.md`.

P1 delivers the **import policy pipeline**: the composable transform layer that
lets heterogeneous JSON Schema sources (PostgreSQL-flavoured schemas being the
shipped example) import into a `Schema`. The pipeline wraps J2's core importer
with **schema transforms** (pre-parse, on the input JSON Schema `Value`) and
**type transforms** (post-parse, on the produced `Schema`), plus a **deref hook**
for external `$ref`. It ships one built-in policy, **`sql`** (PostgreSQL-focused).

Per `docs/vision.md` §"Policy pipeline" (lines 291-309) the public surface is:

```
zerx::register_policy(name, policy)
zerx::from_json_schema(&Value, { policy: "sql" })   // options-carrying variant
zerx::apply_type_transforms(schema, transforms)
```

and the pipeline order is:

```
from_json_schema_with(value, opts)
  → schema transforms   (deref, then policy, then caller)  — mutate the input JSON Schema Value
  → from_json_schema(&effective_value)                     — J2 core import walk
  → type transforms     (policy, then caller)              — rebuild the produced Schema
```

Both transform stages are part of the delivered mechanism. The built-in `sql`
policy populates only the **schema-transform** stage (its `type_transforms` is
empty); the type-transform stage exists for caller-supplied transforms. See
Assumptions "Why the built-in `sql` policy uses schema transforms, not a type
transform" for the reason.

This satisfies `definition.md` success criteria for "Policy-driven import" and the
in-scope item "The policy pipeline with the built-in `sql` policy"
(`CONCEPT_zerx_foundation.md` Scope boundaries). Additional built-in policies
(e.g. OpenAPI) are explicitly out of scope (concept Scope boundaries; captured as
backlog at Concept Closeout); `register_policy` keeps them out-of-tree.

### Boundary (`docs/architecture/policy.md`)

- `policy` owns: the policy registry (`register_policy` + the global table seeded
  with `sql`), the `Policy`/`SchemaTransform`/`TypeTransform`/`RefResolver` public
  types, the pipeline composer `from_json_schema_with`, the standalone
  `apply_type_transforms`, the deref hook, the JSON-`Value` walk helper
  (`deep_map_schema`) the schema transforms use, and the built-in `sql` policy
  (which is realised entirely as **schema transforms** — see Assumptions). It
  declares its own policy `ErrorCode`(s).
- `policy` does **NOT** own the base JSON Schema walk it wraps (`json-schema`):
  P1 reuses the existing **public** `from_json_schema(&Value)` unchanged as the
  core import step. `src/json_schema.rs` is **not edited** by this plan.
- `policy` does **NOT** define the types its transforms produce (`types`) nor the
  `Schema` representation (`schema-core`): it adds no new `SchemaKind` variant and
  no parse-flow logic, and (because the built-in `sql` substitutions are pre-parse
  JSON-`Value` rewrites) it calls no `types` constructor and accesses no
  `pub(crate)` `Schema` internals — the importer produces the `Schema`. The public
  `apply_type_transforms`/`TypeTransform` surface passes `Schema` values through
  for caller-supplied transforms.
- The host-opaque (`mlua`) layer is **out of scope** (Phase 5, `PLAN_M1`). The
  `sql` policy and the pipeline are pure default-build code; no gated code is
  added.

## Breaking Changes

**No.** P1 is purely additive:

- One new module `src/policy.rs` (declared `mod policy;` in `src/lib.rs`).
- New public free functions (`register_policy`, `from_json_schema_with`,
  `apply_type_transforms`) and new public types (`Policy`, `SchemaTransform`,
  `TypeTransform`, `RefResolver`, `ImportOptions`), all re-exported from
  `src/lib.rs`.
- New policy `ErrorCode` constant(s) declared in a `src/policy.rs` `impl
  ErrorCode` block (no edit to `src/error.rs`).
- No existing signature, type, or behaviour changes. `from_json_schema(&Value)`
  keeps its current signature and semantics (it is the core step the pipeline
  calls). J1/J2 code and tests are untouched.

## Reference Patterns

- **Zex policy pipeline (canonical reference):**
  `/Users/martinschlott/Documents/MyProjects/zex/src/zex/json-schema-import.ts`
  — `registerPolicy`/`applyTypeTransforms` (lines 63-73), `deepMapSchema`
  (75-102), `makeDerefTransform` (104-120), `nullableFromAnyOfTransform` /
  `arrayItemsFallbackTransform` (122-153), `makeSqlTypeTransform` (191-316), the
  `registerPolicy('sql', …)` seed (318-322), and the `fromJsonSchema` composer
  (668-705). **P1 deviates from Zex** where zerx's J2 importer already does the
  work or where the deviation is more correct (see Assumptions): zerx's import
  walk already collapses nullable `anyOf`, builds unions from `anyOf`/`oneOf`, and
  maps non-string `enum` to literal-unions, so those normalizations are not
  re-done. Crucially, zerx's `sql` policy performs its pg-type→zerx-type
  substitutions as **pre-parse schema transforms** (on the JSON `Value`), not as a
  post-parse type transform as Zex does — see Assumptions "Why the built-in `sql`
  policy uses schema transforms, not a type transform". The post-parse
  type-transform *mechanism* is still delivered (public API + registry support);
  the built-in `sql` policy simply does not need it.
- `src/json_schema.rs` — the public `from_json_schema` (`src/json_schema.rs:414`)
  is the core step P1 calls; the `to_json_schema` / `to_json_schema_with` pair
  (`src/json_schema.rs:29-34`) is the **naming precedent** for the
  `from_json_schema` / `from_json_schema_with` pair P1 introduces.
- `src/schema.rs` — the public `Schema` type (`src/schema.rs:261-265`) that flows
  through the pipeline (named in `apply_type_transforms`/`TypeTransform`/
  `from_json_schema_with` signatures), and the `impl ErrorCode { pub const … }`
  pattern (`src/schema.rs:14-19`) P1 copies for its policy codes. P1 does **not**
  read the `pub(crate)` `Schema` internals — the built-in `sql` policy operates on
  the JSON `Value` pre-parse and lets the importer build the `Schema`.
- `docs/architecture/json-schema.md` — the import Constraints (format-marker
  mapping, `format:"json"`→`json()` regardless of type, `additionalProperties`
  shapes, nullable `anyOf` collapse, array `items` requirement) that determine the
  JSON shapes the `sql` schema transforms must produce so the importer yields the
  intended `Schema`.

## Dependencies

None new. `serde`, `serde_json`, and `regex` are already core dependencies (C9).
P1 uses `serde_json::{Value, Map}` and standard-library `Arc`, `Mutex`,
`OnceLock`, and `HashMap` only.

## Assumptions & Risks

The following pipeline choices are **concept-internal behaviour**, recorded as
`policy.md` **Constraints** at Doc Update. Per the concept's lifecycle discipline
(`docs/CONCEPT_zerx_foundation.md` §"Decision-reference discipline during the
lifecycle") **no plan writes `decisions.md` mid-concept** — register migration is
the Architect's Concept Closeout step (`CONCEPT_zerx_foundation.md` §"Affected
docs"). Where these choices deviate from Zex, the deviation follows the vision and
zerx's own import contract, which are authoritative.

- **Transform function signatures (closures, `Arc`-boxed, `Send + Sync`).**
  - `SchemaTransform = Arc<dyn Fn(&serde_json::Value) -> Result<serde_json::Value,
    ZerxError> + Send + Sync>` — pre-parse, JSON-`Value` in, JSON-`Value` out.
  - `TypeTransform = Arc<dyn Fn(Schema) -> Result<Schema, ZerxError> + Send +
    Sync>` — post-parse, `Schema` in, `Schema` out.
  - `RefResolver = Arc<dyn Fn(&str) -> Result<serde_json::Value, ZerxError> + Send
    + Sync>` — given an external `$ref` string, return the resolved subschema.

  `Send + Sync + 'static` is required because policies live in a global registry
  shared across threads. `Schema` itself is `!Send` (it holds `Rc`), but that is
  irrelevant: a `Schema` is only ever passed by value to a transform on the
  calling thread; no `Schema` is stored in the registry. Transform functions are
  `Fn` (not `FnOnce`), so a stored policy can be invoked repeatedly.

- **`Result`-returning transforms (C7).** Both transform kinds and the resolver
  return `Result<_, ZerxError>`; the composer propagates the first error with
  `?`. The built-in `sql` transforms always return `Ok` (they cannot fail), but
  the *type* is fallible so caller-supplied transforms and the deref resolver can
  surface errors. `register_policy` is **infallible** (`-> ()`): registration is
  not a fallible domain operation (it overwrites any existing entry under the same
  name, matching Zex). This is consistent with C7, which governs *fallible*
  operations.

- **`Policy` is a plain public struct.**
  `Policy { pub schema_transforms: Vec<SchemaTransform>, pub type_transforms:
  Vec<TypeTransform> }`, deriving `Default` and `Clone` (clone is cheap — `Arc`
  bumps). Callers build a `Policy` and hand it to `register_policy`. No deref is
  stored *in* a `Policy`; deref is a per-call option (see `ImportOptions`),
  mirroring Zex where `deref` is a `fromJsonSchema` option, not a policy field.

- **`ImportOptions` is a plain public struct with `Default`.**
  `ImportOptions { pub policy: Option<String>, pub schema_transforms:
  Vec<SchemaTransform>, pub type_transforms: Vec<TypeTransform>, pub deref:
  Option<RefResolver> }`. Callers use `..Default::default()` for the unused
  fields. The vision's `{ policy: "sql" }` object-literal maps to
  `ImportOptions { policy: Some("sql".into()), ..Default::default() }`.

- **Pipeline composition order (`from_json_schema_with`).** Mirrors Zex
  `fromJsonSchema` (lines 678-704):
  1. Resolve the named policy (if `opts.policy` is `Some`): an unknown name is an
     **error** (`POLICY_UNKNOWN`), not a silent no-op. *(Deliberate deviation from
     Zex, which silently ignores an unknown policy name. A typo'd policy name
     silently doing nothing is a footgun; failing closed matches zerx's
     strict-by-default posture, C4 in spirit.)*
  2. Build the ordered schema-transform list: `[deref?, policy.schema_transforms…,
     opts.schema_transforms…]`. Apply each in sequence to the input `Value`,
     threading the output of one as the input of the next, to produce the
     **effective** `Value`.
  3. Call the existing public `from_json_schema(&effective_value)` (J2) to produce
     the core `Schema`. `from_json_schema` builds its own `ImportContext` from the
     **effective** value's `$defs`, so schema transforms that rewrite `$defs` are
     honoured automatically; P1 does **not** touch `ImportContext` or
     `import_value` and does not edit `src/json_schema.rs`.
  4. Build the ordered type-transform list: `[policy.type_transforms…,
     opts.type_transforms…]`. Apply via `apply_type_transforms`.

- **`apply_type_transforms` is a trivial fold (Zex parity).**
  `apply_type_transforms(schema: Schema, transforms: &[TypeTransform]) ->
  Result<Schema, ZerxError>` folds each transform over the root
  (`current = t(current)?`), exactly like Zex's `applyTypeTransforms`. It does
  **not** itself recurse the tree — any tree recursion is a transform's own
  responsibility. The built-in `sql` policy supplies no type transform (see "Why
  the built-in `sql` policy uses schema transforms, not a type transform").

- **Global registry seeded with `sql`.** The registry is a process-global
  `OnceLock<Mutex<HashMap<String, Policy>>>`, lazily initialised with the built-in
  `sql` policy on first access, so `from_json_schema_with(v, ImportOptions {
  policy: Some("sql".into()), .. })` works without any prior `register_policy`
  call. `register_policy` overwrites an existing name. A poisoned lock MUST be
  recovered (`into_inner` on the poison error) rather than panicking the caller,
  so one panicking custom transform cannot permanently break the registry.

- **`deep_map_schema` helper.** A crate-internal
  `deep_map_schema(value: &Value, mapper: &dyn Fn(&Value) -> Result<Value,
  ZerxError>) -> Result<Value, ZerxError>` mirrors Zex's `deepMapSchema` but MUST
  recurse into **every position from which J2's core importer reads a child
  schema**, not just Zex's set — otherwise deref and the built-in schema
  transforms silently miss those positions. Apply `mapper` to the node, then
  recurse into the mapped result's: `properties` (object values), `$defs` (object
  values), `items` (schema **or** array of schemas), `prefixItems` (array of
  schemas — tuple members, which J2 imports at `src/json_schema.rs` step 10
  array/`prefixItems` arm), `additionalProperties` (when it is a schema object —
  the record value schema J2 imports for `format:"record"`, and the regular-object
  case), and the `anyOf`/`oneOf`/`allOf` arrays. The schema transforms and the
  deref transform are built on top of it. A `$defs`-backed test (Verification 14)
  and a tuple/record-position test (Verification 15) MUST prove the recursion
  reaches these positions.

- **Deref hook semantics and sibling keywords.** The deref transform (built on
  `deep_map_schema`) acts only on nodes with a `$ref` string that is **not** local
  (does not start with `#/`); local `#/$defs/...` refs are left untouched for J2's
  core importer. On a non-local `$ref` node it replaces the node with the
  resolver's result, **merging the `$ref` node's sibling keywords (every key
  except `$ref`) onto the resolved object, with the sibling keys taking
  precedence**. This mirrors how J2 preserves a local `$ref` node's siblings
  (J2 continues a local `$ref` into annotation reconstruction,
  `docs/architecture/json-schema.md` import step 12, rather than returning early),
  so annotations like `description`/`title` written next to an external `$ref`
  survive the deref. *(Deviation from Zex, which returns the resolved node and
  drops siblings; merging is more faithful to JSON Schema 2020-12, where `$ref`
  siblings are significant.)* Verification 4 asserts a sibling `description` on an
  external `$ref` survives.

- **Built-in `sql` schema transforms** (pre-parse, on the JSON `Value`), in this
  order:
  1. **nullable normalisation** — normalise SQL nullability encodings into the
     `anyOf:[<core>, {"type":"null"}]` shape J2 already collapses to
     `.nullable()`:
     - a node whose `type` is an **array** containing `"null"` plus exactly one
       other type `T` → `{"anyOf":[{…node…, "type":T}, {"type":"null"}]}` (drop
       `oneOf`/`allOf` from the rewritten core, matching Zex line 130-131);
     - a node with `oneOf` of exactly two members where exactly one is
       `{"type":"null"}` → `{"anyOf":[<other>, {"type":"null"}]}`.
  2. **array `items` fallback** — a `{"type":"array"}` node **without** `items` →
     add `"items": {}` (J2 errors on an array missing `items`; SQL/foreign sources
     often omit it; `{}` imports as `array(any())`).
  3. **pg-type substitution** — rewrite each node whose pg type signal lives in
     `format` **or** `x-pg-type` (the transform checks both encodings, matching
     Zex; the built-in `sql` policy's `int64Strategy`/`numericStrategy` are both
     `"string"`):
     - `{"type":"string","format":"bytea"}` (case-insensitive) or `{"type":
       "string","x-pg-type":"bytea"}` → `{"type":"string","format":"buffer"}`
       (imports as `buffer()`); the `bytea` signal keyword is removed.
     - `{..., "x-pg-type":"json"|"jsonb"}` (on a string node) → `{"format":"json"}`
       (J2 imports `format:"json"` as `json()` regardless of `type`,
       `src/json_schema.rs:715`); the `x-pg-type` signal keyword is removed.
     - `{"type":"string","format":"timestamp without time zone"|"timestamp with
       time zone"|"timestamptz"}` → set `format` to `"date-time"` (stays a
       `string`; imports with a `date-time` `format` modifier).
     - `{"type":"number","format":"int64"}` or `{"type":"number","x-pg-type":
       "int8"}` → `{"type":"string"}` (imports as `string()`); the `int64`/`int8`
       signal keyword is removed.
     - `{"type":"number","format":"numeric"|"decimal"}` or `{"type":"number",
       "x-pg-type":"numeric"}` → `{"type":"string"}`; the signal keyword removed.

     Each rewrite preserves all sibling keywords on the node except the consumed pg
     signal keyword (so annotations, `description`, etc. carry through; numeric
     bounds left on a node rewritten to `type:"string"` are harmlessly ignored by
     J2's string arm). Because this runs as a **pre-parse** `deep_map_schema` walk,
     it reaches nodes inside `$defs`, `prefixItems`, and `additionalProperties`
     uniformly — the importer then produces the correct `Schema` with correct
     modifiers, with no post-parse tree rebuilding required.

- **Why the built-in `sql` policy uses schema transforms, not a type transform.**
  J2's `from_json_schema` returns a `lazy` wrapper for **every** `#/$defs/...`
  `$ref` (`src/json_schema.rs::ref_schema`), not only cyclic ones. A post-parse
  type transform walking the `Schema` tree would therefore have to either (a)
  treat `Lazy` as a leaf — which silently skips the pg-type substitutions inside
  every reusable `$defs` entry — or (b) force-resolve each `lazy`, transform, and
  re-wrap — which breaks the shared/cyclic identity that makes a cyclic schema
  re-export to a single `$defs` entry. Both are wrong. Performing the
  substitutions as **pre-parse schema transforms** on the JSON `Value` sidesteps
  the problem entirely: `$defs` are plain objects there and `deep_map_schema`
  recurses them, so reusable and cyclic definitions are rewritten correctly before
  J2 ever builds a `lazy`. This is the Reviewer-endorsed resolution and is more
  correct than Zex's post-parse `makeSqlTypeTransform`. The built-in `sql` policy's
  `type_transforms` vector is therefore **empty**; the post-parse type-transform
  *mechanism* (`TypeTransform`, `apply_type_transforms`, registry support) is still
  delivered and exercised by caller-supplied transforms (Verification 2-3).

- **Type transforms apply to the root only (no built-in tree walker).** Because
  the built-in `sql` policy needs no type transform, P1 ships **no** crate-internal
  `Schema`-tree walker and adds **no** public `Schema` introspection accessors
  (YAGNI — the concept ships exactly one built-in policy and defers further
  policies out-of-tree). `apply_type_transforms` folds each `TypeTransform` over
  the root `Schema` (Zex parity); a caller-supplied type transform receives the
  root and may rebuild it via the public constructor/builder surface, but the
  public API does not expose reading a `Schema`'s kind or children, so v1 type
  transforms are limited to whole-`Schema` rewrites. Richer introspection is a
  future concern if a real need arises. *(Recorded as an accepted limitation in
  `policy.md`.)*

- **Risk — global mutable registry across threads.** Mitigated by `Mutex` with
  poison recovery; the registry is keyed by `String` and holds cheaply-cloneable
  `Policy` values (Arc-bumped vectors), cloned out under the lock and invoked
  after the lock is released so a long-running transform does not hold the lock.

## Steps

### 1. Module + policy `ErrorCode`s (`src/policy.rs`, `src/lib.rs`)

Create `src/policy.rs`; add `mod policy;` to `src/lib.rs`. Declare the policy
error code(s) in a `src/policy.rs` `impl ErrorCode` block (mirroring
`src/schema.rs:14-19`; no edit to `src/error.rs`):

```rust
impl ErrorCode {
    /// `from_json_schema_with` was given a policy name with no registered policy.
    pub const POLICY_UNKNOWN: ErrorCode = ErrorCode::new("policy_unknown");
}
```

`POLICY_UNKNOWN` MUST carry a message naming the unknown policy. (Schema-transform
and type-transform failures surface whatever `ZerxError` the transform/resolver
returns; the deref resolver and J2's import codes are reused as-is.)

### 2. Public types (`src/policy.rs`)

Define `SchemaTransform`, `TypeTransform`, `RefResolver` (the three
`Arc<dyn Fn… + Send + Sync>` aliases from Assumptions), `Policy`
(`#[derive(Default, Clone)]`, public `schema_transforms` / `type_transforms`
vectors), and `ImportOptions` (`#[derive(Default)]`, public `policy` /
`schema_transforms` / `type_transforms` / `deref` fields).

### 3. Registry (`src/policy.rs`)

- `fn registry() -> &'static Mutex<HashMap<String, Policy>>` backed by a
  `static OnceLock<…>`, initialised via `get_or_init` with the built-in `sql`
  policy seeded (Step 6). Lock access MUST recover from poison
  (`.lock().unwrap_or_else(|e| e.into_inner())`).
- `pub fn register_policy(name: impl Into<String>, policy: Policy)` — inserts
  (overwriting any existing entry).
- An internal `fn lookup_policy(name: &str) -> Option<Policy>` cloning the entry
  out under the lock.

### 4. `deep_map_schema` + schema transforms + deref (`src/policy.rs`)

- `deep_map_schema(value, mapper)` per Assumptions — recurse **all** child-schema
  positions: `properties`, `$defs`, `items` (schema or array), `prefixItems`,
  `additionalProperties` (when a schema object), and `anyOf`/`oneOf`/`allOf`.
- The three built-in `sql` schema transforms (nullable normalisation, array
  `items` fallback, pg-type substitution) per Assumptions "Built-in `sql` schema
  transforms", as functions returning `SchemaTransform` (or free `fn`s wrapped in
  `Arc`), each implemented over `deep_map_schema`.
- `fn make_deref_transform(resolver: RefResolver) -> SchemaTransform`: a
  `deep_map_schema` walk that, for any node with a `$ref` string **not** starting
  with `#/` (i.e. non-local), replaces the node with the resolver result **merged
  with the `$ref` node's sibling keywords** (every key except `$ref`, siblings
  winning) per Assumptions "Deref hook semantics and sibling keywords"; local
  `#/$defs/…` refs are left untouched for J2's core importer.

### 5. Composer + `apply_type_transforms` (`src/policy.rs`)

- `pub fn apply_type_transforms(schema: Schema, transforms: &[TypeTransform]) ->
  Result<Schema, ZerxError>` — the trivial fold.
- `pub fn from_json_schema_with(value: &serde_json::Value, opts: &ImportOptions)
  -> Result<Schema, ZerxError>` — the composition of Assumptions step 1-4:
  resolve policy (`POLICY_UNKNOWN` on miss), apply ordered schema transforms to
  get the effective `Value`, call `crate::json_schema::from_json_schema(&effective)`,
  then `apply_type_transforms` with the ordered type-transform list.

### 6. Built-in `sql` policy (`src/policy.rs`)

- `fn builtin_sql_policy() -> Policy` assembling the three schema transforms from
  Step 4 into `schema_transforms` and leaving `type_transforms` **empty** (per
  Assumptions "Why the built-in `sql` policy uses schema transforms, not a type
  transform"). No `Schema`-tree walker is written.
- The `registry()` initialiser seeds `"sql"` via `builtin_sql_policy()`.

### 7. Re-exports (`src/lib.rs`)

Add a `policy` re-export line:

```rust
pub use policy::{register_policy, from_json_schema_with, apply_type_transforms,
    Policy, ImportOptions, SchemaTransform, TypeTransform, RefResolver};
```

### 8. Tests (`src/policy.rs`, `#[cfg(test)]`)

See Verification.

### 9. Doc Update (after build + tests pass — Hard Rule 12)

- `docs/architecture/policy.md` — fill the skeleton:
  - **Consumes from**: `json-schema: from_json_schema (core import walk the
    pipeline wraps)`; `schema-core: Schema (the value the pipeline produces and
    type transforms receive)`; `errors: ZerxError (structured pipeline failure)`.
    *(No `types` edge: the built-in `sql` policy performs its substitutions as
    pre-parse JSON-`Value` rewrites, so the `policy` module calls no `types`
    constructor — the importer does. Adding a `types` edge would be a false
    dependency.)*
  - **External Contracts**: `register_policy`, `from_json_schema_with`,
    `apply_type_transforms` (name + one-line purpose each; no signatures per the
    `architecture` rule's forbidden-content list — describe, don't sign).
  - **Architecturally Significant Dependencies**: `serde_json` (the JSON `Value`
    schema transforms operate on).
  - **Constraints** capturing: the four-stage pipeline order; the
    schema-transform ordering (deref → policy → caller) and type-transform
    ordering (policy → caller); `apply_type_transforms` as a non-recursive
    root-only fold; `from_json_schema_with` reusing the public `from_json_schema`
    unchanged; unknown policy name → `POLICY_UNKNOWN` (fail-closed); the global
    registry seeded with `sql`; `register_policy` infallibility; `deep_map_schema`
    recursing all child-schema positions; the deref hook acting only on non-local
    `$ref` and merging siblings onto the resolved node; the `sql` schema transforms
    (nullable normalisation, array `items` fallback, and the pg-type substitutions
    bytea→buffer, json/jsonb→json, int64/int8→string, numeric/decimal→string,
    timestamp→`date-time` — all performed **pre-parse** so they reach `$defs` and
    nested positions); the built-in `sql` policy carrying **no** type transform;
    the one accepted limitation (caller-supplied type transforms are limited to
    whole-`Schema` rewrites in v1 — no public `Schema` introspection).
  - **Related Decisions**: keep the candidate-note form (no `D-` slugs
    mid-concept, per the concept's "Decision-reference discipline") — note that
    `from_json_schema_with`/`apply_type_transforms` return `Result` (candidate C7)
    and the unknown-policy fail-closed posture aligns with candidate C4.
- Cross-concern edge duals (concept "Cross-concern edge discipline" — add the
  dual `Provides to: policy` endpoint on each counterpart):
  - `docs/architecture/json-schema.md` **Provides to**: add
    `policy: from_json_schema (core import walk the pipeline wraps)`.
  - `docs/architecture/schema-core.md` **Provides to**: add
    `policy: Schema (the value the pipeline produces and type transforms receive)`.
  - `docs/architecture/errors.md` **Provides to**: add
    `policy: ZerxError (structured pipeline failure)`.
- `docs/architecture/_overview.md` already carries the `policy → json-schema`
  edge; no overview change is required.
- No `decisions.md` change (lifecycle discipline — register migration is Concept
  Closeout). No new `severity: accepted` bug card. No version bump is prescribed
  by this plan; continuing the per-plan minor-bump pattern is a Product-Owner /
  Archive-step decision at commit time.

## Verification

Run from the repo root:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --features mlua    # P1 adds no gated code; must still build/pass
```

All must pass with no warnings.

**Test isolation for the global registry.** The registry is a process-global
`OnceLock<Mutex<HashMap<String, Policy>>>` and `cargo test` runs tests in
parallel within the binary. Every test that calls `register_policy` MUST use a
**unique policy name** (e.g. derive it from the test function name, such as
`"test_fold_ordering"`), never a bare `"custom"`, so concurrent tests cannot
overwrite one another's entries. Tests MUST NOT register a policy named `"sql"`
(the built-in) and MUST NOT rely on the absence of a name another test might
register. Read-only tests of the built-in `sql` policy are inherently safe (the
seed is never mutated).

The `src/policy.rs` test module MUST cover at least:

1. **Registry + unknown policy.** `from_json_schema_with(&v, &ImportOptions {
   policy: Some("nope".into()), ..Default::default() })` returns
   `Err` with `code == POLICY_UNKNOWN`. The built-in `"sql"` policy resolves
   without any prior `register_policy` call.
2. **`register_policy` round-trip.** Register a custom policy whose single type
   transform replaces the root with `string()`; `from_json_schema_with` with that
   policy name yields a `string`-kind schema (assert via re-export shape).
3. **`apply_type_transforms` fold + ordering.** Two type transforms applied to a
   root schema run in list order (assert by a transform that appends a `meta` key
   and one that reads it / by re-export shape); an error from a transform
   propagates.
4. **deref hook + sibling merge.** An input schema with a non-local `$ref` (e.g.
   `{"$ref":"https://x/y"}`) plus a `deref` resolver that returns
   `{"type":"string"}` imports to `string`. A non-local `$ref` node carrying a
   sibling `description` (`{"$ref":"https://x/y","description":"d"}`) imports so
   the `description` survives (re-export shows `"description":"d"`), proving the
   sibling merge. A local `{"$ref":"#/$defs/S1", …}` is left to the core importer
   (deref does **not** rewrite it). A resolver returning `Err` propagates.
5. **sql schema transform — nullable normalisation.**
   `{"type":["string","null"]}` (array type) imports under `policy:"sql"` to
   `string().nullable()` (re-exports `anyOf:[{type:string},{type:null}]`); a
   `{"oneOf":[{"type":"number"},{"type":"null"}]}` likewise collapses to
   `number().nullable()`.
6. **sql schema transform — array items fallback.** `{"type":"array"}` (no
   `items`) imports under `policy:"sql"` to `array(any())` (whereas the bare
   `from_json_schema` of the same input returns `IMPORT_MALFORMED` — assert both,
   proving the transform is what fixes it).
7. **sql type substitution — bytea→buffer.**
   `{"type":"string","format":"bytea"}` under `policy:"sql"` → `buffer()`
   (re-exports `{"type":"string","format":"buffer"}`); the same via
   `{"type":"string","x-pg-type":"bytea"}`.
8. **sql type substitution — json/jsonb.**
   `{"type":"string","x-pg-type":"jsonb"}` → `json()` (re-exports
   `{"format":"json"}`).
9. **sql type substitution — int64 / numeric → string.**
   `{"type":"number","format":"int64"}` and `{"type":"number","x-pg-type":"int8"}`
   → `string()`; `{"type":"number","format":"numeric"}` → `string()`.
10. **sql type substitution — timestamp → date-time.**
    `{"type":"string","format":"timestamptz"}` → `string` with re-exported
    `format:"date-time"`.
11. **sql nested recursion + modifier preservation.** An object with a `required`
    bytea property and an **optional** int64 property imports under `policy:"sql"`
    so the bytea field becomes a `buffer` (required, re-export shows
    `format:"buffer"` with no stray `format:"bytea"`) and the int64 field becomes
    an optional `string` (optionality preserved); object `strict` mode preserved.
    (Modifiers are preserved because the substitution happens pre-parse and the
    importer builds the node with its keywords — no post-parse carrier copying.)
12. **sql leaves discriminated union intact.** A `oneOf`+discriminator import
    whose variant objects contain an int64 field imports under `policy:"sql"` to a
    discriminated union that still validates (the discriminator field unchanged,
    the int64 field now `string`); re-export still carries `oneOf`+`discriminator`.
13. **End-to-end determinism.** A schema-transform error (e.g. a deref resolver
    error) and a type-transform error each surface as a `ZerxError` from
    `from_json_schema_with` (no panic, no silent drop).
14. **sql substitution reaches `$defs` (the lazy/reusable-def case).** A schema
    `{"$ref":"#/$defs/Row","$defs":{"Row":{"type":"object","properties":{"data":
    {"type":"string","format":"bytea"}},"required":["data"],
    "additionalProperties":false}}}` imported under `policy:"sql"` produces a
    `Schema` whose `$defs/Row` `data` field is a `buffer` (re-export of the result
    shows the `$defs/Row` `data` as `{"type":"string","format":"buffer"}`). This
    is the decisive proof that the pg-type substitution reaches a reusable `$defs`
    entry — the case a post-parse `Lazy`-leaf type transform would have missed.
15. **`deep_map_schema` reaches tuple and record positions.** A schema transform
    (use the built-in `sql` pg-type substitution) applied to a `{"type":"array",
    "prefixItems":[{"type":"string","format":"bytea"}],"items":false}` imports so
    the tuple member is a `buffer`; applied to a `{"type":"object",
    "format":"record","additionalProperties":{"type":"string","format":"bytea"}}`
    imports so the record value schema is a `buffer`. Proves `deep_map_schema`
    recurses `prefixItems` and `additionalProperties`, not only `properties`/
    `items`/`anyOf`.
