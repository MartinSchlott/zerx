# PLAN_VM2: Bound raw JSON document depth at the import boundary

## Context & Goal

Follow-up to PLAN_VM1_value_depth_bound (3.2.1), commissioned by DevLead (supervisor, 2026-11-21,
under the Product Owner's mandate) for zerx 3.2.2. 3.2.1 bounds `ZerxValue` construction, but the
import path handles the caller's raw `serde_json::Value` — cloning, comparing and walking it —
before any depth guard. `serde_json`'s text parser stops at 128 levels; a programmatically built
`Value` does not.

### Reproduction (zerx 3.2.1, debug build, 2 MB thread)

A scratch probe outside the repository built each document iteratively and `mem::forget`-ed it
after the call, so only zerx's import path was measured. Depth = nesting of the deep part.

| Document | Entry | 100 | 1 000 | 10 000 |
|---|---|---|---|---|
| unused `$defs: {X: {const: <deep array>}}` | `from_json_schema`, `from_json_schema_with` | ok | ok | **abort** |
| `$ref` to a `$defs` entry with a deep `const` | `from_json_schema` | ok | refused | **abort** |
| `enum: [<deep array>]` | `from_json_schema` | ok | refused | **abort** |
| deep `default`, deep `x-foo` annotation | `from_json_schema` | ok | refused | refused |
| `items` chain (array of array of …) | `from_json_schema` | refused | refused | refused |
| `items` chain | `from_json_schema_with` (default options) | refused | refused | **abort** |
| `properties` chain | `from_json_schema` | refused | refused | refused |
| `properties` chain | `from_json_schema_with` (default options) | refused | **abort** | **abort** |

Causes, from the code:

- `ImportContext::new` (`src/json_schema.rs`) clones the whole root `$defs` map with `.cloned()`;
  `ref_schema` clones a definition node again before importing it.
- The `enum`/`const` import step clones raw member values (`const_val.clone()`, the `enum` array
  clone) before conversion.
- `from_json_schema_with` (`src/policy.rs`) starts with `value.clone()` of the whole document, and
  the policy walks (`deep_map_schema`, `map_child_schemas`, `deref_walk`, the built-in
  transforms) clone and recurse over it once per schema level. None of these is depth-guarded;
  the import walk's own `MAX_PARSE_DEPTH` nesting guard runs only later, in
  `from_json_schema_inner`.
- `deref_walk` inlines resolver output, so the document handed to the core importer can be deeper
  than the caller's input.

**Goal:** no import entry point clones, compares or walks a raw document nested deeper than a fixed
document bound. Every raw `serde_json::Value` is measured iteratively (no recursion, no clone)
where it enters zerx — the caller's document, every schema transform's output, every deref
resolver result — and a document over the bound is refused with `IMPORT_UNSUPPORTED`, the code
`docs/architecture/json-schema.md` already prescribes for a too-deep document. The deref walk
counts its own recursion against the same bound because inlining grows the document.

The document bound is `4 * MAX_PARSE_DEPTH` (400) JSON levels. It is chosen so that no document
3.2.1 imports is newly refused. In any document 3.2.1 imports successfully, every JSON level the
import uses is one of two kinds:

- a schema position counted by the import walk: at most `MAX_PARSE_DEPTH` walk levels, and at most
  two JSON levels per walk level (`properties`/`$defs`/`anyOf`/`prefixItems` → container →
  schema). This holds for transformed documents too: the `sql` policy's nullable rewrite adds an
  `anyOf` wrap (two JSON levels) but the core import pays one walk level for it, because the
  nullable collapse imports the core node as a further walk level;
- a `const`/`default`/`enum`/annotation member, capped at `MAX_PARSE_DEPTH` containers by the 3.2.1
  value bound.

So every used part of a 3.2.1-imported document sits under ≈ 1 + 2·100 + 1 + 100 ≈ 302 JSON levels,
through every path (plain, deref-inlined, `sql`-transformed). What is newly refused is exactly a
document whose **unused** parts — e.g. a never-walked `$defs` entry, or content under a keyword the
import ignores — push it past 400.

## Breaking Changes

Behaviour narrowing, no API change, no new error code: a JSON Schema document (or schema transform
output, or deref resolver result) nested more than 400 JSON levels deep is refused with
`IMPORT_UNSUPPORTED` before any other import work. Such documents could only be built
programmatically (serde_json text parsing stops at 128) and, where they imported before, did so
only because the deep part was unused. Patch release 3.2.1 → 3.2.2.

## Reference Patterns

- `import_depth_error` and the nesting guard in `import_value` (`src/json_schema.rs`).
- The `active.len() >= MAX_PARSE_DEPTH` cut in `deref_walk` (`src/policy.rs`).
- The 3.2.1 iterative depth helper `value_depth` (`src/value.rs`) for the counting style.
- `deep_json_arrays` test helper in `src/json_schema.rs`.

## Dependencies

None.

## Steps

1. **Failing tests first** (Verification). Deep documents are built iteratively from
   `serde_json::Map`s outside the measured call, the call runs on a spawned 2 MB-stack thread, and
   the caller's input (and any output holding deep parts) is `mem::forget`-ed, so each test proves
   zerx's import path, not the drop of the fixture. Confirm each test for a path the reproduction
   table shows aborting (or a transform/resolver case) aborts or fails on the current code
   (isolated run), then passes after the fix; the 100 000-deep `items`/`properties` chains through
   `from_json_schema` are already refused today and serve as regression guards.

2. **Iterative JSON depth** (`src/json_schema.rs`, crate-private, used by `src/policy.rs` too):
   `fn json_depth_exceeds(value: &serde_json::Value, limit: usize) -> bool` with an explicit work
   stack of `(&Value, depth)`; every array or object counts one level, scalars none; it returns as
   soon as the limit is exceeded (no full walk of a huge document). Constant
   `MAX_IMPORT_DOCUMENT_DEPTH: usize = 4 * MAX_PARSE_DEPTH`, crate-private, with an explanatory
   comment stating the derivation in *Goal*.

3. **Entry guards.**
   - `from_json_schema_inner` (reached by `from_json_schema` and by `from_json_schema_with` after
     its transforms): check the document first, before `ImportContext::new`; over the bound return
     `ZerxError::new(ErrorCode::IMPORT_UNSUPPORTED, "schema document nesting exceeds the import document depth limit of 400")`
     (format the constant), path empty.
   - `from_json_schema_with` (`src/policy.rs`): check the caller's document before the initial
     `value.clone()`; check the output of the deref transform and of every policy and caller
     schema transform before it is handed on, with the same error.
   - `deref_walk`: check every resolver result immediately after the resolver returns — before
     `has_document_local_refs`, which walks it recursively — and before merging it; and carry the walk's JSON
     position depth (one per recursive call along schema positions, plus the levels of the
     container holding the child) so that a walk deeper than the bound returns the same error
     instead of recursing. The existing `active.len()` ref-chain cut stays.

4. **Clones.** With the entry guard in place every remaining `.clone()`/`.cloned()` of raw
   `Value` on the import and policy paths operates on a bounded document. Leave them unless a clone
   is plainly redundant and removable without changing a signature (e.g. reading `$defs` by
   reference inside `ImportContext` is welcome but not required). List in the closing report every
   raw-`Value` clone, comparison and recursive walk on these paths you checked, with why it is now
   bounded.

5. **Existing test change.** `import_deep_const_and_default_refused` (`src/json_schema.rs`)
   asserts `PARSE_DEPTH_EXCEEDED` for a 100 000-deep `const`; the entry guard now refuses that
   document first, so update that one assertion to expect `IMPORT_UNSUPPORTED`. Its `default` arm,
   which already accepts both codes, stays unchanged.

   **No other code change.** No new public item, no change to `ImportOptions`, `RefResolver`,
   `SchemaTransform` signatures.

6. **Version.** Not the Coder's: the Planner bumps to 3.2.2 at finalization.

## Documentation

Updated by the Planner at finalization, after verification:

- `docs/architecture/json-schema.md`, import depth rules: every import entry MUST refuse a raw
  document nested deeper than four times `MAX_PARSE_DEPTH` JSON levels with `IMPORT_UNSUPPORTED`
  before cloning, comparing or walking it. Basis: the existing load-bearing import depth invariant
  in the same section ("The bound MUST take effect before the import exhausts a 2 MB thread
  stack") and the authorized stability goal.
- `docs/architecture/policy.md`: the caller's document, every schema-transform output and every
  deref resolver result MUST pass the same document bound, and the deref walk MUST count its
  recursion against it. Basis: the same invariant, applied at the policy boundary that runs
  before the core import.
- `README.md`: one sentence in the stability paragraph that import refuses a JSON Schema document
  nested more than 400 JSON levels deep with `import_unsupported`.
- `docs/bug.kanban.md`: no card change expected (the accepted caller-held-value card stays as is);
  card any finding the Coder reports.
- No register entry proposed or superseded.

## Verification

Required gate: `scripts/verify all`.

Tests (existing `mod tests` blocks; 2 MB thread, fixtures iteratively built from Maps outside the
call and forgotten):

`src/json_schema.rs`:
- unused `$defs: {X: {const: <10 000-deep array>}}` → `from_json_schema` returns
  `IMPORT_UNSUPPORTED`, no overflow;
- `$ref` to such a definition, and `enum: [<10 000-deep array>]` → `IMPORT_UNSUPPORTED`;
- 100 000-deep `items` chain and `properties` chain → `IMPORT_UNSUPPORTED`;
- boundary: a document of exactly 400 JSON levels reaches the existing import logic (e.g. an
  unused `$defs` entry with a 397-deep array under an unknown annotation key — root 1 + `$defs`
  2 + entry 3 + array levels 4–400 — imports, or fails with an existing non-depth error), and the
  same with a 398-deep array (401 levels) is refused with the document-depth error;
- a 100-level `properties` chain (≈200 JSON levels, the deepest schema nesting) still yields the
  existing import nesting error, unchanged; existing `import_depth_is_bounded` passes unchanged.

`src/policy.rs`:
- `from_json_schema_with` with default options, with the `sql` policy, and with a deref resolver,
  on the 10 000-deep unused-`$defs` document and on 100 000-deep `items`/`properties` chains →
  `IMPORT_UNSUPPORTED`, no overflow;
- a caller schema transform that returns a 10 000-deep document → `IMPORT_UNSUPPORTED`;
- a deref resolver returning a 10 000-deep object → `IMPORT_UNSUPPORTED`;
- a chain of external refs whose inlined documents each stay under the bound but together exceed
  it → `IMPORT_UNSUPPORTED`, no overflow;
- a 199-level `properties` chain (just under the bound) through `from_json_schema_with` with deref
  and `sql` policy runs to its existing result on the 2 MB thread (stack-safety at the bound).

Final: `scripts/verify all` exits 0. No acceptance check needs agent judgment.
