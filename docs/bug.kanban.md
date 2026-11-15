# bug

id: zrx9bug0board0tracking0001

> Issue and bug tracking for Zerx. Cards with `severity: accepted` document
> conscious deviations from the target vision (the "won't fix" bucket).

## Open
id: zrx9bug0col0open00000001

### `pattern` uses Rust regex semantics, not ECMA-262

id: {new}
severity: major
triage: needs-decision

The `regex`/`pattern` validator compiles with the `regex` crate in Unicode mode, so `\d`, `\w` and `\b` match beyond ASCII and `.` matches `\r`, unlike ECMA-262, which JSON Schema `pattern` uses. `string().regex("^\\d+$")` and the import of `{"type":"string","pattern":"^\\d+$"}` both accept `"١٢٣"`, which the exported or source schema forbids (`D-import-fail-closed`). Found in the bug sweep source audit.

#### Triage

Which dialect binds `pattern`, and is translating to ASCII classes enough, or must unsupported ECMA constructs be refused? Recommendation: a plan that compiles patterns with ECMA-equivalent classes (`(?-u:\d)` and similar) and refuses constructs it cannot map, for both the builder and import.

### Colliding `pattern`/`format` validators export only the last

id: {new}
severity: major
triage: needs-decision

`string().regex("^a").regex("b$")` exports only `"pattern":"b$"`, and `string().email().uuid()` exports only `format: uuid`, while the runtime enforces both. The export and its roundtrip are wider than the runtime. Repeated bounds were fixed in d92eb97 by keeping the tightest value; two patterns or formats cannot be merged into one keyword. Found in the bug sweep source audit.

#### Triage

Interim fix landed: a colliding node now exports as `false` (fail closed). Open question for the Product Owner: should the extra fragments export under `allOf`, with import accepting an `allOf` of pure string-validator fragments, so the node round-trips?

### Imported integer bounds beyond 2^53 compare in f64

id: {new}
severity: minor
triage: needs-decision

`{"type":"integer","maximum":9007199254740992}` accepts `9007199254740993`, and a `maximum` of `i64::MAX - 1` accepts `i64::MAX`. The bound passes through `as_f64`. `docs/architecture/types.md` accepts f64 bound precision for v1, but `D-import-fail-closed` requires every imported assertion to be enforced or refused. Found in the bug sweep source audit.

#### Triage

Does the accepted f64 limit cover imported bounds? Recommendation: refuse integer bounds above 2^53 in magnitude on import, or compare them exactly as `compare_json_numbers` does.

### `parse_delta` rejects unknown keys on a passthrough object

id: {new}
severity: minor
triage: needs-decision

With `object([("a", string())]).passthrough()`, `validate({"a":"x","b":1})` succeeds but `parse_delta("/b", &1)` fails with `unknown_property`. Navigation looks only at `shape`. This is a wrong rejection, not fail-open, and no doc rule decides it. Found in the bug sweep source audit.

#### Triage

Should a passthrough object's unknown key navigate to `any()`? Recommendation: yes, matching root validation; strict and strip objects keep rejecting.

### Missing lazy field ignores the inner schema's optional/default

id: {new}
severity: minor
triage: needs-decision

`object([("a", lazy(|| string().optional()))])` and `lazy(|| string().default("d"))` both reject `{}` with `required`: `parse_field` reads only the outer lazy's modifiers. The docs specify only the present-value path, so this may be intended. It errs strict. Found in the bug sweep source audit.

#### Triage

Should a missing value consult the resolved inner schema's `optional`/`default`? Recommendation: decide and state it in `docs/architecture/schema-core.md`. Strict behaviour is safe meanwhile.

## In Progress
id: zrx9bug0col0inprogress01

## Done
id: zrx9bug0col0done00000001

### Non-finite number bounds are silently wrong

id: {new}
severity: minor

`number().min(f64::NAN)` accepts every number (no-op validator, fail-open). `number().min(f64::INFINITY)` and `.max(f64::NAN)` export as `minimum`/`maximum` `0` through the `Number::from_f64` fallback, so the export disagrees with the runtime in both directions. Found in the bug sweep source audit.

### Literals that JSON cannot hold export as `const: null`

