# Decisions

This file lists the project's binding design decisions. Entries are normative and review-hardened. Decisions enter the register only when promoted from a Concept Closeout or a Plan's Doc Update step. Entries may be moved to the Superseded section when replaced, or removed entirely when no longer relevant.

## Active

### D-modifier-composition

**Decision:** We represent a schema as one concrete `Schema` value — an enum kind plus a shared modifier/validator carrier, fronted by typed builder structs — not as a `Box<dyn SchemaType>` trait-object tree and not as a bare enum without typed builders.
**Rationale:** Programmatic assembly needs one uniform cloneable value (which forces type erasure), an enum gives cheap exhaustive matching for export, and typed builders make type-specific methods like `.min()` uncallable on the wrong type at compile time.
**Consequence:** Universal modifiers are exposed on every builder via a blanket `Modify` trait returning the builder type; type-specific validators are inherent methods on their builder only; `SchemaKind` is a closed enum owned by `schema-core` with one delegating dispatch arm per variant; the `Validator` trait and its storage live in `schema-core`, every concrete validator in `types`.

### D-buffer-fidelity

**Decision:** We carry bytes as a first-class `ZerxValue` bytes variant fed by serde's native `serialize_bytes`, not by coercing JSON-style number arrays, and zerx does not depend on `serde_bytes`.
**Rationale:** Serde's data model already includes bytes; coercing `[u8]` arrays would mask data-shape bugs and undermine strict-by-default.
**Consequence:** Callers who need a `Vec<u8>` to arrive as bytes MUST emit it through serde's bytes path themselves (a documented caller-side contract); a `buffer` exports as `format: "buffer"` with `contentMediaType`.

### D-lua-schema-directed

**Decision:** We validate Lua data through a schema-directed `validate_lua` that resolves Lua ambiguity (table → array vs object/record, byte string → string vs buffer) using the schema, with zerx owning the Lua input type (`zerx::lua::LuaValue`) behind a `lua` feature; we do not depend on `mlua`, and `ZerxValue` carries no host-opaque variant.
**Rationale:** The Lua sibling runtime exposes only data (no functions, coroutines, or userdata) at its boundary, and the schema already carries the array-vs-object and string-vs-buffer distinction, so validation and disambiguation are a single pass.
**Consequence:** `ZerxValue` stays purely serde-bridgeable; the `lua` feature adds no external dependency; the Lua input is an owned acyclic tree, so `validate_lua` needs no data-value cycle guard; the dependency runs consumer → zerx, never the reverse.

### D-strict-by-default

**Decision:** Object validation rejects unknown properties by default (`unknown_property`), with relaxing explicit via `.passthrough()` or `.strip()`; we do not default to permissive object validation.
**Rationale:** Rejecting unknown keys at the validation edge is a security boundary that catches typos and injected fields.
**Consequence:** Three modes (`strict`/`passthrough`/`strip`) govern unknown keys; mode setters keep the runtime mode and the exported `additionalProperties` in sync.

### D-serde-value-model

**Decision:** `ZerxValue` is a serde value over serde's full data model including bytes, not modelled on `serde_json::Value` (serde minus bytes), and carries no host-opaque layer.
**Rationale:** The "bastard" is exactly what serde models but JSON discards; buffers must be first-class without being made JSON-clean first.
**Consequence:** `ZerxValue` is a single serde-bridgeable layer with full roundtrip through any serde format; the Lua input type is separate, not a `ZerxValue` layer.

### D-no-type-inference

**Decision:** Zerx validates dynamic data and does not derive schemas from Rust types, infer compile-time types from schemas, or act as a derive-macro field validator; we cede that ground to `schemars`/`typify` and `validator`/`garde`.
**Rationale:** Zerx's consumers validate data whose shape is unknown at compile time, so there is nothing for inference to provide.
**Consequence:** The public API exposes runtime values; `validate` returns `ZerxValue`, not an inferred `T`.

### D-result-only-api

**Decision:** Every fallible operation returns `Result<_, ZerxError>`; we provide no throwing variant and do not mirror Zex's `parse`/`safeParse` split.
**Rationale:** Rust's `Result` and `?` collapse Zex's dual API into one idiomatic path.
**Consequence:** `validate`, `validate_lua`, `from_json_schema`, `parse_delta`, and `replace` return `Result`; `to_json_schema` is infallible.

### D-english-only-errors

**Decision:** `ZerxError` messages are English-only and consumers branch on `code` and `path`, not message text; we do not localise messages.
**Rationale:** Errors are machine-readable artifacts crossing process and tool boundaries; i18n adds cost without serving the consumer.
**Consequence:** `ZerxError` is serialisable (`to_json` / `Serialize`); `received` and `expected` are plain serialisable descriptor strings, never embedded `ZerxValue`s; union failures aggregate per-variant errors into a combined error.

### D-serde-foundation

**Decision:** Zerx is built on `serde` + `serde_json` + `regex` (full `regex` crate, 1.x) as core dependencies; we do not pursue a zero-dependency posture, do not depend on `serde_bytes`, and do not depend on `mlua` or any external Lua crate.
**Rationale:** The serde data model is the premise of the library, and `regex` is required for the arbitrary-pattern validator.
**Consequence:** `regex` is scoped to the `regex`/`pattern` validator and the `url` structural check; the optional `lua` feature adds only a zerx-owned value type; further dependency additions are design-time decisions surfaced in a plan's Dependencies section.

## Superseded

(No superseded decisions yet.)
