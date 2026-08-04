# PLAN_J3: Caller-side strip policy for unknown properties on import

## Context & Goal

A host validates a persisted config file against an application-supplied JSON Schema. When a
field is removed from that schema, every stored file that still carries the removed key becomes
invalid and the application refuses to boot. The file cannot be repaired through the
application's own settings UI either, because the UI validates before it writes. Stripping turns
this into self-healing: the stale key is dropped during validation, and the next save writes the
file without it.

Today that outcome is unreachable for an imported schema:

- `import_object` (`src/json_schema.rs:955-964`) sets `passthrough()` for
  `additionalProperties: true` and for a schema-object value; every other shape falls through to
  the `object()` constructor default `ObjectMode::Strict`. No input shape produces
  `ObjectMode::Strip`.
- Pre-parse `SchemaTransform`s operate on the JSON `Value`, and `Strip` is not expressible in
  JSON Schema — `json-schema.md` already records that `Strict` and `Strip` both export
  `additionalProperties: false`.
- Post-parse `TypeTransform`s receive `Fn(Schema) -> Result<Schema, ZerxError>`, are folded
  root-only (`src/policy.rs:377-386`), and `SchemaKind`/`ObjectBody.mode` are `pub(crate)`.
  `policy.md` records this as an accepted limitation.
- `ObjectSchema::strip()` is public (`src/types.rs:1093`) but `ObjectSchema(Schema)` is a newtype
  with a private field and no `From<Schema>`/`TryFrom<Schema>`/`as_object`.

**Goal:** make the disposition of unknown properties a caller-side option on
`ImportOptions`, so an imported schema can strip instead of reject — without adding a
zerx-specific keyword to the JSON Schema document, and without opening a public `Schema`
introspection surface.

Whether an unknown property is rejected or removed is caller policy, not schema content.
`additionalProperties: false` correctly means "unknown properties do not belong here" in both
cases. The established precedent is AJV's `removeAdditional` option, which pairs with
`additionalProperties: false` exactly this way.

### Design decisions taken during discussion

These were resolved with the Product Owner before this plan was written. They are recorded here
so the implementation does not re-open them.

1. **Bare field on `ImportOptions`, not a named registry policy.** A `Policy` is two vectors of
   transforms (`src/policy.rs:37-40`); neither kind can express the mode, so a policy-based
   answer would need a third `Policy` field plus a merge rule between policy-level and
   caller-level values. Decisive argument: `ImportOptions.policy` is `Option<String>` — exactly
   one policy — so a `strip` policy would exclude the built-in `sql` policy. An orthogonal field
   composes with every policy.
2. **`additionalProperties: true` (and a schema-object value) stay untouched.** The field governs
   what happens to a property the schema *rejects*. An explicit `true` rejects nothing, so there
   is nothing to remove; deleting those keys would be data loss, not self-healing. This matches
   AJV, where `removeAdditional: true` removes only what `additionalProperties` disallows.
3. **Recursive — every reconstructed object node, not only the root.** A config document has
   nested sections, and a stale key in a nested section blocks the boot exactly like a root-level
   one. A root-only policy would not solve the stated problem.
4. **`additionalProperties: false` and absent `additionalProperties` are treated alike.** Both
   import as `Strict` today (`json-schema.md`, fail-closed as a security boundary) and are
   indistinguishable after import. Carrying "was it explicit" through the walk buys nothing,
   because the runtime meaning is identical.
5. **Implemented inside the import walk, not as a post-import `Schema`-tree rewrite.** A
   recursive rewrite over the finished tree would have to write into the shared resolution cell
   of `lazy` nodes — a cycle problem taken on for no reason. `import_object` builds every object
   node exactly once, so `$defs` and cyclic schemas fall out correctly for free.
6. **`bool`, not an enum.** A third import-time disposition (keep unknown keys) is not requested;
   YAGNI.

### Prior art check (Zex)

