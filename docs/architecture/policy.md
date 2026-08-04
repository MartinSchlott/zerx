# Concern: Policy

## Purpose

- This concern owns the composable import pipeline: schema transforms (pre-parse, on the input JSON Schema `Value`) and type transforms (post-parse, on the produced `Schema` values).
- This concern owns the built-in `sql` policy (PostgreSQL-focused) and the policy registry (`register_policy`).
- This concern owns the deref hook for external `$ref` resolution.

## Non-Goals

- This concern does NOT own the base JSON Schema walk it wraps — see `json-schema`.
- This concern does NOT define the types the transforms produce — see `types`.
- This concern does NOT add new `SchemaKind` variants or parse-flow logic.

## Consumes from

- `json-schema`: `from_json_schema_inner` (core import walk the pipeline wraps, carrying the caller's unknown-property disposition)
- `schema-core`: `Schema` (the value the pipeline produces and type transforms receive)
- `errors`: `ZerxError` (structured pipeline failure)

## Provides to

(None — public API surface exposed through `lib.rs`.)

## External Contracts

- `register_policy` — register a named policy (overwrite-safe, infallible).
- `from_json_schema_with` — fallible import of a JSON Schema value with policy and transform options; the primary entry point for policy-driven import. The options carry the caller's unknown-property disposition for the import.
- `apply_type_transforms` — apply an ordered sequence of type transforms to a root `Schema` (non-recursive root-only fold).

## External Services

(None.)

## Architecturally Significant Dependencies

- `serde_json` — the JSON `Value` schema transforms operate on; removing or replacing it would require redesigning the schema-transform contract.

## Constraints

### Pipeline order

- `from_json_schema_with` MUST apply transforms in this fixed order: (1) deref transform (if a resolver is provided), (2) policy schema transforms, (3) caller schema transforms, (4) `from_json_schema` core import walk, (5) policy type transforms, (6) caller type transforms.
- The deref transform MUST be applied before policy and caller schema transforms.

### Schema transform ordering

- Schema transforms MUST be applied sequentially, threading the output of each as the input to the next.
- The ordering within the schema-transform list is: deref → policy → caller.

### Type transform ordering

- `apply_type_transforms` MUST fold type transforms in list order over the root `Schema`.
- The ordering within the type-transform list is: policy → caller.
- `apply_type_transforms` MUST NOT itself recurse the `Schema` tree; any tree recursion is the caller-supplied transform's own responsibility.

### Unknown policy

- An unknown policy name passed to `from_json_schema_with` MUST return `Err(POLICY_UNKNOWN)` naming the unknown name. Unknown policy names MUST NOT silently no-op.

### Registry

- The global registry MUST be seeded with the built-in `sql` policy on first access; `from_json_schema_with` with `policy: "sql"` MUST work without any prior `register_policy` call.
- `register_policy` MUST be infallible; it MUST overwrite any existing entry under the same name.
- A poisoned registry lock MUST be recovered (`into_inner` on the poison error) rather than panicking the caller.

### `deep_map_schema` recursion

- `deep_map_schema` MUST apply the mapper to each node before recursing into the mapped result.
- `deep_map_schema` MUST recurse into every position J2's core importer reads as a child schema: `properties` (object values), `$defs` (object values), `items` (schema or array of schemas), `prefixItems` (array of schemas), `additionalProperties` (when it is a schema object, not a boolean), and `anyOf`/`oneOf`/`allOf` arrays.

### Deref hook

- The deref transform MUST act only on nodes whose `$ref` string does NOT start with `#/`; local `#/$defs/…` refs MUST be left untouched for J2's core importer.
- On a non-local `$ref` node the deref transform MUST replace the node with the resolver's result, merging every sibling keyword (every key except `$ref`) onto the resolved object, with the sibling keys taking precedence over the resolved object's keys.

### Built-in `sql` policy schema transforms (pre-parse, in order)

- Nullable normalisation: a node with `type` as an array containing `"null"` plus exactly one other type MUST be rewritten to `anyOf:[{…node…, type:T}, {type:"null"}]` (dropping `oneOf`/`allOf` from the rewritten core). A node with `oneOf` of exactly two members where exactly one is `{"type":"null"}` MUST be rewritten to `anyOf:[<other>, {type:"null"}]`.
- Array `items` fallback: a `{"type":"array"}` node without `items` MUST have `"items": {}` added (imports as `array(any())`).
- pg-type substitution: nodes carrying PostgreSQL type signals MUST be rewritten as follows, with the signal keyword removed:
  - `{"type":"string","format":"bytea"}` (case-insensitive) or `{"type":"string","x-pg-type":"bytea"}` → `{"type":"string","format":"buffer"}`.
  - `{"x-pg-type":"json"}` or `{"x-pg-type":"jsonb"}` → `{"format":"json"}` (type removed).
  - `{"type":"string","format":"timestamp without time zone"|"timestamp with time zone"|"timestamptz"}` → format set to `"date-time"`.
  - `{"type":"number","format":"int64"}` or `{"type":"number","x-pg-type":"int8"}` → `{"type":"string"}`.
  - `{"type":"number","format":"numeric"|"decimal"}` or `{"type":"number","x-pg-type":"numeric"}` → `{"type":"string"}`.

### Built-in `sql` policy type transforms

- The built-in `sql` policy MUST carry an empty `type_transforms` vector; it performs all substitutions as pre-parse schema transforms.

### Accepted limitation

- Caller-supplied type transforms receive only the root `Schema` and MAY rebuild it using the public constructor/builder surface; in v1 there is no public `Schema` introspection API for reading kind or children, so caller-supplied type transforms are limited to whole-`Schema` rewrites.

### Unknown-property disposition on import

- The caller-supplied disposition MUST apply to every object node the import walk reconstructs, not only the root.
- A node importing as `passthrough` (`additionalProperties: true` or a schema-object value) MUST NOT be affected; the disposition reinterprets only the strict outcome.
- `additionalProperties: false` and an absent `additionalProperties` MUST be treated identically.
- The disposition MUST default to rejection; the default import behaviour MUST be unchanged.
- The disposition MUST NOT be encoded into the exported document; it MUST be re-supplied on every import.
- Union variants are matched first-match-wins; under the strip disposition a variant that strict mode would reject for an extra key MAY match.

## Related Decisions

- `D-strict-by-default`
- `D-result-only-api`
- `D-import-unknown-caller-policy`
