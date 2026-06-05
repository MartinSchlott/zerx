# PLAN_D1_delta_replace

## Context & Goal

First plan of **Phase 4 — Delta/Replace + Policy** in the
`CONCEPT_zerx_foundation` execution. All of Phase 1
(`PLAN_F3_error_model`, `PLAN_F1_value_model`, `PLAN_F2_schema_core`), Phase 2
(`PLAN_T1_basic_types`, `PLAN_T2_complex_types`, `PLAN_T3_object_utilities`,
`PLAN_T4_special_types`), and Phase 3 (`PLAN_J1_export`, `PLAN_J2_import`) are
complete and archived. The full type catalogue, the parse flow, and the JSON
Schema roundtrip are in place. D1 has no JSON-schema dependency; it depends only
on Phase 2 (`docs/CONCEPT_zerx_foundation.md:321`).

D1 delivers the **Delta / Replace** feature named in the vision
(`docs/vision.md:148-152`, `docs/definition.md:30`) and scoped by the concept
(`docs/CONCEPT_zerx_foundation.md:226`):

- `Schema::parse_delta(path, &value)` — validate a value against the **sub-schema**
  located at a JSON Pointer `path` inside this schema. Schema-only navigation
  (no instance).
- `Schema::replace(&instance, path, &value)` — produce a new instance with the
  value at `path` replaced, then run **full root revalidation** (including
  `refine` and `default`) and return the revalidated `ZerxValue`.

Both are `Result`-returning, single-API methods (C7); both take `T: Serialize`
inputs and return `ZerxValue` (no type inference, C6); JSON Pointer is the
addressing scheme (`docs/vision.md:148`).

### Governing candidate decisions (`CONCEPT_zerx_foundation.md`)

- **C7 — single `Result`-returning API.** `parse_delta` and `replace` each
  return `Result<ZerxValue, ZerxError>`. No throwing variant, no `parse`/`safeParse`
  split (`docs/CONCEPT_zerx_foundation.md:169-179`). This contrasts with Zex,
  which exposes `parseDelta`/`safeParseDelta`/`_tryParseDelta` and
  `replace`/`safeReplace`/`_tryReplace` — zerx collapses each family to one
  method.
- **C3 — host-opaque is feature-gated and absent from the default build.** The
  `replace()` host-opaque contract is *fixed* by C3 and *realised* by
  `PLAN_M1_host_opaque`; in the default build no host-opaque value or schema kind
  exists, so D1 is complete on its own
  (`docs/CONCEPT_zerx_foundation.md:118-124`, `:321`). D1 MUST NOT implement any
  host-opaque handling. D1's public `replace` is **scoped to serde-bridgeable
  instances**: its `instance: &I where I: Serialize` bound makes a tree containing a
  host-opaque leaf *unrepresentable as an argument* even under the `mlua` feature
  (host-opaque values are not `Serialize`, `docs/architecture/value-model.md:42-45`).
  So in the default build *and* under `mlua`, the public `replace` never sees a
  host-opaque leaf — consistent with C3 (the replacement value is `Serialize` and the
  instance has no host-opaque leaves to descend into or revalidate). M1's C3
  additions — the `function`/`tvalue` `SchemaKind` navigation arms that error on a
  descend-into-host-opaque-leaf, and the by-kind revalidation of host-opaque leaves
  "elsewhere in the tree" — operate on a **`ZerxValue`-level instance** that can hold
  host-opaque values; M1 exposes that via its own host-opaque-capable surface (e.g. a
  `ZerxValue`/Lua-rooted replace entry), which is **additive** and does not change
  D1's signature. To make that extension cheap, D1 builds its navigation/rebuild on
  the internal `ZerxValue` primitives (Steps 3–4): the `parse_delta` schema walk is a
  closed per-kind dispatch over `SchemaKind`, and `replace`'s `set_at` is a closed
  per-kind dispatch over `ZerxValue`. Note the asymmetry between the two closed
  matches under `--features mlua`: `SchemaKind` has **no** host-opaque variant today
  (M1 *adds* `function`/`tvalue`), so its match is exhaustive in both builds and stays
  arm-free until M1; `ZerxValue` **already** has the gated `HostOpaque` variant
  (`src/value.rs:100-103`), so D1's `set_at` MUST carry a gated, statically-unreachable
  `HostOpaque` arm (`match *h {}`) now, purely for compile-completeness under `mlua`
  (Step 4). That placeholder adds no host-opaque handling; M1 replaces its body with
  the descend-into-leaf error once it inhabits the variant. So M1's work is: add
  `SchemaKind` arms, swap the `set_at` placeholder body, and add its public entry — all
  additive; it never reworks D1's logic.
