# bug

id: zrx9bug0board0tracking0001

> Issue and bug tracking for Zerx. Cards with `severity: accepted` document
> conscious deviations from the target vision (the "won't fix" bucket).

## Open
id: zrx9bug0col0open00000001

### Consumes / Provides duals differ in wording

id: {new}
severity: minor

The permanent-doc consistency check (M2) fails on existing text: the declarations for `types` ↔ `schema-core` (`Validator`), `types` ↔ `json-schema` (validator fragments) and `lua` ↔ `value-model` (`ZerxValue`) name equivalent contracts but do not match exactly. Found during plan review of PLAN_T5_literal_value_equality; pre-existing.

### Plan references in docs/architecture/types.md

id: {new}
severity: minor

The permanent-doc consistency check (M14) fails on existing text: `docs/architecture/types.md` keeps `PLAN_*` references in headings, error-code bullets, its Provides entry and the `all_optional` export rule. The fix drops each reference and keeps the rule. Found during plan review of PLAN_T5_literal_value_equality; pre-existing.

### Import stores unenforced validation keywords as meta

id: {new}
severity: major

`import_value` in `src/json_schema.rs` routes every keyword it does not model into `meta`, including validation keywords: `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`, `uniqueItems`, `minProperties`, `maxProperties` and similar. Reproduced against 538f3c3: `{"type":"number","exclusiveMinimum":0}` accepts `0`, `{"type":"integer","multipleOf":2}` accepts `3`, `{"type":"array","items":{},"uniqueItems":true}` accepts `[1,1]`. Fail-open: the imported schema accepts what the JSON Schema forbids. Required: enforce the keyword or refuse the import; never store a constraint as meta. Commissioned by the supervisor as the follow-up line to PLAN_J4.

### Keywords next to `$ref` are ignored on import

id: {new}
severity: major

A `$ref` node skips import steps 4–11, so every sibling other than annotations is ignored. Since PLAN_J4, `type` is refused there, but any other validation keyword is still dropped. Reproduced against 538f3c3: `{"$ref":"#/$defs/S","required":["x"]}` accepts `{}`. Fail-open; enforce or refuse. Follow-up line to PLAN_J4.

### Constraints next to `anyOf`/`oneOf` and `enum`/`const` are not enforced

id: {new}
severity: major

Validation keywords next to `anyOf`/`oneOf`, the nullable collapse included, are ignored on import, and so is an `enum`/`const` next to `anyOf`/`oneOf`. Example: `{"anyOf":[{"type":"string"},{"type":"number"}],"minLength":3}` accepts `"a"`. PLAN_J4 refuses validation keywords next to `enum`/`const` (and `enum` with `const`) rather than enforcing them. The follow-up enforces or keeps refusing each case, never drops it. Follow-up line to PLAN_J4.

### Programmatic `jsonschema()` accepts any value

id: {new}
severity: major

`check_jsonschema` in `src/types.rs` accepts every value, but `jsonschema()` exports as `{"type":"object","format":"jsonschema"}`. PLAN_J4 checks only the imported form, through an import-side object refinement. The runtime kind and its exported form must agree, with the stricter reading winning (supervisor's instruction). Follow-up line to PLAN_J4.

### `oneOf` without a valid discriminator loses exclusivity

id: {new}
severity: major

`oneOf` without a working discriminator imports as a first-match `union` with `meta("x-oneOf", true)`, which is anyOf semantics. Reproduced against 538f3c3: `{"oneOf":[{"type":"number"},{"type":"integer"}]}` accepts `1`, which matches both members and is forbidden by `oneOf`. Required (supervisor): real exactly-one semantics or refusal, together with the `docs/architecture/json-schema.md` contract update; a silent first-match is not acceptable. Follow-up line to PLAN_J4.

### Validation keywords next to a format marker are dropped

id: {new}
severity: major

The `record`, `jsonschema`, `buffer`, `uri` and `url` marker branches in `import_value` ignore sibling validation keywords. Example: `{"type":"object","format":"jsonschema","required":["x"]}` drops `required`. Found in plan review of PLAN_J4; pre-existing. Enforce or refuse in the follow-up line.

## In Progress
id: zrx9bug0col0inprogress01

## Done
id: zrx9bug0col0done00000001
