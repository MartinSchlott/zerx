# PLAN_J1_export

## Context & Goal

First plan of **Phase 3 — JSON Schema roundtrip** in the
`CONCEPT_zerx_foundation` execution. Phase 1 (`PLAN_F3_error_model`,
`PLAN_F1_value_model`, `PLAN_F2_schema_core`) and all of Phase 2
(`PLAN_T1_basic_types`, `PLAN_T2_complex_types`, `PLAN_T3_object_utilities`,
`PLAN_T4_special_types`) are complete and archived. The full vision type
catalogue (minus the feature-gated host-opaque `function`/`tvalue`, deferred to
`PLAN_M1_host_opaque`) is implemented: the erased `Schema` carrier, the
`SchemaKind` container with its per-kind dispatch seam, the blanket `Modify`
trait, the parse flow, the `Validator` storage contract (every concrete
validator already exposes `json_schema()`), and all type bodies
(`ObjectBody`, `DiscriminatedUnionBody`, …).

J1 delivers **`to_json_schema`**: the one-directional export from a `Schema`
value to a JSON Schema (Draft 2020-12) `serde_json::Value`, including:

- the format markers (`buffer`, `record`, `json`, `jsonschema`) that carry the
  non-JSON "bastard" types through pure-JSON tools (C2; `json-schema.md`
  Purpose);
- `ExportContext`-based `$defs`/`$ref` tracking for `lazy` subschemas, with
  placeholder-first registration so **cyclic** lazy schemas terminate and
  survive the roundtrip (vision §"Cycles, lazy & `$defs`/`$ref`");
