# PLAN_J5: Every imported assertion keyword is enforced or refused

## Context & Goal

This plan follows PLAN_J4 (zerx 2.0.3, commit 411d49a). The external supervisor (DevLead of zerx,
acting on the Product Owner's mandate) commissioned it as a bug fix, with no extra approval needed.
The rule from the Product Owner, as the supervisor stated it: a keyword zerx does not enforce MUST
either be enforced or make the import fail. It MUST never be stored as `meta` in place of a
constraint. The two item-specific instructions:
- `oneOf`: real exactly-one semantics or refusal; a silent first match is not acceptable.
- `jsonschema()`: its runtime check and its exported/imported form agree, and the stricter reading wins.

It closes the six fail-open cards that PLAN_J4 left in `docs/bug.kanban.md`. All were reproduced
against 538f3c3 and still hold at 411d49a:

| Card | Example | Today |
|---|---|---|
| Unenforced validation keywords stored as meta | `{"type":"number","exclusiveMinimum":0}` | accepts `0` |
| Keywords next to `$ref` ignored | `{"$ref":"#/$defs/S","required":["x"]}` | accepts `{}` |
| Constraints next to `anyOf`/`oneOf` and `enum`/`const` | `{"anyOf":[{"type":"string"},{"type":"number"}],"minLength":3}` | accepts `"a"` |
| Programmatic `jsonschema()` accepts any value | `jsonschema()` against `5` | accepted, but it exports `type:"object"` |
| `oneOf` without a valid discriminator loses exclusivity | `{"oneOf":[{"type":"number"},{"type":"integer"}]}` | accepts `1` |
| Validation keywords next to a format marker are dropped | `{"type":"object","format":"jsonschema","required":["x"]}` | drops `required` |

While designing this plan, more instances of the same root cause turned up. They were found by
reading `import_value` in `src/json_schema.rs`:
- An untyped node with assertions, e.g. `{"properties":{…},"required":["a"]}` or `{"minLength":3}`,
  falls through to `any()`.
- `required` naming a key absent from `properties` is not enforced: strict mode rejects the key,
  so `{}` passes.
- A node with both `anyOf` and `oneOf` imports only the `oneOf`.
- For a `$ref` node, `allOf`/`not` and the other unsupported applicators are skipped ("best-effort")
  rather than refused.
- A schema-object `additionalProperties` on a plain object maps to `passthrough()`, which discards
  the extras' value schema. `{"type":"object","properties":{"a":{}},"additionalProperties":{"type":"string"}}`
  accepts `{"a":1,"b":5}`.
- A `$id` on a non-root node silently re-bases the `$ref`s inside it. zerx resolves them against
  the root `$defs`, which can resolve to a different, wider schema.

**Root cause:** each dispatch branch of `import_value` reads the keywords it knows. Step 13 sends
every other key to `meta`, except a fixed list of structural keywords. Nothing checks that every
assertion keyword on a node was actually reconstructed.

**Goal:**
1. Every JSON Schema assertion keyword on an imported node is either *consumed* by the
   reconstruction, *vacuous* for the node's declared type, or the import fails with
   `IMPORT_UNSUPPORTED`.
2. `oneOf` has real exactly-one semantics.
3. `jsonschema()` accepts only objects.
4. The `enum`/`const` sibling constraints that PLAN_J4 refused are enforced exactly where that can be
   done statically, and stay refused where it cannot.

### Settled choices

- **Refuse rather than enforce unmodelled keywords** (`exclusiveMinimum`, `exclusiveMaximum`,
  `multipleOf`, `uniqueItems`, `minProperties`, `maxProperties`). The supervisor allows either.
  - Enforcing them needs new public validators on the builders, which is new API.
  - An import-side refinement is not exported, so a re-import would silently widen.
  - Real validators for them are a backlog feature (card at finalization).
- **Vacuity.**
  - A keyword that applies only to instance types the node's `type` excludes forbids nothing, so
    ignoring it preserves meaning. Example: `minLength` under `type:"number"`.
  - An untyped node has no vacuous keywords.
  - The keywords zerx already refuses outright (`allOf`, `not`, `if`/`then`/`else`, `contains`, …)
    stay refused even where vacuous. Refusals that stand today are not relaxed.
- **`enum`/`const` siblings are evaluated statically against the members.**
  - When every member satisfies every applicable sibling keyword, the keyword is redundant and the
    enum/literal is exact.
  - When a member violates one, it is unreachable. The import is refused, consistent with PLAN_J4's
    "never silently drop a member".
  - An applicable keyword that cannot be evaluated statically (sub-schema keywords such as
    `properties`, `items`) is refused.
  - `enum` + `const` imports as the literal when the `const` equals a member under `literal_eq`, and
    is refused otherwise.
- **`oneOf` becomes a kind of its own, `SchemaKind::OneOf`.** It is crate-internal, built only by
  import, with no public builder (a public builder is new API; backlog card at finalization).
  - It accepts a value iff exactly one variant accepts it, and returns that variant's output.
  - It replaces the PLAN_J4-era fallback `union` + `meta("x-oneOf", true)`.
  - It exports as `{"oneOf":[…]}`.
  - A valid discriminated union stays `DiscriminatedUnion`. Distinct discriminator constants make at
    most one variant match, so it is already exact.
- **The `sql` policy's `oneOf([X,{"type":"null"}])` → `anyOf` rewrite is removed.** With real oneOf
  semantics it is no longer exact: if `X` admits `null`, `oneOf` rejects `null` but `anyOf` accepts
  it. The core imports such a node exactly as a `OneOf`. `deep_map_schema` already recurses into
  `oneOf` members, so the other `sql` transforms still reach `X`.

## Breaking Changes

Yes, by intent. No public builder or signature changes. Error codes are added. Export output changes
for imported `oneOf`.

Newly refused at `from_json_schema` (`IMPORT_UNSUPPORTED` unless stated otherwise):
- An unmodelled assertion keyword (`exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`,
  `uniqueItems`, `minProperties`, `maxProperties`, `maxContains`/`minContains` as before, …) on a node
  where it is not vacuous.
- Any assertion keyword next to `$ref`, `allOf`/`not`/`if`/… included (previously skipped).
- Assertion keywords next to `anyOf`/`oneOf` (nullable collapse and discriminated union included),
  unless vacuous under the node's `type`, and `anyOf` together with `oneOf` on one node.
- Assertion keywords next to a format marker that the marker does not consume (Step 2.3).
- Assertion keywords on an untyped node that no branch consumes, e.g. `{"properties":{…}}` or
  `{"minLength":3}` without `type`.
- `required` naming a key absent from `properties` on a reconstructed object.
- A plain object (no `record` marker) whose `additionalProperties` is a schema object other than `{}`.
  Previously this imported as `passthrough()` and the value schema was discarded.
- A tuple (`prefixItems` + `items:false`) whose `minItems` exceeds, or whose `maxItems` is below, the
  number of `prefixItems`.
- `$id` on any node other than the root.
- `enum`/`const` where a member violates a statically evaluable sibling, or where a sibling applies to
  a member and cannot be evaluated; `enum` + `const` where the `const` is not a member.

Newly accepted at import (previously refused by PLAN_J4, now exact): `enum`/`const` with
statically satisfied scalar siblings, e.g. `{"type":"string","enum":["aa","bbb"],"minLength":2}`;
`enum` + `const` with the `const` among the members.

Validation changes:
- Imported `oneOf` (no or invalid discriminator) rejects a value matched by two or more variants,
  with the new `one_of_ambiguous` code. Zero matches keep `union_mismatch`.
- `jsonschema()` built programmatically rejects non-objects with `type_mismatch`. The imported form
  now fails with `type_mismatch` instead of PLAN_J4's `refinement_failed`.
- Under `policy: "sql"`, `oneOf:[X,{"type":"null"}]` keeps exactly-one semantics. It differs from
  before only if `X` admits `null`.

Export changes:
- An imported `oneOf` re-exports as `{"oneOf":[…]}`; there is no more `anyOf` + `x-oneOf: true`.
  An older document carrying `anyOf` + `x-oneOf` still imports as an `anyOf` union with that `meta`
  key, as its JSON Schema meaning says.
- `meta` can no longer write any assertion keyword; the export-reserved set grows.
- A root `$id` keeps round-tripping as `meta`. A nested `meta("$id", …)` set programmatically still
  exports, but the exported document is refused on re-import, because it puts `$id` on a non-root node.

Consumer recovery: state the intent with keywords zerx enforces. Examples: `minimum` for
`exclusiveMinimum` on integers plus 1; an explicit `type` so siblings become enforceable or vacuous.

## Reference Patterns

- `src/json_schema.rs`: `import_value` (steps 2.5–13 as left by PLAN_J4), `parse_type`, `TypeSpec`,
  `ensure_member_within`, `RESERVED`; tests `import_unsupported`, `import_union_discriminated`.
- `src/types.rs`: `parse_union`, `check_union`, `union` (constructor), `literal_eq`, `check_jsonschema`,
  the string/number/array validators (character counting for `minLength`/`maxLength`).
- `src/lua.rs`: the `SchemaKind::Union` arm of `lua_transform_inner`.
- `src/delta.rs`: the closed `SchemaKind` match in `navigate_schema`.
- `src/policy.rs`: `sql_nullable_normalise` (as left by PLAN_J4).

## Assumptions & Risks

- **Keyword vocabulary (Draft 2020-12, core + applicator + validation).** The assertion keywords, with
  the instance types they apply to:
  - **any type (never vacuous):** `type`, `enum`, `const`, `allOf`, `anyOf`, `oneOf`, `not`, `if`,
    `then`, `else`, `$ref`, `$dynamicRef`;
  - **string:** `minLength`, `maxLength`, `pattern`;
  - **number (integer included):** `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`,
    `multipleOf`;
  - **array:** `items`, `prefixItems`, `contains`, `minContains`, `maxContains`, `minItems`,
    `maxItems`, `uniqueItems`, `unevaluatedItems`;
  - **object:** `properties`, `patternProperties`, `additionalProperties`, `propertyNames`,
    `required`, `dependentRequired`, `dependentSchemas`, `minProperties`, `maxProperties`,
    `unevaluatedProperties`.

  Everything else is an annotation, a core identifier, or unknown. Unknown keywords assert nothing in
  JSON Schema, so `meta` stays correct for them. `format` is annotation-only by default. zerx
  consumes it as a marker, a validator or the `format` modifier, unchanged. `discriminator` is an
  OpenAPI annotation, consumed by the discriminated-union branch.
- **Static `pattern` evaluation** uses the same `regex` crate and the same semantics as the runtime
  `regex` validator. A pattern that does not compile is `IMPORT_MALFORMED`.
- **Static `minLength`/`maxLength`** count characters as the runtime string validators do.
- **Risk: wide new refusals.** Untyped object-shaped schemas without `type:"object"` are common in the
  wild and are now refused. This is intended under the rule, and the list goes into the completion
  report.
- **`OneOf` cost:** every variant is parsed for every value. This is accepted. `oneOf` needs it by
  definition.

## Steps

### 1. `src/types.rs`, `src/json_schema.rs` — `jsonschema()` accepts only objects

- Remove the PLAN_J4 import-side `.refine(|v| matches!(v, ZerxValue::Object(_)), "expected object")` from
  the `jsonschema` marker branch of `import_value`. The kind's own check replaces it.
- `check_jsonschema` MUST accept only `ZerxValue::Object`. On anything else it fails with
  `TYPE_MISMATCH`, `expected` = `object`, `received` = the type tag, following `check_object`.
- Update the `jsonschema` builder doc comment.

### 2. `src/schema.rs`, `src/types.rs`, `src/lua.rs`, `src/delta.rs`, `src/json_schema.rs` — `SchemaKind::OneOf`

- **The kind.** Add `SchemaKind::OneOf(Vec<Schema>)` and a `pub(crate) fn one_of(variants)`
  constructor in `src/types.rs`, next to `union`. No public builder and no re-export from `lib.rs`.
- **Dispatch arms:**
  - `check_type` → `crate::types::check_union()` (variants decide);
  - `parse_inner` → a new `crate::types::parse_one_of`;
  - `Debug` → `"OneOf"`.
- **`parse_one_of`:**
  - Parse the value against every variant with the shared `ctx`.
  - Collect the successes and the per-variant errors.
  - Exactly one success → return its output.
  - Zero → `ZerxError::union(Vec::new(), inner_errors)`.
  - Two or more → a new error with code `ONE_OF_AMBIGUOUS` (`"one_of_ambiguous"`). Declare it in
    the `types` error-code block in `src/types.rs`, next to `INVALID_DISCRIMINATED_UNION`. Message
    `value matched {n} oneOf variants; exactly one is required`. `expected` = `exactly one oneOf
    variant`. No `received` value.
- **`src/lua.rs`:** add a `SchemaKind::OneOf` arm next to the `Union` arm.
  - It transforms and parses against every variant the same way.
  - It returns the transformed value when exactly one variant succeeds.
  - It returns `ZerxError::union` when none does, and the `ONE_OF_AMBIGUOUS` error when several do.
- **`src/delta.rs`:** `navigate_schema` handles `SchemaKind::OneOf(_)` together with `Union`/
  `DiscriminatedUnion` (`UNION_PATH_REQUIRES_INSTANCE`).
- **Export (`base_schema` in `src/json_schema.rs`):** `OneOf(variants)` → `{"oneOf":[<exported variants>]}`.
- **Import (Step 6 of `import_value`):** `oneOf` without a discriminator, or with a discriminator whose
  invariant fails, MUST build `one_of(variants)`. Remove both `meta("x-oneOf", true)` sites. `anyOf`
  stays `union`.

### 3. `src/json_schema.rs` — consumed-or-vacuous-or-refused

**3.1 Keyword tables.** Add a private table of the assertion keywords with their applicable types
(Assumptions). Each keyword maps to `Any`, or to a set of `TypeName`s, where `Number` covers
`Integer`.

**3.2 Track consumption.** `import_value` keeps a per-node set of consumed keyword names. Each branch
adds the keywords it reconstructs:
- **`$ref`:** `$ref`.
- **Nullable collapse / `anyOf` union:** `anyOf`, and `type` when the PLAN_J4 member rule held.
- **`oneOf` (`OneOf` or discriminated union):** `oneOf`, `type` as above. `discriminator` is an
  annotation (not in the table).
- **`enum`/`const`:** `enum`/`const`, `type`, and every sibling that the static evaluation (3.4)
  consumed.
- **`format:"json"`:** nothing beyond `format`, which is not in the table.
- **`record` marker:** `type`, `additionalProperties`, and `properties` only when it is an empty
  object (the export form).
- **`jsonschema`, `buffer`, `uri`, `url` markers:** `type`.
- **`string`:** `type`, `minLength`, `maxLength`, `pattern`.
- **`number`/`integer`:** `type`, `minimum`, `maximum`.
- **`boolean`/`null`:** `type`.
- **Array:** `type`, `items`, `minItems`, `maxItems`.
- **Tuple:** `type`, `prefixItems`, `items`, `minItems`, `maxItems`. When `minItems` > n or
  `maxItems` < n (n = number of `prefixItems`), return `IMPORT_UNSUPPORTED` (unsatisfiable).
  `minItems`/`maxItems` that bracket n are redundant with the fixed-length tuple, so they are
  consumed.
- **Object:** `type`, `properties`, `required`, `additionalProperties`. Every name in `required` MUST
  be a key of `properties`; otherwise return `IMPORT_UNSUPPORTED` naming the key.
  `additionalProperties` is consumed only as `false`, absent, `true` or the empty schema `{}`; the last
  two map to `passthrough()`, which is exact. Any other schema-object value MUST raise
  `IMPORT_UNSUPPORTED`. zerx does not model a per-extra-key value schema on a plain object, and
  `passthrough()` would discard it. `record` covers the no-`properties` case through its marker.
- **`any()` fallthrough:** nothing.

**3.3 The check (replaces the structural part of Step 13).** For every key of the node that is in the
assertion table:
- consumed → fine;
- in today's `IMPORT_UNSUPPORTED_STRUCTURAL` list → `IMPORT_UNSUPPORTED`, also on `$ref` nodes
  (remove the `is_ref` skip);
- otherwise vacuous when the node has a `TypeSpec` and none of the keyword's applicable types is
  admitted by it (`TypeSpec::admits`, with `Integer` ⊂ `Number`);
- anything else → `IMPORT_UNSUPPORTED`, naming the keyword, at the node path.

**3.3a Malformed values of consumed keywords.** A consumed keyword whose value violates the Draft
2020-12 grammar MUST raise `IMPORT_MALFORMED` at the node path. It is not consumed silently. This
covers:
- `properties` or `$defs` not an object;
- `required` not an array of strings (duplicates are allowed by the 2020-12 grammar and import);
- `minLength`, `maxLength`, `minItems` or `maxItems` not a non-negative integer (an integral float such
  as `2.0` counts as an integer);
- `minimum` or `maximum` not a number;
- `pattern` not a string;
- `prefixItems`, `enum`, `anyOf` or `oneOf` not an array. An empty `anyOf`/`oneOf` is valid grammar but
  the never-schema, so it is `IMPORT_UNSUPPORTED`, like the `false` schema;
- `additionalProperties` or `items` neither a boolean nor an object.

Non-assertion keys keep today's handling: annotations are reconstructed, other keys go to `meta`. In
addition, `$id` on a node whose path is not the root → `IMPORT_UNSUPPORTED`. The check MUST run after
the base is built and before `meta` routing, so no assertion keyword ever reaches `meta`.

Remove PLAN_J4's `DROPPED_BY_ENUM` guard and its `enum`+`const` refusal; 3.4 replaces both.
`ensure_member_within` and the PLAN_J4 `type` rules stay unchanged.

**3.4 Static evaluation for `enum`/`const`.**
- **Members.** The member set is the `enum` array, or the single `const`.
- **`enum` + `const` on one node.** The `const` MUST equal a member under `literal_eq`. Then the base is
  `literal(const)` and the member set is `[const]`; otherwise return `IMPORT_UNSUPPORTED`.
- **Applicable siblings.** For each member, take its JSON type (as `value_type_name` in PLAN_J4) and,
  for each sibling assertion keyword that applies to that type:
  - Evaluable, consumed if every applicable member satisfies it:
    - `minLength`, `maxLength`: character count, as the runtime validators count;
    - `pattern`: `regex` crate search; a non-compiling pattern → `IMPORT_MALFORMED`;
    - `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`: numeric comparison, exact for
      integers;
    - `multipleOf`: only when both the member and the divisor are integers (`as_i64`/`as_u64`) and
      the divisor is positive;
    - `minItems`, `maxItems`, `uniqueItems`: `uniqueItems` uses `literal_eq` pairwise;
    - `required`, `minProperties`, `maxProperties`.
  - Not evaluable: every other applicable keyword (`items`, `prefixItems`, `properties`,
    `additionalProperties`, `contains`, …), and `multipleOf` outside the integer case.
- **Outcome.** A member that violates an evaluable keyword → `IMPORT_UNSUPPORTED` naming the member
  index and keyword. A non-evaluable keyword that applies to some member → `IMPORT_UNSUPPORTED`. A
  keyword that applies to no member is vacuous for the enum and also consumed.
- The PLAN_J4 type-membership rule still applies first.

**3.5 Export-reserved set.** Extend `RESERVED` with every keyword of the assertion table, so the `meta`
step can never emit an assertion keyword. `$id` is not reserved: a root `$id` imports as `meta` and
re-exports unchanged. A programmatic nested `meta("$id", …)` still exports and is refused on re-import;
this is a documented limitation (Export changes).

### 4. `src/policy.rs` — remove the `sql` `oneOf` rewrite

- Delete the `oneOf([X, {"type":"null"}])` branch of `sql_nullable_normalise` and `is_movable_sibling`.
- The type-array branch stays as PLAN_J4 left it. Its `NULL_CONSTRAINING` exclusion still lists
  `oneOf`, so a `[T,"null"]` node with `oneOf` goes to the core.

### 5. Tests

Extend the in-module `mod tests` blocks; `docs/tests.md` needs no new suite. Boundary: unit.
Perspectives: failure mode, conformance, round-trip, feature contract. Assert codes, not messages.

**Update existing tests:**
- The two `x-oneOf` assertions in `src/json_schema.rs` (the `oneOf` without discriminator import, and the
  discriminator-fallback test). They now expect a `OneOf` that re-exports as `oneOf` without `x-oneOf`.
- `test_sql_nullable_one_of` (now `oneOf` in the export).
- The PLAN_J4 tests that asserted refusal of `enum`/`const` with satisfiable siblings or of
  `enum`+`const` with the `const` among the members: assert the new exact import instead. Keep the
  refusal assertions where the member violates or the keyword is not evaluable.
- PLAN_J4's `jsonschema` marker tests (code now `type_mismatch`).

**`src/json_schema.rs`, one test per card plus the newly found paths:**
- Unmodelled keywords:
  - `{"type":"number","exclusiveMinimum":0}`, `{"type":"integer","multipleOf":2}`,
    `{"type":"array","items":{},"uniqueItems":true}`, `{"type":"object","minProperties":1}` →
    `import_unsupported`;
  - vacuous: `{"type":"string","exclusiveMinimum":0}` and `{"type":"number","uniqueItems":true}`
    import;
  - `{"type":"string","contains":{}}` stays refused (pre-existing structural refusal not relaxed).
- `$ref` siblings: `{"$ref":…,"required":["x"]}`, `{"$ref":…,"allOf":[…]}`,
  `{"$ref":…,"minLength":1}` → `import_unsupported`. `{"$ref":…,"description":"d","x-k":1}` imports.
- `anyOf`/`oneOf` siblings:
  - `{"anyOf":[{"type":"string"},{"type":"number"}],"minLength":3}`,
    `{"anyOf":[{"type":"string"},{"type":"null"}],"maxLength":3}`, `{"anyOf":[…],"oneOf":[…]}` and
    `{"anyOf":[…],"enum":["a"]}` → `import_unsupported`;
  - `{"type":"object","oneOf":[{"type":"object",…},{"type":"object",…}],"minLength":3}` imports
    (vacuous).
- Untyped assertions: `{"properties":{"a":{"type":"string"}}}`, `{"minLength":3}`,
  `{"required":["a"]}` → `import_unsupported`; `{}` and `{"description":"d"}` still import as `any()`.
- `required` not in `properties` → `import_unsupported`; `required` ⊆ `properties` imports as before.
- On a plain object, `additionalProperties:{"type":"string"}` → `import_unsupported`.
  `additionalProperties:{}` and `true` → passthrough, accepting extras; under the strip disposition the
  `{}` case stays passthrough.
- Malformed consumed values → `import_malformed`: `{"type":"object","properties":5}`,
  `{"type":"object","properties":{"a":{}},"required":[5]}`, `{"type":"array","items":{},"minItems":-1}`,
  `{"type":"string","minLength":"2"}`, `{"enum":"a"}`; `{"anyOf":[]}` → `import_unsupported`.
  `{"type":"string","minLength":2.0}` imports.
- Tuple: `minItems` 3 with two `prefixItems` → `import_unsupported`; exported `tuple` round-trips.
- `$id`: allowed at the root and preserved by export → import → export; refused on a property node.
- Markers: `{"type":"object","format":"jsonschema","required":["x"]}`,
  `{"type":"string","format":"uri","minLength":3}` and
  `{"type":"object","format":"record","additionalProperties":{},"required":["x"]}` →
  `import_unsupported`; exported `record`, `buffer` (with `mime`), `uri`, `url`, `json`, `jsonschema`
  round-trip.
- `enum`/`const` static evaluation:
  - `{"type":"string","enum":["aa","bbb"],"minLength":2}` imports and accepts `"aa"`;
  - `{"type":"string","enum":["a","bb"],"minLength":2}` → `import_unsupported`;
  - `{"enum":["ab",5],"pattern":"^a"}` imports (the pattern is vacuous for `5`);
  - `{"enum":[1,2],"exclusiveMinimum":0}` imports, and with `exclusiveMinimum: 1` it is refused;
  - `{"enum":[[1,2]],"uniqueItems":true}` imports, `{"enum":[[1,1]],"uniqueItems":true}` is refused;
  - `{"const":{"a":1},"required":["a"]}` imports;
  - `{"const":{"a":1},"properties":{"a":{}}}` → `import_unsupported` (not evaluable);
  - `{"enum":["a","b"],"const":"a"}` imports as a literal accepting only `"a"`;
  - `{"enum":["a"],"const":"b"}` → `import_unsupported`;
  - invalid `pattern` next to `enum` → `import_malformed`.
- Legacy document: `{"anyOf":[{"type":"string"},{"type":"number"}],"x-oneOf":true}` imports as an `anyOf`
  union (accepts `"a"` and `1`) and re-exports with `x-oneOf` as a meta key.
- `oneOf` exactly-one:
  - `{"oneOf":[{"type":"number"},{"type":"integer"}]}` rejects `1` with `one_of_ambiguous`, accepts
    `1.5`, and rejects `"x"` with `union_mismatch`;
  - a discriminator-invariant failure case builds `OneOf`;
  - a valid discriminated union is unchanged;
  - export → import → export of an imported `oneOf` is stable.
- No assertion keyword reaches `meta`: for each assertion keyword, a node that imports never carries it
  as a `meta` key. Assert through re-export of every importing case above. Also `meta("minimum", 1)`
  on a programmatic schema does not appear in the export.
- Round-trip: every existing export → import test stays green.

**`src/types.rs`:**
- `jsonschema()` accepts `{}` and rejects `5`, `"x"`, `[]`, `true` with `type_mismatch`.
- `one_of` built crate-internally: one, zero and two matches. With a `strip` object variant, the output
  is that variant's output.

**`src/lua.rs`** (feature-gated): `OneOf` under `validate_lua` with one, zero and two matching variants.

**`src/delta.rs`:** `parse_delta` into a `OneOf` → `union_path_requires_instance`.

**`src/policy.rs`:** `{"oneOf":[{"type":"number"},{"type":"null"}]}` under `sql` accepts `null` and `3`.
`{"oneOf":[{},{"type":"null"}]}` under `sql` rejects `null` with `one_of_ambiguous`.

## Documentation

At finalization, after verification. Basis for every constraint and the register entry below: the
Product Owner's durable standing rule, relayed by the external supervisor. An imported schema accepts
nothing its JSON Schema forbids, and a keyword zerx does not enforce is enforced or refused, never
stored as `meta`. Behind it stand the product goals `Strict validation` and semantically equivalent
roundtrip (`docs/definition.md`).

- `docs/decisions.md`: propose **`D-import-fail-closed`**. *We import a JSON Schema as a schema that
  accepts nothing the source forbids. Every assertion keyword is enforced, vacuous for the node's
  declared type, or refused with `IMPORT_UNSUPPORTED`/`IMPORT_MALFORMED`. We do not import a wider
  schema, fall back to `any()`, or store an unenforced assertion as `meta`.*
  Rationale: the standing rule. Consequence:
  - every assertion keyword on a node is consumed, vacuous for the node's declared type, or refused;
  - `meta` carries only non-assertion keys;
  - new import support must name the keywords it consumes;
  - the one known deliberate narrowing: `prefixItems` + `items:false` imports as a fixed-length
    tuple. Scope: `docs/architecture/json-schema.md`,
  `docs/architecture/policy.md`. It binds future plans beyond this one (every later import feature),
  so it is a register entry, not a concern-file rule.
- `docs/architecture/json-schema.md`:
  - **Import pipeline order:** Step 13 becomes the consumed/vacuous/refused check followed by `meta`
    routing.
  - **New subsection "Assertion keyword coverage":** the vocabulary by applicable type, the
    consumed set per branch, vacuity, and the static `enum`/`const` evaluation.
  - **`oneOf` import:** `OneOf` replaces the `x-oneOf` fallback.
  - **`enum`/`const`:** the PLAN_J4 sibling guard becomes the static evaluation rule.
  - **Format-marker mapping:** the PLAN_J4 `jsonschema` refinement line goes, because the kind now
    checks.
  - **Per-kind base schemas:** add `OneOf` → `{"oneOf":[…]}`.
  - **Export-reserved set:** add the assertion keywords.
  - **`additionalProperties` import shapes:** the schema-object line becomes `{}` → `passthrough()`,
    with any other schema object → `IMPORT_UNSUPPORTED`. The strip-disposition line names `true`/`{}`
    as the unaffected passthrough cases.
  - **`type` import:** qualify the absent-`type` line. A node imports as `any()` only when no other
    base-producing keyword and no unconsumed assertion keyword is present.
  - **Root `$id`:** it is preserved as `meta`; a nested `$id` is refused.
  - **Unsupported applicators:** they are refused on `$ref` nodes too.
  - **Error codes:** add the new `IMPORT_UNSUPPORTED` cases.
  - **Splitting Rationale:** keep it, and recheck the 300-line ceiling.
  - **Related Decisions:** add `D-import-fail-closed`.
- `docs/architecture/types.md`:
  - `jsonschema` accepts only objects (`type_mismatch`);
  - the crate-internal `OneOf` kind: exactly-one semantics, `one_of_ambiguous`, zero matches →
    `union_mismatch`, no public builder;
  - the `one_of_ambiguous` error code.
- `docs/architecture/lua.md`: `OneOf` follows the union rule with exactly-one counting.
- `docs/architecture/schema-core.md`: extend the delta constraint so that descending into a `union`,
  a `discriminated_union` or the crate-internal `OneOf` kind is rejected with
  `UNION_PATH_REQUIRES_INSTANCE`. Basis: the load-bearing invariant that variant selection needs an
  instance, which holds for every union-family kind. Otherwise no change: `OneOf` is one kind variant
  with delegating arms, as the existing rule requires.
- `docs/architecture/policy.md`:
  - remove the `oneOf` rewrite rule;
  - in "Unknown-property disposition on import", the passthrough cases become
    `additionalProperties: true` or `{}`, because the core import now refuses a non-empty
    schema-object value;
  - add `D-import-fail-closed` to Related Decisions.
- `docs/bug.kanban.md`: move the six PLAN_J4 follow-up cards to `Done`.
- `docs/backlog.kanban.md`: one card for enforcing `exclusiveMinimum`/`exclusiveMaximum`/`multipleOf`/
  `uniqueItems`/`minProperties`/`maxProperties` as real exported validators; one card for a public
  `one_of` builder.

**Version:** patch, 2.0.3 → 2.0.4 in `Cargo.toml` and `Cargo.lock`, applied by the Planner at
finalization. The line is commissioned as a bug fix; no public builder or signature changes.

## Verification

Required gate: `scripts/verify all` (lint, `test`, `test core`, `api`) passes on the final functional
state. `test core` matters because this plan adds `#[cfg(feature = "lua")]` code.

Acceptance checks, run and reported by the Coder:
- `git grep -n 'x-oneOf' src/` shows no producer of the key. Remaining matches are only in tests that
  import a legacy `x-oneOf` document.
- The supervisor's fixture at `/tmp/zerx-lead-check` still gives the PLAN_J4 results, run with
  `CARGO_TARGET_DIR` outside the repo. If the fixture is gone (`/tmp` is volatile), report that to the
  Planner; it is not a code failure.
- `git diff --stat` touches only `src/json_schema.rs`, `src/types.rs`, `src/schema.rs`, `src/lua.rs`,
  `src/delta.rs` and `src/policy.rs`.
