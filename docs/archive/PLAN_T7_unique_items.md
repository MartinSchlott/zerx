# PLAN_T7: Native `uniqueItems` modifier on arrays

## Context & Goal

communis (communis-atlas, `src/schema/helpers.rs`) needs arrays whose elements are pairwise
distinct. It emulates this today with `.refine(no duplicates).meta("uniqueItems", true)`. zerx 2+/3
drops `meta("uniqueItems", …)` at export (the key is export-reserved, so `meta` never carries an
unenforced assertion — `D-import-fail-closed`) and refuses `uniqueItems: true` at import with
`IMPORT_UNSUPPORTED` (backlog card "Enforce `exclusiveMinimum`/…/`uniqueItems`/…"). The
Product Owner approved a native modifier as zerx 3.2.0 (commissioned through DevLead,
2026-11-21).

**Goal:** `ArraySchema::unique_items()` — a real validator that rejects an array containing two
elements equal under JSON Schema value semantics, exports as `"uniqueItems": true`, and is
reconstructed by import, so native and imported schemas reject identically and
export → import → export preserves it. Released as 3.2.0.

### Equality

Two elements are duplicates exactly when `literal_eq` (`src/types.rs`) calls them equal — the same
relation `const`/`enum` already use:

- numbers by exact mathematical value across `I64`, `U64`, `I128`, `U128`, `F64` (`1 == 1.0`,
  `0 == -0.0`; `2^53` and `2^53 + 1` stay distinct; no `f64` round trip); `NaN` equals nothing;
- objects by key set and per-key value, key order ignored;
- arrays element-wise, in order;
- `null`, `bool`, `string` by value, variant-strict (a number never equals a string, etc.);
- `Bytes` byte-wise, and only to `Bytes` — never to a `String` or an `Array` of numbers. This is a
  zerx extension: JSON Schema has no bytes; it follows `D-buffer-fidelity`.

`ZerxValue::PartialEq` (variant-strict, order-sensitive) and string/serialisation comparison MUST
NOT be used.

### Safety

`literal_eq` today recurses once per nesting level of its arguments without a bound. For
`check_literal` the constant bounds the depth, but a uniqueness check compares two *input*
values, both of arbitrary depth, and validators run **before** element descent, so the
`MAX_PARSE_DEPTH` guard has not yet seen the elements (and `array(any())`/`array(json())` never
descends at all). This plan therefore makes `literal_eq` iterative (explicit work stack, no
recursion), so neither `check_literal` nor the uniqueness check can overflow the stack on deep
values, and deep values are still compared correctly rather than rejected.

To avoid O(n²) comparisons on large arrays, elements are bucketed by a hash consistent with
`literal_eq` (equal values ⇒ equal hash); full `literal_eq` runs only within a bucket. The hash
descends at most a fixed number of levels (`UNIQUE_HASH_DEPTH = 32`) — beyond that a subtree
contributes only its variant tag — so hashing is bounded-recursion and still consistent. Worst
case (all elements colliding) degrades to O(n²) comparisons but stays correct.

### Phase

JSON Schema applies `uniqueItems` to the input instance. The validator runs in the parse flow's
validators step (`src/schema.rs`, `parse_present_body`): after the `nullable` null short-circuit
and the type check, **before** element descent (`parse_inner`: element defaults, object
`strip`/`passthrough` handling, nested refinement) and before the array's own refinement
predicates. Consequences, documented and tested:

- `[{"a":1,"x":1},{"a":1,"x":2}]` against `array(object([("a", number())]).strip()).unique_items()`
  is accepted (the input elements differ) even though the output elements are equal.
- An element whose missing field is later filled by a `default` is compared without that field.
- A `null` under `.nullable()` is accepted without the check.

## Breaking Changes

No. `unique_items()` is a new method; existing schemas behave as before. Import of
`uniqueItems` on an array node changes from `IMPORT_UNSUPPORTED` to an enforced modifier — a
widening of what imports, not of what any schema accepts. Minor version: 3.1.3 → 3.2.0.

## Reference Patterns

- `ArrayMinLength` / `ArrayMaxLength` and `ArraySchema::min`/`max` in `src/types.rs` — validator
  shape, error construction, JSON Schema fragment.
