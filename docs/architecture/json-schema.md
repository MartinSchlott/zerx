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

- `policy`: `from_json_schema` (core import walk the pipeline wraps)

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
- `Strip` → `additionalProperties: false`. Accepted limitation: `strip` and `strict` both export `false`; the strip runtime behaviour is not representable in standard JSON Schema, so a `strip` schema roundtrips back as `strict`.
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

### Import pipeline order

- The import walk (`import_value`) MUST follow this fixed dispatch order: (1) boolean schema, (2) non-object guard, (3) `$ref`, (4) `allOf`/`not` error, (5) nullable `anyOf` collapse, (6) `oneOf`/`anyOf` union or discriminated union, (7) `enum`, (8) `const`, (9) format-marker kinds, (10) `type` dispatch, (11) object reconstruction, (12) annotation/modifier reconstruction, (13) remaining keyword classification.
- A node with `$ref` MUST skip steps 4–11 and proceed directly to step 12.

### Import keyword→modifier mapping (inverse of export)

- Every annotation keyword J1's `apply_annotations` writes MUST be reconstructed as its modifier: `description`→`describe`, `title`→`title`, `default`→`default`, `examples[0]`→`example`, `deprecated:true`→`deprecated`, `readOnly:true`→`read_only`, `writeOnly:true`→`write_only`, `contentMediaType`→`mime_format`.
- A `format` value that is not a kind marker (`buffer`, `record`, `json`, `jsonschema`, `uri`, `url`) and not a validator format (`email`, `uuid`) MUST be applied as the `format` modifier.
- Annotation keywords MUST NOT be routed into `meta`; they are export-reserved and would be silently dropped on re-export.

### `additionalProperties` import shapes (regular object)

- `false` → strict mode (constructor default; no call needed).
- Absent → strict mode (fail-closed; C4 security boundary).
- `true` → `passthrough()`.
- Schema object → `passthrough()` (zerx does not model a per-extra-key value schema on a plain object; `record` covers that case).

### `required` / `optional` / default import rule

- A property listed in `required` MUST import as non-optional.
- A property not in `required` and with no `default` keyword on its node MUST import as `.optional()`.
- A property not in `required` but with a `default` keyword MUST import as non-optional (default supplies the missing value at parse time, matching the runtime invariant).

### Nullable collapse

- An `anyOf` of exactly two members where exactly one is a bare null schema (`{"type":"null"}` with no other structural keyword) MUST import as the other member with `.nullable()` applied.
- Any other `anyOf` MUST import as `union(variants)`.

### `oneOf` / `anyOf` / discriminator import

- `oneOf` with a `discriminator.propertyName` where all variants reconstruct to valid discriminated-union objects (per the full `build_discriminator_state` invariant) MUST import as `discriminated_union(key, variants)`.
- If the discriminator invariant fails for any reason, `oneOf` MUST fall back to `union(variants)` with `meta("x-oneOf", true)` so the original `oneOf` intent survives re-export.
- `anyOf` (after nullable-collapse check) MUST import as `union(variants)`; a `discriminator` alongside `anyOf` is honoured the same way as on `oneOf`.

### `allOf` / `not` import

- `allOf` MUST raise `IMPORT_UNSUPPORTED` with a message naming the keyword and suggesting `extend()`.
- `not` MUST raise `IMPORT_UNSUPPORTED` with a message naming the keyword and suggesting `union()`.

### Format-marker kind mapping

- `format:"json"` → `json()` (regardless of `type`).
- `type:"object"` + `format:"record"` → `record(import(additionalProperties))` where `true`/absent maps to `any()` and `false` raises `IMPORT_MALFORMED`.
- `type:"object"` + `format:"jsonschema"` → `jsonschema()`.
- `type:"string"` + `format:"buffer"` → `buffer()` (`contentMediaType` applied as `.mime_format()` in step 12).
- `type:"string"` + `format:"uri"` → `uri()`.
- `type:"string"` + `format:"url"` → `url()`.

### `enum` / `const` import

- `enum` of all strings → `enumerate(strings)`.
- `enum` containing any non-string → `union([literal(v), …])`.
- `const` → `literal(ZerxValue::from_serialize(value))`.
- `enum`/`const` MUST be dispatched before `type` so `{"type":"string","enum":[…]}` reconstructs the enum.

### Boolean schema import

- `true` → `any()`.
- `false` → `IMPORT_UNSUPPORTED` (zerx has no never type).

### `$ref` import with memoised lazy

- A `$ref` value MUST start with `#/$defs/`; any other form MUST raise `IMPORT_MALFORMED`.
- All `$ref`s to the same `$defs` id MUST share one `lazy` instance (clones share one resolution cell), so re-export emits a single `$defs` entry per id.
- Structural errors in a referenced `$defs` entry MUST be surfaced eagerly (via `ensure_resolved`) before `from_json_schema` returns; the thunk MUST only clone an already-resolved schema and MUST NOT fail.
- A self- or mutual-reference detected by the cycle guard (`importing` id-set) MUST return the existing placeholder and NOT recurse.

### Import fallibility and error codes

- `from_json_schema` MUST return `Result<Schema, ZerxError>` (C7); every error MUST carry the failing node's `path`.
- `IMPORT_UNSUPPORTED` — well-formed JSON Schema feature outside zerx's model (`allOf`, `not`, `false` schema, unsupported applicators, other).
- `IMPORT_MALFORMED` — structurally invalid input (non-object/bool node, array missing `items`, record with `additionalProperties:false`, `$ref` to missing or non-local target).
- Unsupported applicators that are NOT `allOf`/`not` (`if`, `then`, `else`, `contains`, `minContains`, `maxContains`, `patternProperties`, `propertyNames`, `dependentSchemas`, `dependentRequired`, `unevaluatedProperties`, `unevaluatedItems`, `$dynamicRef`) MUST raise `IMPORT_UNSUPPORTED` rather than being silently routed to `meta`.

### Accepted import limitation — `x-ui-multiline`

- `x-ui-multiline` is imported as a `meta` entry, NOT as a reconstructed `multiline` validator. The JSON document roundtrips exactly; only the runtime validator is not rebuilt. This is deliberate: external schemas without markers import best-effort.

## Related Decisions

- (none — JSON Schema roundtrip behaviour is fully specified by this concern's Constraints; no register decision binds it.)