- **C1 — clone-and-return immutability.** `replace` MUST NOT mutate the supplied
  instance; the rebuilt container tree is freshly cloned
  (`docs/CONCEPT_zerx_foundation.md:50`).

### Boundary and documentation placement

- **Code home.** All D1 code lives in a new module `src/delta.rs`. The two public
  methods are added to `Schema` via an `impl Schema` block in `src/delta.rs`
  (inherent impls are permitted in any module of the same crate). `src/lib.rs`
  gains `mod delta;`. No new free functions and no new public types are exported.
- **No new type, no new `SchemaKind` variant, no new dispatch arm in the parse
  flow.** D1 adds a *navigation* dispatch (a read-only walk over `SchemaKind`),
  not a parse-flow control path. `check_type`/`parse_inner`/`parse_present`/
  `parse_field` are untouched. `replace`'s revalidation reuses the existing
  `parse_present` entry point.
- **`src/schema.rs` is not edited** except that D1 reuses its `pub(crate)` items
  (`SchemaKind`, `Schema.{kind,modifiers,validators}`, `ParseContext::new`,
  `parse_present`, `Lazy::resolve`). All are already crate-visible.
- **`src/types.rs` is not edited.** Navigation reads the `pub(crate)` payloads
  `ObjectBody`, `DiscriminatedUnionBody` (fields `shape`, `key`, `variants`) that
  already exist; descent is trivial structural lookup, not type logic, so it stays
  in `delta.rs` rather than adding helpers to `types`.
- **Documentation target.** The concept assigns D1 no `Fills <concern>.md`
  (`docs/CONCEPT_zerx_foundation.md:318-321`). Delta/Replace is a public `Schema`
  operation whose `replace` revalidation *is* the parse flow and whose
  `parse_delta` navigation is a new per-kind seam over `SchemaKind` — both owned
  by the `schema-core` concern (`docs/architecture/schema-core.md:5-8`).
  Therefore D1's constraints are documented in `docs/architecture/schema-core.md`
  during Doc Update. This placement is a Planner decision open to the
  Reviewer/Architect; if they prefer a dedicated `docs/features/delta_replace.md`
  or a new concern file, that is a doc-only move with no code impact. No new
  `Consumes from`/`Provides to` edge is introduced (the navigation reads type
  payloads under the existing C1 container/variant arrangement, exactly as
  `parse_inner` already does), so the `permanent-doc-consistency-check` M2 duality
  invariant is unaffected.

### Scope decisions (plan-local, archived with this plan)

- **No deletion.** Zex's `replace` overloads `value === undefined` to *delete* an
  optional object property (`zex-base.ts:276-285`). zerx's signature takes
  `value: &T` where `T: Serialize`, which cannot express "absent", and C7 forbids
  a second API shape. Deletion is therefore **out of scope**: `replace` always
  *sets* a value. (A future `Schema::remove(instance, path)` can be added under a
  later plan if a real need surfaces — captured nowhere yet, intentionally, per
  YAGNI.)
