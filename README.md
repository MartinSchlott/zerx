# Zerx

**A Rust schema validator for data that isn't JSON-clean — buffers, Lua values from `mlua`, PostgreSQL JSONBs. Strict by default, serde-native, bidirectional JSON Schema. Schemas you assemble at runtime, not derive from types.**

> **Status:** vision stage — design is settled, implementation has not started. See [`docs/vision.md`](docs/vision.md) for the committed scope and the open architectural decisions. This README describes the target library.

In Rust the type system and `serde` take *types* seriously, and that's where validation usually stops: derive a struct, parse into it, done. But plenty of real data never fits a clean struct — a binary buffer, a Lua table coming back from `mlua`, a PostgreSQL JSONB, a schema you only know at runtime. Zerx steps back: schema validation is useful even when the data isn't JSON-clean and the shape isn't known at compile time. A `zerx::buffer()` is a first-class citizen, and the schema itself is a **value** you build, clone, slice, and extend at runtime — not a type a macro derives. JSON Schema roundtrip still works, through format markers (`format: "buffer"`, `"record"`, `"json"`, `"function"`) that pure-JSON tools can ignore.

If you derive schemas from Rust types, take [`schemars`](https://github.com/GREsau/schemars). If you want derive-macro field validation, take [`garde`](https://github.com/jprochazk/garde). If you push buffers, `mlua` tables, or PostgreSQL JSONBs through a runtime-built validator and need JSON Schema both ways, take Zerx.

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
# with Lua interop (host-opaque values: functions, coroutines, userdata)
cargo add zerx --features mlua
```

Built on `serde` + `serde_json`. The `mlua` layer is an opt-in feature.

## What it does

**Strict-by-default objects.** Unknown properties return a `ZerxError` with code `unknown_property` — a **security boundary**, not a style choice: it rejects typos and unexpected/injected fields at the edge. Relax per schema with `.passthrough()` (preserve unknowns) or `.strip()` (silently drop). Schema-level utilities (`omit`, `omit_read_only`, `omit_write_only`, `partial`, `extend`) and runtime-level ones (`strip_only`, `strip_read_only`, `strip_write_only`) coexist — the runtime layer filters input *before* the mode check, so you can keep strict mode and still drop a known set of keys.

**serde in, serde out.** `validate::<T: Serialize>(&T)` accepts any serde-serializable value directly — no manual conversion step. `ZerxValue` is a serde value over the *full* serde data model, **including bytes**, so it serializes back out to any serde format (json, msgpack, bincode). `serde_json::Value` is serde minus bytes; `ZerxValue` is serde plus validation plus JSON Schema roundtrip.

**First-class non-JSON types.** `zerx::buffer().mime(_)` carries bytes natively and roundtrips through JSON Schema as `format: "buffer"`. `zerx::function()` and `zerx::tvalue()` (the `mlua` feature) validate Lua functions/coroutines and arbitrarily nested userdata returned by `mlua` — host-opaque values conventional validators have to reject or silently coerce. `zerx::json()` validates "anything JSON-serializable" while still rejecting binary data, and roundtrips via `format: "json"`.

**Programmatic assembly.** The schema is a value, not a type. Build it from runtime data, `clone()` it, slice it (`omit`, `partial`), `extend` it — the opposite of derive-based `schemars`. No macro, no compile-time type; the schema exists at runtime and bends to runtime facts (a DB column list, a tenant config, an LLM tool spec).

**mlua normalization.** `validate_lua(&mlua::Value)` normalizes Lua data before validation: 1-based numeric tables become arrays, byte-encoded strings decode to UTF-8, nested tables walk into the right schema variant. Critically, unions run transform-and-validate **per variant**, not "transform once with the first variant's rules" — Lua data shaped for variant B doesn't fail because variant A's transform mangled it.

**Delta and Replace.** Validate sub-tree updates by JSON Pointer without re-sending the whole object. `parse_delta(path, &value)` validates a value against the schema at that path, no instance required. `replace(&instance, path, &value)` returns a new value with the sub-tree replaced and the **whole root** revalidated — `.refine()` cross-field constraints fire correctly.

**Policy-driven JSON Schema import.** `zerx::from_json_schema(&schema, opts.policy("sql"))` runs a composable pre-parse `SchemaTransform` pass over the input and a post-parse `TypeTransform` pass over the resulting types. The built-in `sql` policy maps `int64 → string` (configurable), `jsonb → zerx::json()`, `bytea → zerx::buffer()`, normalizes `anyOf` of `T | null` to `T.nullable()`, and applies SQL-specific format mappings. Register your own with `zerx::register_policy(name, …)`.

**Bidirectional JSON Schema.** `to_json_schema` and `from_json_schema` are designed for roundtrip stability. `$defs`/`$ref` survive, recursive structures survive (lazy placeholders + memoization), format markers survive. `oneOf` imports as a union with `x-oneOf` metadata. `allOf` and `not` raise clear errors instead of being silently dropped. `additionalProperties` handles all four input shapes (`true` / `false` / absent / schema object — the last treated as passthrough). Discriminated unions use Draft 2020-12 `discriminator` and reconstruct correctly even nested inside arrays.

**`Result`, not exceptions.** Rust's `Result` collapses Zex's `parse`/`safeParse` split into one — every operation returns `Result<_, ZerxError>`; use `?`. `ZerxError` carries `path`, `code`, `message`, `received`, `expected`, `inner_errors`, and serializes via `serde` for clean machine-readable handoff.

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

// Lua data through a union — each variant gets its own transform pass
let lua_val: mlua::Value = lua.load("return { 'click', 100, 200 }").eval()?;  // 1-based table
let cmd = zerx::union([
    zerx::tuple([zerx::literal("click"), zerx::number(), zerx::number()]),
    zerx::tuple([zerx::literal("key"),   zerx::string()]),
]);
cmd.validate_lua(&lua_val)?;

// Delta validation without an instance
let post = zerx::object([("title", zerx::string().min(1)), ("body", zerx::string())]);
post.parse_delta("/title", &"New title".into())?;   // Err if invalid

// Replace with full root revalidation
let original = post.validate(&serde_json::json!({ "title": "a", "body": "x" }))?;
let updated  = post.replace(&original, "/title", &"b".into())?;

// SQL JSON Schema import — int64 → string, jsonb → json(), bytea → buffer(), all objects strict
let sql_schema = zerx::from_json_schema(&postgres_json_schema, zerx::Opts::policy("sql"))?;
```

## As a library

```rust
use zerx::{self, ZerxValue, ZerxError};
use serde::Serialize;

fn api_schema() -> zerx::Schema {
    zerx::object([
        ("id",         zerx::uuid()),
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

/// Validate the model's response — even when it arrives as an mlua value.
pub fn validate_from_lua(v: &mlua::Value) -> Result<ZerxValue, ZerxError> {
    api_schema().validate_lua(v)
}
```

## API at a glance

| Group | Members |
|-------|---------|
| Basic | `string`, `number`, `boolean`, `enumerate`, `null`, `any`, `json` |
| Special | `buffer().mime(_)`, `uri`, `url`, `jsonschema`, `function`, `tvalue` |
| Complex | `object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `lazy` |
| Modifiers | `optional`, `nullable`, `default`, `describe`, `title`, `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`, `example`, `refine` |
| Validators | `min`, `max`, `regex`/`pattern`, `int`, `multiline`, `email`, `uuid` |
| Object utils | `passthrough`, `strip`, `partial`, `omit`, `omit_read_only`, `omit_write_only`, `strip_only`, `strip_read_only`, `strip_write_only`, `extend` |
| Validate | `validate`, `validate_lua`, `parse_delta`, `replace` |
| JSON Schema | `to_json_schema`, `from_json_schema`, `register_policy`, `apply_type_transforms` |

## Error handling

Everything returns `Result<_, ZerxError>`; use `?`. No throwing variant — Rust's `Result` already covers what Zex needed two API families for.

```rust
match schema.validate(&bad_data) {
    Ok(value) => { /* ZerxValue, guaranteed schema-conformant */ }
    Err(e) => {
        e.path;          // ["profile", "name"]
        e.code;          // "unknown_property" | "missing_required_field" | "validation_failed" | …
        e.message;       // human-readable, English-only (no i18n)
        e.received;      // the actual value
        e.expected;      // what was expected
        e.inner_errors;  // for unions / nested failures
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

Zerx: a Rust runtime schema validation library built on `serde` + `serde_json`, with an optional `mlua` feature for host-opaque values. It is a conceptual port of [Zex](https://github.com/) (TypeScript) — consult the Zex source for the canonical semantics of every feature: `/Users/martinschlott/Documents/MyProjects/zex` (`docs/definition.md`, `docs/architecture.md`). This section describes the **target** design; see `docs/vision.md` for committed scope and open decisions.

**Core model.** The schema is a runtime value (`zerx::Schema`), not a derived type — built via factory functions (`zerx::object`, `zerx::string`, …), composed, cloned, sliced (`omit`, `partial`), and extended. No type inference: validation returns a dynamic `ZerxValue`, a serde value over the full serde data model (including bytes), not the JSON projection. `ZerxValue` implements `Serialize`/`Deserialize`; `validate::<T: Serialize>` runs an internal serializer so any serde type validates without manual conversion.

**Two-layer values.** (1) serde-bridgeable — everything in serde's data model incl. bytes/buffer; full roundtrip through any serde format. (2) host-opaque (`mlua` feature) — Lua functions/coroutines/userdata, validatable via `validate_lua(&mlua::Value)` (not `Serialize`), surfacing in JSON Schema only as format markers (`function`, `tvalue`); no serde roundtrip.

**Validate flow.** All operations return `Result<_, ZerxError>`. The flow performs (1) circular-reference check, (2) depth limit (Zex: `MAX_PARSE_DEPTH = 100`), (3) default application, (4) optional/nullable handling (default applies on a missing value, not on explicit `null`), (5) type check, (6) validators, (7) type-specific logic. Missing optional properties are omitted from output, never emitted as `null`.

**Object modes.** `strict` (default) → `unknown_property` error; `passthrough` preserves unknowns; `strip` drops them. The runtime-strip layer (`strip_only`, `strip_read_only`, `strip_write_only`) runs before the mode check, so strict + a known strip set is valid. `omit*` changes the schema; `strip*` is runtime-only.

**Unions.** `union` tries each variant in order, collecting per-variant errors into a combined error. `validate_lua` runs transform-and-validate per variant. `discriminated_union` uses a map for O(1) lookup via the discriminator key, Draft 2020-12 `discriminator` on export, and falls back to a plain union on import when variants aren't all objects.

**JSON Schema.** `to_json_schema` composes the base schema + validator schemas + modifier metadata, tracking `$defs`/`$ref` in an export context; recursive/lazy structures get stable registry entries. `from_json_schema` walks the AST and reconstructs types: handles all four `additionalProperties` shapes, imports `oneOf` as a union with `x-oneOf`, recognizes `type: "null"`, applies primitive defaults, keeps defaulted object properties non-optional, raises clear errors on `allOf`/`not`, and resolves `$ref` with memoized lazy placeholders so cycles survive.

**Policy system.** `register_policy(name, { schema_transforms, type_transforms })`. `SchemaTransform`s run pre-parse over the input JSON Schema; `TypeTransform`s run post-parse over the resulting types. Built-in `sql`: int64→string (strategy-driven), jsonb→`json()`, bytea→`buffer()`, nullable normalization, SQL format mapping, `additionalProperties: false` enforcement, enum-as-literals. `apply_type_transforms` applies type transforms only. Deref hook for external `$ref`.

**Error model.** `ZerxError { path, code, message, received, expected, inner_errors }`, `Serialize` for machine-readable handoff. Standard codes include `unknown_property`, `missing_required_field`, `validation_failed`. English-only messages (no i18n).

**Lazy / recursive.** `zerx::lazy(|| schema)` for recursive structures, with a reentrance guard that must not be bypassed. Roundtrip via `$ref` and the export context.

**Open architectural decisions** (must be resolved before implementation — see `docs/vision.md`): (1) modifier composition — enum-variant vs. trait-object wrapper, the decision that defines the internal representation; (2) `Vec<u8>` buffer fidelity — sources must emit bytes via `serialize_bytes` (`serde_bytes`); (3) host-opaque representation in `ZerxValue` — owning vs. handle.

## License

MIT