Zex, zerx's TypeScript sibling, does **not** solve this. `fromJsonSchema`
(`json-schema-import.ts:668-677`) carries the same options as `ImportOptions` and no strip
switch; its import walk sets the mode only from `additionalProperties`. In TypeScript the gap is
reachable anyway (`ZexObject.shape` is `public` and `instanceof ZexObject` works, so a recursive
`typeTransform` can call `.strip()` on every node), which is why it never hurt there. There is no
Zex design to port; the precedent is AJV.

Two Zex divergences found during that check are **explicitly out of scope** for this plan and must
not be "fixed" here:

- Zex maps absent `additionalProperties` on a property-less object to `passthrough`; zerx maps it
  to `strict`. Zerx's behaviour stands.
- Zex's `strip()` exports `additionalProperties: true`; zerx exports `false`. Zerx's behaviour is
  the correct one and stands.

## Breaking Changes

**Yes.**

`ImportOptions` (`src/policy.rs:44-53`) is a plain public struct with all-public fields and no
`#[non_exhaustive]`. Adding a field breaks any downstream caller that constructs it with a
complete struct literal without `..Default::default()`. Under semver that is a major bump for a
crate at `1.0.0`.

- **What breaks:** downstream code of the form
  `ImportOptions { policy: …, schema_transforms: …, type_transforms: …, deref: … }` without
  `..Default::default()` no longer compiles.
- **What the Product Owner must do:** nothing in this repository. The version moves to `2.0.0`
  (Step 7). Downstream callers add `..Default::default()` to their struct literal, or set the
  new field explicitly.
- **Recovery:** none needed — a compile error with an obvious fix, no data or state migration.
- **No runtime behaviour changes for existing callers.** The new field defaults to `false`, which
  reproduces today's import behaviour bit for bit.

`#[non_exhaustive]` is deliberately **not** added: it would forbid struct-literal construction
from outside the crate entirely, including functional-update syntax, which is a worse ergonomic
regression than the one-time break.

## Dependencies

None. No new crates, no feature flags, no build changes.

## Reference Patterns

- `src/json_schema.rs:414-417` — the current public import entry point and `ImportContext`
  construction.
- `src/json_schema.rs:445-465` — `ImportContext` definition and its private `new`.
- `src/json_schema.rs:925-967` — `import_object`, the single place a regular object node is
  reconstructed.
- `src/policy.rs:389-426` — `from_json_schema_with` and its six-step pipeline.
- `src/json_schema.rs:1478-1527` — test block `J2-9` (object `required` / `additionalProperties`
  / default), the closest existing test style for the new import tests.
- `src/policy.rs:451-500` — policy test style (`// N. <topic>` comment headers,
  `test_<subject>_<expectation>` names).

## Assumptions & Risks

- **`import_object` is the only construction site for `SchemaKind::Object` during import.**
  Verified: `record` and `jsonschema` format markers construct different kinds, and the
  discriminated-union path reconstructs its variants through `import_value`, which reaches
  `import_object`. If the implementation finds another site, it must be covered too.
- **Mode does not affect discriminator construction.** `build_discriminator_state`
  (`src/types.rs:1254`) inspects the variant's kind and its discriminant field, not its mode. A
  discriminated union whose variants strip therefore still builds. Test `J3-6` pins this.
- **Union disambiguation changes under strip — accepted and documented.** `parse_union`
  (`src/types.rs:888-901`) is first-match-wins. In strict mode a union variant is rejected by an
  extra key; under strip the first variant matches and the extra keys are dropped. This is
  inherent to strip and the caller opted in, but it MUST be stated in `policy.md` and in the
  `ImportOptions` field doc comment. Test `J3-5` pins the behaviour so it cannot change silently.
- **Roundtrip stays asymmetric by design.** Import with `strip_unknown: true` then export yields
  `additionalProperties: false`; re-importing without the option yields `Strict` again. The
  policy is caller-side and is not carried by the document, so it MUST be re-supplied on every
  import. This MUST be stated in the field doc comment and in `json-schema.md`.
