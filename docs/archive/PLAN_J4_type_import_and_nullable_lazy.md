# PLAN_J4: Nullable lazy keeps its own `nullable`; JSON Schema `type` imports fail-closed

## Context & Goal

Commissioned by the external supervisor (DevLead of zerx, on the Product Owner's mandate) as patch
release 2.0.3. Both defects were reported by the Janus and endymion leads and reproduced against
HEAD 538f3c3. The supervisor's standing rule, from the Product Owner: zerx is strict by design. An
imported schema MUST accept nothing that the JSON Schema it was given forbids; where a construct cannot
be imported with its full meaning preserved, it is refused at import (`IMPORT_UNSUPPORTED` /
`IMPORT_MALFORMED`), never widened.

### Bug 1 — a nullable lazy loses its own `nullable`

`{"anyOf":[{"$ref":"#/$defs/S"},{"type":"null"}], "$defs":{"S":{"type":"object",…}}}` imports, via the
nullable collapse, as the memoised `lazy` for `S` with `.nullable()` applied. Validating `null` against
it fails with `type_mismatch` "expected object". The same node with `default: null` refuses an omitted
field because the materialised default `null` fails the same way. An inline object instead of the
`$ref` accepts `null`, so the collapse itself is correct.

Cause: `Schema::parse_present_body` in `src/schema.rs` resolves a `Lazy` and delegates to the inner
schema *before* the `nullable` null short-circuit, so the outer lazy's `nullable` is never consulted.
This contradicts `docs/architecture/json-schema.md` ("a `lazy` may carry its own modifiers"; export
already emits the nullable wrap around the `$ref`) and the present-value parse order in
`docs/architecture/schema-core.md` (depth guard → `nullable` short-circuit → …), which makes no
exception for `lazy`.

`validate_lua` (`src/lua.rs`) resolves lazies only to pick the Lua transform, and then calls
`parse_present` on the original schema. `parse_delta` (`src/delta.rs`) returns the unresolved field
schema and calls `parse_present` on it. Both get the fix without a change.

### Bug 2 — fail-open `type` import

`import_value` in `src/json_schema.rs` reads `type` only through `as_str()`. A type array such as
`["string","null"]` or `["integer","null"]`, an unknown type name, and a non-string `type` all fall
through to `any()`, so `{"type":["string","null"]}` accepts `5`.

`type` is ignored at further sites of the same walk:

| Schema | Input | Today |
|---|---|---|
| `{"type":"string","enum":[5]}` | `5` | accepted (enum dispatched before `type`, `type` never read) |
| `{"type":"string","const":5}` | `5` | accepted |
| `{"$ref":"#/$defs/S","type":"string"}`, `S` = `{}` | `5` | accepted (`$ref` skips steps 4–11) |
| `{"type":"string","format":"json"}` | `5` | accepted (`json()` accepts any value) |
| `{"type":"object","oneOf":[{"type":"string"},{"type":"number"}]}` | `"x"` | accepted (union ignores `type`) |
| `{"type":"object","format":"jsonschema"}` | `5` | accepted (`jsonschema()` accepts any value) |
| `{"type":"string","enum":["a","bb"],"minLength":2}` | `"a"` | accepted (the enum branch drops `minLength`) |

The built-in `sql` policy (`src/policy.rs`, `sql_nullable_normalise`) rewrites
`{"type":[T,"null"], …rest}` to `{"anyOf":[{…rest, "type":T}, {"type":"null"}]}`. Two defects follow
from that rewrite:
- It removes `oneOf` and `allOf` from the rewritten core, which widens the schema. An `allOf` the
  core would refuse disappears, and so does a `oneOf` restriction.
- The rewrite is exact only when no keyword in `rest` constrains `null`. With
  `{"type":["string","null"],"enum":["a"]}` it produces `enum(["a"]).nullable()`, which accepts
  `null`; JSON Schema forbids it.

Its element filter (`filter_map(as_str)`) also accepts malformed arrays such as
`["string",5,"null"]`. Today the other `sql` transforms (`sql_array_items_fallback`,
`sql_pg_type_substitute`) read `type` only as a string and rely on this rewrite for nullable columns,
so the rewrite stays; it is made exact.

**Goal:**
1. A `lazy`'s own `nullable` is honoured at parse with the same ordering as any other node.
2. `type` is honoured, or the node is refused, at every import site, under the rules in Step 2.
3. The `sql` normalisation only rewrites where the rewrite is exact.
4. (Supervisor addition.) An imported `type:"object"` + `format:"jsonschema"` node rejects non-objects.
5. (Supervisor addition.) An `enum`/`const` node with a sibling validation constraint that the enum
   branch would drop is refused at import. This includes `enum` and `const` on the same node.

Additions 4 and 5 are fail-closed only. Enforcing those constraints, and aligning `jsonschema()`'s
runtime with its export, are commissioned as the follow-up line.

### Interpretations settled in this plan

- **"Null passes only where the outer node is nullable."** This means some node on the reference
  chain admits null. A non-nullable `$ref` to a definition that itself admits null (e.g. `S` =
  `{"type":["object","null"]}`) accepts `null`, because JSON Schema does; a non-nullable `$ref` to a
  definition that does not admit null refuses it. Both are pinned.
- **Refine on a nullable lazy** follows ordinary nullable nodes: an explicit `null` under a nullable
  node returns `Null` without type check, validators or refinement predicates. That holds today for
  `string().nullable().refine(…)` and now holds for `lazy(…).nullable().refine(…)`. A non-null value
  under a nullable lazy runs the inner parse flow, then the outer lazy's refinements, as today.
- **`enum`/`const` with `type`** are refused when a member falls outside the declared type, not
  filtered. Both satisfy the standing rule. Refusal surfaces an author error instead of silently
  dropping a member the author wrote.
- **The `jsonschema` marker check** is attached at import as a refinement on the imported node;
  `check_jsonschema` is not changed. Refusing the marker would break the roundtrip of every
  `jsonschema()` schema, because it exports exactly this form. Changing the kind's runtime check is
  the follow-up's export/runtime alignment. The refinement is not exported, so export output is
  unchanged. The failure code is `refinement_failed`.
- **`enum`/`const` sibling constraints** are refused, not enforced. The dropped keywords are exactly
  the validation keywords `import_value` honours elsewhere: `minLength`, `maxLength`, `pattern`,
  `minimum`, `maximum`, `minItems`, `maxItems`, `items`, `prefixItems`, `properties`, `required` and
  `additionalProperties`. `format` is not among them: a format assertion is not part of JSON Schema's
  meaning, so dropping it widens nothing JSON Schema forbids. Annotations are reconstructed as today.
- **`type` next to `anyOf`/`oneOf`** is honoured, not refused outright, when the intersection is
  provably the union itself (every variant declares a `type` inside the outer set). That keeps the
  common `{"type":"object","oneOf":[{"type":"object",…},…]}` form importing exactly, and refuses
  everything else.

### Out of scope (reported to the supervisor, carded at finalization)

These fail-open import paths do not involve `type`. They were reproduced against 538f3c3 and stay
unchanged by this plan:
- Validation keywords zerx does not model (`exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`,
  `uniqueItems`, `minProperties`, `maxProperties`, …) are routed to `meta`.
- Keywords next to `$ref` other than `type` are ignored.
- Validation keywords next to `enum`/`const`: this plan refuses them (addition 5); enforcing them is
  the follow-up's job. Still ignored: validation keywords next to `anyOf`/`oneOf` (the nullable
  collapse included), and `enum`/`const` next to `anyOf`/`oneOf`.
