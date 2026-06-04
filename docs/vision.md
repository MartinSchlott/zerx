# Zerx — Vision

## Handoff status (read first)

This project is at the **vision stage**. Only `docs/vision.md` exists — there
is no `CLAUDE.md`, no core docs split, no Kanban boards, and no code yet. This
document was authored in a discussion with the Product Owner; an implementing
instance is expected to drive it forward.

**First steps for the implementing instance (in order):**

1. **Adopt the collaboration rules.** Copy/adapt the `CLAUDE.md` from the Zex
   project (`/Users/martinschlott/Documents/MyProjects/zex/CLAUDE.md`) — the
   same roles, workflow, docs structure, and hard rules apply here. The Product
   Owner converses in **German**; all written artifacts are **English**.
2. **Set up the docs structure** per those rules: decide whether this file
   stays `vision.md` or is split into `definition.md` (what/why) +
   `architecture.md` (how) as the convention expects; create
   `bug.kanban.md` and `backlog.kanban.md`.
3. **Resolve the first architectural decision** — *modifier composition*
   (open question 1 below). It governs the internal representation and blocks
   any implementation plan. This is Architect/Concept territory, not a quick
   plan. Decide it with the Product Owner before writing the first plan.
4. Then proceed via the standard workflow (Discussion → Plan → Review →
   Implementation → …).

Treat the scope in this document as committed and battle-tested (see
*Origin* below), not as a menu to trim.

## What it is