- **`record` is unaffected.** A record's extra keys are validated against its value schema; it has
  no `ObjectMode`. `record` with `additionalProperties: false` continues to raise
  `IMPORT_MALFORMED`. Test `J3-7` pins this.

## Steps

### Step 1 — thread the disposition through `ImportContext`

In `src/json_schema.rs`:

- Add a field to `ImportContext` (`:445-450`):
  `strip_unknown: bool`.
- Change the private constructor `ImportContext::new(root: &serde_json::Value)` (`:453`) to
  `ImportContext::new(root: &serde_json::Value, strip_unknown: bool)` and store the flag. It has
  exactly one call site (`:415`), so no compatibility shim is needed.

### Step 2 — add the crate-internal import entry point

In `src/json_schema.rs`, immediately after the existing `from_json_schema` (`:414-417`):

- Add `pub(crate) fn from_json_schema_inner(value: &serde_json::Value, strip_unknown: bool) -> Result<Schema, ZerxError>` containing the current two-line body, with the flag passed to
  `ImportContext::new`.
- Rewrite the public `from_json_schema(value)` to delegate: `from_json_schema_inner(value, false)`.
  Its signature, visibility, and doc comment stay exactly as they are — it remains the documented
  public entry point with today's behaviour.

### Step 3 — apply the disposition in `import_object`

In `src/json_schema.rs:956-964`, replace the `additionalProperties` match with:

- `Some(Value::Bool(true))` → `ob = ob.passthrough()` (unchanged).
- `Some(Value::Object(_))` → `ob = ob.passthrough()` (unchanged).
- everything else (`Some(Value::Bool(false))`, absent, any other value) → `ob = ob.strip()` when
  `ctx.strip_unknown` is set; otherwise leave the constructor default `Strict` (unchanged).

Add a short comment naming the rule: the flag reinterprets only the strict outcome; an explicit
passthrough stays passthrough.

### Step 4 — add the field to `ImportOptions`

In `src/policy.rs:44-53`, add to `ImportOptions`:

```rust
/// Disposition of unknown properties for every imported object node.
///
/// `false` (default): a property the schema rejects yields `unknown_property` at validation
/// time. `true`: it is silently dropped instead.
///
/// Applies to every reconstructed object node, not only the root. Nodes with
/// `additionalProperties: true` or a schema-object value import as passthrough and are NOT
/// affected — the flag reinterprets only the strict outcome.
///
/// This is caller policy and is not carried by the JSON Schema document: a schema imported
/// with `strip_unknown: true` still exports `additionalProperties: false`, so the flag must be
/// supplied again on every import.
///
/// Note: `union` variants are matched first-match-wins. Under `strip_unknown: true` a variant
/// that strict mode would have rejected for an extra key can match, and the extra keys are
/// dropped.
pub strip_unknown: bool,
```

The `#[derive(Default)]` on `ImportOptions` covers the default; do not hand-write a `Default`
impl.

### Step 5 — wire the pipeline

In `src/policy.rs`, step 3 of `from_json_schema_with` (`:417-418`): replace the
`crate::json_schema::from_json_schema(&effective)` call with
`crate::json_schema::from_json_schema_inner(&effective, opts.strip_unknown)`.

The pipeline order is otherwise unchanged: deref → policy schema transforms → caller schema
transforms → core import → policy type transforms → caller type transforms.

### Step 6 — tests

Add to `src/json_schema.rs`'s test module a block headed `// J3-1 … J3-9.` following the existing
`J2-N` comment style. These call `from_json_schema_inner` directly. Test function names MUST be
prefixed `j3_` (e.g. `fn j3_strip_root_object()`) so the whole block is selectable with
`cargo test j3_`.

- **J3-1 — strip applies at the root.** Import
  `{"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}`
  with `strip_unknown: true`; validate `{"a":"x","stale":1}`; assert `Ok` and that the result
  object contains only `a`. The same import with `strip_unknown: false` MUST return
  `Err` with `ErrorCode::UNKNOWN_PROPERTY`.