- `apply_array_bounds` / `check_tuple_bounds` in `src/json_schema.rs` — import consumption of
  array keywords.
- Existing `literal_eq_boundaries` test in `src/types.rs`.

## Dependencies

None. `std::collections::HashMap` and `std::hash` only.

## Steps

1. **Failing tests first.** Add the tests listed under Verification (types, json_schema, lua,
   delta) and confirm they fail (or do not compile) before the implementation.

2. **Iterative `literal_eq`** (`src/types.rs`). Replace the recursive body with an explicit
   work stack of `(&ZerxValue, &ZerxValue)` pairs; pop a pair, compare scalars exactly as today
   (`F64`/`F64` by `==`, otherwise `exact_integer_form` on both, otherwise variant-strict `==` for
   non-container values), and for `Array`/`Array` (equal length) and `Object`/`Object` (equal
   length, every key of the first present in the second via `Map::get`) push the child pairs.
   Return `false` at the first mismatch. Semantics MUST be identical to the current function;
   the existing literal tests are the regression proof. Keep the signature and name.

3. **Equality-consistent hash** (`src/types.rs`, crate-private). `fn unique_hash(value, depth,
   hasher_state) -> u64` (exact signature the Coder's choice) using a per-call
   `std::collections::hash_map::RandomState`:
   - every value hashes a variant-class tag first (null, bool, number, string, bytes, array,
     object), so different classes rarely collide;
   - number: if `exact_integer_form` is `Some((neg, mag))`, hash `(neg, mag)`; otherwise
     (`F64` fractional, non-finite, or ≥ 2^128) hash `f64::to_bits` — all values `literal_eq`
     calls equal hash equally (`0.0`/`-0.0` both have integer form `(false, 0)`);
   - string/bytes: their contents; bool its value;
   - array: length, then element hashes in order;
   - object: length, then an order-independent combination (`wrapping_add`) of the hash of each
     `(key, value_hash)` entry;
   - at `depth == UNIQUE_HASH_DEPTH` a container contributes only its tag and length.

4. **Validator** (`src/types.rs`). `pub(crate) struct UniqueItems;` implementing `Validator`:
   - `validate`: for a non-array value return `Ok(())` (type check precedes it). Otherwise
     iterate `j` in index order; look up the bucket of `unique_hash(a[j])` in a
     `HashMap<u64, Vec<usize>>`; for each earlier index `i` in that bucket, if
     `literal_eq(&a[i], &a[j])` return
     `ZerxError::new(ErrorCode::ARRAY_NOT_UNIQUE, format!("array items at index {i} and {j} are equal"))`
     with `path = [j.to_string()]` (the parse flow's parents prepend their segments, so the full
     path addresses the later duplicate); else push `j` into the bucket. The first reported pair
     is the smallest `j` with an earlier equal `i` (the smallest such `i`).
   - `json_schema`: `{"uniqueItems": true}`.
   - Add `ARRAY_NOT_UNIQUE: ErrorCode = ErrorCode::new("array_not_unique")` to the T2 error-code
     `impl ErrorCode` block in `src/types.rs` (no edit to `src/error.rs`).

5. **Builder** (`src/types.rs`). `ArraySchema::unique_items(mut self) -> Self` pushes
   `Rc::new(UniqueItems)`. Calling it twice is harmless (same fragment key, same verdict). Add a
   short doc comment stating the equality rules, the Bytes extension and the input-instance
   phase. No method on `TupleSchema` or other builders.

6. **Export** (`src/json_schema.rs`). The validator fragment loop already merges
   `uniqueItems: true`; confirm no special casing is needed and that `uniqueItems` stays in
   `RESERVED` (so `meta("uniqueItems", …)` is still never exported).

7. **Import** (`src/json_schema.rs`).
   - `apply_array_bounds` (the `items`-array path): if `uniqueItems` is present, it MUST be a
     boolean, else `IMPORT_MALFORMED` (`"`uniqueItems` must be a boolean"`); `true` applies
     `.unique_items()`, `false` is a no-op; both mark it consumed.
   - `check_tuple_bounds` (the `prefixItems` path): a non-boolean `uniqueItems` →
     `IMPORT_MALFORMED`; `false` is consumed; `true` is left unconsumed so the existing coverage
     check refuses it with `IMPORT_UNSUPPORTED` (zerx has no unique tuple; fail-closed).
   - Add `uniqueItems` wherever `minItems`/`maxItems` are listed as import-handled keywords (e.g.
     `IMPORT_HANDLED`) so it is never routed to `meta`; keep it in the array applicability table.
   - `static_keyword_holds`' `uniqueItems` branch (static evaluation against `enum`/`const`
     members) keeps its semantics; replace its inner O(n²) loop that calls `from_serialize`
     per pair by converting each member once and running the same duplicate finder as the
     validator (share a crate-private `fn first_duplicate(&[ZerxValue]) -> Option<(usize,
     usize)>` used by both).
   - A nullable array (`"type": ["array","null"]`) keeps `uniqueItems` as a sibling constraint
     through the existing `[T, "null"]` path.
   - Update existing tests that asserted `IMPORT_UNSUPPORTED` for `uniqueItems` on an array node.

