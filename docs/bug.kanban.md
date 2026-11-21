# bug

id: zrx9bug0board0tracking0001

> Issue and bug tracking for Zerx. Cards with `severity: accepted` document
> conscious deviations from the target vision (the "won't fix" bucket).

## Open
id: zrx9bug0col0open00000001

### Colliding `pattern`/`format` validators export only the last

id: {new}
severity: accepted

`string().regex("^a").regex("b$")` and `string().email().uuid()` carry two `pattern`/`format` fragments, which one keyword cannot hold. Since 7ebe628 such a node exports as `false` (fail closed), so it does not round-trip. Exporting the extra fragments under `allOf`, with an `allOf` import of pure string-validator fragments, was deferred by the Product Owner (Martin); the fail-closed export stays.

### Missing lazy field ignores the inner schema's optional/default

id: {new}
severity: accepted

`object([("a", lazy(|| string().optional()))])` and `lazy(|| string().default("d"))` both reject `{}` with `required`: `parse_field` reads only the outer lazy's modifiers. The Product Owner decided to keep this strict behaviour for now; `docs/architecture/schema-core.md` records it as an open limitation, not as a rule.

### Non-ASCII group names in patterns are refused

id: {new}
severity: accepted

The pattern translator accepts only ASCII group names (`[A-Za-z_$][A-Za-z0-9_$]*`). ECMA's GroupName grammar uses Unicode ID_Start / ID_Continue, which std does not expose, and `char::is_alphabetic()` / `is_alphanumeric()` are no safe stand-in: in 3.0.0 they let through names ECMA rejects, such as U+0345 (Other_Alphabetic, not ID_Start) and '²' (No, not ID_Continue). Since 3.0.1, any non-ASCII group name is `Unsupported`: `IMPORT_UNSUPPORTED` on import, `PATTERN_INVALID` on a builder pattern. ECMA-valid non-ASCII names such as `ünï` or `Ⅻ` are therefore refused as well. Exact handling would need Unicode ID tables or a dependency.

### Caller-held deep `ZerxValue`/`LuaValue` clone and drop recurse per nesting level

id: {new}
severity: minor

`ZerxValue`'s and `LuaValue`'s derived `Clone`, `Drop`, `PartialEq`, `Debug` and `Serialize` recurse once per nesting level; a value a few thousand levels deep overflows a 2 MB debug stack (clone/eq/Debug ~5 000, drop ~20 000, as `serde_json::Value`). Since 3.2.1 zerx never builds or keeps such a value: serde input, Lua input, `replace` results and builder values (`default`/`meta`/`example`/`literal`) are bounded at `MAX_PARSE_DEPTH` (PLAN_VM1_value_depth_bound). Still outside that guarantee: a caller's own clone/drop/`==`/`Debug`/`Serialize` of a deep value it built itself, and deep values captured in the caller's `refine` closures or custom `Validator`s. An iterative `impl Drop` on the public enum would break by-value variant destructuring (E0509) and needs a major version; open decision for the supervisor.

### Lua depth boundary is one level stricter than the serde boundary

id: {new}
severity: minor

`any().validate(..)` accepts serde input with exactly `MAX_PARSE_DEPTH` (100) nested containers, while `any().validate_lua(..)` refuses 100 nested tables: the root `lua_transform` frame counts one level on top of the per-table count in `free_convert`. Both refuse with `PARSE_DEPTH_EXCEEDED`; only the boundary differs by one. Found while re-probing PLAN_VM1_value_depth_bound.

## In Progress
id: zrx9bug0col0inprogress01

## Done
id: zrx9bug0col0done00000001

### `pattern` uses Rust regex semantics, not ECMA-262

id: {new}
severity: major

The `regex`/`pattern` validator compiles with the `regex` crate in Unicode mode, so `\d`, `\w` and `\b` match beyond ASCII and `.` matches `\r`, unlike ECMA-262, which JSON Schema `pattern` uses. `string().regex("^\\d+$")` and the import of `{"type":"string","pattern":"^\\d+$"}` both accept `"١٢٣"`, which the exported or source schema forbids (`D-import-fail-closed`). Found in the bug sweep source audit. Fixed by PLAN_T6_ecma_pattern_dialect: patterns are read as ECMA-262 with intersection semantics.

### `parse_delta` rejects unknown keys on a passthrough object

id: {new}
severity: minor

With `object([("a", string())]).passthrough()`, `validate({"a":"x","b":1})` succeeds but `parse_delta("/b", &1)` fails with `unknown_property`. Navigation looks only at `shape`. This is a wrong rejection, not fail-open, and no doc rule decides it. Found in the bug sweep source audit.

### Imported integer bounds beyond 2^53 compare in f64

id: {new}
severity: minor

`{"type":"integer","maximum":9007199254740992}` accepts `9007199254740993`, and a `maximum` of `i64::MAX - 1` accepts `i64::MAX`. The bound passes through `as_f64`. `docs/architecture/types.md` accepts f64 bound precision for v1, but `D-import-fail-closed` requires every imported assertion to be enforced or refused. Found in the bug sweep source audit.

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
