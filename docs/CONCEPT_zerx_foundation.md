# Concept: Zerx Foundation

## Purpose & framing

This concept takes Zerx from the vision stage to a working v1. It resolves the
three open design questions from `docs/vision.md` into named candidate
decisions, locks the vision's already-committed positions into candidate form so
they migrate cleanly into `docs/decisions.md` at closeout, and sequences the
work into plans that each leave the system in a coherent state.

The vision's scope is treated as **battle-tested, not a menu to trim** (it is a
port of Zex, where every feature earned its place through pain). The question
this concept answers is *how* to do it well in Rust — not *whether*.

Code snippets below are **illustrative, not normative**: plans determine the
exact code. The permanent contracts land in `docs/architecture/<concern>.md` at
Concept Closeout.

## Candidate decisions

Candidates are numbered C1–C9 for in-concept reference. Each is stated in
*We do X, not Y* form so migration into `docs/decisions.md` is mechanical. The
proposed slug ID for each is given in parentheses.

### C1 — Modifier composition: hybrid enum-core + typed builders (`D-modifier-composition`)

**Decision.** We represent a schema as a single concrete `Schema` value — an
`enum SchemaKind` over the type variants plus a shared carrier for universal
modifiers and validators — fronted by typed builder structs that expose
type-specific methods. We do **not** use a pure `Box<dyn SchemaType>`
trait-object tree, and we do **not** use a pure enum without typed builders.

**Rationale.** Programmatic assembly (`object([...])`, `omit`, `partial`,
`extend`) requires one uniform, cloneable value type, which forces type erasure;
an enum gives cheap exhaustive pattern-matching for JSON Schema export; the
typed-builder front gives the compile-time guarantee the vision demands —
`.min()` on a boolean must not be callable.

**Consequences.**
- Universal modifiers (`optional`, `nullable`, `default`, `describe`, `refine`,
  `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`,
  `example`, `title`) live on the shared carrier and are exposed on every builder
  via a blanket trait, each returning the builder type so chaining is preserved.
- Type-specific validators (`min`, `max`, `regex`, `int`, `multiline`, `email`,
  `uuid`, …) are inherent methods on the relevant builder only, hence
  unrepresentable on builders where they make no sense.
- Builders convert into `Schema` via `Into<Schema>`; constructors that accept
  children (`object`, `array`, `tuple`, `union`, …) accept `Into<Schema>`.
- `Schema` is `Clone`; immutability is clone-and-return (Zex's `clone()`),
  forcing modifiers to be cheap to clone.
- Whether the blanket trait is hand-written or macro-generated is an
  implementation detail for `PLAN_F2_schema_core`, not a concept decision.
- `SchemaKind` is a closed enum owned by `schema-core` as a *container*; each
  type variant (its tag + payload) is owned and added by the type plan that
  introduces it. `PLAN_F2_schema_core` seeds only the variants it owns;
  `PLAN_T1_basic_types` / `PLAN_T2_complex_types` / `PLAN_T4_special_types`
  extend the enum with their variants. `schema-core` exposes a single per-kind
  dispatch seam in the parse flow so a new variant adds one delegating arm, never
  new parse-flow control logic. No plan may leave `todo!()` dispatch arms — every
  plan leaves a coherent state.
- The `Validator` trait contract (its signature plus the carrier's
  `Vec<Box<dyn Validator>>` storage field) is defined in `schema-core` by
  `PLAN_F2_schema_core`, because the carrier must name the trait to store it. The
  validator *system* and every concrete validator are owned by `types`
  (`PLAN_T1_basic_types`). At closeout, `schema-core.md` records the validator
  storage contract and `types.md` the validator system and concretes.

Illustrative shape (not normative):

```rust
// erased, cloneable value — what object([...]) stores and omit/partial/extend operate on
pub struct Schema { kind: SchemaKind, modifiers: Modifiers, validators: Vec<Box<dyn Validator>> }
enum SchemaKind { String, Number, Boolean, Object(ObjectBody), Array(Box<Schema>), /* … */ }

// typed front — .min() exists here, never on a bool builder
pub struct StringSchema(Schema);
impl StringSchema { pub fn min(self, n: usize) -> Self { /* … */ } }
impl From<StringSchema> for Schema { /* … */ }

// universal modifiers on every builder via blanket trait
pub trait Modify: Sized { fn optional(self) -> Self; fn describe(self, s: &str) -> Self; /* … */ }
```

### C2 — Buffer fidelity via serde bytes, no coercion (`D-buffer-fidelity`)

**Decision.** We carry bytes as a first-class `ZerxValue` bytes variant fed by
serde's native `serialize_bytes` path and stored internally as `Vec<u8>`. We do
**not** coerce JSON-style number arrays into buffers, and zerx does **not** take
a dependency on `serde_bytes`.

**Rationale.** Serde's data model already includes bytes; only the buffer source
must emit `serialize_bytes`, which zerx receives through the native hook without
any helper crate. Silently coercing `[u8]` arrays would mask data-shape bugs and
undermine strict-by-default.

**Consequence.** `validate<T: Serialize>` receives bytes through serde's
`serialize_bytes` hook; no `serde_bytes` dependency is added to zerx. Callers who
need a `Vec<u8>` field to arrive as bytes rather than a number array must emit it
through serde's bytes path themselves (e.g. the `serde_bytes` crate / `ByteBuf`
on their own types) — a documented caller-side contract surfaced in the README
and the `value-model` concern, not a zerx dependency. A `buffer` exports as
`format: "buffer"` with `contentMediaType` for MIME.