8. **Lua and delta** need no code change: `validate_lua` runs the parse flow on the transformed
   `ZerxValue`; `replace` revalidates from the root; `parse_delta` at an element pointer
   validates the element's sub-schema only and cannot see siblings (unchanged, by design — the
   array's own validators run only when the array itself is the target).

9. **Version.** Not the Coder's: the Planner bumps `Cargo.toml`/`Cargo.lock` to 3.2.0 at
   finalization.

## Documentation

Updated by the Planner at finalization, after verification:

- `docs/architecture/types.md`, `#### array`: add the `unique_items()` constraint (validator,
  `ARRAY_NOT_UNIQUE`, path = index of the later duplicate, fragment `{"uniqueItems": true}`), the
  equality rule (literal equality; `Bytes` only to `Bytes`, a zerx extension), the phase (input
  instance, validators step, before element descent and refinement), and that equality comparison
  MUST NOT recurse per nesting level; add `ARRAY_NOT_UNIQUE` to "Error codes — complex types".
  Basis: the Product-Owner-authorized product goal (native `uniqueItems` with JSON Schema value
  semantics, commissioned 2026-11-21) and the load-bearing literal-equality invariant already in
  the same file; the no-recursion rule follows from the existing stack-safety invariant of the
  parse flow (`MAX_PARSE_DEPTH`, `json-schema` import depth bound).
- `docs/architecture/json-schema.md`: add `uniqueItems` to the export-reserved list sentence and
  the export mapping (array validators export their keyword); in the import coverage section, state
  that `uniqueItems` on an `items` array node is consumed (`true` → `unique_items()`, `false`
  no-op), refused on a `prefixItems` tuple when `true`, and `IMPORT_MALFORMED` when not boolean.
  Basis: `D-import-fail-closed` (every assertion keyword consumed, vacuous or refused) and the
  authorized product goal of import/export parity.
- `docs/tests.md`: no inventory change (tests extend existing `mod tests` blocks).
- `docs/backlog.kanban.md`: remove `uniqueItems` from the "Enforce `exclusiveMinimum`/…" card's
  title and text (the other keywords stay open), and add a card in `Done` recording that
  `uniqueItems` is enforced (PLAN_T7, zerx 3.2.0).
- `README.md`: mention `unique_items()` next to the array validators if the README lists them.
- No register entry is proposed or superseded; `D-import-fail-closed` and `D-buffer-fidelity`
  stay true as written.

## Verification

Required gate: `scripts/verify all` (fmt, lint, test, test core, api) — the `api` gate because
the public builder surface grows. Targeted runs (`scripts/verify test unique`) are diagnostic
only.

Tests added in the owning modules' existing `mod tests` blocks (each must fail before Step 2–7):

`src/types.rs`:
- distinct items accepted: `["a","b"]`, `[1,2]`, `[{"id":"a"},{"id":"b"}]` (communis reference
  cases: distinct strings, duplicate scalar, duplicate object);