- Draft 2020-12 `discriminator` on discriminated unions
  (vision §"A schema with all the trimmings" / "The emitted JSON Schema
  bastard").

J1 is **export only**. Import (`from_json_schema`) is `PLAN_J2_import` and is the
plan that proves full roundtrip stability; J1's verification covers the export
shape and prepares the fixtures J2 will roundtrip. `to_json_schema` is
**infallible** (C7): it returns `serde_json::Value`, never `Result`.

### Boundary (`docs/architecture/json-schema.md`, `schema-core.md`, `types.md`)

- `json-schema` owns export. J1 owns: the export walk over `SchemaKind`, the
  format markers, the `ExportContext` (`$defs`/`$ref` + lazy identity tracking),
  the modifier→keyword mapping, the nullable representation, the
  `additionalProperties`/`required` derivation, and the public `to_json_schema`
  surface (`ExportOptions`).
- `json-schema` does **NOT** define the types it serialises (`types`) nor the
  `Schema` representation / parse flow (`schema-core`). J1 reads existing
  `pub(crate)` payloads; it adds no `SchemaKind` variant and no parse-flow logic.
- `json-schema` does **NOT** own import or the policy pipeline (`PLAN_J2_import`,
  `PLAN_P1_policy_pipeline`).
- The host-opaque `function`/`tvalue` export markers are **out of scope**: those
  `SchemaKind` variants do not exist until `PLAN_M1_host_opaque`. The export
  match is exhaustive over the *current* `SchemaKind`; M1 adds its own export
  arms when it adds the variants (same one-arm-per-type discipline as the parse
  dispatch).

## Breaking Changes

**No.** J1 is purely additive: a new module `src/json_schema.rs`, new public
items (`Schema::to_json_schema`, `Schema::to_json_schema_with`, `ExportOptions`,
`DRAFT_2020_12`), and one `pub(crate)` accessor on `Lazy` in `src/schema.rs`. No
existing signature, type, or behaviour changes.

## Reference Patterns

- `src/types.rs` — the `pub(crate)` delegate-function pattern
  (`check_string`, `parse_object`, …) called from a central `SchemaKind` match
  in `src/schema.rs`. J1 mirrors this with a central export match in
  `src/json_schema.rs`.
- `src/schema.rs:160-218` — the `check_type` / `parse_inner` exhaustive matches
  over `SchemaKind`. The export walk is a third exhaustive match of the same
  shape (one arm per variant).
- `src/types.rs` validator `json_schema()` impls (`MinLength`, `Pattern`,
  `Email`, `IntValidator`, `ArrayMinLength`, …) — already return
  `serde_json::Map<String, serde_json::Value>` fragments J1 merges.
- Zex reference (`/Users/martinschlott/Documents/MyProjects/zex`):
  `src/zex/base/export-context.ts` (`$defs`/`$ref` context, lazy identity via
  `idFor`), `src/zex/base/zex-lazy.ts` (placeholder-first cycle break),
  `src/zex/complex-types/object.ts` `getBaseJsonSchema` (required/additional),
  `src/zex/basic-types.ts` / `special-types.ts` (per-kind base schema). J1
  deviates from Zex where the vision is more specific (see Assumptions).

## Dependencies

None new. `serde`, `serde_json`, and `regex` are already core dependencies
(C9). J1 uses `serde_json::{Value, Map, Number}` only.

## Assumptions & Risks

The following export choices are **concept-internal roundtrip constraints**.
They are recorded as `json-schema.md` **Constraints** at Doc Update — a
permanent normative concern file whose Constraints every implementation MUST
respect, including `PLAN_J2_import`'s reader. They are deliberately *not* written
to `docs/decisions.md` by this plan, for two grounded reasons: (1) the concept
explicitly settles roundtrip behaviour in the concern file, not the register —
`json-schema.md`'s `Related Decisions` reads "roundtrip behaviour is settled
within the JSON Schema plans"; (2) under the concept's lifecycle discipline
(`CONCEPT_zerx_foundation.md` §"Decision-reference discipline" and
`.claude/rules/decisions.md` "Promotion"), **no plan writes the register
mid-concept** — register migration is exclusively the Architect's Concept
Closeout step (§8). "Binding a later plan" does not by itself force a register
entry: concern-file Constraints are equally binding and are the concept's chosen
home for roundtrip behaviour. If the Reviewer judges any of these should instead
become a register decision, the correct path is for the Architect to add it as a
candidate decision to the concept (an Architect edit), not for J1 to write
`decisions.md`. Where these choices deviate from Zex, the deviation follows the
vision, which is the newer and authoritative spec for zerx.

- **Discriminated union → `oneOf` + `discriminator`.** The vision's emitted
  bastard (`docs/vision.md:228`) shows `{ "oneOf": [ … ], "discriminator":
  { "propertyName": "kind" } }`. J1 exports `discriminated_union` as
  `{"oneOf": [variants…], "discriminator": {"propertyName": <key>}}` (Draft
  2020-12 + OpenAPI convention). Plain `union` exports as `{"anyOf": [variants…]}`.
  *(Zex emitted `anyOf` for discriminated unions; J1 follows the vision excerpt.)*
- **`buffer` marker uses `type: "string"`.** The vision excerpt
  (`docs/vision.md:226`) shows `{ "type": "string", "format": "buffer",
  "contentMediaType": "image/png" }`. J1 emits `{"type":"string","format":
  "buffer"}` plus `contentMediaType` when `modifiers.mime` is set. *(Zex used
  `type:"object"`; J1 follows the vision.)*
- **`uri`/`url` format strings.** `uri` → `{"type":"string","format":"uri"}`;
  `url` → `{"type":"string","format":"url"}`. Two distinct format strings keep
  the roundtrip unambiguous. *(Zex used `uri`/`uri-reference`; J1 uses the
  clearer `uri`/`url` pair, which J2 will read back.)*
- **`nullable` → `anyOf` wrap.** A schema with `modifiers.nullable` exports as
  `{"anyOf": [<core>, {"type":"null"}]}`, where `<core>` carries the node's own
  type/validators/annotations. This is the symmetric form J2 collapses back to
  `.nullable()` (matching Zex's import `nullableFromAnyOfTransform`). The wrap is
  applied uniformly; for `any`/`null` cores it is harmlessly redundant.
- **Object modes → `additionalProperties`.** `strict` → `false`,
  `passthrough` → `true`, `strip` → `false` (the stripped output conforms to
  `additionalProperties: false`). The runtime mode and the exported value stay in
  sync (C4, avoiding the Zex roundtrip footgun). **Accepted limitation:** `strip`
  and `strict` both export `false`, so a `strip` schema roundtrips back to
  `strict` (the strip behaviour is runtime-only and not representable in standard
  JSON Schema). This is noted in `json-schema.md`.
- **`record(any)` → `additionalProperties: true`.** When the record value
  schema is a plain `any` (no modifiers, no validators), J1 emits
  `additionalProperties: true` (Zex parity); otherwise it emits the exported
  value schema. `properties: {}` is always included for parser compatibility.
- **`example` modifier → `examples` array.** The singular `example` modifier
  exports as Draft 2020-12 `"examples": [<value>]`.
- **Risk — lazy identity.** `$defs` dedup relies on `lazy` schema identity. Two
  clones of the same `lazy` share one `Rc<RefCell<LazyState>>`
  (`src/schema.rs:97-101`); J1 keys identity on `Rc::as_ptr(&state)`. Distinct
  `lazy(...)` calls (even with identical thunks) are distinct subschemas and get
  distinct `$defs` ids — correct, matching Zex's `WeakMap` identity. Mitigation:
  a dedicated cyclic-lazy test asserts termination and a single `$defs` entry.
- **Risk — infallibility.** Exporting a value (`default`/`example`/`literal`/
  `meta`/`enum`) goes through `serde_json::to_value(&ZerxValue)`, which is
  fallible in principle. In the default build no fallible `ZerxValue` variant
  exists; J1 uses `.unwrap_or(serde_json::Value::Null)` to guarantee
  infallibility regardless.

## Steps

### 1. Lazy identity seam (`src/schema.rs`)

Add a single `pub(crate)` accessor to `impl Lazy` (the only `schema-core` touch):

```rust
impl Lazy {
    /// Stable per-instance identity for export `$defs` dedup. All clones of the
    /// same `lazy` share one resolution cell, hence one identity.
    pub(crate) fn export_id(&self) -> usize {
        std::rc::Rc::as_ptr(&self.state) as *const () as usize
    }
}
```

`resolve()` is already `pub(crate)` and is reused by the export walk. No other
change to `src/schema.rs`.

### 2. New module `src/json_schema.rs`, declared in `src/lib.rs`

Add `mod json_schema;` to `src/lib.rs` and re-export the public surface:

```rust
pub use json_schema::{ExportOptions, DRAFT_2020_12};
```

(`to_json_schema` / `to_json_schema_with` are inherent methods on `Schema`,
defined in `src/json_schema.rs`, so no extra re-export is needed for them.)

### 3. Public surface (`src/json_schema.rs`)

```rust
/// Draft 2020-12 dialect URI, for `ExportOptions::dialect`.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Options for JSON Schema export. `Default` emits no `$schema`.
#[derive(Default, Clone)]
pub struct ExportOptions {
    /// When set, the root schema gains `"$schema": <dialect>`.
    pub dialect: Option<String>,
}

impl Schema {
    /// Export this schema to JSON Schema (Draft 2020-12). Infallible (C7).
    pub fn to_json_schema(&self) -> serde_json::Value {
        self.to_json_schema_with(&ExportOptions::default())
    }

    pub fn to_json_schema_with(&self, opts: &ExportOptions) -> serde_json::Value {
        // build ExportContext, export root node, attach $defs and $schema
    }
}
```

`to_json_schema_with` constructs an `ExportContext`, calls the export walk on
`self`, then: if `ctx.defs` is non-empty, inserts `"$defs"` into the root object;
if `opts.dialect` is set, inserts `"$schema"`. Returns the root `Value` (always a
JSON object).

### 4. `ExportContext` (`src/json_schema.rs`, private)

```rust
struct ExportContext {
    defs: serde_json::Map<String, serde_json::Value>, // id -> exported subschema
    ids: std::collections::HashMap<usize, String>,    // lazy identity -> id
    seq: usize,                                        // id counter
}
```

- `id_for(&mut self, identity: usize) -> (String, bool)` — returns the stable id
  (`S1`, `S2`, … in encounter order) and whether it is newly assigned.

### 5. Export walk (`src/json_schema.rs`, private)

`fn export_node(schema: &Schema, ctx: &mut ExportContext) -> serde_json::Value`,
built in this fixed order:

1. **Lazy short-circuit.** If `schema.kind` is `Lazy(lzy)`:
   - `(id, is_new) = ctx.id_for(lzy.export_id())`.
   - If `is_new`: insert `ctx.defs[id] = {}` (placeholder, breaks cycles), then
     `match lzy.resolve()` → on `Ok(inner)` set `ctx.defs[id] =
     export_node(&inner, ctx)`; on `Err(_)` leave the placeholder `{}`
     (infallible best-effort — cannot occur during a defs-guarded walk).
   - Build `core = {"$ref": format!("#/$defs/{id}")}` and continue to step 4
     (annotations/nullable still apply to the outer `$ref` node — a `lazy` may
     carry its own modifiers).
2. **Base map** (`core: serde_json::Map`) from `base_schema(&schema.kind, ctx)`
   — the central exhaustive match (step 6). For `Lazy` this step is replaced by
   step 1's `$ref` core.
3. **Validator fragments.** For each `v` in `schema.validators` (storage order),
   merge every key of `v.json_schema()` into `core` (last write wins).
4. **Modifier annotations**, merged into `core` (each only when set):
   - `description` → `"description"` (string)
   - `default` → `"default"` (`to_value(default).unwrap_or(Null)`)
   - `example` → `"examples": [ to_value(example) ]`
   - `deprecated` → `"deprecated": true`
   - `read_only` → `"readOnly": true`
   - `write_only` → `"writeOnly": true`
   - `title` → `"title"` (string)
   - `mime` → `"contentMediaType"` (string)
   - `format` (universal modifier) → write `"format"` **only if `core` does not
     already contain a `format` key**. Base kind markers (`buffer`, `record`,
     `json`, `jsonschema`, `uri`, `url` — step 6) and validator-contributed
     formats (`email`, `uuid` — step 3) are semantic and produced *before* this
     step, so they **take precedence and MUST NOT be overwritten** by the user
     `format` modifier: overwriting a marker would export a schema that no longer
     roundtrips to its original kind and misrepresents the runtime validators
     (`vision.md` format-marker sections; `definition.md` roundtrip success
     criterion). The user `format` modifier therefore annotates only schemas with
     no pre-existing semantic format (e.g. `string().format("date-time")`,
     `number().format("int64")`). `Modify::format` is on every builder via the
     blanket impl (`src/schema.rs`), so this guard is load-bearing, not
     hypothetical.
5. **Meta map** (`modifiers.meta`), merged as the final step but **constrained
   so it can never contradict the schema shape**: an entry `(k, v)` is written as
   `core[k] = to_value(v).unwrap_or(Null)` **only if** `k` is not an
   export-reserved keyword **and** `k` is not already present in `core` (from
   steps 2–4). Entries colliding with a reserved keyword or an
   already-produced key are **skipped**. Meta MUST NOT overwrite a structural,
   validator, annotation, or root keyword — this protects semantic roundtrip and
   keeps J2's import contract unambiguous. In practice `meta` carries only
   extension keys (e.g. user `meta("x-internal", true)` and the
   `x-ui-multiline` hint). The **export-reserved keyword set** is:
   `type`, `properties`, `required`, `additionalProperties`, `items`,
   `prefixItems`, `minItems`, `maxItems`, `anyOf`, `oneOf`, `discriminator`,
   `const`, `enum`, `$ref`, `$defs`, `$schema`, `format`, `contentMediaType`,
   `minLength`, `maxLength`, `pattern`, `minimum`, `maximum`, `description`,
   `default`, `examples`, `deprecated`, `readOnly`, `writeOnly`, `title`
   (defined as a single `const` slice in `src/json_schema.rs`).
6. **Nullable wrap.** If `schema.modifiers.nullable`, return
   `{"anyOf": [ Value::Object(core), {"type":"null"} ]}`; else return
   `Value::Object(core)`.

`optional` is **not** emitted on the node; it is consumed only by the parent
object's `required` computation (step 6 `Object` arm). A `default` likewise
removes the field from `required` but is still emitted as `"default"`.

### 6. `base_schema` — central exhaustive match over `SchemaKind`

`fn base_schema(kind: &SchemaKind, ctx: &mut ExportContext) -> serde_json::Map`,
one arm per variant:

- `Any` → `{}`
- `String` → `{"type":"string"}`
- `Number` → `{"type":"number"}`
- `Boolean` → `{"type":"boolean"}`
- `Enum(set)` → `{"enum": [<each string as Value::String>]}`
- `Null` → `{"type":"null"}`
- `Object(body)` → `{"type":"object", "properties": {<key: export_node(field)>},
  "required": [<keys where !body.all_optional && !field.modifiers.optional &&
  field.modifiers.default.is_none()>] (omitted when empty),
  "additionalProperties": <Strict→false, Strip→false, Passthrough→true>}`.
  Properties and `required` follow `body.shape` order.
- `Array(item)` → `{"type":"array", "items": export_node(item)}` (min/maxItems
  arrive via the array-length validator fragments in step 3).
- `Record(value)` → `{"type":"object", "format":"record", "properties": {},
  "additionalProperties": <true if `value` is a plain `Any` with no modifiers
  and no validators, else export_node(value)>}`.
- `Tuple(items)` → `{"type":"array", "prefixItems": [<export_node each>],
  "items": false, "minItems": n, "maxItems": n}` where `n = items.len()`.
- `Union(variants)` → `{"anyOf": [<export_node each>]}`.
- `DiscriminatedUnion(body)` → `{"oneOf": [<export_node each of body.variants>],
  "discriminator": {"propertyName": body.key}}`.
- `Literal(c)` → `{"const": to_value(c).unwrap_or(Null)}`.
- `Buffer` → `{"type":"string", "format":"buffer"}` (`contentMediaType` added in
  step 4 from `modifiers.mime`).
- `Uri` → `{"type":"string", "format":"uri"}`.
- `Url` → `{"type":"string", "format":"url"}`.
- `Json` → `{"format":"json"}`.
- `JsonSchema` → `{"type":"object", "format":"jsonschema"}`.
- `Lazy(_)` → `unreachable!()` (intercepted in `export_node` step 1), mirroring
  the `check_type`/`parse_inner` `Lazy` arms.

A small helper `fn is_plain_any(s: &Schema) -> bool` (kind is `Any`, no
validators, no meaningful modifiers) supports the `Record` arm.

### 7. Tests (`src/json_schema.rs`, `#[cfg(test)]`)

See Verification.

### 8. Doc Update (after build + tests pass — Hard Rule 12)

- Fill `docs/architecture/json-schema.md`: replace the skeleton status note;
  add the **Constraints** section capturing the export contract (per-kind base
  schemas and format markers; merge precedence base → validators → modifiers →
  meta; `nullable` anyOf wrap; `required` rule = not(`all_optional` ∨ `optional`
  ∨ has-`default`); mode→`additionalProperties` mapping incl. the strip-roundtrip
  limitation; `oneOf`+`discriminator` for discriminated unions; `$defs`/`$ref`
  with placeholder-first cycle break and lazy-identity dedup;
  `to_json_schema`/`to_json_schema_with` infallibility per C7). Add an **External
  Contracts** entry: the `to_json_schema` Draft 2020-12 JSON Schema document.
  Keep the existing `Consumes from` edges; their duals already exist in
  `schema-core.md` and `types.md` (no new cross-concern edge is introduced).

No `decisions.md` change: under the concept's lifecycle discipline no plan
writes the register mid-concept (register migration is the Architect's Concept
Closeout step §8), and the concept settles roundtrip behaviour in
`json-schema.md`, whose `Related Decisions` explicitly states "roundtrip
behaviour is settled within the JSON Schema plans". No new `severity: accepted`
bug card is required.

No version bump or release step is prescribed by this plan: versioning is not
part of J1's functional scope and is not grounded in any normative doc. The
git history shows a per-plan minor-version-bump pattern (e.g. `0.7.0` with
PLAN_T3, `0.8.0` with PLAN_T4); whether to continue it is a Product-Owner /
Archive-step (§7) decision at commit time, not a J1 Step.

## Verification

Run from the repo root:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --features mlua    # J1 adds no gated code; must still build/pass
```

All must pass with no warnings. The `src/json_schema.rs` test module MUST cover
at least:

1. **Scalars.** `string()`, `number()`, `boolean()`, `null()`, `any()` export the
   expected base (`{}` for `any`).
2. **Enum.** `enumerate(["a","b"])` → `{"enum":["a","b"]}`.
3. **Literal.** `literal("x")` → `{"const":"x"}`; `literal(5i64)` →
   `{"const":5}`; `literal(())`/null literal → `{"const":null}`.
4. **String validators merge.** `string().min(2).max(8).regex("^x").email()`
   yields `minLength`, `maxLength`, `pattern` (original source string), and
   `format:"email"` together with `type:"string"`.
5. **Number validators.** `number().int().min(0).max(9)` → `type:"integer"`
   (validator overrides base `number`), `minimum:0.0`, `maximum:9.0`.
6. **Array.** `array(string()).min(1).max(3)` →
   `{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":3}`.
7. **Tuple.** `tuple([number(), string()])` → `prefixItems` of length 2,
   `items:false`, `minItems:2`, `maxItems:2`.
8. **Record.** `record(json())` → `{"type":"object","format":"record",
   "properties":{},"additionalProperties":{"format":"json"}}`; `record(any())` →
   `additionalProperties:true`.
9. **Object required / additionalProperties.** `object([("a", string()),
   ("b", string().optional()), ("c", string().default("x"))])` (default
   `strict`) → `required:["a"]` only, `additionalProperties:false`,
   `properties.c.default == "x"`. `.passthrough()` → `additionalProperties:true`;
   `.strip()` → `false`; `.partial()` → `required` omitted entirely.
10. **Buffer + MIME.** `buffer().mime("image/png")` →
    `{"type":"string","format":"buffer","contentMediaType":"image/png"}`.
11. **uri / url / json / jsonschema** markers export exactly as specified in
    step 6.
11b. **User `format` cannot clobber markers.** `buffer().format("date-time")`
    still exports `format:"buffer"` (and keeps `contentMediaType` when a MIME is
    set); `json().format("x")` still exports `format:"json"`;
    `string().email().format("phone")` still exports `format:"email"`. A plain
    `string().format("date-time")` (no marker, no validator format) exports
    `format:"date-time"`. This guards finding-3's roundtrip-marker protection.
12. **Modifiers.** A node with `.describe`, `.deprecated`, `.read_only`,
    `.write_only`, `.title`, `.example`, `.meta("x-internal", true)`,
    `.multiline(3)` (string) emits `description`, `deprecated:true`,
    `readOnly:true`, `writeOnly:true`, `title`, `examples:[…]`,
    `x-internal:true`, `x-ui-multiline:3`.
12b. **Meta cannot corrupt the shape.** `string().meta("type", "object")
    .meta("description", "evil").meta("x-ok", 1)` exports with
    `type == "string"` (the reserved `type` meta entry is skipped) and
    `x-ok == 1` (the non-reserved extension key is written). A
    `meta("$ref", "…")` / `meta("properties", …)` entry MUST NOT appear in the
    output. This guards finding-2's semantic-roundtrip protection.
13. **Nullable.** `string().nullable()` →
    `{"anyOf":[{"type":"string"},{"type":"null"}]}`;
    `string().describe("d").nullable()` keeps `description` inside the first
    `anyOf` branch.
14. **Discriminated union.** A two-variant `discriminated_union("kind", …)`
    exports `{"oneOf":[…], "discriminator":{"propertyName":"kind"}}`; plain
    `union([...])` exports `{"anyOf":[…]}`.
15. **Lazy → `$defs`/`$ref`.** A `lazy(|| object([...]))` used once exports
    `{"$ref":"#/$defs/S1"}` at its position and a single `$defs.S1` entry holding
    the resolved object schema.
16. **Cyclic lazy terminates.** A self-referential tree (e.g.
    `let node = object([("next", lazy(|| node_clone).optional())])` built via a
    shared `Rc`/clone so the inner `lazy` resolves to a schema containing the
    same `lazy`) exports in finite time, emits exactly one `$defs` entry for that
    `lazy`, and the entry's body references the same `$ref` (no infinite
    recursion, no panic).
17. **`$schema` option.** `to_json_schema_with(&ExportOptions{ dialect:
    Some(DRAFT_2020_12.into()) })` sets root `"$schema"`; the default
    `to_json_schema()` omits it.
18. **Infallibility / determinism.** `to_json_schema` returns a `Value` (no
    `Result`); exporting the same schema twice yields byte-identical
    `serde_json::to_string` output (stable `$defs` ids in encounter order).

Manual spot-check: assemble the `message` schema from `docs/vision.md:181-216`
(omitting the `mlua`-only `function` field) and confirm `to_json_schema()`
produces an object resembling the vision excerpt (`docs/vision.md:221-232`):
`additionalProperties:false`, `format:"buffer"` on `avatar`, `format:"record"`
on `metadata`, `oneOf`+`discriminator` on `source`, and `required` listing the
non-optional, non-defaulted fields.
</content>
</invoke>