- **J3-2 — strip applies to nested objects.** A root object with a nested object property, both
  `additionalProperties: false`; validate a value carrying a stale key inside the nested object;
  assert `Ok` and that the stale key is gone. This is the regression guard for the recursive
  decision.
- **J3-3 — absent `additionalProperties` also strips.** Same as J3-1 but with the keyword omitted
  entirely; assert the stale key is dropped.
- **J3-4 — `additionalProperties: true` is untouched.** Import with `strip_unknown: true`;
  validate a value with an extra key; assert `Ok` and that the extra key **survives**. Repeat for
  `additionalProperties: {"type":"string"}` (also passthrough).
- **J3-5 — union first-match-wins under strip.** A `anyOf` of two object variants that differ by
  one extra field, both `additionalProperties: false`. With `strip_unknown: true`, validate a
  value matching the second variant's shape; assert the documented behaviour (first variant
  matches, extra keys dropped). This test pins the accepted consequence; it is not a bug report.
- **J3-6 — discriminated union still builds under strip.** Import a `oneOf` with
  `discriminator.propertyName`; assert the import succeeds with `strip_unknown: true` and that
  validation dispatches on the discriminant and drops an unknown key on the selected variant.
- **J3-7 — `record` is unaffected.** `{"type":"object","format":"record","additionalProperties":{"type":"string"}}`
  with `strip_unknown: true` still imports as a record and still validates arbitrary string-valued
  keys; `additionalProperties: false` on a record still raises `IMPORT_MALFORMED`.
- **J3-8 — `$ref`/`$defs` reached through a lazy node strip too.** A schema whose root references
  `#/$defs/S1`, where `S1` is a strict object; with `strip_unknown: true`, validate a value with
  a stale key and assert it is dropped. This is the regression guard for decision 5.
- **J3-9 — export is unchanged.** Import with `strip_unknown: true` and re-export; assert
  `additionalProperties` is `false`, i.e. the option does not leak into the document.

Add to `src/policy.rs`'s test module, following the `// N. <topic>` style, a block
`// 13. strip_unknown`:

- **`test_strip_unknown_default_is_reject`** — `from_json_schema_with` with
  `..Default::default()` on a strict object rejects an unknown key.
- **`test_strip_unknown_via_import_options`** — the same schema with
  `strip_unknown: true` drops it.
- **`test_strip_unknown_composes_with_sql_policy`** — `policy: Some("sql".into())` **and**
  `strip_unknown: true` on a schema that exercises one `sql` substitution (e.g. a
  `{"type":"string","format":"bytea"}` property) plus a stale key: assert both the substitution
  and the stripping took effect. This is the regression guard for decision 1.
- **`test_strip_unknown_heals_nested_config`** — the driving use case, end to end, as a permanent
  test. Import
  `{"type":"object","properties":{"host":{"type":"string"},"tls":{"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"],"additionalProperties":false}},"required":["host","tls"],"additionalProperties":false}`
  through `from_json_schema_with` with `strip_unknown: true`; validate
  `{"host":"a","legacy_port":1,"tls":{"enabled":true,"legacy_ca":"x"}}`; assert `Ok` and that the
  returned `ZerxValue` contains neither `legacy_port` nor `legacy_ca` — i.e. re-serialising it
  writes the healed config. Distinct from J3-1/J3-2: those exercise the crate-internal
  `from_json_schema_inner` at one nesting level each, this one exercises the **public**
  `from_json_schema_with` entry point and pins that a root-level and a nested stale key disappear
  in the **same** pass. It is a permanent test in the committed diff, not a temporary artifact.

### Step 7 — version bump

Per Breaking Changes, the crate moves to `2.0.0`. Three places record the version and MUST agree:

- `Cargo.toml`: `version = "1.0.0"` → `"2.0.0"`.
- `Cargo.lock`: the `[[package]] name = "zerx"` entry currently reads `version = "1.0.0"`
  (`Cargo.lock:134-135`). It is regenerated by the build in Verification item A1 and verified in
  item A6; commit the resulting lockfile change. Do not hand-edit it.