id: {new}
severity: minor

`literal(u128::MAX)`, `literal(i128::MIN)` and `literal(f64::NAN)` export `{"const":null}` through the documented `unwrap_or(Null)` fallback. The runtime rejects `null` and the export accepts only `null`, and the re-import becomes `literal(null)`. Found in the bug sweep source audit.

### Invalid discriminated union exports as an accepting `oneOf`

id: {new}
severity: minor

`discriminated_union("k", [object([("k", literal("a").optional())])])` is `DiscriminatorState::Invalid` and rejects every value, but exports `{"oneOf":[…],"discriminator":…}`, which accepts `{"k":"a"}`. The re-import becomes `OneOf`, which accepts it. Found in the bug sweep source audit.

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

`import_value` in `src/json_schema.rs` routes every keyword it does not model into `meta`, including validation keywords: `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`, `uniqueItems`, `minProperties`, `maxProperties` and similar. Reproduced against 538f3c3: `{"type":"number","exclusiveMinimum":0}` accepts `0`, `{"type":"integer","multipleOf":2}` accepts `3`, `{"type":"array","items":{},"uniqueItems":true}` accepts `[1,1]`. Fail-open: the imported schema accepts what the JSON Schema forbids. Required: enforce the keyword or refuse the import; never store a constraint as meta. Commissioned by the supervisor as the follow-up line to PLAN_J4. Fixed by PLAN_J5_import_assertions_fail_closed: refused at import.

### Keywords next to `$ref` are ignored on import

id: {new}
severity: major

A `$ref` node skips import steps 4–11, so every sibling other than annotations is ignored. Since PLAN_J4, `type` is refused there, but any other validation keyword is still dropped. Reproduced against 538f3c3: `{"$ref":"#/$defs/S","required":["x"]}` accepts `{}`. Fail-open; enforce or refuse. Fixed by PLAN_J5_import_assertions_fail_closed: every assertion sibling of `$ref` is refused.

### Constraints next to `anyOf`/`oneOf` and `enum`/`const` are not enforced

id: {new}
severity: major

Validation keywords next to `anyOf`/`oneOf`, the nullable collapse included, are ignored on import, and so is an `enum`/`const` next to `anyOf`/`oneOf`. Example: `{"anyOf":[{"type":"string"},{"type":"number"}],"minLength":3}` accepts `"a"`. PLAN_J4 refuses validation keywords next to `enum`/`const` (and `enum` with `const`) rather than enforcing them. Fixed by PLAN_J5_import_assertions_fail_closed: `anyOf`/`oneOf` siblings are refused unless vacuous; `enum`/`const` siblings are evaluated statically against the members.

### Programmatic `jsonschema()` accepts any value

id: {new}
severity: major

`check_jsonschema` in `src/types.rs` accepts every value, but `jsonschema()` exports as `{"type":"object","format":"jsonschema"}`. PLAN_J4 checks only the imported form, through an import-side object refinement. The runtime kind and its exported form must agree, with the stricter reading winning (supervisor's instruction). Fixed by PLAN_J5_import_assertions_fail_closed: `jsonschema()` accepts only objects.

### `oneOf` without a valid discriminator loses exclusivity

id: {new}
severity: major

`oneOf` without a working discriminator imports as a first-match `union` with `meta("x-oneOf", true)`, which is anyOf semantics. Reproduced against 538f3c3: `{"oneOf":[{"type":"number"},{"type":"integer"}]}` accepts `1`, which matches both members and is forbidden by `oneOf`. Required (supervisor): real exactly-one semantics or refusal, together with the `docs/architecture/json-schema.md` contract update; a silent first-match is not acceptable. Fixed by PLAN_J5_import_assertions_fail_closed: crate-internal `OneOf` kind with exactly-one semantics.

### Validation keywords next to a format marker are dropped

id: {new}
severity: major

The `record`, `jsonschema`, `buffer`, `uri` and `url` marker branches in `import_value` ignore sibling validation keywords. Example: `{"type":"object","format":"jsonschema","required":["x"]}` drops `required`. Found in plan review of PLAN_J4; pre-existing. Fixed by PLAN_J5_import_assertions_fail_closed: unconsumed marker siblings are refused.