Zerx is a **runtime schema validation library for Rust** that validates
real-world data — including data that isn't JSON-clean — and lets you
**assemble schemas programmatically at runtime**. It is the Rust sibling of
[Zex](#relationship-to-zex), carrying over Zex's "bastard" philosophy:
schema validation is useful even when the data being validated isn't pure JSON
(buffers, binary payloads, Lua tables/coroutines from `mlua`).

Two capabilities are the reason zerx exists:

1. **A validator** for dynamic values, where the schema is built and composed
   as a *value* — not derived from a Rust type.
2. **Bidirectional JSON Schema "bastards"** — emit and re-import JSON Schema
   (Draft 2020-12) with format markers (`format: "buffer"`, `"record"`,
   `"json"`, `"function"`, …) that pure-JSON tools can ignore.

## Origin — pain-driven, not greenfield

Zerx is a port of **Zex**, and Zex was not designed by clever people drawing a
beautiful API on a whiteboard the way Zod was. Zex grew **out of necessity**,
feature by feature, each one added because a real project hurt without it:
buffers were needed, Lua interop was needed, structured errors were needed,
cyclic schemas happened, Delta/Replace was needed, SQL import via policies was
needed. **Every feature in this document earned its place through pain.** A
fresh implementer should treat the scope as battle-tested, not speculative —
the question is *how* to do it well in Rust, not *whether* it is needed.

## Positioning — what zerx is *not*

The Rust ecosystem already covers the adjacent ground; zerx deliberately does
**not** duplicate it:

- **No type generation.** `schemars` (Rust type → JSON Schema) and `typify`
  (JSON Schema → Rust type) own that. Zerx does not derive schemas from types.
- **No compile-time type inference.** Zex's `parse(): T` inference existed to
  help humans and IDE assistants. Zerx's consumers are primarily AIs and code
  with full test coverage; the schema validates *dynamic* data whose shape is
  not known at compile time anyway. There is nothing for inference to give.
- **Not a derive macro.** `validator` / `garde` cover derive-based field
  validation. Zerx schemas are runtime values you build, clone, slice, extend.

What no existing crate does — and what zerx is for — is the **combination**:
a fluent, runtime-built combinator that validates non-JSON-clean values *and*
roundtrips them through JSON Schema via format markers.

## Core idea: a bastard extension of serde

`serde` is a trait framework over a data model of ~29 types — and that model
**already includes bytes** (`serialize_bytes`). The "bastard" is mostly what
serde models but JSON throws away. So:

> **`ZerxValue` = a serde value over the *full* serde data model, not the JSON
> projection.** `serde_json::Value` is serde minus bytes; `ZerxValue` is serde
> plus validation plus JSON Schema roundtrip.

This yields a two-layer value model:

1. **serde-bridgeable** — everything in serde's data model, incl. bytes/buffer.
   Full roundtrip through any serde format (json, msgpack, bincode, …).
   `validate<T: Serialize>(&T)` accepts any serde type directly — no manual
   conversion step.
2. **host-opaque** (`mlua` feature) — Lua functions, coroutines, userdata
   returned by `mlua`. Validatable at runtime via a dedicated
   `validate_lua(&mlua::Value)` path (these are not `Serialize`), but **not**
   serde-roundtrip-capable. In JSON Schema they appear only as format markers
   (`format: "function"`, `format: "tvalue"`).

## Strict by default — a security feature

Object validation is **strict by default**: unknown properties raise an error
(`unknown_property`). This is not ergonomics, it is **security** — it catches
typos and rejects unexpected/injected fields at the validation boundary. Three
modes govern unknown keys:

- `strict` (**default**) — unknown keys → error
- `passthrough` — unknown keys preserved in the output
- `strip` — unknown keys silently removed

The default is never silently relaxed; opting out is explicit
(`.passthrough()` / `.strip()`).

## Type catalogue

Ported from Zex. All in scope for v1.

| Group | Types |
|-------|-------|
| Basic | `string`, `number`, `boolean`, `enumerate`, `null`, `any` |
| Complex | `object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `lazy` |
| Special | `buffer` (with MIME), `uri`, `url`, `json`, `jsonschema` |
| Host-opaque (`mlua`) | `function` (coroutines), `tvalue` (userdata) |

## Modifier & validator catalogue

- **Modifiers:** `optional`, `nullable`, `default`, `describe`, `refine`,
  `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`,
  `example`, `title`.
- **Built-in validators:** `min`, `max`, `regex`/`pattern`, `int`,
  `multiline`, `email`, `uuid` (extensible — validators are a plugin trait
  exposing `validate(value)` + `get_json_schema()`).
- **Object utilities:** `passthrough`, `strip`, `partial`, `extend`, `omit`,
  `omit_read_only`/`omit_write_only`, `strip_only`,
  `strip_read_only`/`strip_write_only`.
  - `omit*` changes the **schema** shape; `strip*` is **runtime-only**
    (shape unchanged, input filtered before the mode check).

## API surface

Rust's `Result` collapses Zex's parse/safeParse split into one. There is **no
throwing variant** — everything returns `Result<_, ZerxError>`; callers use `?`.

```rust
schema.validate<T: Serialize>(&T)        -> Result<ZerxValue, ZerxError>
schema.validate_lua(&mlua::Value)        -> Result<ZerxValue, ZerxError>   // mlua feature
schema.to_json_schema(opts?)             -> serde_json::Value
zerx::from_json_schema(&Value, opts?)    -> Result<Schema, ZerxError>

// Delta / Replace (JSON Pointer based)
schema.parse_delta(path, &value)         -> Result<ZerxValue, ZerxError>   // validate value
                                          //   against the sub-schema at `path`
schema.replace(&instance, path, &value)  -> Result<ZerxValue, ZerxError>   // replace at `path`,
                                          //   then FULL root revalidation (incl. refine)

// Policy pipeline
zerx::register_policy(name, policy)
zerx::from_json_schema(&Value, { policy: "sql" })
zerx::apply_type_transforms(schema, transforms)
```

### Runtime safety in the parse flow

Faithful to Zex: validation performs a **circular-reference check** and a
**depth limit** (Zex: `MAX_PARSE_DEPTH = 100`) before type checks, so malicious
or accidental deep/cyclic input cannot exhaust the stack. Default application
precedes optional/nullable handling; `default` applies on a missing value, not
on an explicit `null`. Missing optional properties are **omitted** from the
output, never emitted as `null`/`undefined`.

## A schema with all the trimmings

```rust
use zerx::zerx;

// reusable building block
let address = zerx::object([
    ("street", zerx::string().min(1)),
    ("zip",    zerx::string().regex(r"^\d{5}$")),
    ("geo",    zerx::tuple([zerx::number(), zerx::number()]).optional()),
]);

let message = zerx::object([
    ("id",       zerx::uuid()),
    ("role",     zerx::enumerate(["user", "assistant", "system"]).describe("who sent it")),
    ("content",  zerx::string().min(1).max(8192).describe("message body")),

    // serde-bridgeable bastard: buffer carried natively, MIME on the marker
    ("avatar",   zerx::buffer().mime("image/png").optional()),

    ("metadata", zerx::record(zerx::json()).default(serde_json::json!({}))),
    ("priority", zerx::number().int().min(0).max(9).default(5).meta("x-internal", true)),
    ("tags",     zerx::array(zerx::string().min(1)).max(10).nullable()),
    ("origin",   address.clone().optional()),

    // discriminated union, O(1) variant lookup by "kind"
    ("source",   zerx::discriminated_union("kind", [
        zerx::object([
            ("kind",    zerx::literal("human")),
            ("session", zerx::string()),
        ]),
        zerx::object([
            ("kind", zerx::literal("tool")),
            ("tool", zerx::string()),
            // host-opaque bastard (mlua feature): runtime-validatable, no serde roundtrip
            ("call", zerx::function().describe("callback the tool invokes")),
        ]),
    ])),

    ("attachment", zerx::union([zerx::buffer(), zerx::url()]).optional()),
    ("legacy_id",  zerx::string().deprecated().read_only().optional()),
])
.strict()                                  // default; shown for clarity
.refine(                                   // cross-field rule, re-run on replace()
    |v| v.get("priority").and_then(ZerxValue::as_u64).unwrap_or(0) < 9
        || v.get("role").and_then(ZerxValue::as_str) == Some("system"),
    "priority 9 is reserved for system messages",
);
```

### The emitted JSON Schema bastard (excerpt)

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "avatar":   { "type": "string", "format": "buffer", "contentMediaType": "image/png" },
    "metadata": { "type": "object", "format": "record", "additionalProperties": { "format": "json" } },
    "source":   { "oneOf": [ … ], "discriminator": { "propertyName": "kind" } },
    "call":     { "format": "function" }
  },
  "required": ["id", "role", "content", "source"]
}
```

Pure JSON-Schema tools ignore the `format` markers and see a valid (poorer)
schema; zerx reads them back and reconstructs the bastard.

## Programmatic assembly — the differentiating strength

The schema is a **value**, not a type. Built from runtime data, composed, derived:

```rust
let mut fields = vec![("id", zerx::uuid())];
for col in db_columns {
    fields.push((col.name.as_str(), map_sql_type(&col)));  // built at runtime
}
let row = zerx::object(fields).strict();

let create_dto = row.clone().omit(["id"]);   // derived schema
let patch_dto  = row.clone().partial();      // all-optional variant
let with_audit = row.clone().extend([("created_at", zerx::string())]);
```

This is `schemars`' opposite: no derive, no compile-time type — the schema is
assembled, cloned, sliced, and extended at runtime.

## Error model

Structured and **machine-readable** (English-only messages; no i18n).
Ported from Zex's `ZexError`:

- Fields: `path`, `code`, `message`, `received`, `expected`, `inner_errors`.
- Serializable (`to_json()` / `Serialize`) so errors cross process and tool
  boundaries.
- Consumers branch on `code` and `path`, not on message text.
- Union failures collect per-variant errors into a combined error.

## Cycles, lazy & `$defs`/`$ref`

- `zerx::lazy(|| schema)` for recursive/cyclic schemas, with a reentrance guard
  that prevents infinite recursion on cyclic data — **must not be bypassed**.
- JSON Schema export tracks shared/recursive subschemas in an export context and
  emits stable `$defs` + `$ref`. Import resolves `$ref` with memoized lazy
  placeholders so cycles survive the roundtrip.

## JSON Schema roundtrip (details)

- **Format markers** carry the bastard types: `buffer`, `record`, `json`,
  `jsonschema`, `function`, `tvalue`. External schemas without markers import
  best-effort.
- `oneOf` → union (with `x-oneOf` metadata); discriminated unions use Draft
  2020-12 `discriminator` on export, and fall back to a plain union on import
  when variants are not all objects.
- `additionalProperties` handles all four shapes (`true` / `false` / absent /
  schema object — the latter treated as passthrough).
- `allOf` / `not` on import raise clear errors rather than being silently
  ignored. `type: "null"` is recognized directly.
- Defaults are applied for primitives on import; defaulted object properties
  stay non-optional (matching the runtime invariant).

## Policy pipeline

Composable import pipeline for heterogeneous sources (PostgreSQL, OpenAPI,
custom):

```
from_json_schema(schema, { policy: "sql" })
  → SchemaTransform[]   pre-parse, mutates the input JSON Schema
  → AST walk → zerx types
  → TypeTransform[]     post-parse, mutates the zerx types
```

- Built-in **`sql`** policy (PostgreSQL-focused): `int64 → string`
  (strategy-driven), `jsonb → zerx::json()`, `bytea → zerx::buffer()`,
  nullable normalization (`anyOf` of `T | null` → `T.nullable()`), SQL format
  mapping.
- `zerx::register_policy(name, { schema_transforms, type_transforms })`.
- `zerx::apply_type_transforms(schema, transforms)` for type transforms only.
- Deref hook for external `$ref`.

## Dependencies

Unlike Zex (zero runtime deps), zerx is **built on serde** — that is the whole
premise:

- **Core:** `serde` + `serde_json` (the JSON Schema side and the dynamic value).
- **Optional feature `mlua`:** the host-opaque layer.
- **Likely:** `serde_bytes` for buffer fidelity (see open question below).

Keep the dependency set lean beyond these; no convenience crates without need.

## Open design questions (how, in Rust — not whether)

1. **Modifier composition.** How do common modifiers (`optional`, `nullable`,
   `default`, `refine`, `describe`) compose across types without making
   type-specific methods (`.min()` on a bool) callable? Enum-variant model vs.
   trait-object wrapper. **This decides the internal representation and is the
   first architectural decision** — it must be settled before any plan.
2. **`Vec<u8>` buffer fidelity.** For a serde field to arrive as a *buffer*
   (not a number array), the source must emit bytes via `serialize_bytes`
   (`serde_bytes::ByteBuf` or `#[serde(with = "serde_bytes")]`). Decide how
   zerx documents and/or enforces this.
3. **Host-opaque representation.** How `ZerxValue` holds Lua functions/userdata
   (owning vs. handle/reference) and how that interacts with the cycle guard
   and with `replace()` revalidation.

## Success criteria

1. **Non-JSON data is first-class.** Buffers (with MIME) and `mlua`
   functions/userdata validate and (for buffers) roundtrip through format
   markers — without first being made `serde_json`-clean.
2. **Roundtrip stability.** `schema → to_json_schema → from_json_schema` yields
   a semantically equivalent schema; `$defs`/`$ref`, cycles, and format markers
   survive.
3. **Strict mode catches typos and rejects unexpected fields at runtime** — the
   default is strict.
4. **serde in, serde out.** Any `T: Serialize` validates directly; any
   `ZerxValue` serializes to any serde format.
5. **Programmatic assembly works.** Schemas are built, cloned, sliced
   (`omit`/`partial`), and extended at runtime.
6. **AI-ready.** Schemas hand off to LLMs/tool-use as JSON Schema, and their
   output (including Lua data from `mlua`) validates.

## Relationship to Zex

Zerx is a conceptual port of **Zex**, the TypeScript original — the reference
for the type catalogue, modifier set, JSON Schema marker conventions,
Delta/Replace semantics, the error model, and the policy pipeline. Consult it
before designing each piece.

- **Zex source & docs:** `/Users/martinschlott/Documents/MyProjects/zex`
- **Zex vision:** `/Users/martinschlott/Documents/MyProjects/zex/docs/definition.md`
- **Zex architecture:** `/Users/martinschlott/Documents/MyProjects/zex/docs/architecture.md`

Zerx is a sibling, not a fork: it reuses Zex's *ideas*, not its TypeScript
constraints. Where Rust offers a cleaner path (serde data model, no type
inference, `Result` instead of dual parse APIs), zerx takes it.
