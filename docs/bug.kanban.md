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
severity: accepted

`ZerxValue`'s and `LuaValue`'s derived `Clone`, `Drop`, `PartialEq`, `Debug` and `Serialize` recurse once per nesting level; a value a few thousand levels deep overflows a 2 MB debug stack (clone/eq/Debug ~5 000, drop ~20 000, as `serde_json::Value`). Since 3.2.1 zerx never builds or keeps such a value: serde input, Lua input, `replace` results and builder values (`default`/`meta`/`example`/`literal`) are bounded at `MAX_PARSE_DEPTH` (PLAN_VM1_value_depth_bound). Still outside that guarantee: a caller's own clone/drop/`==`/`Debug`/`Serialize` of a deep value it built itself, and deep values captured in the caller's `refine` closures or custom `Validator`s. An iterative `impl Drop` on the public enum would break by-value variant destructuring (E0509) and needs a major version. Decided by the supervisor (DevLead, 2026-11-21): no `impl Drop` and no major version for this; revisit only if a consumer needs it.

### Lua depth boundary is one level stricter than the serde boundary

id: {new}
severity: minor

`any().validate(..)` accepts serde input with exactly `MAX_PARSE_DEPTH` (100) nested containers, while `any().validate_lua(..)` refuses 100 nested tables: the root `lua_transform` frame counts one level on top of the per-table count in `free_convert`. Both refuse with `PARSE_DEPTH_EXCEEDED`; only the boundary differs by one. Found while re-probing PLAN_VM1_value_depth_bound.

### `parse_delta` navigates past a defective schema node

id: {new}
severity: minor

A node whose `default`/`meta`/`example`/`literal` value was refused as too deep rejects every non-null value under full validation, but `parse_delta` navigation never checks the defect on the nodes it passes through, so `parse_delta("/child", v)` under such an object or lazy accepts `v`. That is wider than validating from the root, which the delta rules forbid elsewhere (prestripped keys). Navigation should refuse with `PARSE_DEPTH_EXCEEDED` when it enters a defective node.

### Lua union in a later field pre-empts an earlier field's error

id: {new}
severity: minor

`docs/architecture/lua.md` requires `validate_lua` to keep the native error precedence, but `union`/`OneOf` variant selection runs the parse flow inside the transform pass. For `object{a: number, b: union[string, boolean]}` and Lua `{a="x", b={}}`, `validate_lua` reports `union_mismatch` at `["b"]` while native `validate` reports `type_mismatch` at `["a"]`. Reproduced during the 2026-11-22 documentation pass.

### `sql` policy breaks a nullable root that carries `$id`

id: {new}
severity: minor

The `sql` nullable rewrite lifts only `$defs` and `$schema` to the outer node when it wraps a `type: [T, "null"]` node in `anyOf`; a root `$id` lands inside the `anyOf` member, and the importer refuses `$id` on a non-root node. A root `{"$id": …, "type": ["string", "null"]}` therefore fails with `IMPORT_UNSUPPORTED` under `sql`.

### Local `#` and `#anchor` refs are sent to the external resolver

id: {new}
severity: minor

The deref transform treats every `$ref` that does not start with `#/` as external, so a document-local `"#"` or `"#anchor"` ref in the caller's own document is passed to the caller's resolver, while the same kind of ref inside resolved content is refused. Local refs other than `#/$defs/…` should be refused or left to the core importer consistently.

### An unresolvable `lazy` exports as an accepting `{}`

id: {new}
severity: minor

Export registers `{}` in `$defs` before resolving a lazy and overwrites it only when resolution succeeds. A lazy whose resolution fails (`LAZY_REENTRANCE`) rejects every value at runtime but exports `{}`, which accepts everything, against the fail-closed export rule. Reachable only through a thunk that re-enters its own export.

### `{"type":"array"}` without `items` is refused as malformed

id: {new}
severity: minor

Import raises `IMPORT_MALFORMED` for an array node without `items`, although `{"type":"array"}` is valid Draft 2020-12. It could import as `array(any())`, or at least raise `IMPORT_UNSUPPORTED`. Behaviour and `docs/architecture/json-schema.md` agree; the classification is the question.

### `allOf`/`not` next to `$ref` lose the refusal hint

id: {new}
severity: minor

`allOf`/`not` are refused with a hint towards `extend()`/`union()` only when no base exists; on a `$ref` node the generic coverage refusal fires instead. Both refuse; only the message differs from the documented hint.

### Malformed annotation values are silently dropped on import

id: {new}
severity: minor

A non-string `description`/`title`/`format`/`contentMediaType`, a non-array `examples`, a non-boolean `deprecated`/`readOnly`/`writeOnly` or a non-object `discriminator` is ignored, while malformed assertion keywords raise `IMPORT_MALFORMED`. Nothing is widened, since these are annotations; the grammar handling is inconsistent.

### `{"enum": []}` imports although zerx claims no never type

id: {new}
severity: minor

An empty `enum` imports as `enumerate([])`, which accepts nothing, while an empty `anyOf` and the `false` schema are refused because "zerx has no never type". Safe, but the stated refusal reason is inaccurate for one path.

### `scripts/verify` can leak its watchdog

id: {new}
severity: minor

When the gate group survives TERM and KILL after watchdog ownership failed, the runner exits 71 before killing the watchdog; the cleanup handler then returns early, and the surviving watchdog may signal the recorded process group up to the gate timeout later, by which time the group id can be reused. Found by reading `scripts/verify` during the 2026-11-22 documentation pass; not reproduced.

## In Progress
id: zrx9bug0col0inprogress01

## Done
id: zrx9bug0col0done00000001
