# Zerx

**A Rust schema validator for data that isn't JSON-clean — buffers, Lua values, PostgreSQL JSONBs. Strict by default, serde-native, bidirectional JSON Schema. Schemas you assemble at runtime, not derive from types.**

> **Status:** v2.0.0 — implemented and tested. The What/Why lives in [`docs/definition.md`](docs/definition.md), the binding design decisions in [`docs/decisions.md`](docs/decisions.md), the structure in [`docs/architecture/`](docs/architecture/).

In Rust the type system and `serde` take *types* seriously, and that's where validation usually stops: derive a struct, parse into it, done. But plenty of real data never fits a clean struct — a binary buffer, a Lua table coming back from a sandboxed runtime, a PostgreSQL JSONB, a schema you only know at runtime. Zerx steps back: schema validation is useful even when the data isn't JSON-clean and the shape isn't known at compile time. A `zerx::buffer()` is a first-class citizen, and the schema itself is a **value** you build, clone, slice, and extend at runtime — not a type a macro derives. JSON Schema roundtrip still works, through format markers (`format: "buffer"`, `"record"`, `"json"`) that pure-JSON tools can ignore.

If you derive schemas from Rust types, take [`schemars`](https://github.com/GREsau/schemars). If you want derive-macro field validation, take [`garde`](https://github.com/jprochazk/garde). If you push buffers, Lua tables, or PostgreSQL JSONBs through a runtime-built validator and need JSON Schema both ways, take Zerx.

```rust
use zerx::{self, ZerxError};

let user = zerx::object([
    ("name",   zerx::string().min(2)),
    ("email",  zerx::string().email()),
    ("role",   zerx::enumerate(["admin", "user", "guest"])),
    ("avatar", zerx::buffer().mime("image/jpeg").optional()),
]); // strict by default

user.validate(&my_user_struct)?;  // ok — any T: Serialize, no manual conversion

user.validate(&serde_json::json!({
    "name": "Alice", "email": "a@b.com", "role": "admin", "typo": 1,
}))?;                              // Err(unknown_property) — strict by default

let json_schema = user.to_json_schema();          // serde_json::Value with format markers
let recreated   = zerx::from_json_schema(&json_schema)?;
```

## Install

```bash
cargo add zerx
# with schema-directed Lua-value validation (zerx-owned LuaValue input type)
cargo add zerx --features lua
```

Built on `serde` + `serde_json` + `regex`. The `lua` feature is opt-in and adds no further dependency — only a zerx-owned `LuaValue` input type and the `validate_lua` entry point.

## What it does

**Strict-by-default objects.** Unknown properties return a `ZerxError` with code `unknown_property` — a **security boundary**, not a style choice: it rejects typos and unexpected/injected fields at the edge. Relax per schema with `.passthrough()` (preserve unknowns) or `.strip()` (silently drop), or per import with `ImportOptions.strip_unknown` (drops what an imported schema would otherwise reject). Schema-level utilities (`omit`, `omit_read_only`, `omit_write_only`, `partial`, `extend`) and runtime-level ones (`strip_only`, `strip_read_only`, `strip_write_only`) coexist — the runtime layer filters input *before* the mode check, so you can keep strict mode and still drop a known set of keys.

**serde in, serde out.** `validate::<T: Serialize>(&T)` accepts any serde-serializable value directly — no manual conversion step. `ZerxValue` is a serde value over the *full* serde data model, **including bytes**, so it serializes back out to any serde format (json, msgpack, bincode). `serde_json::Value` is serde minus bytes; `ZerxValue` is serde plus validation plus JSON Schema roundtrip.

**First-class non-JSON types.** `zerx::buffer().mime(_)` carries bytes natively and roundtrips through JSON Schema as `format: "buffer"`. `zerx::json()` validates "anything serde-serializable" while still rejecting nothing structurally, and roundtrips via `format: "json"`; `zerx::record(_)` validates open string-keyed maps and roundtrips via `format: "record"`. Buffers are produced only by serde's `serialize_bytes` path — a number array is not silently coerced into bytes.

**Schema-directed Lua validation.** Under the `lua` feature, `validate_lua(&zerx::LuaValue)` validates a Lua data value (`nil`/`bool`/`int`/`float`/`bytes`/`table`) and disambiguates it **using the schema**: at an `array()` node a table reads its array part, at `object()`/`record()` its hash part; at `string()` a byte string decodes as UTF-8, at `buffer()` it stays raw bytes. zerx owns the `LuaValue` input type — a sandboxed Lua runtime (e.g. a sibling crate) maps its boundary values into it; the dependency runs runtime → zerx. There is no live-handle interop and no `mlua` dependency.

**Programmatic assembly.** The schema is a value, not a type. Build it from runtime data, `clone()` it, slice it (`omit`, `partial`), `extend` it — the opposite of derive-based `schemars`. No macro, no compile-time type; the schema exists at runtime and bends to runtime facts (a DB column list, a tenant config, an LLM tool spec).

**Delta and Replace.** Validate sub-tree updates by JSON Pointer without re-sending the whole object. `parse_delta(path, &value)` validates a value against the schema at that path, no instance required. `replace(&instance, path, &value)` returns a new value with the sub-tree replaced and the **whole root** revalidated — `.refine()` cross-field constraints fire correctly.

**Policy-driven JSON Schema import.** `from_json_schema_with(&schema, &opts)` runs a composable pre-parse `SchemaTransform` pass over the input and a post-parse `TypeTransform` pass over the resulting types. The built-in `sql` policy (PostgreSQL-focused) maps `int64 → string`, `jsonb → zerx::json()`, `bytea → zerx::buffer()`, normalizes `anyOf` of `T | null` to `T.nullable()`, and applies SQL format mappings (e.g. `timestamp* → date-time`). Register your own with `zerx::register_policy(name, …)`. `ImportOptions.strip_unknown` is a separate, orthogonal switch: it makes an imported schema *drop* unknown properties instead of rejecting them, at every object node — the self-healing move for a persisted config file whose schema has since lost a field. It composes with any policy, and it never touches nodes the schema declared `additionalProperties: true`.

**Bidirectional JSON Schema.** `to_json_schema` and `from_json_schema` are built for roundtrip stability. `$defs`/`$ref` survive, recursive structures survive (lazy placeholders + memoization), format markers survive. `oneOf` imports as a union with `x-oneOf` metadata. `allOf` and `not` raise clear errors instead of being silently dropped. `additionalProperties` handles all four input shapes (`true` / `false` / absent / schema object — the last treated as passthrough). Discriminated unions use Draft 2020-12 `discriminator` and reconstruct correctly even nested inside arrays.

**`Result`, not exceptions.** Every operation returns `Result<_, ZerxError>`; use `?`. No throwing variant, no `parse`/`safeParse` split. `ZerxError` carries `path`, `code`, `message`, `received`, `expected`, `inner_errors`, and serializes via `serde` for clean machine-readable handoff.

## What it does not do

- **No type generation or inference.** Zerx never derives a schema from a Rust type and never infers a compile-time `T` from a schema — `schemars`/`typify` and `validator`/`garde` own that ground. `validate` returns a dynamic `ZerxValue`, not an inferred type.
- **No live Lua handles.** Functions, coroutines, and userdata are not representable; zerx validates Lua *data* only, via schema-directed disambiguation. There is no `mlua` dependency and no host-opaque value variant.
- **No zerx keywords in your JSON Schema.** Behaviour never enters through the document. JSON Schema says what is *valid*, not what to do with what isn't — so `additionalProperties: false` means "unknown properties do not belong here" and nothing more. Whether that means reject or drop is caller policy (`ImportOptions.strip_unknown`), which keeps an exported schema portable to tools that never heard of zerx. There is no `x-zerx-*` vocabulary and there will not be one.
- **No i18n.** Error messages are English-only; consumers branch on `code` and `path`, never on message text.
- **No async or streaming validation.** Validation is synchronous and `Result`-returning.

## Quick taste

```rust
// Refinement with a custom message
let password = zerx::string().min(8).refine(
    |v| v.as_str().is_some_and(|s| s.bytes().any(|b| b.is_ascii_uppercase())
                                    && s.bytes().any(|b| b.is_ascii_digit())),
    "must contain an uppercase letter and a digit",
);

// Discriminated union, O(1) variant lookup by "kind"
let event = zerx::discriminated_union("kind", [
    zerx::object([("kind", zerx::literal("click")), ("x", zerx::number()), ("y", zerx::number())]),
    zerx::object([("kind", zerx::literal("key")),   ("code", zerx::string())]),
]);

// Schema-directed Lua validation (feature = "lua")
// The schema decides: this table is a tuple, its byte strings are strings/literals.
use zerx::{LuaValue, LuaTable};
let lua_val = LuaValue::Table(LuaTable {
    array: vec![LuaValue::Bytes(b"click".to_vec()), LuaValue::Integer(100), LuaValue::Integer(200)],
    hash: vec![],
});
let cmd = zerx::tuple([zerx::literal("click"), zerx::number(), zerx::number()]);
cmd.validate_lua(&lua_val)?;

// Delta validation without an instance
let post = zerx::object([("title", zerx::string().min(1)), ("body", zerx::string())]);
post.parse_delta("/title", &"New title".into())?;   // Err if invalid

// Replace with full root revalidation
let original = post.validate(&serde_json::json!({ "title": "a", "body": "x" }))?;
let updated  = post.replace(&original, "/title", &"b".into())?;

// SQL JSON Schema import — int64 → string, jsonb → json(), bytea → buffer()
use zerx::ImportOptions;
let sql_schema = zerx::from_json_schema_with(
    &postgres_json_schema,
    &ImportOptions { policy: Some("sql".into()), ..Default::default() },
)?;

// Self-healing config: the schema dropped a field, the stored file still carries it
let config = zerx::from_json_schema_with(
    &app_json_schema,
    &ImportOptions { strip_unknown: true, ..Default::default() },
)?;
let healed = config.validate(&stored_file)?;   // stale keys gone, at every depth
```

## As a library

```rust
use zerx::{self, ZerxValue, ZerxError};
use serde::Serialize;

fn api_schema() -> zerx::Schema {
    zerx::object([
        ("id",         zerx::string().uuid()),
        ("payload",    zerx::json()),
        ("signature",  zerx::buffer().mime("application/octet-stream")),
        ("created_at", zerx::string().format("date-time")),
    ])
}

/// Validate any serde-serializable input.
pub fn validate<T: Serialize>(input: &T) -> Result<ZerxValue, ZerxError> {
    api_schema().validate(input)
}

/// Hand the JSON Schema to an LLM tool-use API.
pub fn api_tool_schema() -> serde_json::Value {
    api_schema().to_json_schema()
}

/// Validate the model's response — even when it arrives as Lua data (feature = "lua").
#[cfg(feature = "lua")]
pub fn validate_from_lua(v: &zerx::LuaValue) -> Result<ZerxValue, ZerxError> {
    api_schema().validate_lua(v)
}
```

## API at a glance

| Group | Members |
|-------|---------|
| Basic | `string`, `number`, `boolean`, `enumerate`, `null`, `any`, `json` |
| Special | `buffer().mime(_)`, `uri`, `url`, `jsonschema` |
| Complex | `object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `lazy` |
| Modifiers | `optional`, `nullable`, `default`, `describe`, `title`, `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`, `example`, `refine` |
| Validators | `min`, `max`, `regex`/`pattern`, `int`, `email`, `uuid` |
| Object utils | `passthrough`, `strip`, `partial`, `omit`, `omit_read_only`, `omit_write_only`, `strip_only`, `strip_read_only`, `strip_write_only`, `extend` |
| Validate | `validate`, `validate_lua` (feature `lua`), `parse_delta`, `replace` |
| JSON Schema | `to_json_schema`, `to_json_schema_with`, `from_json_schema`, `from_json_schema_with`, `register_policy`, `apply_type_transforms` |

`string().multiline(n)` is a UI meta hint (`x-ui-multiline`), not a validator — it has no validation effect.

## Error handling

Everything returns `Result<_, ZerxError>`; use `?`. No throwing variant — Rust's `Result` already covers what Zex needed two API families for.

```rust
match schema.validate(&bad_data) {
    Ok(value) => { /* ZerxValue, guaranteed schema-conformant */ }
    Err(e) => {
        e.path;          // Vec<String>, e.g. ["profile", "name"]
        e.code;          // ErrorCode, e.g. "unknown_property" | "required" | "type_mismatch"
        e.message;       // human-readable, English-only (no i18n)
        e.received;      // Option<String> — a compact type tag, never the raw value
        e.expected;      // Option<String> — what the schema required
        e.inner_errors;  // Vec<ZerxError>, for unions / nested failures
        // ZerxError: Serialize — clean machine-readable serialization
    }
}
```

Consumers branch on `code` and `path`, never on message text.

---

*The compiler validates the types you wrote. Zerx validates the data you got.* — Claude Opus 4.8

*Das ist kein Zod wo schlaue Menschen schöne Dinge gemacht haben. Das ist aus der Not gewachsen.* — AI-Inquisitor

---

## LLM Reference

Zerx: a Rust runtime schema-validation library (v2.0.0) built on `serde` + `serde_json` + `regex`, with an optional `lua` feature for schema-directed validation of Lua data. It is a conceptual port of Zex (TypeScript); the type catalogue, modifier set, JSON Schema marker conventions, Delta/Replace semantics, error model, and policy pipeline carry over. Normative docs: `docs/definition.md`, `docs/decisions.md`, `docs/architecture/`.

**Core model.** The schema is a runtime value (`zerx::Schema`), not a derived type — built via factory functions (`zerx::object`, `zerx::string`, …), composed, cloned, sliced (`omit`, `partial`), and extended. No type inference: validation returns a dynamic `ZerxValue`, a serde value over the full serde data model (including bytes), not the JSON projection. `ZerxValue` is `'static`, `Clone + PartialEq + Debug`, implements `Serialize`; `validate::<T: Serialize>` runs an internal serializer so any serde type validates without manual conversion. There is no host-opaque layer.

**Representation.** `Schema` is one concrete value: a `SchemaKind` enum, a shared modifier carrier, and a `Vec<Box<dyn Validator>>`. Typed builder structs (`StringSchema`, `NumberSchema`, …) front it and expose type-specific methods (`.min()` exists on `StringSchema`/`NumberSchema`, never on `BooleanSchema`); universal modifiers come from the blanket `Modify` trait. Builders convert to `Schema` via `Into<Schema>`. Every modifier is clone-and-return; `Schema` is not mutated in place. `Schema` is not `Send`/`Sync`.

**Validate flow.** All operations return `Result<_, ZerxError>`. For a present value: depth guard → `nullable` null short-circuit → type check → validators (in storage order) → type-specific logic → refinement predicates. For a missing value: apply `default` if set, else omit if `optional`, else `Err(required)`. `default` applies on a missing value, not on explicit `null`. Missing optional properties are omitted from output, never emitted as `Null`. `MAX_PARSE_DEPTH = 100`.

**Lua (`lua` feature).** `LuaValue` is `{ Nil, Boolean(bool), Integer(i64), Float(f64), Bytes(Vec<u8>), Table(LuaTable) }`; `LuaTable { array: Vec<LuaValue>, hash: Vec<(LuaValue, LuaValue)> }`. `schema.validate_lua(&LuaValue) -> Result<ZerxValue, ZerxError>` is schema-directed: the schema node resolves table → array vs object/record and byte string → string vs buffer. A non-string hash key under `object`/`record` errors (`lua_invalid_key`); a shape that does not fit the node errors (`lua_shape_mismatch`). The input is an owned, acyclic tree — no cycle guard, no live handles, no `mlua`.

**JSON Schema.** `to_json_schema` / `to_json_schema_with(&ExportOptions)` compose base schema + validator fragments + modifier metadata, tracking `$defs`/`$ref`; recursive/lazy structures get stable registry entries. `from_json_schema` / `from_json_schema_with(&ImportOptions)` walk the AST: all four `additionalProperties` shapes, `oneOf` → union with `x-oneOf`, `type: "null"`, primitive defaults (defaulted object properties stay non-optional), clear errors on `allOf`/`not`, `$ref` resolved with memoized lazy placeholders so cycles survive. When `ImportOptions.strip_unknown` is set, `false`/absent `additionalProperties` reconstructs as `strip` mode instead of `strict` at every object node the walk builds, not only the root; `true` and a schema-object value stay `passthrough`, untouched.

**Policy system.** `register_policy(name, Policy { schema_transforms, type_transforms })`. `SchemaTransform`s run pre-parse over the input JSON `Value`; `TypeTransform`s run post-parse over the resulting `Schema`. `ImportOptions { policy: Option<String>, schema_transforms, type_transforms, deref, strip_unknown: bool }` — `strip_unknown` (default `false`) is the caller-side disposition for unknown object properties on import; see the Invariants below. Order: deref → policy schema transforms → caller schema transforms → core import → policy type transforms → caller type transforms. Built-in `sql` policy does all its work as pre-parse schema transforms (its `type_transforms` is empty): int64→string, jsonb/json→`format: json`, bytea→`format: buffer`, `T | null` nullable normalization, timestamp/numeric format mapping. An unknown policy name errors `policy_unknown`. `apply_type_transforms` applies type transforms only.

**Public API (verbatim from `src/lib.rs`).**
- `error`: `ErrorCode`, `ZerxError`
- `value`: `Map`, `ZerxValue`
- `schema`: `Schema`, `Modify`, `Validator`, `AnySchema`, `LazySchema`, `any`, `lazy`, `MAX_PARSE_DEPTH`
- `types` (factories): `string`, `number`, `boolean`, `enumerate`, `null`, `object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `buffer`, `uri`, `url`, `json`, `jsonschema` (plus the matching `*Schema` builder structs)
- `json_schema`: `ExportOptions`, `DRAFT_2020_12`, `from_json_schema`
- `policy`: `register_policy`, `from_json_schema_with`, `apply_type_transforms`, `Policy`, `ImportOptions`, `SchemaTransform`, `TypeTransform`, `RefResolver`
- `lua` (feature `lua`): `LuaValue`, `LuaTable`
- methods on `Schema`: `validate`, `validate_lua` (feature `lua`), `to_json_schema`, `to_json_schema_with`, `parse_delta`, `replace`

**Error model.** `ZerxError { path: Vec<String>, code: ErrorCode, message: String, received: Option<String>, expected: Option<String>, inner_errors: Vec<ZerxError> }`, `Serialize` only (errors flow outward). `received`/`expected` are compact descriptor strings, never an embedded value. `ErrorCode` wraps a stable lowercase-snake string. Common codes: `unknown_property`, `required`, `type_mismatch`, `invalid_enum_value`, `invalid_literal`, `union_mismatch`, `invalid_discriminant`, `invalid_discriminated_union`, `string_too_short`, `string_too_long`, `pattern_invalid`, `pattern_mismatch`, `invalid_email`, `invalid_uuid`, `number_too_small`, `number_too_large`, `not_integer`, `array_too_short`, `array_too_long`, `tuple_length_mismatch`, `invalid_uri`, `invalid_url`, `refinement_failed`, `parse_depth_exceeded`, `lazy_reentrance`, `invalid_pointer`, `missing_parent`, `policy_unknown`, `lua_invalid_key`, `lua_shape_mismatch`.

**Runtime dependencies.** `serde` (with `derive`), `serde_json`, `regex` (full crate). The `lua` feature adds no dependency. No `serde_bytes`, no `mlua`.

**Invariants — things that will bite you if you assume otherwise:**

Buffers arrive only through serde's `serialize_bytes`. A plain `Vec<u8>` serializes as a number array and fails a `buffer()` schema; the field's source must emit bytes (e.g. `serde_bytes` / `ByteBuf` on the caller's own type). Zerx never coerces a number array into bytes.

`received` and `expected` are `Option<String>` descriptor tags (a type tag, the expected shape), never the offending value itself. Do not expect to recover the raw input from a `ZerxError`.

Strict is the default. The first unknown key errors `unknown_property` and field validation stops there. You must opt out explicitly with `.passthrough()` or `.strip()` on the schema, or with `ImportOptions.strip_unknown` on import. The runtime strip layer (`strip_only`/`strip_read_only`/`strip_write_only`) runs *before* the unknown-key check, so strict mode plus a known strip set is valid; `omit*` changes the schema shape, `strip*` is runtime-only.

`ImportOptions.strip_unknown` (default `false`) is a third, import-time-only relaxation channel, orthogonal to `policy` — it composes with any registered policy, including `sql`. When `true`, it recursively drops unknown properties at every object node the import walk reconstructs, not only the root, but only where the node would otherwise be `strict`: `additionalProperties: false` and an absent `additionalProperties` are treated alike, while `true` or a schema-object value (passthrough) are untouched. It is never encoded into the exported document — a schema imported with `strip_unknown: true` still exports `additionalProperties: false`, so the flag must be re-supplied on every import, not just the first. `union`/`anyOf` variants are matched first-match-wins, so under `strip_unknown: true` a variant that strict mode would have rejected for an extra key can match instead, silently dropping that key.

`default` applies only when a value is absent, not on an explicit `null`. A missing optional field is omitted from output entirely — it is never emitted as `Null`.

`replace` revalidates the whole root (refinements fire), not just the replaced sub-tree, and it only sets a position — it never deletes and never creates an absent parent (`missing_parent`). The replacement value is `T: Serialize`, so a Lua value cannot be passed as the new value.

`validate_lua` is schema-directed, not a fixed `LuaValue → ZerxValue` map: the same Lua table validates as an array under `array()` and as a map under `object()`/`record()`, and the same byte string validates as text under `string()` and as bytes under `buffer()`. The `lua` surface (`LuaValue`, `LuaTable`, `validate_lua`) exists only under the `lua` feature.

`discriminated_union` construction is infallible; a structural defect (e.g. a variant whose discriminator is not a required literal) surfaces as `invalid_discriminated_union` at validate time and wins over `type_mismatch` for any input.

Deep or cyclic serde input errors `parse_depth_exceeded` at depth 100 rather than overflowing the stack; `lazy` schemas carry a reentrance guard (`lazy_reentrance`) that must not be bypassed.

## Verification

`scripts/verify all` is the canonical verification entrypoint — lint, both test-feature configurations, and the compile-time API doctests, run once each, quiet on success. `docs/tests.md` is the verification contract: gate table, exit-status meanings, and the test inventory.

## License

MIT. See `LICENSE`.
