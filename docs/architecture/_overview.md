# Architecture Overview

This file defines the topology of `docs/architecture/`. Every concern file in this directory MUST be listed below; every entry below MUST point to an existing concern file. Concern files are slim skeletons; each is filled by the plan named in `docs/CONCEPT_zerx_foundation.md`.

## Concerns

- [`value-model.md`](value-model.md) — the `ZerxValue` dynamic value spanning serde's full data model plus the host-opaque layer.
- [`schema-core.md`](schema-core.md) — the `Schema` representation, modifier composition, the parse flow, and lazy/cyclic schemas.
- [`types.md`](types.md) — the type catalogue (basic, complex, special) and the pluggable validators.
- [`json-schema.md`](json-schema.md) — bidirectional JSON Schema export/import, format markers, and `$defs`/`$ref`.
- [`policy.md`](policy.md) — the composable import pipeline for heterogeneous schema sources.
- [`errors.md`](errors.md) — the structured, machine-readable `ZerxError` model.
- [`mlua.md`](mlua.md) — the optional host-opaque feature integrating `mlua` values.

## Key Relationships

- `schema-core` → `value-model`: validation produces a `ZerxValue`.
- `schema-core` → `errors`: validation failures are `ZerxError`.
- `types` → `schema-core`: every type is a `Schema` variant built on the core representation.
- `json-schema` → `schema-core`: export reads, and import reconstructs, `Schema` values.
- `policy` → `json-schema`: policies transform input before, and types after, the import walk.
- `mlua` → `value-model`: the host-opaque variant of `ZerxValue` is realised here.

## System Seams

- Serde boundary — owned by `value-model` (any `T: Serialize` in, any serde format out).
- JSON Schema boundary — owned by `json-schema`.
- Host boundary (`mlua` feature) — owned by `mlua`.