- A programmatic `jsonschema()` accepts any value although it exports as `type:"object"`. This plan
  checks only the imported form (addition 4).
- `oneOf` without a valid discriminator loses exclusivity.

## Breaking Changes

Yes, by intent: schemas that imported before are now refused at import, and some values that were
accepted are now rejected. No public API, error code, or export output changes.

Newly refused at `from_json_schema` (previously imported, mostly as `any()` or with `type` ignored):
- `type` arrays other than exactly one non-null type plus `"null"`: `["string"]`, `["null"]`,
  `["string","number"]`, `["string","number","null"]` (→ `IMPORT_UNSUPPORTED`).
- Empty `type` arrays, arrays with non-string or duplicate elements, unknown type names such as
  `"str"` or `"int"`, and non-string non-array `type` values such as `5`, `null` or `{}`
  (→ `IMPORT_MALFORMED`).
- `enum`/`const` with a member or value outside the declared `type` (→ `IMPORT_UNSUPPORTED`).
- `type` next to `$ref` (→ `IMPORT_UNSUPPORTED`).
- `type` next to `format:"json"` (→ `IMPORT_UNSUPPORTED`).
- With or without `type`, an `enum`/`const` next to any of `minLength`, `maxLength`, `pattern`,
  `minimum`, `maximum`, `minItems`, `maxItems`, `items`, `prefixItems`, `properties`, `required` or
  `additionalProperties`, and `enum` together with `const` on one node (→ `IMPORT_UNSUPPORTED`).