- **`replace` is serde-bridgeable-only, permanently.** The `instance: &I where
  I: Serialize` bound scopes D1's public `replace` to instances with no host-opaque
  leaf, in the default build and under `mlua` alike (host-opaque is not `Serialize`,
  `docs/architecture/value-model.md:42-45`). M1 does **not** extend this method to
  host-opaque trees; it adds a separate `ZerxValue`-level replace surface (additive,
  non-breaking) that reuses D1's internal `set_at`/navigation. See the C3 bullet above.
- **Instance-only navigation for `replace`.** Zex threads both schema and instance
  through `replace` because it locally re-validates the leaf against the leaf
  sub-schema before rebuilding (`zex-base.ts:274-289`). zerx does **not** locally
  pre-validate: it locates the leaf by walking the *instance* alone, rebuilds, and
  relies on **full root revalidation** to enforce every schema rule (type,
  validators, `refine`, `default`, strict-unknown-key) with natural parse-flow
  paths. This is simpler (KISS), removes the schema/instance variant-selection
  duplication, and matches the concept's "full root revalidation incl. refine"
  exactly (`docs/CONCEPT_zerx_foundation.md:320`). `parse_delta`, having no
  instance, necessarily navigates the schema.
- **`replace` does not create absent positions.** An object segment whose key is
  absent from the instance, or an array/tuple index out of range, is an error
  (`MISSING_PARENT` / `INDEX_OUT_OF_RANGE`), not an insertion. Matches Zex
  (`zex-base.ts:519-524`, `:582-587`).
- **RFC 6901 JSON Pointer parsing.** zerx parses pointers per RFC 6901: the empty
  string addresses the root; any non-empty pointer MUST begin with `/`; segments
  are `/`-split; `~1`→`/` then `~0`→`~` un-escaping (in that order). This is
  stricter than Zex's lenient normaliser (which prepends a missing `/` and treats
  `"/"` as root); a non-empty pointer not starting with `/` is `INVALID_POINTER`,
  and `"/"` addresses the single empty-string key `""` (not the root).

## Breaking Changes

**No.** D1 is purely additive: two new methods on `Schema`, a new module, new
error-code constants, and a version bump. No existing signature, type, or
behaviour changes. No DB, env, or build-config changes.

## Reference Patterns

- `src/types.rs` — the per-kind dispatch style (`check_*`/`parse_*` functions,
  one arm per `SchemaKind` variant) that the navigation walk mirrors; the
  path-prefix-on-unwind convention (`e.path.insert(0, segment)`,
  `docs/architecture/types.md:97`).
- `src/schema.rs:166-224` — `SchemaKind::check_type`/`parse_inner` as the model
  for a closed match over every variant with no catch-all (so a future variant is
  a compile error until handled — the seam C3/M1 relies on).
- `src/schema.rs:117-137` — `Lazy::resolve` (memoised, reentrance-guarded); the
  navigation walk MUST resolve `Lazy` through this, never bypass it
  (`docs/vision.md:270-271`).
- `src/schema.rs:14-19`, `src/value.rs:17-19`, `src/types.rs` `impl ErrorCode`
  blocks — the open error-code-catalogue pattern: declare D1's codes in an
  `impl ErrorCode` block in `src/delta.rs`, next to the code that raises them
  (`docs/architecture/errors.md:45`).
- `src/error.rs:77-137` — `ZerxError` builder (`.at`, `.expected`, `.received`)
  used to attach `path`/descriptors.
- `docs/archive/PLAN_T2_complex_types.md`, `docs/archive/PLAN_T3_object_utilities.md`
  — plan structure and the test-table convention.

## Dependencies

None. No new packages. D1 uses only `serde` (already a dependency, for the
`T: Serialize` inputs via the existing `ZerxValue::from_serialize`) and the
existing internal modules. The declared dependency set in `Cargo.toml`
(`serde`, `serde_json`, `regex`) is unchanged.

## Assumptions & Risks

- **Assumption — payload visibility.** `ObjectBody.shape`, `DiscriminatedUnionBody.{key,variants}`,
  `SchemaKind`, and the `Schema` fields are `pub(crate)` (verified:
  `src/types.rs:600-630`, `src/schema.rs:143-265`). If any required field were
  private, D1 would add a `pub(crate)` accessor in the owning module — but none is
  needed.
