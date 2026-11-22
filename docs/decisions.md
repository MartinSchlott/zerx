# Decisions

This file lists the project's binding design decisions. Entries are normative and review-hardened. Decisions enter the register only when promoted from a Concept Closeout or a Plan's Doc Update step. Entries may be moved to the Superseded section when replaced, or removed entirely when no longer relevant.

## Active

### D-modifier-composition

**Decision:** We represent a schema as one concrete `Schema` value — an enum kind plus a shared modifier/validator carrier, fronted by typed builder structs — not as a `Box<dyn SchemaType>` trait-object tree and not as a bare enum without typed builders.
**Rationale:** Programmatic assembly needs one uniform cloneable value (which forces type erasure), an enum gives cheap exhaustive matching for export, and typed builders make type-specific methods like `.min()` uncallable on the wrong type at compile time.
**Consequence:** Universal modifiers MUST reach every builder through one blanket trait and type-specific validators MUST stay inherent to their builder; the representation, dispatch and ownership rules live in `docs/architecture/schema-core.md` and `docs/architecture/types.md`.

### D-buffer-fidelity

**Decision:** We carry bytes as a first-class `ZerxValue` bytes variant fed by serde's native `serialize_bytes`, not by coercing JSON-style number arrays, and zerx does not depend on `serde_bytes`.
**Rationale:** Serde's data model already includes bytes; coercing `[u8]` arrays would mask data-shape bugs and undermine strict-by-default.
**Consequence:** Callers who need a `Vec<u8>` to arrive as bytes MUST emit it through serde's bytes path themselves (a documented caller-side contract); how a buffer crosses JSON Schema is stated in `docs/architecture/json-schema.md`.

### D-lua-schema-directed

**Decision:** We validate Lua data through a schema-directed `validate_lua` that resolves Lua ambiguity (table → array vs object/record, byte string → string vs buffer) using the schema, with zerx owning the Lua input type (`zerx::lua::LuaValue`) behind a `lua` feature; we do not depend on `mlua` or carry Lua host handles in `ZerxValue`.
**Rationale:** The Lua sibling runtime exposes only data (no functions, coroutines, or userdata) at its boundary, and the schema already carries the array-vs-object and string-vs-buffer distinction, so validation and disambiguation are a single pass.
**Consequence:** The `lua` feature adds no external dependency; the Lua input is an owned acyclic tree, so `validate_lua` needs no data-value cycle guard; the dependency runs consumer → zerx, never the reverse.

### D-strict-by-default

**Decision:** Object validation rejects unknown properties by default (`unknown_property`), with relaxing explicit via `.passthrough()` or `.strip()`; we do not default to permissive object validation.
**Rationale:** Rejecting unknown keys at the validation edge is a security boundary that catches typos and injected fields.
**Consequence:** Three modes (`strict`/`passthrough`/`strip`) govern unknown keys; mode setters keep the runtime mode and the exported `additionalProperties` in sync.

### D-serde-value-model

**Decision:** `ZerxValue` is a serde value over serde's full data model including bytes, not modelled on `serde_json::Value` (serde minus bytes), and carries no host-opaque layer.
**Rationale:** The "bastard" is exactly what serde models but JSON discards; buffers must be first-class without being made JSON-clean first.
**Consequence:** `ZerxValue` is a single serde-bridgeable layer with full roundtrip through any serde format; the Lua input type is separate, not a `ZerxValue` layer.

### D-result-only-api

**Decision:** Every fallible operation returns `Result<_, ZerxError>`; we provide no throwing variant and do not mirror Zex's `parse`/`safeParse` split.
**Rationale:** Rust's `Result` and `?` collapse Zex's dual API into one idiomatic path.
**Consequence:** Every fallible public operation MUST return `Result` with no panicking or `Option`-returning twin; JSON Schema export is infallible by design (`docs/architecture/json-schema.md`).

### D-serde-foundation

**Decision:** Zerx is built on `serde` + `serde_json` + `regex` (full `regex` crate, 1.x) as core dependencies; we do not pursue a zero-dependency posture.
**Rationale:** The serde data model is the premise of the library, and `regex` is required for the arbitrary-pattern validator.
**Consequence:** Where `regex` is used is stated in `docs/architecture/types.md`; `serde_bytes` and Lua crates stay out (`D-buffer-fidelity`, `D-lua-schema-directed`).

### D-import-unknown-caller-policy

**Decision:** The disposition of unknown properties on JSON Schema import is a caller-side option in `ImportOptions`, not a schema keyword, vendor extension, or named registry policy.
**Rationale:** JSON Schema is a validation vocabulary, not a transform vocabulary; `additionalProperties: false` says unknown properties do not belong, and whether that means reject or remove is the caller's choice — encoding it in the document would make a portable schema carry zerx-specific meaning.
**Consequence:** Import-time behaviour that is not a statement about validity MUST enter through `ImportOptions` and MUST NOT be introduced as an `x-`-prefixed keyword; such an option MUST NOT be encoded into the exported document and MUST be re-supplied on every import; it stays orthogonal to `policy` so it composes with any registered policy.
**Scope:** docs/architecture/json-schema.md, docs/architecture/policy.md

### D-import-fail-closed

**Decision:** We import a JSON Schema as a schema that accepts nothing the source forbids, refusing with `IMPORT_UNSUPPORTED`/`IMPORT_MALFORMED` what zerx cannot reconstruct. We do not import a wider schema, fall back to `any()`, or store an unenforced assertion keyword as `meta`.
**Rationale:** zerx is strict by design. An imported schema that silently accepts what its source forbids is a security-boundary failure, and a refusal surfaces it to the author.
**Consequence:** Every assertion keyword on an imported node MUST be enforced by the reconstruction, vacuous for the node's declared type, or refused; `meta` MUST carry only non-assertion keys. A deliberate narrowing, which accepts less than the source, MAY exist only where `docs/architecture/json-schema.md` names it.
**Scope:** docs/architecture/json-schema.md, docs/architecture/policy.md

## Superseded

(No superseded decisions yet.)