- `type` next to `anyOf`/`oneOf` where some variant does not declare its own `type` inside the outer
  type set — including `$ref`, `enum`/`const`-only and boolean variants (→ `IMPORT_UNSUPPORTED`).
- Under `policy: "sql"`, nodes with `type:[T,"null"]` plus `oneOf`/`allOf`: previously the `oneOf`/`allOf`
  was dropped. Now the node reaches the core unchanged. There an `allOf` is refused and a `oneOf` takes
  the anyOf/oneOf rule above.
- Under `policy: "sql"`, a node with `oneOf:[X, {"type":"null"}]` and a sibling outside `$defs`,
  `$schema`, annotations and `x-` keys is no longer rewritten (previously the siblings were dropped). It
  reaches the core unchanged. With a `type` sibling it takes the anyOf/oneOf rule above; without one it
  imports as a union, with the other siblings handled as on any `oneOf` node.

Newly rejected at validation:
- Under `policy: "sql"`, `{"type":[T,"null"],"enum":[…]}` (or `const`) without `null` among the members
  now rejects `null`.
- An imported `{"type":"object","format":"jsonschema"}`, nullable form included, rejects non-object
  values with `refinement_failed`.

Newly accepted at validation (the bug fix): `null` under a nullable `lazy`, and an omitted field whose
nullable lazy carries `default: null`.

Consumer recovery: a refused schema must state its intent exactly. Examples: `anyOf` instead of a
multi-type array, `null` added to an `enum` that is meant to admit it, the `type` sibling dropped
where the variants already type themselves.

## Reference Patterns

- `src/schema.rs` — `parse_present_body`, in-module tests `nullable_and_explicit_null`, `lazy_memoisation`.
- `src/json_schema.rs` — `import_value`, `is_bare_null_schema`, in-module tests `import_nullable_collapse`,
  `import_unsupported`, `import_ref_defs`, `import_error_path` (helpers `from`, `validate_zv`, `obj`).
- `src/policy.rs` — `sql_nullable_normalise`, tests `test_sql_nullable_array_type`, `import_sql`, `re_export`.
- `src/lua.rs` — test `nil_under_nullable_string`.

## Assumptions & Risks

- **JSON Schema `type` grammar (Draft 2020-12 validation vocabulary, §6.1.1):** a string, or a
  non-empty array of unique strings, each one of `null`, `boolean`, `object`, `array`, `number`,
  `string`, `integer`. `integer` is any number with a zero fractional part and is a subset of `number`.
  Anything outside this grammar is malformed. A well-formed form other than a single name or
  `[T,"null"]` is unsupported.
- **Exactness of the `[T,"null"]` split.** `{"type":[T,"null"], K…}` means "(T or null) and K". The
  rewrite to "(T and K) or null" is exact iff no keyword in K constrains `null`. Type-specific keywords
  such as `minLength`, `properties`, `items` and `required` do not apply to `null`. The keywords that
  can constrain `null` are `enum`, `const`, `anyOf`, `oneOf`, `allOf`, `not`, `$ref`, `$dynamicRef`, `if`,
  `then` and `else`. The same reasoning backs core Step 2.5's treatment of `type:[T,"null"]` as
  `T`-with-siblings plus `.nullable()`, because the core reaches that dispatch only after
  `enum`/`const`/`anyOf`/`oneOf`/`$ref` have been handled and `allOf`/`not` refused. `if`/`then`/`else`/
  `$dynamicRef` are refused in step 13.
