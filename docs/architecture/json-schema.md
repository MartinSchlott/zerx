# Concern: JSON Schema

## Purpose

- This concern owns bidirectional JSON Schema (Draft 2020-12): export from `Schema` and import back to `Schema`.
- This concern owns the format markers (`buffer`, `record`, `json`, `jsonschema`) that carry non-JSON types through pure-JSON tools.
- This concern owns `$defs`/`$ref` tracking for shared/recursive subschemas and the memoised lazy placeholders that let cycles survive the roundtrip.

## Non-Goals

- This concern does NOT own the import policy pipeline for external sources — see `policy`.
- This concern does NOT define the types it serialises — see `types`.
- The host-opaque format markers (`function`, `tvalue`) belong to `PLAN_M1_host_opaque`, not to this concern.

## Consumes from

- `schema-core`: `Schema` (schema value that export reads and import reconstructs)
- `types`: validator `json_schema()` fragment contributions (per-validator JSON Schema keyword maps merged during export)

## Provides to

- (none yet — bidirectional public API surface exposed through `lib.rs`)

## External Contracts

- `Schema::to_json_schema()` — infallible export producing a Draft 2020-12 JSON Schema document (`serde_json::Value`).
- `Schema::to_json_schema_with(opts: &ExportOptions)` — same, with optional `$schema` dialect URI injection.

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

## Related Decisions

- (none yet — roundtrip behaviour is settled within the JSON Schema plans)