- **Assumption — `parse_present` is the full-revalidation entry.** `replace`
  revalidates by calling `self.parse_present(&rebuilt, &mut ParseContext::new())`,
  which runs depth guard → nullable → type → validators → `parse_inner` → `refine`
  (`src/schema.rs:324-359`). This is exactly `validate`'s body minus the
  `from_serialize` step (`src/schema.rs:318-321`). Confirmed sufficient for
  "full root revalidation incl. refine".
- **Risk — lazy cycles during schema navigation.** A self-referential `lazy`
  chain that resolves without consuming a pointer segment is terminated by the
  existing reentrance guard (`LAZY_REENTRANCE`), so navigation cannot loop
  forever; finite pointers over memoised lazies terminate. No extra depth bound is
  added to navigation. Tests cover a recursive `lazy` object navigated to a finite
  depth.
- **Risk — error path fidelity.** Navigation errors must carry the consumed-prefix
  path; `replace` revalidation errors carry the natural parse-flow path. Tests
  assert both. Mitigation: the walk builds the consumed path incrementally and
  attaches it via `.at(...)`.
- **Risk — placement of D1 docs in `schema-core.md`** (see Boundary). Surfaced for
  Reviewer/Architect ratification; doc-only, no code impact.

## Steps

### Step 1 — Module scaffold and error codes

1. Create `src/delta.rs`. Add `mod delta;` to `src/lib.rs` (after `mod json_schema;`).
   No new `pub use` is required — the public surface is the two `Schema` methods.
2. In `src/delta.rs`, declare D1's error codes in a single `impl ErrorCode` block
   (open catalogue; no edit to `src/error.rs`):
   - `INVALID_POINTER` (`"invalid_pointer"`) — malformed JSON Pointer: a non-empty
     pointer not starting with `/`, or an array/tuple index segment that is not a
     non-negative decimal integer.
   - `INVALID_PATH` (`"invalid_path"`) — the pointer descends into a schema kind or
     instance value that cannot be traversed (a scalar/leaf, `any`, a literal, a
     buffer, etc.).
   - `INDEX_OUT_OF_RANGE` (`"index_out_of_range"`) — a tuple index (schema nav) or
     array/tuple index (instance nav) is outside the valid range.
   - `UNION_PATH_REQUIRES_INSTANCE` (`"union_path_requires_instance"`) —
     `parse_delta` cannot descend *into* a `union` or `discriminated_union`
     (variant selection needs an instance; use `replace`). Targeting the union
     node itself (pointer stops on it) is allowed.
   - `MISSING_PARENT` (`"missing_parent"`) — `replace`: an object-key segment is
     absent in the instance, so the position to replace does not exist. This covers
     **any** missing object key, intermediate or final (including a key declared
     `.optional()` in the schema but not present in the instance), because `replace`
     never creates absent positions — it sets existing positions only.
   - Reused existing codes (do not redeclare): `UNKNOWN_PROPERTY` (T2,
     `src/types.rs`) for an object segment whose key is not in `shape` during
     `parse_delta`; `INVALID_PATH`/`TYPE_MISMATCH` as noted.

### Step 2 — JSON Pointer parser

Implement `fn parse_pointer(path: &str) -> Result<Vec<String>, ZerxError>` (module-private):

- `path == ""` → `Ok(vec![])` (root).
- `path` not starting with `'/'` → `Err(INVALID_POINTER)` with an empty `path` and a
  message naming the offending pointer.
- Otherwise: strip the leading `'/'`, split the remainder on `'/'` (so `"/"` →
  `[""]`, `"/a/b"` → `["a","b"]`, `"/a/"` → `["a",""]`), and un-escape each segment
  by replacing `"~1"`→`"/"` **then** `"~0"`→`"~"` (this order is required by RFC
  6901; reversing it mis-decodes `~01`).