### C3 — Schema-directed Lua validation, zerx-owned value type (`D-lua-schema-directed`)

**Decision.** zerx validates Lua data through a **schema-directed** path: given a
`Schema` and a raw Lua value, `validate_lua` walks both together and the schema
resolves every Lua ambiguity — table → array vs object/record, byte string →
string vs buffer — producing a disambiguated `ZerxValue`. zerx **owns** the Lua
input type (`zerx::lua::LuaValue` + `LuaTable { array, hash }`) behind a `lua`
feature. We do **not** depend on `mlua` or any external Lua crate, and `ZerxValue`
does **not** carry a host-opaque variant.

**Rationale.** The Lua sibling runtime (endymion) exposes only data at its host
boundary — `nil`/`bool`/`int`/`float`/`bytes`/`table`; functions, coroutines, and
userdata are not representable there. A blind `LuaValue → ZerxValue` map would
have to guess array-vs-object and string-vs-buffer; the schema already carries
that information, so validation and disambiguation are a single pass. Owning the
Lua value type in zerx (rather than mirroring it per consumer) gives users of both
zerx and the runtime one shared type with no conversion.

**Consequence.** `ZerxValue` stays purely serde-bridgeable — no host-opaque layer,
no `function`/`tvalue` types, no `mlua` feature. `zerx::lua::LuaValue` is an owned,
acyclic Rust tree, so `validate_lua` needs no data-value cycle guard (the depth
guard still applies; the producer delivers acyclic data). The `lua` feature adds
no external dependency. Consumers (e.g. endymion) own the Lua *runtime* types (VM,
host commands, CBOR serialisation) and adopt `zerx::lua::LuaValue` as their data
boundary; the dependency runs consumer → zerx, never the reverse.

### C4 — Strict by default (`D-strict-by-default`)

**Decision.** Object validation rejects unknown properties by default
(`unknown_property`); relaxing is explicit via `.passthrough()` or `.strip()`.
We do **not** default to permissive object validation.

**Rationale.** This is a security boundary: it catches typos and rejects
unexpected/injected fields at the validation edge.