- duplicate scalar `["a","b","a"]` → `ARRAY_NOT_UNIQUE`, path `["2"]`;
- duplicate object `[{"id":"a"},{"id":"a"}]` → `ARRAY_NOT_UNIQUE`, path `["1"]`;
- `[I64(1), F64(1.0)]` and `[U64(1), I128(1)]` duplicates; `[F64(0.0), F64(-0.0)]` duplicate;
  `[NaN, NaN]` accepted;
- `[U64(2^53), U64(2^53+1)]` and `[U64(2^53+1), F64(2^53 as f64)]` distinct;
- objects with the same entries in different key order → duplicate;
- nested arrays: `[[1,2],[1,2]]` duplicate, `[[1,2],[2,1]]` distinct;
- Bytes: `[Bytes([1]), Bytes([1])]` duplicate; `[Bytes([1]), Array([1])]` and
  `[Bytes(b"a"), String("a")]` distinct (under `array(any())`);
- nullable: `array(string()).unique_items().nullable()` accepts `null`, rejects `["x","x"]`;
- path through a parent: `object([("tags", array(string()).unique_items())])` with duplicate tags
  → path `["tags","1"]`;
- phase: the strip example from *Phase* is accepted; an array refinement sees the parsed output
  only after uniqueness passed (a duplicate input yields `ARRAY_NOT_UNIQUE`, not
  `REFINEMENT_FAILED`);
- deep nesting, machinery level (no stack overflow): on a thread with a 2 MB stack in a debug
  build, with values built iteratively 100 000 levels deep and `mem::forget`-ed afterwards
  (`ZerxValue`'s derived `Drop`/`Clone` recurse per level), call the crate-private functions
  directly: `UniqueItems.validate` / `first_duplicate` report two equal deep values as duplicates
  and accept two that differ only at the bottom; `literal_eq` and `check_literal` return `true`
  for the matching pair and `false` for the non-matching pair;
- deep nesting, through the schema: the same equal/differing pairs at depth 1 000 against
  `array(any()).unique_items()` via `parse_present` with a fresh `ParseContext` (duplicate rejected,
  differing accepted), on the same 2 MB thread. The through-schema depth is bounded by the parse
  flow's recursive identity clone of accepted elements (`Ok(value.clone())` for `Any`/`Literal`),
  which is outside this plan's scope; 1 000 is still far beyond `MAX_PARSE_DEPTH`;
- large array: 50 000 distinct small objects validate, and the same array with one duplicate
  appended is rejected with path `["50000"]`;
- existing `literal_eq_boundaries` and literal tests pass unchanged.

`src/json_schema.rs`:
- export of `array(string()).unique_items()` contains `"uniqueItems": true`; without the modifier
  it is absent; `meta("uniqueItems", true)` alone still exports no `uniqueItems`;
- import `{"type":"array","items":{"type":"string"},"uniqueItems":true}` rejects `["a","a"]` with
  `ARRAY_NOT_UNIQUE` path `["1"]`; `false` imports and accepts `["a","a"]`; `"yes"` and `1` →
  `IMPORT_MALFORMED`;
- `{"type":["array","null"],"items":{…},"uniqueItems":true}` accepts `null`, rejects duplicates;
- tuple `prefixItems` with `uniqueItems: true` → `IMPORT_UNSUPPORTED`; with `false` imports;
- round trip: export → import → export of a schema shaped like communis' harness records —
  `object([("tags", array(string()).unique_items()), ("steps", array(object([("id", string()),
  ("kind", enumerate(["a","b"]))])).unique_items())])` — yields identical JSON, and native and
  imported schemas give the same `Ok`/error code/path on: distinct input, duplicate tag,
  duplicate step object (key order swapped), a step with an unknown key;
- static evaluation: `{"enum":[[1,1]],"uniqueItems":true}` still refuses, `{"enum":[[1,2]],
  "uniqueItems":true}` still imports.

`src/lua.rs` (feature-gated): a Lua array `{1, 1.0}` against `array(number()).unique_items()` →
`ARRAY_NOT_UNIQUE`.

`src/delta.rs`: `replace` that makes two array elements equal under a `unique_items()` array →
`ARRAY_NOT_UNIQUE`.

Final: `scripts/verify all` exits 0. No acceptance check needs agent judgment.
