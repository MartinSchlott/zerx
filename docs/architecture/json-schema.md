# Concern: JSON Schema

## Purpose

- This concern owns bidirectional JSON Schema (Draft 2020-12): export from `Schema` and import back to `Schema`.
- This concern owns the format markers (`buffer`, `record`, `json`, `jsonschema`) that carry non-JSON types through pure-JSON tools.
- This concern owns `$defs`/`$ref` tracking for shared/recursive subschemas and the memoised lazy placeholders that let cycles survive the roundtrip.

## Non-Goals

- This concern does NOT own the import policy pipeline for external sources — see `policy`.
- This concern does NOT define the types it serialises — see `types`.

## Consumes from

- `schema-core`: `Schema` (schema value that export reads and import reconstructs)
- `types`: validator `json_schema()` fragment contributions (per-validator JSON Schema keyword maps merged during export)
- `types`: type constructors (the builder/constructor surface import reconstructs)
- `errors`: `ZerxError` (structured import failure)

## Provides to

- `policy`: `from_json_schema_inner` (core import walk the pipeline wraps, carrying the caller's unknown-property disposition)

## External Contracts

- `Schema::to_json_schema()` — infallible export producing a Draft 2020-12 JSON Schema document (`serde_json::Value`).
- `Schema::to_json_schema_with(opts: &ExportOptions)` — same, with optional `$schema` dialect URI injection.
- `from_json_schema(&serde_json::Value) -> Result<Schema, ZerxError>` — fallible import of a Draft 2020-12 JSON Schema document back into a `Schema` (C7).

## External Services

- (none)

## Architecturally Significant Dependencies

- `serde_json` — the JSON value model used by all exported documents and validator fragment contributions.

## Constraints

### Export pipeline order

- The export of a `Schema` node MUST follow this fixed merge order: (1) base kind keywords, (2) validator fragment keywords, (3) modifier annotation keywords, (4) `meta` extension keys.
- A later step MUST NOT overwrite a key produced by an earlier step. The order is: base → validators → modifiers → meta.
- The `format` modifier MUST NOT overwrite a semantically meaningful `format` key produced in step (1) or (2) (kind markers and validator-contributed formats take precedence).
- `meta` entries MUST be skipped when the key is export-reserved or already present in the output map.

### Per-kind base schemas and format markers

- `Any` → `{}` (no keywords).
- `String` → `{"type":"string"}`.
- `Number` → `{"type":"number"}`.
- `Boolean` → `{"type":"boolean"}`.
- `Null` → `{"type":"null"}`.
- `Enum(set)` → `{"enum":[…]}`.
- `Literal(c)` → `{"const":<value>}`.
- `Object(body)` → `{"type":"object","properties":{…},"required":[…],"additionalProperties":<mode>}`.
- `Array(item)` → `{"type":"array","items":<exported item>}`.
- `Record(value)` → `{"type":"object","format":"record","properties":{},"additionalProperties":<value or true>}`.
- `Tuple(items)` → `{"type":"array","prefixItems":[…],"items":false,"minItems":n,"maxItems":n}`.
- `Union(variants)` → `{"anyOf":[…]}`.
- `DiscriminatedUnion(body)` → `{"oneOf":[…],"discriminator":{"propertyName":<key>}}`.
- `OneOf(variants)` (crate-internal, built only by import) → `{"oneOf":[…]}`.
- `Buffer` → `{"type":"string","format":"buffer"}` (`contentMediaType` added by `mime` modifier).
- `Uri` → `{"type":"string","format":"uri"}`.
- `Url` → `{"type":"string","format":"url"}`.
- `Json` → `{"format":"json"}`.
- `JsonSchema` → `{"type":"object","format":"jsonschema"}`.

### `required` rule

- A field is included in `required` if and only if `all_optional` is false AND the field's `optional` modifier is false AND the field has no `default` modifier.
- When `all_optional` is true (`partial()` mode), the `required` key MUST be omitted entirely.

### Object mode → `additionalProperties` mapping

- `Strict` → `additionalProperties: false`.
- `Strip` → `additionalProperties: false`. Accepted limitation: `strip` and `strict` both export `false`; the strip runtime behaviour is not representable in standard JSON Schema, so a `strip` schema roundtrips back as `strict`. An import-time strip disposition is likewise not representable in the exported document and MUST be re-supplied on every import.
- `Passthrough` → `additionalProperties: true`.

### `record(any())` shorthand

- When the record value schema is a plain `Any` (no modifiers, no validators), the export MUST emit `additionalProperties: true`; otherwise the exported value schema is used.

### `nullable` representation

- A schema with `nullable` set MUST export as `{"anyOf":[<core schema>,{"type":"null"}]}`.
- The `<core schema>` carries all base, validator, and modifier keywords of the node; the nullable wrap is the outermost node.

### `example` modifier

- The singular `example` modifier MUST export as the Draft 2020-12 `"examples"` array with one element.

### `$defs`/`$ref` and lazy identity

- Every `Lazy` schema MUST be exported as `{"$ref":"#/$defs/<id>"}` at its usage site and as a top-level `$defs` entry.
- The `$defs` id is assigned in encounter order (`S1`, `S2`, …); all clones of the same `lazy` share one id.
- The placeholder-first registration protocol MUST be used: insert an empty object `{}` into `$defs` before resolving the thunk, so cyclic schemas terminate rather than recurse infinitely.
- Modifier annotations and the nullable wrap MUST still be applied to the outer `$ref` node (a `lazy` may carry its own modifiers).

### Infallibility

- `to_json_schema` and `to_json_schema_with` MUST be infallible and MUST return `serde_json::Value` directly (no `Result`).
- `ZerxValue` → `serde_json::Value` conversions inside export MUST use `.unwrap_or(serde_json::Value::Null)` to preserve infallibility.

### Export-reserved keyword set

- The following keys MUST NOT be written by the `meta` step: `type`, `properties`, `required`, `additionalProperties`, `items`, `prefixItems`, `minItems`, `maxItems`, `anyOf`, `oneOf`, `discriminator`, `const`, `enum`, `$ref`, `$defs`, `$schema`, `format`, `contentMediaType`, `minLength`, `maxLength`, `pattern`, `minimum`, `maximum`, `description`, `default`, `examples`, `deprecated`, `readOnly`, `writeOnly`, `title`.
- Every assertion keyword of the import coverage table MUST also be export-reserved, so `meta` can never emit an assertion. `$id` is not reserved: a root `$id` round-trips as `meta`.

### Import pipeline order

- The import walk (`import_value`) MUST follow this fixed dispatch order: (1) boolean schema, (2) non-object guard, (2.5) `type` parsing, (3) `$ref`, (4) `allOf`/`not` error, (5) nullable `anyOf` collapse, (6) `oneOf`/`anyOf` union or discriminated union, (7) `enum`, (8) `const`, (9) format-marker kinds, (10) `type` dispatch, (11) object reconstruction, (12) annotation/modifier reconstruction, (13) assertion coverage check, then `meta` routing of the remaining keys.
- A node with `$ref` MUST skip steps 4–11 and proceed directly to step 12.
- `type` parsing MUST run on every object node, `$ref` nodes included, so a malformed `type` is never skipped.

### `type` import

- An absent `type` MUST impose no type restriction; a node with no other base-producing keyword and no unconsumed assertion keyword imports as `any()`.
- A single string naming one of the seven Draft 2020-12 types MUST import as that type; `integer` imports as `number().int()`.
- An array of exactly two distinct type names, one of them `"null"`, in either order, MUST import as the non-null type with `.nullable()` applied, keeping that type's sibling constraints.
- `.nullable()` from a type array MUST be applied only to a base built by format-marker or `type` dispatch; a base from `$ref`, `anyOf`/`oneOf` or `enum`/`const` MUST NOT receive it, so `null` passes there only where the members admit it.
- An unknown type name, an empty array, an array holding a non-string, an unknown name or a duplicate, and any `type` that is neither a string nor an array MUST raise `IMPORT_MALFORMED`.
- Every other well-formed type array (one element, two non-null names, three or more names) MUST raise `IMPORT_UNSUPPORTED`; only a single type name or `[T, "null"]` is accepted, even where zerx could represent the form.
- `integer` MUST be treated as a subset of `number` wherever type sets are compared.

### Assertion keyword coverage

- The assertion keywords are the Draft 2020-12 core, applicator and validation keywords that constrain instances. Each MUST be classified by the instance types it applies to: any type (`type`, `enum`, `const`, `allOf`, `anyOf`, `oneOf`, `not`, `if`, `then`, `else`, `$ref`, `$dynamicRef`), string, number (integer included), array, or object.
- Every assertion keyword on an imported node MUST be consumed by the branch that reconstructs the node, vacuous, or refused with `IMPORT_UNSUPPORTED`; an assertion keyword MUST NOT reach `meta` (`D-import-fail-closed`).
- A keyword MUST count as vacuous only when the node declares a `type` that admits none of the instance types the keyword applies to; an untyped node has no vacuous keywords.
- The unsupported applicators MUST stay refused even where they would be vacuous.
- Each reconstruction branch MUST consume exactly the keywords it rebuilds into the schema; a keyword the branch reads but does not enforce MUST NOT count as consumed.
- An object node MUST refuse a `required` name that is not a key of `properties`, because strict mode would reject the key instead of requiring it.
- A tuple node MUST refuse `minItems` above, or `maxItems` below, the number of `prefixItems`; bracketing values are consumed.
- A tuple MAY import narrower than its source: `prefixItems` with `items:false` reconstructs a fixed-length tuple, although the source also accepts shorter arrays. This is the one named narrowing under `D-import-fail-closed`.
- A consumed keyword whose value violates the Draft 2020-12 grammar MUST raise `IMPORT_MALFORMED`; an integral float counts as an integer.
- An empty `anyOf`/`oneOf` MUST raise `IMPORT_UNSUPPORTED`; it accepts nothing, and zerx has no never type.
- `$id` on any node other than the root MUST raise `IMPORT_UNSUPPORTED`; a nested `$id` would re-base the `$ref`s inside it. A root `$id` imports as `meta`.

### Import keyword→modifier mapping (inverse of export)

- Every annotation keyword J1's `apply_annotations` writes MUST be reconstructed as its modifier: `description`→`describe`, `title`→`title`, `default`→`default`, `examples[0]`→`example`, `deprecated:true`→`deprecated`, `readOnly:true`→`read_only`, `writeOnly:true`→`write_only`, `contentMediaType`→`mime_format`.
- A `format` value that is not a kind marker (`buffer`, `record`, `json`, `jsonschema`, `uri`, `url`) and not a validator format (`email`, `uuid`) MUST be applied as the `format` modifier.
- Annotation keywords MUST NOT be routed into `meta`; they are export-reserved and would be silently dropped on re-export.

### `additionalProperties` import shapes (regular object)

- `false` → strict mode (constructor default; no call needed).
- Absent → strict mode (fail-closed; C4 security boundary).
- `true` → `passthrough()`.
- `{}` → `passthrough()`.
- Any other schema object MUST raise `IMPORT_UNSUPPORTED`; zerx does not model a per-extra-key value schema on a plain object, and `record` covers that case through its marker.
- When the caller-supplied unknown-property disposition is strip, the `false` and absent cases MUST reconstruct as `strip` mode instead of `strict`; the `true` and `{}` cases remain `passthrough()`, unaffected by the disposition.

### `required` / `optional` / default import rule

- A property listed in `required` MUST import as non-optional.
- A property not in `required` and with no `default` keyword on its node MUST import as `.optional()`.
- A property not in `required` but with a `default` keyword MUST import as non-optional (default supplies the missing value at parse time, matching the runtime invariant).

### Nullable collapse

- An `anyOf` of exactly two members where exactly one is a bare null schema (`{"type":"null"}` with no other structural keyword) MUST import as the other member with `.nullable()` applied.
- A `type` sibling of a nullable collapse MUST be honoured by the `anyOf`/`oneOf` `type` rule below; the bare null member counts as a member of type `null`.
- Any other `anyOf` MUST import as `union(variants)`.

### `oneOf` / `anyOf` / discriminator import

- `oneOf` with a `discriminator.propertyName` where all variants reconstruct to valid discriminated-union objects (per the full `build_discriminator_state` invariant) MUST import as `discriminated_union(key, variants)`.
- `oneOf` without a `discriminator`, or whose discriminator invariant fails, MUST import as the crate-internal `OneOf` kind, which accepts a value only when exactly one variant accepts it.
- A legacy `anyOf` carrying `x-oneOf: true` MUST import as `union(variants)` with that `meta` key, as its JSON Schema meaning is `anyOf`.
- `anyOf` (after nullable-collapse check) MUST import as `union(variants)`; a `discriminator` alongside `anyOf` is honoured the same way as on `oneOf`.
- A `type` sibling of `anyOf`/`oneOf` MUST be honoured only when every member is an object declaring its own `type` within the outer type set; otherwise the node MUST raise `IMPORT_UNSUPPORTED`. Under that condition the union equals the intersection, so no extra `.nullable()` is applied.

### `allOf` / `not` import

- `allOf` MUST raise `IMPORT_UNSUPPORTED` with a message naming the keyword and suggesting `extend()`.
- `not` MUST raise `IMPORT_UNSUPPORTED` with a message naming the keyword and suggesting `union()`.

### Format-marker kind mapping

- `format:"json"` without `type` → `json()`; `format:"json"` with any `type` MUST raise `IMPORT_UNSUPPORTED`, because `json()` accepts every value.
- `type:"object"` + `format:"record"` → `record(import(additionalProperties))` where `true`/absent maps to `any()` and `false` raises `IMPORT_MALFORMED`.
- `type:"object"` + `format:"jsonschema"` → `jsonschema()`.
- `type:"string"` + `format:"buffer"` → `buffer()` (`contentMediaType` applied as `.mime_format()` in step 12).
- `type:"string"` + `format:"uri"` → `uri()`.
- `type:"string"` + `format:"url"` → `url()`.
- The markers MUST match on the non-null type name of a `[T, "null"]` type array, with `.nullable()` applied to the marker kind.

### `enum` / `const` import

- `enum` of all strings → `enumerate(strings)`.
- `enum` containing any non-string → `union([literal(v), …])`.
- `const` → `literal(ZerxValue::from_serialize(value))`.
- `enum`/`const` MUST be dispatched before `type` so `{"type":"string","enum":[…]}` reconstructs the enum.
- With a `type` present, every `enum` member and the `const` value MUST be an instance of the declared type set; otherwise the node MUST raise `IMPORT_UNSUPPORTED`. A member is never filtered out silently.
- A `[T, "null"]` type array MUST NOT add `null` to an `enum`/`const`; `null` passes only if it is a member.
- A sibling assertion keyword of `enum`/`const` MUST be evaluated statically against every member it applies to: when every applicable member satisfies it the keyword is consumed, and when a member violates it the node MUST raise `IMPORT_UNSUPPORTED`.
- Static evaluation MUST use the runtime semantics of the matching validator, such as character counting for string lengths and the `regex` crate for `pattern`; a non-compiling `pattern` MUST raise `IMPORT_MALFORMED`.
- A sibling keyword that applies to a member but cannot be evaluated statically, such as a sub-schema keyword, MUST raise `IMPORT_UNSUPPORTED`.
- A node carrying both `enum` and `const` MUST import as `literal(const)` when the `const` equals an `enum` member, and MUST raise `IMPORT_UNSUPPORTED` otherwise.

### Boolean schema import

- `true` → `any()`.
- `false` → `IMPORT_UNSUPPORTED` (zerx has no never type).

### `$ref` import with memoised lazy

- A `$ref` value MUST start with `#/$defs/`; any other form MUST raise `IMPORT_MALFORMED`.
- Every assertion sibling of `$ref`, `type` and the unsupported applicators included, MUST raise `IMPORT_UNSUPPORTED`; the `$ref` path reconstructs only the reference and annotations.
- All `$ref`s to the same `$defs` id MUST share one `lazy` instance (clones share one resolution cell), so re-export emits a single `$defs` entry per id.
- Structural errors in a referenced `$defs` entry MUST be surfaced eagerly (via `ensure_resolved`) before `from_json_schema` returns; the thunk MUST only clone an already-resolved schema and MUST NOT fail.
- A self- or mutual-reference detected by the cycle guard (`importing` id-set) MUST return the existing placeholder and NOT recurse.

### Import fallibility and error codes

- `from_json_schema` MUST return `Result<Schema, ZerxError>` (C7); every error MUST carry the failing node's `path`.
- `IMPORT_UNSUPPORTED` — well-formed JSON Schema feature outside zerx's model (`allOf`, `not`, `false` schema, unsupported applicators, non-canonical `type` arrays, `type` not honourable at its site, unconsumed assertion keywords, other).
- `IMPORT_MALFORMED` — structurally invalid input (non-object/bool node, array missing `items`, record with `additionalProperties:false`, `$ref` to missing or non-local target, `type` or another consumed keyword outside the Draft 2020-12 grammar).
- Unsupported applicators that are NOT `allOf`/`not` (`if`, `then`, `else`, `contains`, `minContains`, `maxContains`, `patternProperties`, `propertyNames`, `dependentSchemas`, `dependentRequired`, `unevaluatedProperties`, `unevaluatedItems`, `$dynamicRef`) MUST raise `IMPORT_UNSUPPORTED` rather than being silently routed to `meta`, on `$ref` nodes too.

### Accepted import limitation — `x-ui-multiline`

- `x-ui-multiline` is imported as a `meta` entry, NOT as a reconstructed `multiline` validator. The JSON document roundtrips exactly; only the runtime validator is not rebuilt. This is deliberate: external schemas without markers import best-effort.

## Splitting Rationale

- Export and import stay one concern: they are inverse mappings of the single JSON Schema boundary, and each import rule is checked against the export rule it inverts.
- The import constraints are the first extraction candidate; they move to their own concern once they can no longer be held as one rule per bullet within this file's ceiling.

## Related Decisions

- `D-import-unknown-caller-policy`
- `D-import-fail-closed`