- Return the decoded segments as `Vec<String>`. Array/tuple-index validity is
  checked at descent time, not here (a segment may be a key in one container and an
  index in another).

Provide a helper `fn parse_index(seg: &str, consumed: &[String]) -> Result<usize, ZerxError>`
that accepts only `^[0-9]+$` (no sign, no leading-`+`, no whitespace), returning
`INVALID_POINTER` (with `consumed` as the error path) otherwise.

### Step 3 — `parse_delta`: schema-only navigation

Add `impl Schema { pub fn parse_delta<T: serde::Serialize + ?Sized>(&self, path: &str, value: &T) -> Result<ZerxValue, ZerxError> }`:

1. `let segments = parse_pointer(path)?;`
2. Walk to the target sub-schema with a module-private
   `fn navigate_schema<'a>(root: &'a Schema, segments: &[String]) -> Result<Schema, ZerxError>`.
   The walk keeps a `consumed: Vec<String>` for error paths and returns the target
   `Schema` (cloned — cheap per C1). For each segment it first resolves any `Lazy`
   (looping `Lazy::resolve` until the kind is non-lazy; the reentrance guard bounds
   this), then dispatches on `SchemaKind`:
   - `Object(body)` → find `seg` in `body.shape`; descend into that field schema, or
     `Err(UNKNOWN_PROPERTY)` (`expected = "property defined in schema"`,
     `received` = the key) at `consumed + [seg]`.
   - `Record(value_schema)` → descend into `*value_schema` for any `seg` (record keys
     are unconstrained).
   - `Array(item)` → `parse_index(seg, …)` (numeric required; not range-checked — an
     array schema has one item schema and no fixed length); descend into `*item`.
   - `Tuple(items)` → `parse_index`; if `idx >= items.len()` → `Err(INDEX_OUT_OF_RANGE)`;
     else descend into `items[idx]`.
   - `Union(_)` → `Err(UNION_PATH_REQUIRES_INSTANCE)` (cannot pick a variant without an
     instance).
   - `DiscriminatedUnion(_)` → `Err(UNION_PATH_REQUIRES_INSTANCE)` for **every** segment,
     uniform with `Union`. Each variant's discriminator field is its *own* `Literal`
     with a distinct const, and `types` only guarantees those consts are unique across
     variants (`docs/architecture/types.md:136-141`); they are not a single shared
     schema. A `/key` descent therefore has no single well-defined sub-schema —
     validating against any one variant's literal (e.g. `body.variants[0]`) would
     wrongly reject the valid discriminator values of every other variant. The valid
     discriminator set *is* schema-knowable (a union of the per-variant literals), but
     constructing that synthetic union for a niche, instance-free delta path is
     deferred (YAGNI); targeting a discriminator value uses `replace` with an instance.
   - `Any`, `String`, `Number`, `Boolean`, `Enum`, `Null`, `Literal`, `Buffer`, `Uri`,
     `Url`, `Json`, `JsonSchema` → `Err(INVALID_PATH)` (no descent possible) at
     `consumed + [seg]`.
   - The match is **closed** (no `_` arm). When M1 adds `function`/`tvalue` variants
     they become compile errors here until M1 supplies their arms (the C3 seam:
     descending into a host-opaque schema kind MUST error).
