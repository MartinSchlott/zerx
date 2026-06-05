# PLAN_J2_import

## Context & Goal

Second and final plan of **Phase 3 — JSON Schema roundtrip** in the
`CONCEPT_zerx_foundation` execution. Phase 1 (`PLAN_F3_error_model`,
`PLAN_F1_value_model`, `PLAN_F2_schema_core`), all of Phase 2
(`PLAN_T1_basic_types`, `PLAN_T2_complex_types`, `PLAN_T3_object_utilities`,
`PLAN_T4_special_types`) and `PLAN_J1_export` are complete and archived. Export
(`to_json_schema` / `to_json_schema_with`) is implemented in `src/json_schema.rs`
and its contract is fixed in `docs/architecture/json-schema.md` **Constraints**.

J2 delivers **`from_json_schema`**: the one-directional import from a JSON Schema
(Draft 2020-12) `serde_json::Value` back to a `Schema` value. It is the plan that
**proves full roundtrip stability** — `schema → to_json_schema → from_json_schema`
yields a semantically equivalent schema, with `$defs`/`$ref`, cycles, and format
markers surviving (`definition.md` Success Criteria; vision §"JSON Schema
roundtrip (details)"). `from_json_schema` is **fallible** (C7): it returns
`Result<Schema, ZerxError>`.

J2 covers, per vision §"JSON Schema roundtrip (details)" (lines 276-289):

- the AST walk over a JSON Schema `Value`, reconstructing every kind and the
  validators/modifiers J1 emits, as the exact inverse of J1's export walk;
- `$ref` resolution against `$defs` with **memoised lazy placeholders**, so shared
  and **cyclic** subschemas survive the roundtrip and re-export to a single
  `$defs` entry each;
- all four `additionalProperties` shapes (`true` / `false` / absent / schema
  object);
- the `nullable` collapse: `anyOf:[<core>, {"type":"null"}]` → `<core>.nullable()`
  (the symmetric inverse of J1's nullable wrap, declared in J1's Assumptions);
- `oneOf` → discriminated union when a `discriminator` is present and all variants
  are objects, else a plain union carrying `x-oneOf` metadata;
- `anyOf` → union;
- `allOf` / `not` raise clear errors instead of being silently ignored;
- default application on import, with defaulted object properties kept
  non-optional (matching the runtime invariant).

### Boundary (`docs/architecture/json-schema.md`)

- `json-schema` owns import. J2 owns: the import walk (`import_value`), the
  `ImportContext` (`$defs` table + lazy memoisation + cycle guard), the
  keyword→modifier reconstruction (the inverse of J1's `apply_annotations`), the
  `additionalProperties`/`required`→object-mode/optional derivation, the
  `nullable` collapse, the union/oneOf reconstruction, and the public
  `from_json_schema` surface plus its import `ErrorCode`s.
- `json-schema` does **NOT** define the types it reconstructs (`types`) nor the
  `Schema` representation / parse flow (`schema-core`). J2 calls the existing
  **public constructor + builder surface** (`string()`, `object()`, `.min()`,
  `.optional()`, `.nullable()`, …) exactly as a library user would; it adds no
  `SchemaKind` variant and no parse-flow logic.
- `json-schema` does **NOT** own the import **policy pipeline** — that is
  `PLAN_P1_policy_pipeline` (Phase 4). J2 delivers the bare core importer
  `from_json_schema(&Value)`. P1 will wrap this core with pre-parse schema
  transforms and post-parse type transforms; to keep that seam available without
  pre-empting P1, the internal walk entry (`import_value`) is `pub(crate)`. J2
  adds **no** `ImportOptions`, no `policy` argument, no `register_policy`, no
  `deref`, and no `sql` policy.
- The host-opaque `function`/`tvalue` markers are **out of scope**: those
  `SchemaKind` variants do not exist until `PLAN_M1_host_opaque`. A JSON Schema
  carrying `format:"function"` / `format:"tvalue"` imports best-effort as `any()`
  with the marker preserved via the **`format` modifier** (`any().format(
  "function")`), so it re-exports its `format` faithfully — **not** via `meta`,
  because J1 re-emits `format` only from `modifiers.format` and suppresses reserved
  keys in the `meta` step (`src/json_schema.rs:203-213`), so a `format` routed into
  `meta` would be dropped on re-export. M1 may later add dedicated arms. J2 does
  **not** introduce them.

## Breaking Changes

**No.** J2 is purely additive: new private/`pub(crate)` items in the existing
`src/json_schema.rs` module, one new public free function
(`from_json_schema`), and two new import `ErrorCode` constants declared in a
`json_schema.rs` `impl ErrorCode` block (no edit to `src/error.rs`). One new
re-export line in `src/lib.rs`. No existing signature, type, or behaviour
changes; J1's export code and tests are untouched.

## Reference Patterns

- `src/json_schema.rs` (J1 export) — the central exhaustive `base_schema` match
  and `apply_annotations`. J2's `import_value` is the **inverse**: every keyword
  J1 emits, J2 reads back. Keep the two walks visibly symmetric; a reader should
  be able to line up an export arm with its import arm.
- `src/types.rs` public constructors and builders — the exact surface J2 calls:
  `string()/number()/boolean()/null()/any()/enumerate()` (basic),
  `object()/array()/record()/tuple()/union()/discriminated_union()/literal()`
  (complex, `src/types.rs:1198-1338`), `buffer()/uri()/url()/json()/jsonschema()`
  (special). Builder validators: `StringSchema::{min,max,regex,pattern,email,uuid}`
  (`src/types.rs:518-555`), `NumberSchema::{min,max,int}` (`src/types.rs:557-572`),
  `ArraySchema::{min,max}` (`src/types.rs:1182-1192`),
  `ObjectSchema::{passthrough,strip,partial}` (`src/types.rs:1080-1107`). Universal
  modifiers via `Modify` (`src/schema.rs:414-490`):
  `optional/nullable/default/describe/format/mime_format/deprecated/read_only/write_only/meta/example/title`.
- `src/schema.rs:530` `lazy(f)` and `src/schema.rs:97-137` `Lazy` — the memoised
  thunk J2 uses for `$ref` placeholders; clones of one `lazy` share one resolution
  cell, which is exactly what makes a cyclic import re-export to a single `$defs`
  entry.
- `src/schema.rs:14-19` and `src/types.rs:1373` — the `impl ErrorCode { pub const
  … }` pattern J2 copies to declare its import codes in `src/json_schema.rs`.
- Zex reference (`/Users/martinschlott/Documents/MyProjects/zex/src/zex/json-schema-import.ts`)
  — `fromJsonSchemaInternal` is the canonical AST walk: `$ref`/lazy memoisation
  (lines 357-380), `allOf`/`not`/`oneOf` handling (lines 408-424), per-type
  reconstruction (lines 426-647), object `required`/`additionalProperties` mapping
  (lines 573-617), discriminator detection (lines 626-638). **J2 deviates from Zex
  where J1's export or the vision differs** (see Assumptions): zerx's buffer marker
  is `type:"string"` not `type:"object"`; zerx's url marker is `format:"url"` not
  `uri-reference`; zerx collapses nullable-`anyOf` in the **core** importer, not in
  the `sql` policy; the `sql` policy and all transforms belong to P1, not J2.

## Dependencies

None new. `serde`, `serde_json`, and `regex` are already core dependencies (C9).
J2 uses `serde_json::{Value, Map}` and standard-library `Rc`/`RefCell`/`HashMap`/
`HashSet` only.

## Assumptions & Risks

The following import choices are **concept-internal roundtrip constraints**,
recorded as `json-schema.md` **Constraints** at Doc Update (the symmetric
counterpart to J1's export Constraints). They are deliberately **not** written to
`docs/decisions.md`: under the concept's lifecycle discipline
(`CONCEPT_zerx_foundation.md` §"Decision-reference discipline";
`.claude/rules/decisions.md` "Promotion") **no plan writes the register
mid-concept** — register migration is the Architect's Concept Closeout step (§8),
and the concept settles roundtrip behaviour in the concern file
(`json-schema.md` `Related Decisions`: "roundtrip behaviour is settled within the
JSON Schema plans"). Where these choices deviate from Zex, the deviation follows
J1's export contract and the vision, which are authoritative for zerx.

- **Inverse-of-J1 principle.** Every keyword J1 emits, J2 reads back to the
  modifier/validator/kind that produced it. This is the load-bearing roundtrip
  guarantee. In particular the annotation keywords J1's `apply_annotations`
  writes — `description`, `title`, `default`, `examples`, `deprecated`,
  `readOnly`, `writeOnly`, `contentMediaType`, user `format` — are **export-
  reserved** in J1's `meta` step, so if J2 routed them into `meta` they would be
  **dropped on re-export** and the roundtrip would lose them. J2 therefore MUST
  map each back to its modifier:
  `description`→`describe`, `title`→`title`, `default`→`default`,
  `examples`→`example` (first element; J1 writes `examples:[<example>]`),
  `deprecated:true`→`deprecated`, `readOnly:true`→`read_only`,
  `writeOnly:true`→`write_only`, `contentMediaType`→`mime_format`, and a
  non-marker `format`→`format`. Only genuinely non-standard keys (e.g. user
  `x-internal`, `x-ui-multiline`) go to `meta`.

- **`additionalProperties` four shapes (regular object).**
  `false`→`strict` (default; no relax call), `true`→`passthrough`, a **schema
  object**→`passthrough` (extra keys allowed; zerx does not model a per-extra-key
  value schema on a plain object — `record` covers that case via its format
  marker), **absent**→`strict`. Absent maps to **strict**, not to JSON Schema's
  permissive default: this is a deliberate fail-closed choice grounded in C4
  (strict-by-default is a security boundary, "never silently relaxed"). zerx's own
  export always writes `additionalProperties` explicitly, so the absent case never
  arises on a zerx roundtrip; permissive foreign schemas that need relaxing are the
  policy pipeline's concern (P1), not the core importer's. Recorded as a Constraint.

- **`required`/`optional` and defaults.** A property listed in `required` imports
  non-optional. A property **not** in `required` and **without** a `default`
  imports `.optional()`. A property **not** in `required` but **with** a `default`
  imports **non-optional** (the default supplies the value when the field is
  missing — matching zerx's runtime invariant and J1's `required` rule, where a
  defaulted field is excluded from `required` yet still produced). The `default`
  decision reads the property's own `default` keyword.

- **`nullable` collapse.** An `anyOf` of **exactly two** members where exactly one
  is a bare null schema (`{"type":"null"}`) imports as the **other** member with
  `.nullable()` applied — the exact inverse of J1's nullable wrap. Any other
  `anyOf` imports as a plain `union([...])`.

- **`oneOf` / `anyOf` / discriminator.** `oneOf` with a `discriminator`
  (`{"propertyName": <key>}`) and **all** variants reconstructing to object
  schemas imports as `discriminated_union(<key>, variants)`; otherwise `oneOf`
  imports as `union(variants)` carrying `meta("x-oneOf", true)` so the original
  `oneOf` intent survives a re-export (vision line 281). `anyOf` (after the
  nullable-collapse check) imports as `union(variants)`. A `discriminator`
  alongside `anyOf` is honoured the same way as on `oneOf` for foreign-schema
  robustness. "All variants are objects" is checked on the reconstructed
  `Schema`s (kind is `SchemaKind::Object`); the fallback-to-union path matches the
  vision's "fall back to a plain union on import when variants are not all
  objects".

- **`allOf` / `not`.** Both raise a clear `IMPORT_UNSUPPORTED` error with a
  message naming the keyword and the suggested alternative (`extend()` for
  composition; `union()` for `not`), never silently ignored (vision line 287).

- **Format markers drive the kind, before generic string/object handling.**
  `format:"buffer"` (with `type:"string"`, per J1) → `buffer()`, with
  `contentMediaType`→`.mime_format()`. `format:"record"` (with `type:"object"`) →
  `record(<additionalProperties schema, or any() if true/absent>)`; `format:
  "record"` with `additionalProperties:false` is an error (a record that forbids
  all keys is meaningless). `format:"json"` → `json()`. `format:"jsonschema"`
  (with `type:"object"`) → `jsonschema()`. `type:"string", format:"uri"` →
  `uri()`; `type:"string", format:"url"` → `url()`. On a plain string,
  `format:"email"`→`.email()`, `format:"uuid"`→`.uuid()`, any other format →
  `.format(<fmt>)` modifier.

- **`enum` and `const`.** A `const` keyword → `literal(<value>)` (value imported as
  `ZerxValue` via `ZerxValue::from_serialize`). An `enum` of **all strings** →
  `enumerate([...])` (zerx's `Enum` is string-only); an `enum` containing any
  non-string member → `union([literal(v), …])` (lossless fallback). `enum`/`const`
  are checked before the `type` dispatch so a foreign `{"type":"string","enum":…}`
  reconstructs the enum rather than a bare string.

- **Boolean schemas.** `true` → `any()`. `false` (a "reject everything" schema)
  → `IMPORT_UNSUPPORTED` (zerx has no "never" type). J1 emits booleans only as
  `items:false` (tuple) and `additionalProperties:true|false` (object), both
  consumed in their parent context; a **bare** boolean node reaches the generic
  importer only for foreign input.

- **Empty / meta-only schemas.** `{}` → `any()`. A schema with only annotation /
  extension keys and **no** structural keyword (`type`, `properties`, `items`,
  `prefixItems`, `enum`, `const`, `anyOf`, `oneOf`, `allOf`, `not`, `$ref`,
  `additionalProperties`, the numeric/string validators) → `any()` carrying its
  annotations/meta.

- **`x-ui-multiline` not reconstructed as a validator (accepted limitation).**
  J1 emits `string().multiline(n)` as the meta key `x-ui-multiline:n`. J2 imports
  it as a `meta` entry, **not** as a reconstructed `multiline` validator. Because
  `x-ui-multiline` is not export-reserved, the meta entry survives re-export, so
  the **JSON document** roundtrips exactly; only the runtime `multiline` validator
  is not rebuilt. Recorded as an accepted limitation in `json-schema.md`,
  consistent with "external schemas without markers import best-effort".

- **Risk — `$ref` cycle + lazy memoisation + Result.** The lazy thunk is
  `Fn() -> Schema` (infallible), but importing a `$def` is fallible. Mitigation
  (see Step 4): structural errors in a referenced `$def` are surfaced **eagerly**
  while building the schema (via `ensure_resolved`, which runs the fallible import
  once per referenced id before `from_json_schema` returns), so the thunk only ever
  **clones an already-imported** schema from a shared `Rc<RefCell<HashMap<…>>>` and
  cannot fail. The cycle guard (an `importing` id-set) makes a self/mutual
  reference return the already-created placeholder instead of recursing. A `$ref`
  to a missing `$defs` id, or a `$ref` whose target is not `#/$defs/<id>`, is an
  eager `IMPORT_MALFORMED` error.

- **Risk — lazy identity preserved across refs.** All `$ref`s to the same id MUST
  share **one** `lazy` instance (clones share the resolution cell), so re-export
  (J1 keys `$defs` on `Lazy::export_id` = `Rc::as_ptr` of that cell) emits a single
  `$defs` entry per id. Mitigation: the importer stores one placeholder `lazy` per
  id in the context and returns **clones** of it for every `$ref` to that id; a
  dedicated roundtrip test asserts a cyclic schema re-exports to exactly one
  `$defs` entry.

## Steps

### 1. Import `ErrorCode`s (`src/json_schema.rs`)

Add a `json_schema.rs` `impl ErrorCode` block (mirroring `src/schema.rs:14-19`),
no edit to `src/error.rs`:

```rust
impl ErrorCode {
    /// A JSON Schema feature zerx cannot represent (allOf, not, false-schema,
    /// unknown structural keywords, non-string-or-literal enum that cannot be
    /// represented). The schema is well-formed JSON Schema but out of zerx's model.
    pub const IMPORT_UNSUPPORTED: ErrorCode = ErrorCode::new("import_unsupported");
    /// A structurally invalid input (node is not an object/bool, array without
    /// items, record with additionalProperties:false, $ref to a missing or
    /// non-local target).
    pub const IMPORT_MALFORMED: ErrorCode = ErrorCode::new("import_malformed");
}
```

Every error MUST carry the failing node's `path` via `ZerxError::at(path)` so
consumers branch on `code` + `path` (C8), and a message naming the offending
keyword/shape.

### 2. Public surface (`src/json_schema.rs` + `src/lib.rs`)

```rust
/// Import a JSON Schema (Draft 2020-12) document back into a `Schema`. Fallible (C7).
pub fn from_json_schema(value: &serde_json::Value) -> Result<Schema, ZerxError> {
    let ctx = ImportContext::new(value);
    import_value(value, &ctx, &[])
}
```

`src/lib.rs`: add `from_json_schema` to the `json_schema` re-export
(`pub use json_schema::{ExportOptions, DRAFT_2020_12, from_json_schema};`).

### 3. `ImportContext` (`src/json_schema.rs`, private)

Holds the `$defs` table, the per-id lazy placeholders, the resolved-def store
(shared into the lazy thunks), and the cycle guard:

```rust
struct ImportContext {
    defs: serde_json::Map<String, serde_json::Value>,           // root["$defs"] (owned; empty if none)
    resolved: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Schema>>>, // id -> imported def
    placeholders: std::cell::RefCell<std::collections::HashMap<String, Schema>>,          // id -> lazy $ref node
    importing: std::cell::RefCell<std::collections::HashSet<String>>,                     // ids being imported (cycle guard)
}
```

- `new(root)` extracts `root["$defs"]` (an object, else empty map).
- `ref_schema(&self, id: &str, path: &[String]) -> Result<Schema, ZerxError>`:
  1. If `!self.defs.contains_key(id)` → `Err(IMPORT_MALFORMED)` ("$ref target
     `#/$defs/<id>` not found") `.at(path)`.
  2. If `placeholders` already has `id`, take a **clone** of it (shared lazy
     identity). Else build the placeholder lazy and insert it:
     ```rust
     let resolved = self.resolved.clone();
     let id_owned = id.to_string();
     let lz: Schema = lazy(move || {
         resolved.borrow().get(&id_owned).cloned()
             .expect("def is resolved before its $ref lazy is forced")
     }).into();
     ```
     Insert `lz.clone()` into `placeholders[id]`.
  3. Call `self.ensure_resolved(id, path)?` so the def is imported **eagerly**
     (surfacing structural errors before `from_json_schema` returns) and
     `resolved[id]` is populated before any thunk runs.
  4. Return the placeholder clone.
- `ensure_resolved(&self, id: &str, path: &[String]) -> Result<(), ZerxError>`:
  1. If `resolved.borrow().contains_key(id)` → `Ok(())`.
  2. If `importing.borrow().contains(id)` → `Ok(())` (cycle in progress; the
     placeholder already exists, so the ref above resolves to it).
  3. `importing.borrow_mut().insert(id)`; **drop the borrow**; clone
     `node = self.defs[id]`; `let s = import_value(&node, self, &[..,"$defs",id])?;`
     `resolved.borrow_mut().insert(id, s);`
     `importing.borrow_mut().remove(id);` `Ok(())`.
  Borrows MUST NOT be held across the recursive `import_value` call.

### 4. Import walk — `import_value` (`src/json_schema.rs`, `pub(crate)`)

`pub(crate) fn import_value(node: &serde_json::Value, ctx: &ImportContext, path:
&[String]) -> Result<Schema, ZerxError>`, in this fixed order (mirroring J1's
export walk, inverted). `path` is the JSON-pointer-ish segment list threaded for
error reporting (extend a `Vec<String>` per descent).

1. **Boolean schema.** `Value::Bool(true)` → `Ok(any().into())`.
   `Value::Bool(false)` → `Err(IMPORT_UNSUPPORTED)` ("a `false` schema rejects all
   values; zerx has no never type").
2. **Non-object guard.** If `node` is not an object → `Err(IMPORT_MALFORMED)`
   ("schema must be an object or boolean") `.at(path)`.
3. **`$ref`.** If `node["$ref"]` is a string: it MUST start with `#/$defs/`
   (else `IMPORT_MALFORMED`, "only local `#/$defs/<id>` refs are supported");
   `id = ref[8..]`; set `schema = ctx.ref_schema(id, path)?` and **continue to
   step 12** (do **not** return early). J1 exports a `lazy`'s own modifiers on the
   outer `$ref` node (`src/json_schema.rs:135-149`: it builds the `$ref` core, then
   runs `apply_annotations` and `wrap_nullable` on it), so the importer MUST
   reconstruct those annotations on the `$ref` node via step 12 — an early return
   would silently drop every modifier carried by a `lazy()` (e.g.
   `lazy(...).describe("…").deprecated()`), breaking the roundtrip. A `$ref`
   node's annotation/extension keywords are the only siblings J1 ever emits;
   structural sibling keywords on a `$ref` (rare, foreign — combining `$ref` with
   structural keywords is not representable in zerx) are ignored best-effort and do
   not reach the step-13 unsupported-keyword classification.
4. **`allOf` / `not`.** Present → `Err(IMPORT_UNSUPPORTED)` with the keyword-named
   message.
5. **`nullable` collapse.** If `node["anyOf"]` is an array of exactly 2 where
   exactly one element is a bare null schema (`{"type":"null"}` with no other
   structural keyword): import the non-null element, apply `.nullable()`, then
   continue to step 12 (apply the outer node's own annotations/meta). 
6. **`oneOf` / `anyOf`.** If `node` has `oneOf` (or `anyOf` not consumed by
   step 5): reconstruct each variant via `import_value`. If a `discriminator.
   propertyName` (`key`) is present, attempt a discriminated union but **validate
   the full `types` invariant first** — an object-kind check alone is insufficient:
   `types` requires each variant to carry a **required, non-optional,
   non-nullable, non-defaulted literal** discriminator field with **unique keyable**
   values (`docs/architecture/types.md`; `build_discriminator_state` in
   `src/types.rs:1256-1334`). Construct `discriminated_union(key,
   variants.clone())` and inspect the resulting `DiscriminatedUnionBody.state`
   (`pub(crate)`, reachable from this module): use the discriminated union **only
   if** the state is `DiscriminatorState::Ok`; if it is `DiscriminatorState::
   Invalid(_)` (or no discriminator is present), fall back to `union(variants)`.
   This reuses the canonical invariant check rather than re-implementing it, and
   prevents importing a foreign schema into a discriminated union that would later
   fail at parse with `ErrorCode::INVALID_DISCRIMINATED_UNION`
   (`src/types.rs:714-715`). When the source keyword was `oneOf` and the result is
   a plain union, attach `meta("x-oneOf", true)`. Continue to step 12.
7. **`enum`.** If `node["enum"]` is an array: all-strings →
   `enumerate(strings)`; otherwise `union(values.map(|v| literal(ZerxValue::
   from_serialize(v)?)))`. Continue to step 12.
8. **`const`.** If `node` has `const` → `literal(ZerxValue::from_serialize(c)?)`.
   Continue to step 12.
9. **Format-marker kinds** (checked before generic `type` handling):
   - `format=="json"` → `json()`.
   - `type=="object"` && `format=="record"` → record: if
     `additionalProperties==false` → `Err(IMPORT_MALFORMED)`; else
     `record(import(additionalProperties))` where a missing/`true`
     `additionalProperties` imports as `any()`.
   - `type=="object"` && `format=="jsonschema"` → `jsonschema()`.
   - `type=="string"` && `format=="buffer"` → `buffer()`, `contentMediaType`
     applied as `.mime_format()` in step 12.
   - `type=="string"` && `format=="uri"` → `uri()`.
   - `type=="string"` && `format=="url"` → `url()`.
10. **`type` dispatch** (the central reconstruction; one arm per J1 base kind):
    - `"string"` → `string()`, then `minLength`→`.min()`, `maxLength`→`.max()`,
      `pattern`→`.regex()` (the original source string), `format=="email"`→
      `.email()`, `format=="uuid"`→`.uuid()`. (A non-marker, non-email/uuid
      `format` is applied as the `format` **modifier** in step 12.)
    - `"number"` → `number()`, `minimum`→`.min()`, `maximum`→`.max()`.
    - `"integer"` → `number().int()`, `minimum`→`.min()`, `maximum`→`.max()`.
    - `"boolean"` → `boolean()`.
    - `"null"` → `null()`.
    - `"array"` with `prefixItems` (array): a zerx tuple is **closed** (fixed
      length, no typed/open tail — `parse_tuple` enforces exact length). Import as
      `tuple(prefixItems.map(import))` **only when `items` is `false`** — J1's
      closed-tuple export shape. If `prefixItems` is present with `items` being a
      **schema object**, `true`, or **absent** (Draft 2020-12 "heterogeneous head +
      typed/open tail"), zerx cannot represent it → `Err(IMPORT_UNSUPPORTED)`
      ("`prefixItems` with an open or typed tail is not representable as a zerx
      tuple") `.at(path)`; silently importing it as a closed tuple would drop the
      tail semantics. `minItems`/`maxItems` (J1 emits both `= n`) are redundant for
      the closed tuple and ignored.
    - `"array"` without `prefixItems` → array: `items` MUST be present
      (`IMPORT_MALFORMED` "array schema missing `items`" otherwise); `items==true`
      or `items=={}` → `array(any())`; else `array(import(items))`; then
      `minItems`→`.min()`, `maxItems`→`.max()`.
    - `"object"` (not a record/jsonschema marker) → object reconstruction
      (step 11).
11. **Object reconstruction.** Build `Vec<(String, Schema)>` from `properties`
    (preserving the map's key order) by importing each property; for each
    property decide optionality: in `required` → non-optional; not in `required`
    and the property node has **no** `default` → `.optional()`; not in `required`
    and the property node **has** a `default` → non-optional. Construct via
    `object(fields)`; then map `additionalProperties` to mode:
    `false`/absent → `strict` (no call — strict is the constructor default),
    `true` → `.passthrough()`, schema object → `.passthrough()`. (No `.strip()` on
    import: J1 exports both `strict` and `strip` as `false`, so `false` always
    imports as `strict` — the strip-roundtrip limitation already recorded in J1.)
12. **Annotation / modifier reconstruction** (the inverse of J1's
    `apply_annotations`), applied to the schema built above, each only when the
    keyword is present: `description`→`describe`, `title`→`title`,
    `default`→`default` (`ZerxValue::from_serialize`), `examples` (array, first
    element)→`example`, `deprecated==true`→`deprecated`, `readOnly==true`→
    `read_only`, `writeOnly==true`→`write_only`, `contentMediaType`→`mime_format`,
    a non-marker/non-`email`/non-`uuid` `format`→`format` modifier.
13. **Remaining-keyword classification + extension `meta`.** Every key of `node`
    not consumed by steps 1–12 is classified against **import-specific** keyword
    sets (declared as `const` slices in `src/json_schema.rs`, **separate** from
    J1's export-oriented `RESERVED` — reusing `RESERVED` here is wrong because it
    is not an exhaustive list of JSON Schema structural vocabulary and would let
    unsupported applicators leak into `meta`):
    - `IMPORT_HANDLED` — keywords steps 1–12 read and reconstruct (`type`,
      `properties`, `required`, `additionalProperties`, `items`, `prefixItems`,
      `enum`, `const`, `anyOf`, `oneOf`, `$ref`, `discriminator`, `minLength`,
      `maxLength`, `pattern`, `minimum`, `maximum`, `minItems`, `maxItems`,
      `format`, `contentMediaType`, plus the annotation keywords `description`,
      `title`, `default`, `examples`, `deprecated`, `readOnly`, `writeOnly`), **plus
      the root-level structural keywords `$defs` and `$schema`**. `$defs` is
      pre-consumed by `ImportContext::new` and resolved through `$ref` (Step 3), and
      `$schema` is the dialect marker the importer ignores (it carries no schema
      shape); both MUST be in `IMPORT_HANDLED` so they are **not** routed into
      `meta`. Routing `$defs`/`$schema` to `meta` would be structurally wrong and,
      because both are export-reserved, J1 would silently drop them on re-export
      (`src/json_schema.rs:53-64`, `210-214`), masking the bug. A key in this set is
      already applied or deliberately ignored; it is **not** routed anywhere.
    - `IMPORT_UNSUPPORTED_STRUCTURAL` — validation applicators zerx cannot
      represent and that would **silently alter or lose validation semantics** if
      dropped: `allOf`, `not` (already errored eagerly in step 4), `if`, `then`,
      `else`, `contains`, `minContains`, `maxContains`, `patternProperties`,
      `propertyNames`, `dependentSchemas`, `dependentRequired`,
      `unevaluatedProperties`, `unevaluatedItems`, `$dynamicRef`. Any such key →
      `Err(IMPORT_UNSUPPORTED)` `.at(path)` naming the keyword. This extends the
      vision's explicit `allOf`/`not` errors (vision line 287) under the same
      principle — "raise clear errors rather than being silently ignored" — to the
      other semantics-bearing applicators, so foreign constraints are never lost
      unnoticed.
    - **Extension keys** — every remaining key (neither handled nor an unsupported
      applicator; e.g. `x-internal`, `x-ui-multiline`, `x-oneOf` on re-import, and
      any unrecognised `$`-vocabulary annotation) → `meta(key,
      ZerxValue::from_serialize(value)?)`. These survive re-export because they are
      not export-reserved.
14. **Fallthrough.** A node that reaches the end with **no** structural keyword
    (only annotations/extensions consumed by steps 12–13) → `any()` carrying its
    annotations/meta (already applied). A node whose only structural content was an
    unsupported applicator has already errored in step 13.

Modifier application uses the `Modify` blanket trait on `Schema` directly
(`src/schema.rs` impls `Modify for Schema`), so the walk can apply modifiers to
the erased `Schema` without naming each typed builder. Builder-specific validators
(`min`/`max`/`pattern`/`email`/`uuid`/`int`) are applied on the typed builder
**before** `.into(): Schema`, since those methods are inherent to the builder
(e.g. `StringSchema::min`), then converted.

### 5. Tests (`src/json_schema.rs`, `#[cfg(test)]`)

See Verification. The decisive tests are the **roundtrip** tests: build a `Schema`
with the existing constructors, `to_json_schema()`, `from_json_schema()`, then
`to_json_schema()` again and assert the two exported `Value`s are **byte-identical**
(`serde_json::to_string`). Byte-identical re-export is the operational definition of
"semantically equivalent schema survives the roundtrip" and sidesteps the absence
of `PartialEq` on `Schema` (`schema-core.md` forbids it).

### 6. Doc Update (after build + tests pass — Hard Rule 12)

- `docs/architecture/json-schema.md`:
  - Add an **import** subsection to **Constraints** capturing: the inverse-of-J1
    keyword→modifier mapping; the four `additionalProperties` shapes incl.
    absent→strict (fail-closed, C4); the `required`/`default`→optional rule; the
    `nullable` `anyOf` collapse; `oneOf`→discriminated-union-or-`x-oneOf`-union and
    `anyOf`→union; `allOf`/`not`→error; the format-marker→kind mapping (buffer via
    `type:"string"`, url via `format:"url"`); `enum` all-strings vs literal-union;
    `const`→literal; `true`→any / `false`→error; `$ref` `#/$defs` resolution with
    one shared memoised `lazy` per id and the cycle guard; `from_json_schema`
    fallibility (C7) with the two import error codes; the `x-ui-multiline`
    not-reconstructed accepted limitation.
  - Add `from_json_schema(&serde_json::Value) -> Result<Schema, ZerxError>` to
    **External Contracts** (the import counterpart to the export entries).
  - **Consumes from**: add `errors: ZerxError (structured import failure)` and
    `types: type constructors (the builder/constructor surface import
    reconstructs)`.
- Cross-concern edge duals (concept "Cross-concern edge discipline"):
  - `docs/architecture/errors.md` **Provides to**: add
    `json-schema: ZerxError (structured import failure)`.
  - `docs/architecture/types.md` **Provides to**: add
    `json-schema: type constructors (the builder/constructor surface import
    reconstructs)`.
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
cargo test --features mlua    # J2 adds no gated code; must still build/pass
```

All must pass with no warnings. The `src/json_schema.rs` test module MUST cover at
least:

1. **Scalars.** `{"type":"string"|"number"|"boolean"|"null"}` and `{}` import to
   `string`/`number`/`boolean`/`null`/`any` (assert kind via re-export shape).
2. **Integer.** `{"type":"integer"}` imports to `number().int()` (re-exports
   `type:"integer"`); `{"type":"integer","minimum":0,"maximum":9}` re-exports
   `minimum`/`maximum`.
3. **String validators.** `{"type":"string","minLength":2,"maxLength":8,
   "pattern":"^x","format":"email"}` re-exports the same `minLength`/`maxLength`/
   `pattern`/`format:"email"`.
4. **uuid / user format.** `format:"uuid"` → re-exports `format:"uuid"`; a plain
   `{"type":"string","format":"date-time"}` re-exports `format:"date-time"`.
5. **Enum.** `{"enum":["a","b"]}` → `enumerate`, re-exports `{"enum":["a","b"]}`;
   `{"type":"string","enum":["a","b"]}` likewise (enum precedes type). A mixed
   `{"enum":["a",1]}` imports to a literal-union and re-exports `anyOf` of two
   `const`s.
6. **Const / literal.** `{"const":"x"}`→`literal("x")`; `{"const":5}`;
   `{"const":null}` round-trip (`to`→`from`→`to` byte-identical).
7. **Array & tuple.** `{"type":"array","items":{"type":"string"},"minItems":1,
   "maxItems":3}` re-exports identically; `{"type":"array","prefixItems":[…],
   "items":false,"minItems":2,"maxItems":2}` imports to a tuple and re-exports the
   same prefixItems/items:false/minItems/maxItems; `{"type":"array","items":true}`
   and `items:{}` import as `array(any())`; a `type:"array"` with no `items` →
   `IMPORT_MALFORMED`; a `prefixItems` **with an `items` schema** (open/typed tail,
   not J1's closed shape) → `IMPORT_UNSUPPORTED`.
8. **Record.** `{"type":"object","format":"record","additionalProperties":
   {"format":"json"}}` → `record(json())`, re-exports identically;
   `additionalProperties:true` → `record(any())` (re-exports
   `additionalProperties:true`); `additionalProperties:false` → `IMPORT_MALFORMED`.
9. **Object required / additionalProperties / default.**
   `{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"string"},
   "c":{"type":"string","default":"x"}},"required":["a"],
   "additionalProperties":false}` imports with `a` required, `b` optional, `c`
   non-optional with default, strict mode; re-exports byte-identical to the input
   (modulo key ordering produced by export). `additionalProperties:true` →
   passthrough; absent → strict; `additionalProperties:{"type":"string"}` →
   passthrough.
10. **Buffer + MIME / special markers.**
    `{"type":"string","format":"buffer","contentMediaType":"image/png"}` →
    `buffer().mime_format("image/png")`, re-exports identically; `uri`/`url`/`json`/
    `jsonschema` markers round-trip.
11. **Nullable collapse.** `{"anyOf":[{"type":"string"},{"type":"null"}]}` imports
    to `string().nullable()` and re-exports the same `anyOf`; a nullable string
    with `description` inside the first branch round-trips with the description
    preserved.
12. **Union vs discriminated union.** `{"anyOf":[{"type":"string"},
    {"type":"number"}]}` → union, re-exports `anyOf`; a `{"oneOf":[…],
    "discriminator":{"propertyName":"kind"}}` with two object variants → discriminated
    union, re-exports `oneOf`+`discriminator`; a `oneOf` **without** discriminator →
    union with `x-oneOf:true` (re-export carries `x-oneOf` via meta); a `oneOf`+
    discriminator whose variants are **not** all objects → falls back to union; a
    `oneOf`+discriminator whose variants are objects **but violate the discriminator
    invariant** (e.g. the discriminator field is a plain `string`, not a `literal`,
    or is missing/optional in one variant) → falls back to union (asserts the
    `DiscriminatorState::Invalid` path, not just the non-object path).
13. **Modifiers round-trip.** A node with `description`, `title`, `deprecated`,
    `readOnly`, `writeOnly`, `examples:["ex"]`, `x-internal:true`,
    `x-ui-multiline:3` imports so that re-export reproduces every one of those
    keys (proves the reserved-annotation-keywords-are-not-lost guarantee).
14. **allOf / not / false / unsupported applicators.** `{"allOf":[…]}`,
    `{"not":{…}}`, and `false` each return `IMPORT_UNSUPPORTED` with a non-empty
    `path` where applicable. An unsupported applicator that is **not** `allOf`/`not`
    — e.g. `{"type":"object","if":{…},"then":{…}}` or `{"type":"object",
    "patternProperties":{…}}` — also returns `IMPORT_UNSUPPORTED` (proves the
    step-13 classification errors rather than silently routing the applicator into
    `meta`).
15. **`$ref` / `$defs`.** A root `{"$ref":"#/$defs/S1","$defs":{"S1":{"type":
    "object","properties":{"id":{"type":"string"}},"additionalProperties":false}}}`
    imports (root is a `lazy` over the def) and re-exports to a single-`$defs`
    `$ref` document. A `$ref` to a missing id → `IMPORT_MALFORMED`; a non-`#/$defs/`
    `$ref` → `IMPORT_MALFORMED`.
16. **Cyclic `$ref` terminates and re-exports to one `$defs` entry.** Import the
    output of J1's cyclic-lazy export (a `$defs` entry whose body references its own
    `$ref`), confirm `from_json_schema` returns without recursing, and that
    `to_json_schema()` of the result has exactly **one** `$defs` entry whose body's
    back-reference is the same `$ref` (proves shared lazy identity).
17. **Full roundtrip — the vision `message` schema.** Assemble the `message`
    schema from `docs/vision.md:181-216` **omitting the `mlua`-only `call`
    function field** (out of scope until M1), `to_json_schema()` it, then
    `from_json_schema()` and `to_json_schema()` again, and assert the two exported
    `Value`s are **byte-identical**. This is the end-to-end proof of
    `definition.md` Success Criterion 2 (roundtrip stability with `$defs`/`$ref`,
    cycles, format markers) for the foundation feature set.
18. **Error path determinism.** A malformed nested schema (e.g. an object property
    that is an array without `items`) yields a `ZerxError` whose `path` points at
    the offending property, confirming path threading.