- **Risk: consumers hit new refusals.** This is intended. The list is in Breaking Changes and goes into the
  completion report.

## Steps

### 1. `src/schema.rs` — nullable short-circuit before lazy delegation

In `parse_present_body`, move the existing block

```rust
if value.is_null() && self.modifiers.nullable {
    return Ok(ZerxValue::Null);
}
```

above the `if let SchemaKind::Lazy(lzy) = &self.kind` branch. Change nothing else. Lazy delegation,
the outer lazy's `run_refine` on non-null results, depth accounting (`parse_present` still wraps the
body with `enter`/`exit`) and `parse_field`'s default handling stay as they are. Update the comment on
the lazy branch so it no longer implies the lazy is the first step.

### 2. `src/json_schema.rs` — parse and honour `type`

**2.1 Type model (private to the module).**

```rust
enum TypeName { Null, Boolean, Object, Array, Number, Integer, String }
struct TypeSpec { name: TypeName, nullable: bool }
```

A `TypeSpec` admits a type name `x` iff `x == name`, or `x == Integer && name == Number`, or
`x == Null && nullable`. Add:
- `fn parse_type(obj, path) -> Result<Option<TypeSpec>, ZerxError>`
  - absent → `Ok(None)`;
  - a string naming one of the seven → `TypeSpec { name, nullable: false }`;
  - an unknown string → `IMPORT_MALFORMED`;
  - an array of exactly two distinct known names, one `"null"`, in either order →
    `TypeSpec { name: <the other>, nullable: true }`;
  - an array that is empty, holds a non-string, holds an unknown name, or holds a duplicate →
    `IMPORT_MALFORMED`;
  - any other well-formed array (one element, two non-null names, three or more names) →
    `IMPORT_UNSUPPORTED`. One-element arrays such as `["string"]` would be representable. They are
    refused deliberately, because the supervisor's commission admits only the canonical forms. The
    message and the doc subsection say "only a single type name or `[T, "null"]` is accepted", not
    "not representable";
  - any other JSON value (number, bool, null, object) → `IMPORT_MALFORMED`.
  - Every error carries `path` and a message naming `type`.
- `fn admits_value(spec, &serde_json::Value) -> bool`, for `enum`/`const` members. The value's type name
  is `Null`/`Boolean`/`String`/`Array`/`Object` by JSON kind. A number is `Integer` when `as_i64()` or
  `as_u64()` is `Some`, or its `f64` is finite with `fract() == 0.0`; otherwise it is `Number`.
  `admits_value` returns whether `spec` admits that name.
- `fn within(inner: &TypeSpec, outer: &TypeSpec) -> bool`: `outer` admits `inner.name`, and
  (`!inner.nullable` or `outer` admits `Null`).

**2.2 Wire it into `import_value`.**
- **Step 2.5 (new), after the non-object guard and before `$ref`:**
  `let spec = parse_type(obj, path)?;`. This runs on every object node, including `$ref` nodes, so a
  malformed `type` is never skipped.
