# PLAN_J6: String validators next to the `uri`/`url` markers are enforced

## Context & Goal

Commissioned by the external supervisor (DevLead of zerx, on the Product Owner's mandate) as patch release
2.0.5. Janus found the bug on re-pin, and the supervisor confirmed it against db1c75f.

`{"type":"string","minLength":1,"format":"uri"}` is refused at import with `IMPORT_UNSUPPORTED`
("`minLength` cannot be enforced on this node"). The same happens with `maxLength` or `pattern`, and with
`format:"url"`. zerx's own `string().min(1).format("uri")` exports exactly this shape, so zerx's export
fails to re-import. Real schemas in daphnis and pythia break on it. Without `format`, or with a format
that is not a marker (`email`, `date-time`, …), the same schema imports.

**Cause.** Step 9 of `import_value` in `src/json_schema.rs` builds `uri()`/`url()` for
`type:"string"` + `format:"uri"`/`"url"`, but it consumes only `type`. The assertion coverage check
added by PLAN_J5 (`D-import-fail-closed`) therefore refuses the unconsumed string keywords. Before
PLAN_J5 they were silently dropped.

**Goal.**
- `minLength`, `maxLength` and `pattern` next to the `uri`/`url` markers on a string node are enforced
  together with the marker's own URI/URL check. Neither is dropped.
- Assertions a marker cannot enforce stay refused.
- The export → import roundtrip of string validators combined with a marker-colliding `format` value is
  pinned.

### The other string-typed markers, under the same rule

The rule: a marker consumes a keyword only if it enforces it.
- **`buffer`** (`type:"string"` + `format:"buffer"`) holds `ZerxValue::Bytes`. The string validators
  skip non-string values (`MinLength`/`MaxLength`/the pattern validator in `src/types.rs` return
  `Ok` for anything but `ZerxValue::String`). Attached to a buffer they would be consumed without
  being enforced, so `buffer` keeps refusing `minLength`/`maxLength`/`pattern`. Consequence:
  `string().min(1).format("buffer")` exports a document that re-imports as a refusal. This is pinned
  as a documented limitation; the alternatives below go to the supervisor.
- **`json`**: with any `type`, the marker is already refused by PLAN_J4, because `json()` accepts
  every value. So `string().format("json")`, with or without validators, re-imports as a refusal.
  Unchanged, and documented the same way.
- **`record` and `jsonschema`** match only `type:"object"`. A string node with
  `format:"record"`/`"jsonschema"` takes the plain string dispatch, which enforces its string
  validators. Only the `format` annotation is dropped (`FORMAT_SKIP`). This plan does not change it.

Possible fixes for the buffer/json collision, outside this plan and reported to the supervisor:
1. On export, do not emit a user `format` modifier whose value is a marker the node's kind is not.
2. On import, read a marker node that carries assertions the marker cannot enforce as a plain string
   with a `format` annotation. That is JSON Schema's own reading, and exact.

Both change a documented contract, so they are left for a separate decision.

### Out of scope

Untyped nodes with trivially-true subschemas, e.g. `{"properties":{"m":{}}}`, stay refused, as the
supervisor directed. A backlog card records them as a possible exact refinement.

## Breaking Changes

No refusals are added and no export output changes.
- **Newly accepted at import:** `type:"string"` (or `["string","null"]`) + `format:"uri"`/`"url"` with
  `minLength`, `maxLength` and/or `pattern`. Since PLAN_J5 this was `IMPORT_UNSUPPORTED`; before that
  the keywords were dropped.
- **Validation:** such a schema rejects values that fail the marker check (`invalid_uri` /
  `invalid_url`) and values that fail a string validator (`string_too_short`, `string_too_long`,
  `pattern_mismatch`; `pattern_invalid` for a non-compiling pattern, as on `string()`).
- **Re-import narrowing, unchanged in kind:** `string().format("uri")` re-imports as `uri()`, so the
  re-imported schema checks URI structure, which the annotation-only original did not. The supervisor
  accepts this ("do not stop checking the URI"). The marker reading predates PLAN_J4.

## Reference Patterns

- `src/json_schema.rs`: Step 9 marker branch and the Step 10 `TypeName::String` arm in `import_value`.
  The string arm shows the keyword parsing, the `as_non_negative_u64` grammar check and the `consumed`
  bookkeeping to reuse. Also the tests from PLAN_J5 for markers (search `format":"uri"`).
- `src/types.rs`: `StringSchema::min`/`max`/`regex`; `MinLength`, `MaxLength` and the regex validator;
  `check_uri`, `check_url`.

## Assumptions & Risks

- **Parse order.** `check_type` runs the URI/URL structural check first, then the validators
  (`docs/architecture/schema-core.md` parse order). A value that is both too long and not a URI reports
  `invalid_uri`/`invalid_url`. This is the same type-check-first precedence as everywhere else.
- **Export.** A `Uri`/`Url` schema carrying these validators exports base `{"type":"string","format":"uri"}`
  plus the validator fragments (`minLength`, `maxLength`, `pattern`), the same shape the import reads.
  `base_schema` and the validator fragments need no change.
- **Builder surface unchanged.** `UriSchema`/`UrlSchema` keep exposing no string validator methods (the
  `api` gate's `compile_fail` doctests stay valid). Only import attaches the validators, through the
  crate-internal validator storage, the same way `OneOf` is import-only.

## Steps

### 1. `src/json_schema.rs`: `uri`/`url` consume the string validators

- Factor the `minLength`/`maxLength`/`pattern` handling of the Step 10 string arm into one private
  helper.
  - Same grammar checks and `IMPORT_MALFORMED` messages.
  - Same validator order: `minLength`, `maxLength`, `pattern`.
  - It inserts each present keyword into `consumed`.
  - It produces the validators exactly as `string().min(n)`, `.max(n)` and `.regex(pat)` create them.
- The string arm uses the helper. Its behaviour is unchanged.
- The `(TypeName::String, Some("uri"))` and `(TypeName::String, Some("url"))` arms of Step 9:
  - build `uri()`/`url()`;
  - apply the helper;
  - append the resulting validators to the marker schema's validator storage, in that order.
  The `[T,"null"]` nullable handling stays as PLAN_J4 left it: `.nullable()` is applied after the base.
- The `buffer` arm is unchanged: it consumes only `type`, so the string keywords stay refused by the
  coverage check. Add a short comment at the arm. The string validators skip `Bytes`, so consuming them
  here would not enforce them.

### 2. Tests (`src/json_schema.rs`, in-module)

Boundary: unit. Perspectives: conformance, failure mode, round-trip. Assert codes, not messages.

The PLAN_J5 test `import_format_marker_siblings_refused` pins `{"type":"string","format":"uri","minLength":3}`
as `import_unsupported`. This plan deliberately flips that case, so remove it from that test; the new
tests below cover it. The test's `record`, `jsonschema` and other cases stay as they are.
- **The reported shape.** `{"type":"string","minLength":1,"format":"uri"}` imports.
  - Accepts `"a:b"`.
  - Rejects `"nope"` with `invalid_uri`.
  - Rejects `5` with `type_mismatch`.
- **Validator enforcement with `uri`.** `{"type":"string","maxLength":5,"format":"uri"}`:
  - accepts `"a:b"`;
  - rejects `"https://example.com"` with `string_too_long`;
  - rejects `"abc"` with `invalid_uri`.
- **`pattern` with `uri`.** `{"type":"string","pattern":"^https:","format":"uri"}`:
  - accepts `"https://x"`;
  - rejects `"http://x"` with `pattern_mismatch`.
- **`url` with all three.** `{"type":"string","minLength":12,"maxLength":40,"pattern":"\\.org/","format":"url"}`:
  - accepts `"https://example.org/"`;
  - rejects `"https://example.com/"` with `pattern_mismatch`;
  - rejects `"ftp://example.org/"` with `invalid_url`;
  - rejects a too-short and a too-long URL with the length codes.
- **Nullable.** `{"type":["string","null"],"maxLength":10,"format":"uri"}`:
  - accepts `null` and `"a:b"`;
  - rejects `"a:bcdefghijkl"` with `string_too_long`.
- **Malformed.** `{"type":"string","minLength":-1,"format":"uri"}` → `import_malformed`.
- **Roundtrip (supervisor pin).** For `string().min(1).max(50).regex("^h").format("uri")` and the same with
  `format("url")`:
  - `to_json_schema` → `from_json_schema` → `to_json_schema` yields an identical document;
  - the re-imported schema enforces each validator and the marker check: one accepting value, then one
    rejection per validator code and the marker code.
- **Roundtrip of the plain markers.** `uri()` and `url()` built programmatically export → import → export
  unchanged and carry no validators.
- **`buffer` stays refused.**
  - `{"type":"string","format":"buffer","minLength":1}` → `import_unsupported`; likewise `maxLength`
    and `pattern`.
  - The export of `string().min(1).format("buffer")` re-imports as `import_unsupported` (pinned
    limitation).
  - `buffer()` with `mime` still round-trips.
- **Other assertions next to `uri`/`url` behave as before.** `{"type":"string","format":"uri","minimum":1}` is
  vacuous for a string and imports. `{"type":"string","format":"uri","enum":["a:b"]}` takes the `enum`
  branch as before.
  `{"$ref":"#/$defs/U","minLength":1}` with `U` = `{"type":"string","format":"uri"}` → `import_unsupported`
  (unchanged `$ref` rule).

## Documentation

At finalization, after verification. Basis for each constraint: `D-import-fail-closed` (an assertion is
enforced, vacuous or refused) and the product goal of a semantically equivalent export → import roundtrip
(`docs/definition.md`). The `uri`/`url` validator consumption restores the roundtrip, and the buffer
refusal follows from the decision.

- `docs/architecture/json-schema.md`, **Format-marker kind mapping:**
  - `type:"string"` + `format:"uri"`/`"url"` MUST consume `minLength`, `maxLength` and `pattern` as string
    validators enforced together with the marker check.
  - The `buffer` marker MUST NOT consume them, because string validators do not apply to bytes.
  - A string node whose `format` modifier names a marker behaves as follows on re-import:
    - `uri`/`url` re-import as that marker, string validators included;
    - `buffer` re-imports as `buffer()`, and is refused when string validators are present;
    - `json` is refused, because a `type` is present;
    - `record`/`jsonschema` do not match on a string node: the node takes the plain string dispatch,
      and the `format` annotation is dropped (unchanged).

    The refusals and the dropped annotation are an accepted limitation.
- `docs/architecture/types.md`, special types: an imported `uri()`/`url()` MAY carry string-length and pattern
  validators attached by JSON Schema import; the builders still expose no validator methods.
- `docs/backlog.kanban.md`, two cards:
  - vacuity of untyped nodes whose only assertions are trivially-true subschemas (e.g.
    `{"properties":{"m":{}}}`), as a possible exact refinement of `D-import-fail-closed`;
  - marker-name `format` collisions on re-import (a user `format` naming a marker the node's kind is not:
    `buffer`/`json` refusals and the dropped `record`/`jsonschema` annotation), with the two fix options
    above.

No register entry is proposed or superseded. `docs/tests.md` is unchanged.

**Version:** patch, 2.0.4 → 2.0.5 in `Cargo.toml` and `Cargo.lock`, applied by the Planner at finalization.

## Verification

Required gate: `scripts/verify all` (lint, `test`, `test core`, `api`) passes on the final functional state.

Acceptance checks, run and reported by the Coder:
- `git diff --stat` touches only `src/json_schema.rs`.
- The supervisor's fixture at `/tmp/zerx-lead-check`, if still present, gives the PLAN_J4 results (run with
  `CARGO_TARGET_DIR` outside the repo). If it is gone, report that; it is not a code failure.