- `README.md`: the two version claims — the status line (`README.md:5`, "**Status:** v1.0.0 …")
  and the LLM Reference paragraph (`README.md:188`, "(v1.0.0)"). These are documentation and are
  therefore updated in Step 9, after verification, together with the rest of the README edits.

### Step 8 — pre-documentation verification gate

Run **Verification group A** below — build, clippy, full suite, targeted new tests, regression
guard, the acceptance test, the `Cargo.toml`/`Cargo.lock` version check, and semver sanity — and
confirm every item in that group passes.

Group A is the complete verification of everything Steps 1–7 produce. Group B is not part of this
gate: it asserts a property of the README that Step 9 has not created yet.

Hard Rule 12: no documentation changes happen before group A is green. If any item fails, fix the
implementation and re-run; if the plan itself turns out to be wrong, stop and report
(Hard Rule 7).

### Step 9 — documentation

Only after Verification group A is green (Hard Rule 12).

- **`docs/architecture/policy.md`**
  - `Consumes from`: change the `json-schema` entry to name `from_json_schema_inner` (core import
    walk carrying the caller's unknown-property disposition).
  - `External Contracts`: extend the `from_json_schema_with` line to mention that options carry
    the unknown-property disposition.
  - New `Constraints` subsection **Unknown-property disposition on import**:
    - The caller-supplied disposition MUST apply to every object node the import walk
      reconstructs, not only the root.
    - A node importing as `passthrough` (`additionalProperties: true` or a schema-object value)
      MUST NOT be affected; the disposition reinterprets only the strict outcome.
    - `additionalProperties: false` and an absent `additionalProperties` MUST be treated
      identically.
    - The disposition MUST default to rejection; the default import behaviour MUST be unchanged.
    - The disposition MUST NOT be encoded into the exported document; it MUST be re-supplied on
      every import.
    - Union variants are matched first-match-wins; under the strip disposition a variant that
      strict mode would reject for an extra key MAY match.
- **`docs/architecture/json-schema.md`**
  - `Provides to`: add `policy`: `from_json_schema_inner` as the dual of the `policy.md`
    `Consumes from` entry (the bidirectional contract requires the pair).
  - `additionalProperties` import shapes (regular object): add that when the caller-supplied
    disposition is strip, the `false` and absent cases MUST reconstruct as `strip` mode instead
    of `strict`; `true` and schema-object remain `passthrough`.
  - Object mode → `additionalProperties` mapping: extend the existing `Strip` limitation note to
    state that an import-time strip disposition is likewise not representable and MUST be
    re-supplied per import.
- **`README.md`**
  - The policy paragraph (`:200`) — add `strip_unknown` to the `ImportOptions` field list with a
    one-clause description.
  - The JSON Schema paragraph (`:198`) — mention the caller-side disposition.
  - The strict-by-default paragraph (`:43`) — add the import option as the third explicit
    relaxation channel, alongside `.passthrough()` and `.strip()`.
  - The status line (`:5`) — `**Status:** v1.0.0` → `v2.0.0`.
  - The LLM Reference paragraph (`:188`) — `(v1.0.0)` → `(v2.0.0)`.
- **`docs/definition.md`** — no change. The `Strict validation` feature line reads "unknown object
  properties are rejected by default; relaxing is explicit"; the import option is another explicit
  relaxation and the statement stays true.
- **`docs/bug.kanban.md`** — no new `severity: accepted` card. The union first-match-wins
  consequence is a documented property of strip mode, pinned by test J3-5 and stated in
  `policy.md`, not a deviation from the target vision.

### Step 10 — post-documentation check

Run **Verification group B** below and confirm it passes. It asserts that no stale version
reference survives in the README after Step 9. If it fails, the fix belongs in Step 9's edits, not
in the implementation.

### Step 11 — archive and commit

Move this plan to `docs/archive/` and commit all changes from this session.

## Proposed Decisions

Candidate for promotion into `docs/decisions.md` at Doc Update. To be confirmed or rejected during
Plan Review; the Coder MUST NOT promote it without that confirmation.

### `D-import-unknown-caller-policy`

**Decision:** The disposition of unknown properties on JSON Schema import is a caller-side option
in `ImportOptions`, not a schema keyword, vendor extension, or named registry policy.
**Rationale:** JSON Schema is a validation vocabulary, not a transform vocabulary;
`additionalProperties: false` says unknown properties do not belong, and whether that means reject
or remove is the caller's choice — encoding it in the document would make a portable schema carry
zerx-specific meaning.
**Consequence:** Import-time behaviour that is not a statement about validity MUST enter through
`ImportOptions` and MUST NOT be introduced as an `x-`-prefixed keyword; such an option MUST NOT be
encoded into the exported document and MUST be re-supplied on every import; it stays orthogonal to
`policy` so it composes with any registered policy.
**Scope:** docs/architecture/json-schema.md, docs/architecture/policy.md

Admission tests: the Choice/Independence test passes — the rejected alternative (a
`x-zerx-strip`-style keyword, or a registry policy) is a live option that no higher-ranking
decision excludes, and the choice survives a rewrite. The Altitude test passes — a future plan
adding any further import-time behavioural knob would otherwise plausibly reach for a document
keyword and be wrong.

## Verification

All commands run from the repository root. The section is split into two groups that run at
different points in the Steps sequence:

- **Group A (items A1–A7)** runs at Step 8, before any documentation change. It verifies
  everything Steps 1–7 produce.
- **Group B (item B1)** runs at Step 10, after the documentation change. It verifies a property
  Step 9 creates and therefore MUST NOT be part of the Step 8 gate.

### Group A — pre-documentation gate (Step 8)

A1. **Build and lint**
   ```
   cargo build --all-features
   cargo clippy --all-features -- -D warnings
   ```
   Both MUST pass with no warnings.

A2. **Full test suite**
   ```
   cargo test --all-features
   ```
   All existing tests MUST pass unchanged. In particular the `J2-*` import tests and the
   `test_sql_*` policy tests MUST NOT need edits — if any existing test needs a change, the
   default-path behaviour was altered and the implementation is wrong (Hard Rule 7: stop and
   report).

A3. **Targeted new tests**
   ```
   cargo test --all-features j3_
   cargo test --all-features strip_unknown
   ```
   All new tests from Step 6 MUST pass.

A4. **Regression guard — default behaviour is bit-for-bit unchanged**
   ```
   cargo test --all-features import
   ```
   Confirms the public `from_json_schema` path is untouched.

A5. **Acceptance criterion — the driving use case end to end**
   ```
   cargo test --all-features test_strip_unknown_heals_nested_config
   ```
   The permanent test added in Step 6 MUST pass. It is the acceptance criterion for this plan:
   a root-level and a nested stale key both disappear in one pass, through the public
   `from_json_schema_with` entry point. No temporary verification artifact is created at any
   point; every test written for this plan is permanent and belongs in the committed diff.

A6. **Crate release metadata is consistent**
   ```
   grep -n '^version' Cargo.toml
   grep -A1 'name = "zerx"' Cargo.lock
   ```
   `Cargo.toml` MUST report `2.0.0`, and the `zerx` entry in `Cargo.lock` MUST report `2.0.0` —
   the lockfile has been regenerated by the A1 build. This item covers the crate metadata only;
   the README version claims are Step 9 documentation and are checked in group B.

A7. **Semver sanity.** The crate's own test suite (which uses `..Default::default()` throughout)
   compiles unchanged, confirming the break is limited to full-literal downstream construction.

### Group B — post-documentation check (Step 10)

B1. **No stale version reference survives in the README**
   ```
   grep -n 'v1\.0\.0' README.md
   ```
   MUST return no hits. Both claims — the status line and the LLM Reference paragraph — were
   rewritten to `v2.0.0` in Step 9. This item asserts a property Step 9 creates and therefore
   MUST NOT be evaluated as part of the Step 8 gate.