- **Step 3 (`$ref`):** if `spec.is_some()`, return `IMPORT_UNSUPPORTED` ("`type` alongside `$ref` is
  not supported"). It applies before the ref is resolved.
- **Step 5 (nullable collapse) and Step 6 (`anyOf`/`oneOf`):** when `spec` is `Some(outer)`, after the
  variants are imported, every member of the `anyOf`/`oneOf` array (the bare-null member of a collapse
  included) must be a JSON object whose own `type` parses to `Some(v)` with `within(v, outer)`.
  - If any member fails, return `IMPORT_UNSUPPORTED` naming the keyword and the member index, at the
    node path.
  - Import members first, so a member's own malformed `type` surfaces at the member's path.
  - The resulting union or collapsed schema is not given an extra `.nullable()` from `outer`. The
    variants already decide null.
- **Steps 7/8 (`enum`/`const`), sibling guard:** when the node has `enum` or `const` and any of the
  keywords listed under *Interpretations* (`minLength` … `additionalProperties`), return
  `IMPORT_UNSUPPORTED` naming the first such keyword, at the node path. This applies whether or not
  `type` is present, and runs before the enum/literal is built. `enum` and `const` on the same node are
  refused the same way (`IMPORT_UNSUPPORTED`): today step 7 builds the enum and step 8 never reads the
  `const`, so the `const` would be dropped.
- **Steps 7/8 (`enum`/`const`), type membership:** when `spec` is `Some(t)`, every `enum` member (resp. the `const`
  value) must satisfy `admits_value(t, member)`. Otherwise return `IMPORT_UNSUPPORTED` naming the
  first offending member index (resp. `const`).
  - On success, build the enum/literal exactly as today and do **not** apply `.nullable()` from `t`.
    `null` passes only if it is a member.
- **Step 9 (format markers):** use `spec`'s `name` in place of today's `typ` string.
  - `format:"json"` with `spec.is_some()` → `IMPORT_UNSUPPORTED`; with `type` absent it stays `json()`.
  - The `record`, `jsonschema`, `buffer`, `uri` and `url` markers match on `name` (so
    `{"type":["string","null"],"format":"buffer"}` builds `buffer()`).
  - The `jsonschema` marker builds `jsonschema()` with a refinement that accepts only
    `ZerxValue::Object` (message `expected object`). The refinement goes on before the type array's
    `.nullable()`, so `null` still short-circuits under `["object","null"]`.
- **Step 10 (type dispatch):** dispatch on `spec`'s `name` instead of the `typ` string. Today's arms stay
  as they are, including sibling validators (`minLength`, `maximum`, `minItems`, …) and the
  `integer` → `number().int()` mapping.
- **Nullable from the type array:** if the base came from Step 9 or Step 10 and `spec.nullable`, apply
  `.nullable()` to the base right after it is built, before Step 12 annotations. A base from Steps
  3/5/6/7/8 never receives it.
- **Fallthrough:** only `spec == None` with no other base-producing keyword reaches `any()`. Every
  `Some(spec)` produces a base in Steps 3–10 or an error. Make that explicit: the final `match` arm for
  an unhandled name is unreachable by construction, so write the dispatch over `TypeName` exhaustively,
  with no `_ => None` arm.
- Remove the now-unused `typ` local. `is_bare_null_schema` stays as it is: `{"type":["null"]}` is not
  bare, so it is imported as a variant and refused there.

Keep the step comments in `import_value` consistent with the new step 2.5 and the changed steps.

### 3. `src/policy.rs` — make the `sql` nullable split exact

Rewrite the type-array branch of `sql_nullable_normalise`:
- It applies only when `type` is an array of exactly two elements, both strings, one `"null"` and the
  other a different string, and the node holds none of
  `enum`, `const`, `anyOf`, `oneOf`, `allOf`, `not`, `$ref`, `$dynamicRef`, `if`, `then`, `else`.
  Otherwise the node is returned unchanged, and the core decides (Step 2).
- The rewrite produces `{"anyOf":[<core>, {"type":"null"}]}`. `<core>` is the node with `type` set to
  the non-null name and with `$defs` and `$schema` removed. Those two keys, when present, move to the
  outer node, so a root-level `$defs` stays where `ImportContext` reads it.
- It no longer removes `oneOf` or `allOf`; such nodes are not rewritten at all.

Restrict the `oneOf([X, {"type":"null"}])` branch. Today it rebuilds the node as
`{"anyOf":[X, {"type":"null"}]}` and drops every sibling key, `type` included. Under `sql`,
`{"type":"string","oneOf":[{"type":"number"},{"type":"null"}]}` thus becomes `number().nullable()`.
New rule:
- It fires only when every other key of the node is in this set: `$defs`, `$schema`, the annotation
  keywords `description`, `title`, `default`, `examples`, `deprecated`, `readOnly` and `writeOnly`, and
  keys starting with `x-`.
- These keys move unchanged onto the outer `anyOf` node, where the core reconstructs them on the
  collapsed schema (Step 12) or as `meta`, as it does for any node.
- A node with any other sibling (`type`, `enum`, `const`, `minLength`, `format`, …) is returned
  unchanged, and the core decides, honouring or refusing `type` per Step 2.

For a node of the allowed shape the rewrite is exact. The core imports `oneOf` without a discriminator
as a first-match union, so turning it into `anyOf` changes no acceptance. Annotations are no longer
dropped.

### 4. Tests

Extend the existing in-module `mod tests` blocks. `docs/tests.md` needs no new suite. Boundary: unit.
Perspectives: failure mode, conformance, round-trip, feature contract. Name tests by behaviour, not by
plan ID. Assert error **codes**, not messages, except where a message must name `type`.

**`src/schema.rs`** (builder level):
- `lazy(|| object(…)).nullable()` accepts `Null` and returns `Null`. The same lazy without `.nullable()`
  refuses `Null` with `type_mismatch`. A wrong object content under the nullable lazy still fails.
- Refine parity: with an always-false refinement, `string().nullable().refine(…)` and
  `lazy(…).nullable().refine(…)` both accept `Null`. Both reject a non-null valid value with
  `refinement_failed`.
- Default: an object field `lazy(…).nullable().default(ZerxValue::Null)` yields `Null` in output when
  omitted. A non-null default on a lazy still parses through the inner schema.
- A non-nullable lazy over a nullable inner (`lazy(|| string().nullable().into())`) accepts `Null`: the
  inner node admits it.

**`src/json_schema.rs`**:
- Nullable `$ref` (Majordomo pin). Use `S` = an object with a required string `format` property and
  `additionalProperties:false`, and an object with property `sig` = `anyOf:[{$ref S},{type:null}]`.
  - The three Janus cases:
    - explicit `sig: null` is accepted;
    - `sig` omitted, not in `required`, no default, is accepted and absent from output;
    - `sig` omitted with `default:null` on the property node is accepted, with `sig: Null` in output.
  - `sig` in `required`, omitted, no default → `required`.
  - `sig: {"format": 1}` and `sig: 5` are rejected. The root node `anyOf:[{$ref S},{type:null}]` also
    accepts `null` and accepts a valid `S`.
  - A non-nullable `{$ref S}` refuses `null`.
  - `$ref` to `S2` = `{"type":["object","null"], …}` accepts `null`.
  - Export → import → validate of the nullable-lazy schema keeps accepting `null`.
- `type` arrays:
  - `["string","null"]` and `["null","string"]` accept `"a"` and `null` and reject `5`.
  - `["integer","null"]` accepts `3` and `null` and rejects `"x"` and `1.5`.
  - Sibling constraints survive: `{"type":["string","null"],"minLength":2}` rejects `"a"` and accepts
    `null`.
  - `{"type":["array","null"],"items":{"type":"string"}}` rejects `[1]`.
  - `{"type":["object","null"],"properties":{…}}` stays strict on unknown keys.
  - `{"type":["string","null"],"format":"buffer"}` builds a nullable buffer.
- Absent `type`: `{}` still imports as `any()` and accepts `5`.
- Unknown type (Majordomo pin): `{"type":"str"}`, and `display_name: {"type":"strin"}` inside an object,
  are refused with `import_malformed`, and the error path points at the node.
- Refused forms, by code:
  - `import_malformed`: `type` `5`, `null`, `{}`, `true`, `[]`, `["string",5]`, `["string","string"]`,
    `["str","null"]`.
  - `import_unsupported`: `["string"]`, `["null"]`, `["string","number"]`, `["string","number","null"]`.
- `enum`/`const` with `type`:
  - `{"type":"string","enum":[5]}` and `{"type":"string","const":5}` → `import_unsupported`;
  - `{"type":"integer","enum":[1,2.5]}` → `import_unsupported`;
  - `{"type":"integer","enum":[1,2.0]}` imports;
  - `{"type":"number","const":3}` imports;
  - `{"type":["string","null"],"enum":["a"]}` and `{"type":["string","null"],"const":"a"}` import and
    **reject `null`**;
  - `{"type":["string","null"],"enum":["a",null]}` imports and accepts `null`.
- `enum`/`const` sibling guard:
  - `{"type":"string","enum":["a","bb"],"minLength":2}`, `{"enum":["a"],"pattern":"^a$"}`,
    `{"type":["string","null"],"const":"a","maxLength":1}`, `{"type":"number","enum":[1,2],"minimum":0}`
    and `{"type":"object","const":{},"required":["x"]}` → `import_unsupported`;
  - `{"enum":["a","b"],"const":"a"}`, with and without `"type":"string"`, → `import_unsupported`;
  - `{"type":"string","enum":["a"],"description":"d","default":"a"}` and
    `{"type":"string","enum":["a"],"format":"email"}` still import.
- `jsonschema` marker:
  - `{"type":"object","format":"jsonschema"}` accepts `{}` and an object, and rejects `5` and `"x"` with
    `refinement_failed`;
  - `{"type":["object","null"],"format":"jsonschema"}` accepts `null` and rejects `5`;
  - export → import → export of a programmatic `jsonschema()` is unchanged, and the imported schema
    rejects `5`.
- `$ref` with `type` sibling → `import_unsupported`, also when the def is `{}`.
- `{"type":"string","format":"json"}` → `import_unsupported`; `{"format":"json"}` still imports as `json()`.
- `anyOf`/`oneOf` with `type`:
  - `{"type":"object","oneOf":[{"type":"string"},{"type":"number"}]}` → `import_unsupported`;
  - `{"type":"object","oneOf":[{"type":"object",…},{"type":"object",…}]}` imports and validates as
    before;
  - `{"type":"number","anyOf":[{"type":"integer"},{"type":"number","minimum":5}]}` imports;
  - an untyped `{$ref …}` variant under a `type` sibling → `import_unsupported`;
  - `{"type":"object","anyOf":[{"type":"object",…},{"type":"null"}]}` → `import_unsupported`;
  - `{"type":["object","null"],"anyOf":[{"type":"object",…},{"type":"null"}]}` imports and accepts
    `null`.
- Round-trip: every existing export→import test stays green unchanged. No export output changes.

**`src/policy.rs`** (`sql` policy):
- `{"type":["string","null"]}` and `{"type":["integer","null"]}` accept `null` and the base type, and
  reject the other (existing test extended).
- `{"type":["string","null"],"format":"bytea"}` still becomes a nullable buffer, and
  `{"type":["array","null"]}` without `items` still imports via the items fallback.
- `{"type":["string","null"],"enum":["a"]}` imports and rejects `null`; with `null` among the members it
  accepts `null`.
- `{"type":["object","null"],"allOf":[…]}` → `import_unsupported`, no longer silently dropped.
  `{"type":["string","null"],"oneOf":[{"type":"string"},{"type":"null"}]}` imports and accepts `null` and
  `"a"`.
- `{"type":["string",5,"null"]}` → `import_malformed`.
- A root `{"type":["object","null"],"properties":{"a":{"$ref":"#/$defs/A"}},"$defs":{"A":{"type":"string"}}}`
  imports, keeping `$defs` reachable.
- Existing `test_sql_nullable_one_of` stays green.
- `oneOf` branch:
  - `{"type":["string","null"],"oneOf":[{"type":"number"},{"type":"null"}]}` and
    `{"type":"string","oneOf":[{"type":"number"},{"type":"null"}]}` → `import_unsupported`;
  - `{"oneOf":[{"type":"number"},{"type":"null"}],"description":"d","default":null}` imports as a
    nullable number that keeps `description` and `default` (re-export shows both, and an omitted
    property yields `Null`);
  - `{"oneOf":[{"type":"number"},{"type":"null"}],"$defs":{…}}` at the root keeps its `$defs` reachable.

**`src/lua.rs`** (feature-gated): `validate_lua` with `LuaValue::Nil` against
`lazy(|| object(…)).nullable()` returns `Null`; against the same lazy without `.nullable()` it fails with
`type_mismatch`.

## Documentation

At finalization, after verification. Basis for every constraint below: the Product Owner's durable
standing rule, relayed by the external supervisor, that an imported schema accepts nothing its JSON
Schema forbids and an unpreservable construct is refused at import. Behind it stand the product goals
`Strict validation` and semantically equivalent JSON Schema roundtrip in `docs/definition.md`.

- `docs/architecture/schema-core.md`, present-value parse flow: add that for a `lazy` the outer lazy's own
  `nullable` short-circuit precedes resolution and delegation. The resolved inner schema then runs its own
  full parse flow, followed by the outer lazy's refinement predicates. Basis as above, and the load-bearing
  invariant that the universal modifier set applies to every schema including `lazy` (`D-modifier-composition`).
- `docs/architecture/json-schema.md`:
  - **Import pipeline order:** insert `type` parsing as a step between the non-object guard and `$ref`.
    The `$ref` skip rule is unchanged otherwise.
  - **New subsection `type` import:** the accepted forms (absent; one known name; exactly one non-null
    known name plus `"null"`, either order, which imports as that type plus `.nullable()` and keeps its
    sibling constraints) and the refusals with their codes (Step 2.1).
  - **`enum`/`const` import:** add the membership rule; state that a nullable type array adds no
    `null`; add the sibling guard (the listed validation keywords next to `enum`/`const` →
    `IMPORT_UNSUPPORTED`).
  - **`$ref` import:** a `type` sibling is refused.
  - **`oneOf`/`anyOf` import and nullable collapse:** add the `type` sibling rule (every member declares a
    `type` within the outer set, else `IMPORT_UNSUPPORTED`).
  - **Format-marker kind mapping:** `format:"json"` with a `type` is refused; markers match the
    non-null name of a nullable type array; the imported `jsonschema` marker rejects non-objects.
  - **Import fallibility and error codes:** add the `type` cases to the `IMPORT_MALFORMED` and
    `IMPORT_UNSUPPORTED` lists.
- `docs/architecture/json-schema.md` gains a `## Splitting Rationale` section. The additions take the
  file past 200 lines, so M8 requires one. It states why export and import stay in one concern: they are
  inverse mappings of the one JSON Schema boundary, and each import rule is checked against the export
  rule it inverts.
- `docs/architecture/policy.md`, built-in `sql` nullable normalisation: replace both rules.
  - The type-array split applies only to an exact two-element `[T,"null"]` with no null-constraining
    keyword, keeps `$defs` and `$schema` on the outer node, and drops nothing.
  - The `oneOf` rewrite applies only when every sibling is `$defs`, `$schema`, an annotation or an `x-`
    key; those keys move to the outer node.
- `docs/bug.kanban.md`:
  - one card per out-of-scope fail-open path listed in Context;
  - a card for validation keywords next to a format marker. For example,
    `{"type":"object","format":"jsonschema","required":["x"]}` drops `required`; the same happens
    with the record/buffer/uri/url markers.

No register entry is proposed or superseded. A general "import fails closed" decision would make the
existing `meta` routing of unmodelled keywords a register contradiction. It belongs with the follow-up
plan for those cards. `docs/tests.md` is unchanged. `docs/decisions.md` is unchanged.

**Version:** patch, 2.0.2 → 2.0.3 in `Cargo.toml` and `Cargo.lock`, applied by the Planner at finalization.

## Verification

Required gate: `scripts/verify all` (lint, `test`, `test core`, `api`) passes on the final functional state.
`test core` matters because this plan adds a `#[cfg(feature = "lua")]` test.

Acceptance checks, run and reported by the Coder:
- The supervisor's fixture at `/tmp/zerx-lead-check` (path dependency on this repo), run with
  `CARGO_TARGET_DIR` outside the repo. Expected:
  - `type[string,null] with 5` and `type[integer,null] with "x"` → `Err(type_mismatch)`;
  - `anyOf[$ref,null] null` → `Ok(Null)`;
  - `anyOf[inline obj,null] null` → `Ok(Null)`;
  - `anyOf[$ref,null] obj ok` → `Ok`;
  - `anyOf[$ref,null] bad obj` → `Err`.
- `git diff --stat` touches only `src/schema.rs`, `src/json_schema.rs`, `src/policy.rs` and `src/lua.rs`.
  No export function (`export_node`, `apply_annotations`, `wrap_nullable`, `base_schema`) changes.
