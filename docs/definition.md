# Product Definition: Zerx

## Vision

- Zerx exists to validate real-world data in Rust at runtime — including data that is not JSON-clean (binary buffers, Lua values from `mlua`, PostgreSQL JSONBs) — against schemas assembled programmatically as runtime values.
- It serves Rust programs, and the AIs that drive them, that must validate dynamic data whose shape is not known at compile time and that need that validation to roundtrip through JSON Schema.
- Zerx is the Rust sibling of Zex; it reuses Zex's ideas, not its TypeScript constraints.

## Scope

- The product covers runtime validation of dynamic values against schemas that are built, cloned, sliced, and extended at runtime.
- The product covers bidirectional JSON Schema (Draft 2020-12) export and import, carrying non-JSON types through format markers.
- The product covers validation of non-JSON-clean values: binary buffers (with MIME) natively, and host-opaque Lua values via a dedicated path under an optional feature.
- The product covers an import policy pipeline for heterogeneous schema sources (e.g. PostgreSQL).

## Non-Goals (product-level)

- Zerx MUST NOT derive schemas from Rust types — that ground is owned by `schemars` and `typify`.
- Zerx MUST NOT provide compile-time type inference from schemas.
- Zerx MUST NOT be a derive-macro field validator — that ground is owned by `validator` and `garde`.
- Zerx MUST NOT localise error messages; messages are English-only.

## Features

- `Runtime schema assembly` — schemas are values built from runtime data and composed via `omit`, `partial`, and `extend`.
- `Dynamic value validation` — any `T: Serialize` validates directly against a schema and returns a structured result.
- `Non-JSON-clean validation` — buffers and host-opaque Lua values validate without first being made JSON-clean.
- `Strict validation` — unknown object properties are rejected by default; relaxing is explicit.
- `Bidirectional JSON Schema` — schemas export to and import from JSON Schema, with format markers preserving non-JSON types.
- `Delta / Replace` — validate a value against a sub-schema, or replace a value at a path with full root revalidation.
- `Policy-driven import` — heterogeneous JSON Schema sources import through a composable transform pipeline.

## Entities

- `Schema` — a runtime-built, cloneable validation rule; the central value the user assembles.
- `ZerxValue` — a validated dynamic value spanning serde's full data model plus host-opaque values.
- `ZerxError` — a structured, machine-readable validation failure.
- `Validator` — a pluggable constraint attached to a schema (e.g. `min`, `regex`).
- `Modifier` — a schema attribute that composes across types (e.g. `optional`, `default`, `describe`).
- `Policy` — a named set of transforms for importing a class of external schemas.

## Success Criteria

- The product is successful when non-JSON data is first-class: buffers (with MIME) and `mlua` functions/userdata validate, and buffers roundtrip through format markers, without first being made `serde_json`-clean.
- The product is successful when `schema → to_json_schema → from_json_schema` yields a semantically equivalent schema, with `$defs`/`$ref`, cycles, and format markers surviving.
- The product is successful when strict mode catches typos and rejects unexpected fields at runtime by default.
- The product is successful when any `T: Serialize` validates directly and any `ZerxValue` serialises to any serde format.
- The product is successful when schemas are built, cloned, sliced (`omit`/`partial`), and extended at runtime.
- The product is successful when schemas hand off to LLMs/tool-use as JSON Schema and their output, including Lua data from `mlua`, validates.
