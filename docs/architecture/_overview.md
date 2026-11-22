# Architecture Overview

This file defines the topology of `docs/architecture/`. Every concern file in this directory MUST be listed below; every entry below MUST point to an existing concern file. Concern-to-concern edges live in the concern files' `Consumes from` / `Provides to` sections, not here.

## Concerns

- [`value-model.md`](value-model.md) — the `ZerxValue` dynamic value spanning serde's full data model.
- [`schema-core.md`](schema-core.md) — the `Schema` representation, modifier composition, the parse flow, and lazy/cyclic schemas.
- [`types.md`](types.md) — the type catalogue (basic, complex, special) and the pluggable validators.
- [`json-schema.md`](json-schema.md) — bidirectional JSON Schema export/import, format markers, and `$defs`/`$ref`.
- [`policy.md`](policy.md) — the composable import pipeline for heterogeneous schema sources.
- [`errors.md`](errors.md) — the structured, machine-readable `ZerxError` model.
- [`lua.md`](lua.md) — the optional `lua` feature: zerx-owned Lua input types and schema-directed validation.

## System Seams

- Serde boundary — owned by `value-model` (any `T: Serialize` in, any serde format out).
- JSON Schema boundary — owned by `json-schema`.
- External `$ref` resolution through the caller's resolver — owned by `policy`.
- Lua data boundary (`lua` feature) — owned by `lua`.