**Consequence.** The default is never silently relaxed. Three modes
(`strict`/`passthrough`/`strip`) govern unknown keys; mode setters keep the
runtime mode and the exported `additionalProperties` in sync (avoiding the Zex
roundtrip footgun recorded in Zex's `bug.kanban.md`).

### C5 — `ZerxValue` spans the full serde data model (`D-serde-value-model`)

**Decision.** `ZerxValue` is a serde value over serde's *full* data model
(including bytes). We do **not** model on `serde_json::Value` (which is serde
minus bytes), and we do **not** add a host-opaque layer.

**Rationale.** The "bastard" is exactly what serde models but JSON throws away;
buffers must be first-class without first being made JSON-clean.

**Consequence.** `ZerxValue` is a single serde-bridgeable layer with full
roundtrip through any serde format; it is the umbrella over C2 (bytes). The Lua
input type (C3) is a separate, feature-gated value type, not a `ZerxValue` layer;
`validate_lua` produces an ordinary serde-bridgeable `ZerxValue`.

### C6 — No type inference, no type generation, no derive macro (`D-no-type-inference`)

**Decision.** Zerx validates dynamic data and does **not** derive schemas from
Rust types, infer compile-time types from schemas, or act as a derive-macro
field validator. We cede that ground to `schemars`/`typify` and `validator`/`garde`.

**Rationale.** Zerx's consumers are AIs and fully-tested code validating data
whose shape is unknown at compile time; there is nothing for inference to give.

**Consequence.** The public API exposes runtime values, not generic type
parameters representing the validated shape. `validate` returns `ZerxValue`, not
an inferred `T`.

### C7 — Single `Result`-returning API, no throwing or dual parse (`D-result-only-api`)

**Decision.** Every fallible operation returns `Result<_, ZerxError>`. We do
**not** provide a throwing variant, and we do **not** mirror Zex's
`parse`/`safeParse` split.

**Rationale.** Rust's `Result` + `?` collapses Zex's dual API into one idiomatic
path.

**Consequence.** `validate`, `validate_lua`, `from_json_schema`, `parse_delta`,
and `replace` all return `Result`. `to_json_schema` is infallible.

### C8 — English-only, machine-readable errors (`D-english-only-errors`)

**Decision.** `ZerxError` messages are English-only and consumers branch on
`code` and `path`, not message text. We do **not** localise messages.

**Rationale.** Errors are machine-readable artifacts that cross process and tool
boundaries; i18n adds cost without serving the consumer.

**Consequence.** `ZerxError` is serialisable (`to_json` / `Serialize`); union
failures aggregate per-variant errors into a combined error. The `received` and
`expected` fields are plain serialisable descriptor strings — `expected`
describes the schema's requirement, `received` a compact description (a type tag,
not the raw value) of what arrived. They do **not** embed a `ZerxValue`: keeping
the payload as descriptor strings avoids coupling the error model to `value-model`
and keeps `PLAN_F3_error_model` self-contained with no dependency on
`PLAN_F1_value_model`.

### C9 — Built on serde, lean beyond it (`D-serde-foundation`)

