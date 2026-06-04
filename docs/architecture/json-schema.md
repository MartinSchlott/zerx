# Concern: JSON Schema

> Status: skeleton. Constraints and contracts are filled by `PLAN_J1_export`
> and `PLAN_J2_import`.

## Purpose

- This concern owns bidirectional JSON Schema (Draft 2020-12): export from `Schema` and import back to `Schema`.
- This concern owns the format markers (`buffer`, `record`, `json`, `jsonschema`, `function`, `tvalue`) that carry non-JSON types through pure-JSON tools.
- This concern owns `$defs`/`$ref` tracking for shared/recursive subschemas and the memoised lazy placeholders that let cycles survive the roundtrip.

## Non-Goals

- This concern does NOT own the import policy pipeline for external sources — see `policy`.
- This concern does NOT define the types it serialises — see `types`.

## Related Decisions

- (none yet — roundtrip behaviour is settled within the JSON Schema plans)