3. If `segments` is empty, the target is `self`.
4. Convert and validate: `let v = ZerxValue::from_serialize(value)?;` then
   `target.parse_present(&v, &mut ParseContext::new())`. Return its result directly
   (the validated delta value, or the sub-schema's `ZerxError`).

Note: the target sub-schema keeps its own modifiers, so validating against an
`.optional()` field via `parse_delta` validates the *present* supplied value
through `parse_present` (which honours `nullable`); `default`/optional-omission
semantics apply only to *missing* values and are out of scope for a method that
always receives a value.

### Step 4 — `replace`: instance navigation + rebuild + root revalidation

Add `impl Schema { pub fn replace<I, T>(&self, instance: &I, path: &str, value: &T) -> Result<ZerxValue, ZerxError> where I: serde::Serialize + ?Sized, T: serde::Serialize + ?Sized }`:

1. `let segments = parse_pointer(path)?;`
2. `let new_value = ZerxValue::from_serialize(value)?;`
3. **Root replacement.** If `segments` is empty, return
   `self.parse_present(&new_value, &mut ParseContext::new())` (validate the
   replacement as the whole document).
4. `let current = ZerxValue::from_serialize(instance)?;`
5. Rebuild immutably with a module-private recursive
   `fn set_at(value: &ZerxValue, segments: &[String], new: &ZerxValue, consumed: &[String]) -> Result<ZerxValue, ZerxError>`:
   - Base case `segments.is_empty()` → `Ok(new.clone())`.
   - Otherwise let `seg = &segments[0]`, `rest = &segments[1..]`, and dispatch on the
     **value** kind (schema-independent):
     - `ZerxValue::Object(map)` → the key `seg` MUST be present, else
       `Err(MISSING_PARENT)` (`expected = "existing parent key"`, `received` = the key)
       at `consumed + [seg]`. **This presence check applies to every segment, including
       the final one**: a `replace` whose last segment names an object key absent from
       the instance errors `MISSING_PARENT` — it does **not** insert the key (enforcing
       the "`replace` does not create absent positions" scope rule above; this holds
       even when the key is declared `.optional()` in the schema but simply not present
       in the instance). Recurse `set_at(child, rest, new, …)`; on `Ok(child2)` clone
       the map and overwrite `seg` in place (`Map::insert` preserves first-occurrence
       order, `src/value.rs:46-53`); return `ZerxValue::Object(new_map)`.
     - `ZerxValue::Array(items)` → `parse_index(seg, …)`; if `idx >= items.len()` →
       `Err(INDEX_OUT_OF_RANGE)`; recurse into `items[idx]`; clone the vec, set
       `[idx]`, return `ZerxValue::Array(new_vec)`.
     - Every remaining non-container scalar variant — `Null`, `Bool`, `I64`, `U64`,
       `I128`, `U128`, `F64`, `String`, `Bytes` — with segments still to consume →
       `Err(INVALID_PATH)` (cannot descend into a non-container) at `consumed + [seg]`.
       Enumerate these explicitly (no `_` catch-all) so the match stays **closed** and
       a future `ZerxValue` variant is a compile error here until handled — the seam
       M1 relies on.
     - `#[cfg(feature = "mlua")] ZerxValue::HostOpaque(h) => match *h {}` — a
       statically-unreachable compile-completeness arm, mirroring `src/value.rs:285`.
       In D1 (and the whole foundation) `HostOpaque` is the **uninhabited** placeholder
       `enum HostOpaque {}` (`src/value.rs:88-90`), so this arm matches the never type
       and adds **zero** host-opaque handling — it exists only so the closed match
       compiles under `--features mlua`. When `PLAN_M1_host_opaque` inhabits the variant
       with a real handle type, M1 replaces this arm's body with the C3
       descend-into-host-opaque-leaf error (the variant becomes inhabited, so `match *h {}`
       is no longer valid and M1 must supply real behavior — the seam fires).
   - Object-vs-array disambiguation is by the instance value's kind, so tuples
     (arrays at runtime) and records (objects at runtime) are handled by the
     `Array`/`Object` arms without schema knowledge.
6. `let rebuilt = set_at(&current, &segments, &new_value, &[])?;`
7. **Full root revalidation:** `self.parse_present(&rebuilt, &mut ParseContext::new())`.
   This re-runs type checks, validators, `refine`, `default` application, and
   strict-unknown-key rejection over the whole tree, with natural parse-flow error
   paths. Return its result.

(Host-opaque note, C3/M1: because the public `replace` takes `instance: &I where
I: Serialize`, `current` can **never** contain a host-opaque leaf — in the default
build *or* under `mlua` (host-opaque values are not `Serialize`). D1's public
`replace` is thus permanently serde-bridgeable-only. Under `--features mlua` the
closed `set_at` match still needs a `HostOpaque` arm to compile; D1 supplies the
statically-unreachable placeholder `match *h {}` (the variant is uninhabited in the
foundation), which is compile-completeness only and **not** host-opaque handling. The
host-opaque "descend into leaf" error and by-kind revalidation that C3 promises for
trees *containing* host-opaque leaves are M1's: when M1 inhabits the variant, it
replaces this placeholder arm's body — and adds its own `ZerxValue`-level replace
surface that can actually carry such instances. Both are additive changes; D1's
signature does not move.)

### Step 5 — Tests (inline `#[cfg(test)] mod tests` in `src/delta.rs`)

Use the public constructors (`object`, `array`, `tuple`, `record`, `union`,
`discriminated_union`, `string`, `number`, `literal`, `lazy`, `any`) and `Modify`
methods. Cover at minimum:

**Pointer parser**
- `""` → root; `"/a"` → `["a"]`; `"/a/b/0"` → `["a","b","0"]`; `"/"` → `[""]`;
  `"/a/"` → `["a",""]`.
- Un-escape: `"/a~1b"` → `["a/b"]`; `"/m~0n"` → `["m~n"]`; `"/~01"` → `["~1"]`
  (order check).
- `"a/b"` (no leading slash) → `INVALID_POINTER`.

**`parse_delta`**
- Object field: `object([("age", number())]).parse_delta("/age", &30)` → `Ok(I64(30))`;
  `…parse_delta("/age", &"x")` → `Err(TYPE_MISMATCH)`.
- Unknown key → `UNKNOWN_PROPERTY` with `path == ["bogus"]`.
- Nested object/array/tuple/record descent (`/a/0`, `/items/2`, a record value).
- Array item: numeric required → `parse_delta("/x", …)` on an `array` →
  `INVALID_POINTER`.
- Tuple index out of range → `INDEX_OUT_OF_RANGE`.
- Descend into scalar (`object([("name", string())]).parse_delta("/name/0", …)`) →
  `INVALID_PATH`.
- Empty pointer validates against the whole schema (success and failure cases).
- `union(...)` descent → `UNION_PATH_REQUIRES_INSTANCE`; targeting the union node
  itself (pointer stops on it) validates against the union.
- `discriminated_union(key, variants)`: **both** `parse_delta("/<key>", …)` and
  `parse_delta("/<other>", …)` → `UNION_PATH_REQUIRES_INSTANCE` (parse_delta never
  descends into a discriminated union without an instance). Use a multi-variant union
  whose variants carry *different* discriminator literals to confirm no single
  variant's literal is silently used.
- `lazy` recursive object navigated to a finite depth resolves and validates.

**`replace`**
- Object leaf: `replace(&instance, "/age", &31)` returns the revalidated object with
  `age == 31`, other fields intact, order preserved.
- Nested: `replace(&instance, "/profile/city", &"Berlin")`.
- Array element and tuple element replacement.
- Record value replacement.
- Original instance is unchanged (clone-and-return; assert by reusing it).
- `MISSING_PARENT` when an **intermediate** object key is absent (`/a/b` where `a` has
  no `b`).
- `MISSING_PARENT` when the **final** object key is absent (`/c` where the instance has
  no `c`) — including the case where `c` is `.optional()` in the schema but missing from
  the instance: `replace` errors, it does not insert. Asserts no silent insertion.
- `INDEX_OUT_OF_RANGE` when an array index ≥ len; `INVALID_POINTER` for a
  non-numeric array index; `INVALID_PATH` when descending into a scalar.
- Empty pointer replaces the whole root (success and a type-mismatch failure).
- **Full root revalidation incl. `refine`:** an object with a cross-field
  `.refine(...)` (e.g. the `priority < 9 unless role == "system"` rule from
  `docs/vision.md:212-216`); a `replace` that *individually* type-checks but
  *violates the refine* returns `REFINEMENT_FAILED`; a compliant `replace` succeeds.
- **Revalidation applies `default`:** replacing a sibling does not drop a defaulted
  field; assert the default reappears in the output.
- **Strict mode still enforced after replace (nested):** given an outer object with a
  field `profile` that is itself a `strict` object, `replace(&instance, "/profile",
  &{known_field: …, extra: …})` — where `extra` is not in `profile`'s shape — MUST fail
  full root revalidation with `UNKNOWN_PROPERTY` at `path == ["profile", "extra"]`. The
  replacement value is placed at an existing position yet still introduces an unknown
  key *within* the replaced subtree, which root revalidation rejects. The compliant
  replacement (no `extra`) succeeds. (Also covers: replacing a scalar leaf whose
  schema rejects the new value's type fails with `TYPE_MISMATCH` at the leaf path.)

## Verification

1. `cargo build` — compiles clean (default build; no `mlua` feature).
2. `cargo build --features mlua` — still compiles. `ZerxValue` carries the gated
   uninhabited `HostOpaque` variant in this build (`src/value.rs:88-90`, `:100-103`),
   so `set_at`'s closed match includes the gated `match *h {}` placeholder arm
   (Step 4); that is the *only* host-opaque-related line D1 adds, and it is
   statically unreachable (no behavior). The `SchemaKind` navigation match needs no
   gated arm because `SchemaKind` has no host-opaque variant until M1 adds one.
3. `cargo test` — all existing tests plus the new `src/delta.rs` tests pass. Expected:
   every behaviour in Step 5 asserted, including the `refine` and `default`
   revalidation cases and the original-instance-immutability case.
4. `cargo clippy --all-targets -- -D warnings` — no warnings (match the repo's clean
   clippy baseline; the `result_large_err` allow is already module-level where used).
5. Manual confirmation of the C7 surface: exactly two new public methods
   (`Schema::parse_delta`, `Schema::replace`), each returning
   `Result<ZerxValue, ZerxError>`; no `safe*`/`try*` variants exist.

### Doc Update (after verification passes)

- `docs/architecture/schema-core.md` — add a **Delta / Replace** constraints
  subsection under Constraints capturing: the two methods and their `Result<ZerxValue, …>`
  signatures (C7); RFC 6901 pointer parsing (empty = root; leading-`/` required;
  `~1`→`/` then `~0`→`~`); `parse_delta` = schema-only navigation then
  `parse_present` against the target; `replace` = instance-only navigation, immutable
  rebuild, then full root revalidation via `parse_present` (incl. `refine` and
  `default`); the closed per-kind navigation seam and that descending into
  `union`/`discriminated_union` requires an instance; no deletion; the C3 forward
  note that M1 adds host-opaque descent errors. Record the D1 error codes
  (`INVALID_POINTER`, `INVALID_PATH`, `INDEX_OUT_OF_RANGE`,
  `UNION_PATH_REQUIRES_INSTANCE`, `MISSING_PARENT`) as declared in `src/delta.rs`
  (open catalogue). Add a `candidate C7`, `candidate C3`, `candidate C1` note to the
  concern's Related Decisions line if not already implied (non-slug, per the
  concept's decision-reference discipline, `docs/CONCEPT_zerx_foundation.md:356-363`).
- `docs/definition.md` — no change (the `Delta / Replace` feature bullet already
  exists, `:30`).
- `docs/decisions.md` — no change (D1 introduces no new future-binding decision
  beyond candidates C7/C3/C1, which migrate at Concept Closeout).
- `Cargo.toml` — bump `version` from `0.10.0` to `0.11.0` (minor, additive feature),
  mirroring the per-plan bump convention (J2 → 0.10.0).

### Archive & Commit

Move this plan to `docs/archive/PLAN_D1_delta_replace.md` and commit all session
changes (`feat(delta): implement Delta/Replace (PLAN_D1_delta_replace)` plus the
version-bump chore), per the standard workflow §7.