**Decision.** Zerx is built on `serde` + `serde_json` + `regex` (full `regex`
crate, 1.x) as core dependencies. We do **not** pursue Zex's
zero-runtime-dependency posture, and we add no convenience crates without need —
in particular zerx does **not** depend on `serde_bytes` (buffer fidelity rides on
serde's native `serialize_bytes`, see C2) nor on `mlua` or any external Lua crate
(the `lua` feature adds only a zerx-owned value type and the `validate_lua` path,
see C3). `regex` is scoped to the `regex`/`pattern` validator only; `regex-lite`
was rejected in favour of the full crate's performance and feature set
(Product-Owner decision).

**Rationale.** The serde data model *is* the premise of the library; reusing it
is the whole point.

**Consequence.** Dependency additions beyond this set are design-time decisions
(Hard Rule 11), surfaced in a plan's Dependencies section, not introduced ad hoc
during implementation.

## Scope boundaries

**In scope (this concept):**

- The full type catalogue, modifier set, and validators from the vision.
- Strict/passthrough/strip modes and object utilities (`partial`, `extend`,
  `omit*`, `strip*`).
- Bidirectional JSON Schema roundtrip with format markers, `$defs`/`$ref`, and
  cycles.
- Delta/Replace (JSON Pointer based).
- The policy pipeline with the built-in `sql` policy.
- Schema-directed Lua validation (`validate_lua`) with a zerx-owned Lua value type
  behind a `lua` feature (C3).

**Out of scope (deferred or ceded):**

- Type generation / compile-time inference / derive macros (ceded — C6).
- Live Lua handles (functions, coroutines, userdata) — not representable at the
  Lua data boundary; owned by the runtime, not zerx (C3). No `mlua` dependency.
- Async validation, streaming validation, and non-serde wire formats beyond what
  serde already provides.
- Additional built-in policies beyond `sql` (e.g. OpenAPI) — captured as backlog
  at closeout; `register_policy` keeps them out-of-tree.
- Performance tuning beyond the algorithmic choices already named (O(1)
  discriminator/key lookup); micro-optimisation is post-v1.

## Architecture (illustrative)

The concern topology is fixed in `docs/architecture/_overview.md`: `value-model`,
`schema-core`, `types`, `json-schema`, `policy`, `errors`, `lua`.

The spine is C1: `schema-core` defines the erased `Schema` value and the
typed-builder front; `types` adds variants and validators on top; `value-model`
defines what validation produces; `json-schema` reads/reconstructs `Schema`;
`policy` wraps the import walk; `errors` is produced by the parse flow; `lua`
provides the zerx-owned Lua value type and the schema-directed `validate_lua` path.

Parse flow (ported from Zex, faithful ordering): circular-reference check →
depth limit (`MAX_PARSE_DEPTH = 100`) → default application → optional/nullable
handling (default precedes `null` on a missing value) → type check → validators
→ type-specific logic. Missing optional properties are omitted from output, never
emitted as `null`.

## Plan breakdown

Plans execute in phase order; within a phase, listed dependencies hold. Each plan
fills the constraints of its named `architecture/<concern>.md` during its Doc
Update step.

**Phase 1 — Foundation** (unblocks everything)

- `PLAN_F3_error_model` — `ZerxError` (`path`, `code`, `message`, `inner_errors`,
  plus `received`/`expected` as serialisable descriptor strings per C8),
  `Serialize`/`to_json`, combined-error aggregation. Fills `errors.md`. Deps: none.
  **Done.**
- `PLAN_F1_value_model` — `ZerxValue` over the full serde data model incl. bytes;
  serde bridge (`Serialize` in, serialise out); gated host-opaque placeholder
  variant (later removed by `PLAN_L1_lua_validate` — C3 revised); no `serde_bytes`
  dependency (bytes via native `serialize_bytes`, per C2). Fills `value-model.md`.
  Deps: none.
  **Done.**
- `PLAN_F2_schema_core` — the `Schema` representation (enum core + carrier +
  typed builders + blanket modifier trait), the parse flow (depth/cycle guard,
  default/optional/nullable ordering), `lazy` with reentrance guard, `Clone`
  immutability, the `Validator` trait contract + carrier storage field (concretes
  in T1). Seeds `SchemaKind` with `lazy` and `any` (the carrier-only proof type)
  and proves the chaining/guard mechanism with those two; the negative
  compile-guarantee and type-specific-method composition land in T1. Fills
  `schema-core.md`. Deps: F3, F1.
  **Done.**

**Phase 2 — Type catalogue** (deps: Phase 1)

- `PLAN_T1_basic_types` — `string`, `number`, `boolean`, `enumerate`, `null`
  (`any` is already seeded by F2); their builders and inherent validators (`min`,
  `max`, `regex`, `int`, `multiline`, `email`, `uuid`) as concrete implementations
  of the `Validator` trait F2 defines. Lands the negative compile-guarantee
  (`.min()` unrepresentable on `bool`) and the type-specific-method composition.
  Extends `SchemaKind` with its variants and their dispatch arms. Deps: F2.
  **Done.**
- `PLAN_T2_complex_types` — `object`, `array`, `record`, `tuple`, `union`,
  `discriminated_union` (O(1) variant lookup), `literal`; object modes
  (strict/passthrough/strip). Deps: T1.
  **Done.**
- `PLAN_T3_object_utilities` — `partial`, `extend`, `omit`,
  `omit_read_only`/`omit_write_only` (shape changes) and `strip_only`,
  `strip_read_only`/`strip_write_only` (runtime-only). Deps: T2.
  **Done.**
- `PLAN_T4_special_types` — `buffer` (with MIME, per C2), `uri`, `url`, `json`,
  `jsonschema`. Deps: T1.
  **Done.**

**Phase 3 — JSON Schema roundtrip** (deps: Phase 2)

- `PLAN_J1_export` — `to_json_schema` with format markers, `ExportContext`
  `$defs`/`$ref` tracking, Draft 2020-12 `discriminator`. Fills `json-schema.md`
  (export). Deps: T2, T4.
  **Done.**
- `PLAN_J2_import` — `from_json_schema` AST walk, `$ref` resolution with memoised
  lazy placeholders, all four `additionalProperties` shapes, `oneOf`→union with
  `x-oneOf`, `allOf`/`not` raise clear errors, default application on import.
  Completes `json-schema.md`. Deps: J1.
  **Done.**

**Phase 4 — Delta/Replace + Policy**

- `PLAN_D1_delta_replace` — `parse_delta(path, value)` and `replace(instance,
  path, value)` with full root revalidation incl. `refine`. `replace()` operates
  on the serde-bridgeable `ZerxValue` tree. Deps: Phase 2. **Done.**
- `PLAN_P1_policy_pipeline` — `register_policy`, schema transforms (pre-parse) and
  type transforms (post-parse), built-in `sql` policy, deref hook. Fills
  `policy.md`. Deps: J2. **Done.**

**Phase 5 — Lua validation (optional, `lua` feature)**

- `PLAN_L1_lua_validate` — the `lua` feature: the zerx-owned `lua::LuaValue` +
  `LuaTable { array, hash }` input types and `validate_lua(&lua::LuaValue) ->
  Result<ZerxValue, ZerxError>`, a schema-directed validate+parse where the schema
  node resolves each Lua ambiguity (table → array vs object/record; byte string →
  string vs buffer; non-string hash keys rejected under `object`/`record`).
  Removes the obsolete `mlua` feature and the uninhabited `HostOpaque` placeholder
  from `src/value.rs` and `Cargo.toml`. Creates a new `lua.md` concern and, at its
  Doc Update, reconciles the now-archived permanent docs that referenced
  host-opaque/mlua: `value-model.md` (drop the `HostOpaque` variant and its edge to
  `mlua`), `types.md` (drop the `function`/`tvalue` non-goal and the `host_opaque`
  type tag), `json-schema.md` (drop the `function`/`tvalue` format markers),
  `schema-core.md` (drop the "data-value cycle detection deferred to M1" note — the
  Lua input is acyclic by construction), `_overview.md` (`mlua` → `lua`), and
  `definition.md` (host-opaque/mlua wording → schema-directed Lua validation).
  Deps: Phase 2. **Done.**

## Risks

- **Modifier-composition ergonomics.** The blanket-trait-over-builders pattern
  (C1) may need a macro to avoid forwarding boilerplate, and `Into<Schema>`
  conversions must stay frictionless. Mitigation: `PLAN_F2_schema_core` prototypes
  the chaining end-to-end before Phase 2 builds on it.
- **serde_bytes caller contract (C2).** Bytes fidelity depends on the *caller*
  emitting `serialize_bytes`; a plain `Vec<u8>` arrives as a number array.
  Mitigation: document prominently and add tests asserting the failure mode is
  legible.
- **Lua mapping ambiguity (C3).** The schema-directed mapping must resolve
  byte-string → string vs buffer and table → array vs object/record, and reject
  non-string hash keys under `object`/`record`. Mitigation: drive every mapping
  branch from the schema node in `PLAN_L1_lua_validate`, error at the exact path,
  and port Zex's Lua-interop tests.
- **Roundtrip fidelity.** Discriminated unions nested in arrays and the
  `additionalProperties` four-shape handling were Zex bug sources. Mitigation:
  port Zex's regression tests in Phase 3.

## Affected docs (at Concept Closeout)

- `docs/decisions.md` — candidates C1–C9 migrate as slug-ID'd active entries.
- `docs/architecture/*.md` — the seven concern files (`value-model`, `schema-core`,
  `types`, `json-schema`, `policy`, `errors`, `lua`) gain their permanent
  constraints (filled incrementally per plan; reconciled at closeout).
- **host-opaque → Lua reconciliation (in `PLAN_L1_lua_validate`'s Doc Update).**
  The mlua/host-opaque design was dropped mid-concept (C3 revised). L1 removes the
  obsolete code (`mlua` feature, `HostOpaque` placeholder) and, atomically in its
  Doc Update, reconciles the archived permanent docs that referenced it
  (`value-model.md`, `types.md`, `json-schema.md`, `schema-core.md`, `_overview.md`,
  `definition.md`) and replaces the `mlua.md` concern with `lua.md`. The check
  stays green because code and docs change together in one reviewed plan.

**Decision-reference discipline during the lifecycle.** Candidates C1–C9 stay in
this concept and do **not** appear as `D-<slug>` references in any permanent doc
until they are promoted to `docs/decisions.md` **Active** at Concept Closeout.
While the concept is open, concern `Related Decisions` sections carry only
non-slug candidate notes (e.g. "candidate C8"); the `D-<slug>` lines are added at
Closeout when the entries go Active. This keeps the permanent docs consistent
with the `permanent-doc-consistency-check` M3 invariant (concern slug references
MUST point to Active entries) throughout the concept, not just at the end.

**Cross-concern edge discipline during the lifecycle.** Whenever a plan's Doc
Update adds a `Consumes from` or `Provides to` edge to one concern file, the same
Doc Update MUST add the dual endpoint to the counterpart concern file — even when
that counterpart is still an unfilled skeleton or belongs to a later plan.
Adding the dual is mechanical mirroring (no design), it does not pre-empt the
counterpart plan's Constraints, and it keeps the
`permanent-doc-consistency-check` M2 duality invariant green at every plan's
pre-review. A one-sided edge is drift, not a finding to defer.
- `docs/definition.md` — revisited if feature detail outgrows the single file
  (extract to `docs/features/<feature>.md` only if needed — YAGNI).
- `docs/backlog.kanban.md` — deferred scope (e.g. OpenAPI policy) captured as cards.
