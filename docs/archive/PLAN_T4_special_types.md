# PLAN_T4_special_types

## Context & Goal

Fourth and final plan of **Phase 2 — Type catalogue** in the
`CONCEPT_zerx_foundation` execution. Phase 1 (`PLAN_F3_error_model`,
`PLAN_F1_value_model`, `PLAN_F2_schema_core`) and the first three Phase-2 plans
(`PLAN_T1_basic_types`, `PLAN_T2_complex_types`, `PLAN_T3_object_utilities`) are
complete and archived. The `schema-core` spine is in place: the erased `Schema`
carrier, the `SchemaKind` container with its per-kind dispatch seam
(`check_type` / `parse_inner`), the blanket `Modify` trait (which already
provides `mime_format`, writing `modifiers.mime`), the parse flow, the
`Validator` storage contract, and the F1 `ZerxValue::Bytes(Vec<u8>)` variant
(populated only via serde's native `serialize_bytes`, per C2).

T4 delivers the **special types** from the vision catalogue
(`docs/vision.md:120`):

- `buffer` (with MIME) — accepts only the first-class `ZerxValue::Bytes` variant;
  carries an optional MIME type for the export marker. **No coercion** of number
  arrays into buffers (C2).
- `uri` — a string carrying a structural URI check (scheme + path).
- `url` — a string carrying a structural HTTP/HTTPS URL check (scheme, hostname,
  optional port).
- `json` — accepts any serde-bridgeable `ZerxValue`; identity parse; a distinct
  marker for export.
- `jsonschema` — accepts any serde-bridgeable `ZerxValue`; identity parse; a
  distinct marker for export.

T4 follows the established T1/T2 pattern exactly: each type is **one new
`SchemaKind` variant** fronted by **one typed builder**, with **one delegating
arm per variant** in `check_type`, `parse_inner`, and the `Debug` match —
introducing **no new parse-flow control logic** in `schema-core` (C1 /
`schema-core.md` constraints). All five new variants are leaves (no children, no
recursion): their `parse_inner` arm is the identity (`Ok(value.clone())`), like
the T1 scalars. T4 fills the `types` concern's special-type Constraints at Doc
Update.

T4 closes Phase 2: after it, the full vision type catalogue
(`docs/vision.md:116-121`, excluding the feature-gated host-opaque
`function`/`tvalue` which belong to `PLAN_M1_host_opaque`) is implemented.

### Boundary (`docs/architecture/types.md`, `docs/architecture/json-schema.md`)

- `types` owns the type catalogue. T4 adds five special-type variants, their
  typed builders, their per-kind check delegates, and the `buffer` MIME method
  plus the URI/URL structural validators.
- `types` does **NOT** define the core `Schema` representation, the parse flow,
  the depth/cycle guard, or the `Validator` **trait** — those are `schema-core`
  (F2). T4 only adds variants + delegating dispatch arms.
- `types` does **NOT** own JSON Schema export/import. The format markers
  (`buffer`, `json`, `jsonschema`, …) are owned by `json-schema` and emitted by
  `PLAN_J1_export` by reading the `SchemaKind` variant and `modifiers.mime`. **T4
  implements no `to_json_schema` and writes no `modifiers.format`.** The MIME
  string is stored in the existing `modifiers.mime` carrier (written by the
  `buffer` MIME method) for J1 to read; T4 adds no export code.
- `types` does **NOT** own the host-opaque `function`/`tvalue` types — see
  `mlua` / `PLAN_M1_host_opaque`. T4's `json`/`jsonschema` checks reject the
  feature-gated `HostOpaque` variant (C5: `json` is the serde-bridgeable layer),
  but do not realise it.

### Governing candidate decisions (`CONCEPT_zerx_foundation.md`)

- **C1 — hybrid enum-core + typed builders.** Each special type is one
  `SchemaKind` variant fronted by one typed builder. The builders expose the
  blanket `Modify` modifiers via `BuilderInner`; only `buffer` adds a
  type-specific inherent method (`mime`). The negative compile-guarantee holds:
  `uri`/`url`/`json`/`jsonschema`/`buffer` builders expose **no** string/number
  validator methods (e.g. `.min()` is unrepresentable on them).
- **C2 — buffer fidelity via serde bytes, no coercion.** `buffer()` accepts
  **only** `ZerxValue::Bytes`. It MUST NOT accept a number array (`ZerxValue::
  Array` of integers) or any `{ "type": "Buffer", "data": [...] }` object shape —
  silent coercion is explicitly rejected. The MIME type rides on
  `modifiers.mime`; export (`format: "buffer"` + `contentMediaType`) is J1's job.
- **C5 — `ZerxValue` spans the full serde data model.** `json()` and
  `jsonschema()` accept any value on the **serde-bridgeable** layer; under the
  `mlua` feature they reject the host-opaque layer (`TYPE_MISMATCH`).
- **C7 — single `Result`-returning API.** All constructors are infallible and
  return their builder; all validation failures surface as `Err(ZerxError)` at
  `validate` time. No throwing variant.
- **C8 — English-only, machine-readable errors.** Each new failure mode carries
  a distinct `code`; `expected`/`received` are descriptor strings (a type tag or
  a format name), never raw `ZerxValue`s.
- **C9 — built on serde, lean beyond it.** T4 adds **no new dependency**. The
  `url` validator reuses the **already-present** `regex` crate (a core
  dependency per C9, currently used only by the `Pattern` validator); the `uri`
  validator is hand-rolled (no regex), mirroring the hand-rolled `email`/`uuid`
  validators from T1.

### Architect rulings to encode

None pending. T4 introduces no new dependency (no entry under Hard Rule 11) and
no new `SchemaKind` payload type beyond five unit variants.

## Breaking Changes

**No.** T4 is additive: five new `SchemaKind` unit variants plus one delegating
arm per variant in `check_type`, `parse_inner`, and the `Debug` match in
`src/schema.rs`; new builder newtypes, check delegates, validators, constructors,
and one new `impl ErrorCode { … }` block in `src/types.rs`; and new re-exports
from `src/lib.rs`. It changes no existing public signature and does not edit
`src/error.rs` or `src/value.rs`.

## Dependencies

**None added.** No new crate. `Cargo.toml` is untouched. The `url` validator
uses the existing `regex` dependency.

## Reference Patterns

- `src/types.rs` (T1/T2/T3) — the authoritative pattern for everything T4 does:
  - the leaf type-check delegates `check_string`/`check_null` (kind match →
    `TYPE_MISMATCH` with `.expected(...).received(type_tag(value))`) — the model
    for `check_buffer`/`check_uri`/`check_url`/`check_json`/`check_jsonschema`,
  - the hand-rolled `is_valid_email` / `is_valid_uuid` structural checks (no
    regex) — the model for `is_valid_uri`,
  - the `Pattern` validator (`src/types.rs:194-203`) only as the precedent for
    **using the `regex` crate** — **not** as a lifecycle model. `Pattern::new`
    compiles **eagerly at builder construction** and stores the compiled regex in
    the validator instance; that path does not apply to `url`, whose variant is a
    unit `SchemaKind::Url` with nowhere to store a compiled regex. T4 instead
    compiles the `url` regex **once, lazily, into a module-level
    `std::sync::OnceLock<regex::Regex>`** (the *Structural validators* section is
    authoritative for the `url` helper shape). This is a deliberate divergence
    from `Pattern`'s per-instance eager compilation; it does not touch
    `Pattern` or its `validate`-time-deferral contract
    (`docs/architecture/types.md:61`),
  - the builder newtype + `BuilderInner` impl (grants blanket `Modify`) +
    `From<Builder> for Schema` + constructor, repeated per type,
  - the `BooleanSchema` `compile_fail` doctest proving the negative
    compile-guarantee — the model for the special-builder doctests,
  - the open `impl ErrorCode { … }` catalogue block in `src/types.rs` (**no edit
    to `src/error.rs`**),
  - the `type_tag` helper (reused for `received` and for `check_buffer`'s
    expected/received descriptors),
  - the `#[cfg(test)] mod tests` layout and the `val(builder, &value)` helper.
- `src/schema.rs` (F2) — the dispatch seam T4 extends:
  - `SchemaKind::check_type` / `parse_inner` as a single `match self` with one
    arm per kind (T4 adds five arms to each, plus the `Debug` match); the leaf
    `parse_inner` group (`Any | String | Number | … => Ok(value.clone())`) that
    T4's five leaves join,
  - the `Modifiers` carrier (F2) — `modifiers.mime: Option<String>` already
    exists and is written by the blanket `mime_format`; `buffer().mime(...)`
    writes the same field.
- `src/value.rs` (F1) — `ZerxValue::Bytes(Vec<u8>)` (the only value `buffer`
  accepts); `type_tag` already returns `"bytes"` for it; the feature-gated
  `ZerxValue::HostOpaque` placeholder (uninhabited in T4).
- **Zex semantics** (`/Users/martinschlott/Documents/MyProjects/zex`):
  - `src/zex/basic-types.ts` `ZexBuffer` (lines 377-435) — binary-only, MIME on
    the marker, export `type: object, format: buffer, contentMediaType`. zerx
    diverges by accepting **only** native bytes (C2) — no `{type:Buffer}` /
    ArrayBuffer / number-array coercion.
  - `src/zex/special-types.ts` `ZexUri` (lines 9-50) — URI pattern
    `^[a-zA-Z][a-zA-Z0-9+.-]*:.*$` plus non-empty scheme and non-empty content
    after the colon; marker `format: uri`.
  - `src/zex/special-types.ts` `ZexUrl` (lines 53-118) — the HTTP/HTTPS URL regex
    plus hostname rules (no consecutive dots, no leading/trailing dot, contains a
    dot or is `localhost`) and port range 1–65535; marker `format: uri-reference`.
  - `src/zex/basic-types.ts` `ZexJson` (lines 481-490) and
    `src/zex/special-types.ts` `ZexJsonSchema` (lines 121-132) — both inherit the
    default accept-anything `validateType` (`zex-base.ts:774`); markers
    `format: json` and `type: object, format: jsonschema` respectively. The
    `transformLua` overrides are mlua-only and out of T4 scope.

## Assumptions & Risks

- **All five variants are leaves.** None carries a child schema, so none recurses
  and none touches `ParseContext` beyond the depth frame that `parse_present`
  already manages. Their `parse_inner` arm is the identity `Ok(value.clone())`,
  joining the existing T1-leaf group in `parse_inner`. No new parse-flow control
  logic enters `schema-core` (C1).
- **`buffer` is bytes-only by design (C2).** It accepts only `ZerxValue::Bytes`.
  A number array, a `{type:"Buffer", data:[...]}` object, or any other shape is a
  `TYPE_MISMATCH`. This is a deliberate divergence from Zex's permissive buffer
  acceptance, mandated by C2's no-coercion stance — silently coercing would mask
  data-shape bugs. The caller-side contract (emit via `serialize_bytes`, e.g.
  `serde_bytes`) is the F1/C2 documented contract. **Flagged for Reviewer.**
- **MIME storage reuses `modifiers.mime`.** `buffer().mime("image/png")` writes
  `modifiers.mime` — the same field the blanket `mime_format` writes. No new
  storage; J1 reads it to emit `contentMediaType`. `buffer().mime(...)` and
  `buffer().mime_format(...)` are therefore equivalent in effect; `mime` is a
  buffer-specific convenience matching the vision snippet
  (`docs/vision.md:186`). **Flagged for Reviewer** (intentional aliasing).
- **`uri` / `url` carry their structural check in the type-check step**, like
  `enum` carries its value-set check in `check_enum`. A non-string value yields
  `TYPE_MISMATCH` (expected `"uri"` / `"url"`); a string that fails the format
  check yields `INVALID_URI` / `INVALID_URL`. They expose **no** inherent string
  validators (no `.min()`/`.regex()`), honouring the negative compile-guarantee;
  chaining string validators onto a URI is out of scope (YAGNI; the vision uses
  `uri()`/`url()` bare). **Flagged for Reviewer.**
- **`url` uses the `regex` crate, compiled once.** The Zex URL regex
  (`special-types.ts:83`) ports directly to Rust `regex` (non-capturing groups,
  `\w`, case-insensitive flag; no backreferences) and is compiled once into a
  `std::sync::OnceLock<regex::Regex>`, not per `validate`. The hostname and port
  rules are applied in code after the regex match, exactly as Zex does. This
  broadens the `regex` dependency's role from "the `Pattern` validator only" to
  "`Pattern` and `url`" — a Doc-Update note on `types.md`'s ASD section, not a
  new dependency. **Flagged for Reviewer.** zerx's `url` enforces Zex's
  scheme/host/port structure faithfully; the exact path/query/fragment character
  classes ride on the same ported regex.
- **`uri` is hand-rolled (no regex).** Its check is a simple scheme/colon/path
  inspection mirroring `is_valid_email`/`is_valid_uuid` — cheaper and dependency-
  free for a pattern this simple.
- **`json` and `jsonschema` are serde-bridgeable `any` with distinct markers.**
  Both accept every serde-bridgeable `ZerxValue` (identity parse) and differ only
  in their `SchemaKind` variant — which J1 maps to `format: "json"` vs
  `format: "jsonschema"`. zerx does **not** structurally validate that a
  `jsonschema()` value is a well-formed JSON Schema document (v1 simplification,
  YAGNI; matches Zex, whose `ZexJsonSchema` also accepts anything). Under the
  `mlua` feature both reject the host-opaque layer (C5). **Flagged for Reviewer.**
- **Host-opaque rejection is forward-correct but currently vacuous.** In T4 the
  `mlua`-gated `ZerxValue::HostOpaque` is uninhabited, so the rejection arm in
  `check_json`/`check_jsonschema` cannot fire; it encodes the C5 contract for
  when `PLAN_M1_host_opaque` realises the variant. Every other check delegate's
  `_` arm already treats host-opaque as a mismatch; `json`/`jsonschema` need the
  explicit arm precisely because their default is accept-all.

## Design

### Module layout

- `src/schema.rs`: two changes only.
  - **(a)** Extend `SchemaKind` with five unit variants (`Buffer`, `Uri`, `Url`,
    `Json`, `JsonSchema`) and add one delegating arm per variant in `check_type`,
    `parse_inner` (joining the leaf identity group), and the `Debug` match.
  - **(b)** No other change. `parse_present`, `parse_field`, `Schema::new`, the
    `Schema` fields, `BuilderInner`, and the blanket `Modify` are reused
    unchanged.
- `src/types.rs`: append
  - the T4 `impl ErrorCode { … }` catalogue block (`INVALID_URI`, `INVALID_URL`),
  - the check delegates `check_buffer`, `check_uri`, `check_url`, `check_json`,
    `check_jsonschema`,
  - the structural helpers `is_valid_uri` (hand-rolled) and `url_regex`
    (`OnceLock`-cached) + the hostname/port post-checks,
  - the five builder newtypes (`BufferSchema`, `UriSchema`, `UrlSchema`,
    `JsonSchema`, `JsonschemaSchema`) with `BuilderInner`, `From<…> for Schema`,
    and (for `BufferSchema` only) the inherent `mime` method,
  - the five constructors (`buffer`, `uri`, `url`, `json`, `jsonschema`),
  - new `#[cfg(test)] mod tests` cases (appended).
- `src/lib.rs`: extend the `pub use types::{…}` lines with the new constructors
  and builder types: `buffer, uri, url, json, jsonschema, BufferSchema,
  UriSchema, UrlSchema, JsonSchema, JsonschemaSchema`.

### Builder naming

Builders follow the established `<ctor_pascal>Schema` convention (T1/T2:
`string`→`StringSchema`, `discriminated_union`→`DiscriminatedUnionSchema`):
`buffer`→`BufferSchema`, `uri`→`UriSchema`, `url`→`UrlSchema`, `json`→
`JsonSchema`, `jsonschema`→`JsonschemaSchema`. The single-token `jsonschema`
yields the mechanical `JsonschemaSchema` (the doubled "Schema" is the convention
applied verbatim). The json-schema **concern** exposes its public surface as
functions (`to_json_schema`/`from_json_schema`), not a type named `JsonSchema`,
so the `JsonSchema` builder for `json()` introduces no symbol clash.

### `SchemaKind` extension (in `src/schema.rs`)

Add five unit variants (T4-owned, living syntactically in the container per C1):

```rust
pub(crate) enum SchemaKind {
    // … F2/T1/T2 variants …
    Buffer,                 // T4
    Uri,                    // T4
    Url,                    // T4
    Json,                   // T4
    JsonSchema,             // T4
}
```

`SchemaKind` stays `Clone` (all five are unit variants).

Dispatch arms:

- `check_type` (kind/format check; `_ctx` unused):
  - `Buffer => crate::types::check_buffer(value)`
  - `Uri => crate::types::check_uri(value)`
  - `Url => crate::types::check_url(value)`
  - `Json => crate::types::check_json(value)`
  - `JsonSchema => crate::types::check_jsonschema(value)`
- `parse_inner` (identity — add the five tags to the existing leaf group):
  - `… | SchemaKind::Buffer | SchemaKind::Uri | SchemaKind::Url
    | SchemaKind::Json | SchemaKind::JsonSchema => Ok(value.clone())`
- `Debug` match: render `"Buffer"`, `"Uri"`, `"Url"`, `"Json"`, `"JsonSchema"`.

### Type-check delegates (`check_*` in `src/types.rs`)

- **`check_buffer(value)`** — accept `ZerxValue::Bytes`; any other kind →
  `TYPE_MISMATCH` with `.expected("buffer").received(type_tag(value))`. No
  coercion (C2).
- **`check_uri(value)`** — accept `ZerxValue::String(s)` iff `is_valid_uri(s)`;
  a valid-form string → `Ok(())`; a string failing the form →
  `Err(INVALID_URI)` with `.expected("uri")`; a non-string →
  `TYPE_MISMATCH` with `.expected("uri").received(type_tag(value))`.
- **`check_url(value)`** — accept `ZerxValue::String(s)` iff `is_valid_url(s)`;
  failing string → `Err(INVALID_URL)` with `.expected("url")`; non-string →
  `TYPE_MISMATCH` with `.expected("url").received(type_tag(value))`.
- **`check_json(value)`** — accept every serde-bridgeable variant; under the
  `mlua` feature reject host-opaque:
  ```rust
  pub(crate) fn check_json(value: &ZerxValue) -> Result<(), ZerxError> {
      match value {
          #[cfg(feature = "mlua")]
          ZerxValue::HostOpaque(_) => Err(ZerxError::new(ErrorCode::TYPE_MISMATCH, "expected json")
              .expected("json")
              .received(type_tag(value))),
          _ => Ok(()),
      }
  }
  ```
- **`check_jsonschema(value)`** — identical body to `check_json` but with the
  `"jsonschema"` expected descriptor.

### Structural validators

```rust
fn is_valid_uri(s: &str) -> bool {
    // scheme ':' path  — scheme: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ); path non-empty
}
```

Rules (port of `ZexUri`, `special-types.ts:37-48`):
- a `':'` MUST exist; `scheme` = bytes before it, `rest` = bytes after it,
- `scheme` non-empty; first char `ascii_alphabetic`; every scheme char is
  `ascii_alphanumeric` or one of `+ - .`,
- `rest` non-empty.

```rust
fn url_regex() -> &'static regex::Regex { /* OnceLock-cached */ }
fn is_valid_url(s: &str) -> bool { /* regex match + hostname/port post-checks */ }
```

`is_valid_url` rules (port of `ZexUrl`, `special-types.ts:83-116`):
- the cached regex (ported verbatim, case-insensitive) MUST match the whole
  string,
- extract the hostname (between the `http(s)://` scheme and the first `/`,
  whitespace, or `:`); hostname non-empty,
- hostname MUST NOT contain `".."`, MUST NOT start or end with `'.'`,
- hostname MUST contain a `'.'` OR equal `"localhost"`,
- if a port is present (`:` + digits before the path), it MUST parse to
  `1..=65535`.

The ported regex source (Rust `regex`, leading `(?i)` for case-insensitivity):

```text
(?i)^https?://(?:[-\w.])+(?::[0-9]+)?(?:/(?:[\w/_.-])*)?(?:\?(?:[\w&=%.~!$'()*+,;:@/-])*)?(?:#(?:[\w.~!$'()*+,;:@/-])*)?$
```

### Builders, methods, constructors

Per builder (mirrors the T1/T2 newtype pattern; relies on the `pub(crate)
BuilderInner` trait for the blanket `Modify`):

```rust
#[derive(Clone)] pub struct BufferSchema(Schema);
impl BuilderInner for BufferSchema { fn schema_mut(&mut self) -> &mut Schema { &mut self.0 } }
impl From<BufferSchema> for Schema { fn from(b: BufferSchema) -> Schema { b.0 } }
```

Same for `UriSchema`, `UrlSchema`, `JsonSchema`, `JsonschemaSchema`.

Inherent methods (T4):

- `BufferSchema::mime(self, mime_type: impl Into<String>) -> Self` — set
  `self.0.modifiers.mime = Some(mime_type.into())`, return `Self`.
- `UriSchema`, `UrlSchema`, `JsonSchema`, `JsonschemaSchema`: **no** inherent
  methods (modifiers only, via blanket `Modify`).

Each builder carries a `compile_fail` doctest proving the negative
compile-guarantee, e.g. on `UriSchema`:

```rust
/// ```compile_fail
/// use zerx::uri;
/// let _ = uri().min(3);   // ERROR: no method `min` on UriSchema
/// ```
```

Constructors (all infallible, C7):

- `buffer() -> BufferSchema` — `BufferSchema(Schema::new(SchemaKind::Buffer))`.
- `uri() -> UriSchema` — `UriSchema(Schema::new(SchemaKind::Uri))`.
- `url() -> UrlSchema` — `UrlSchema(Schema::new(SchemaKind::Url))`.
- `json() -> JsonSchema` — `JsonSchema(Schema::new(SchemaKind::Json))`.
- `jsonschema() -> JsonschemaSchema` —
  `JsonschemaSchema(Schema::new(SchemaKind::JsonSchema))`.

### Error-code catalogue (open, in `src/types.rs`, no edit to `error.rs`)

```rust
impl ErrorCode {
    pub const INVALID_URI: ErrorCode = ErrorCode::new("invalid_uri");
    pub const INVALID_URL: ErrorCode = ErrorCode::new("invalid_url");
}
```

`TYPE_MISMATCH` (seeded in `errors`) is reused for `buffer`, `json`,
`jsonschema`, and the non-string cases of `uri`/`url`. T4 adds **no** edit to
`src/error.rs`.

## Steps

1. `src/schema.rs`: add the five `SchemaKind` unit variants (`Buffer`, `Uri`,
   `Url`, `Json`, `JsonSchema`) and one delegating arm per variant in
   `check_type`, `parse_inner` (extend the leaf identity group), and the `Debug`
   match. No other change.
2. `src/types.rs`: add the T4 `impl ErrorCode { … }` block (`INVALID_URI`,
   `INVALID_URL`).
3. `src/types.rs`: implement `is_valid_uri`, `url_regex` (`OnceLock`-cached), and
   `is_valid_url` (regex match + hostname/port post-checks).
4. `src/types.rs`: implement the five check delegates (`check_buffer`,
   `check_uri`, `check_url`, `check_json`, `check_jsonschema`).
5. `src/types.rs`: implement the five builder newtypes (`BuilderInner`,
   `From<…> for Schema`, the `BufferSchema::mime` method, and the `compile_fail`
   doctests) and the five constructors (`buffer`, `uri`, `url`, `json`,
   `jsonschema`).
6. `src/lib.rs`: extend the `pub use types::{…}` lines with the new constructors
   and builder types.
7. Add the `#[cfg(test)] mod tests` cases per Verification (appended).
8. Run the full build/lint/test matrix below; resolve every warning.

**Doc Update (workflow Step 6, by the Coder after Validation — not now):** fill
`docs/architecture/types.md` with the review-hardened special-type contract:

- the five special-type variants and the one-arm-per-dispatch-method rule (no new
  shared parse-flow control logic; all five are leaves with identity
  `parse_inner`);
- `buffer`: accepts **only** `ZerxValue::Bytes` (`TYPE_MISMATCH` otherwise); **no
  coercion** of number arrays or object shapes (C2); the `mime` method writes
  `modifiers.mime` (read by J1 for `contentMediaType`);
- `uri`: non-string → `TYPE_MISMATCH` (expected `"uri"`); malformed string →
  `INVALID_URI`; the scheme/colon/path rule;
- `url`: non-string → `TYPE_MISMATCH` (expected `"url"`); malformed string →
  `INVALID_URL`; the scheme/hostname/port rules; the `regex`-backed match;
- `json` / `jsonschema`: accept any serde-bridgeable value (identity parse);
  reject host-opaque under `mlua` (C5); distinct variants for distinct export
  markers; no structural JSON-Schema-document validation (v1);
- the negative compile-guarantee for all five builders (no string/number
  validator methods);
- the T4 error codes block (`INVALID_URI`, `INVALID_URL`);
- broaden the **Architecturally Significant Dependencies** `regex` entry to note
  that `url` (in addition to the `Pattern`/`regex` validator) compiles and
  matches a regular expression.

The same Doc Update MUST update `types.md` **Related Decisions** to add the
candidate T4 substantiates — **C2** (buffer no-coercion) — alongside the existing
C1/C4/C7/C8/C9 notes, keeping it a **non-slug candidate reference** (e.g.
"candidate C2") per the concept's Decision-Reference Discipline; **no `D-<slug>`
reference** is added (migration is at Concept Closeout). The
`types ↔ schema-core` `Consumes from`/`Provides to` edges already exist on both
sides (`Schema`, `Validator`); the `types → json-schema` validator-fragment edge
already exists and T4 adds no new export edge (the variant→marker mapping is J1
reading `Schema` over the existing `json-schema → schema-core` edge). **No new
cross-concern edge** is opened by this plan, so no dual-endpoint mirroring is
required.

## Verification

Build & lint — the clippy invocations are self-enforcing (`-D warnings` fails
the command on any warning, so "warning-free" is verified, not assumed). The
`mlua` runs in T4 guard **compile-time feature compatibility only**: because
`ZerxValue::HostOpaque` is still an uninhabited placeholder
(`src/value.rs:83-90`), they prove the crate compiles under the feature flag and
that the new `match` arms stay exhaustive with the gated variant present — they
**cannot** exercise the `check_json`/`check_jsonschema` host-opaque-rejection arm
at runtime, since no host-opaque value can be constructed yet. That rejection arm
is **forward-correct but untestable until `PLAN_M1_host_opaque`** inhabits the
variant; T4 asserts no runtime host-opaque behaviour.

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --all-targets --features mlua -- -D warnings`

Unit tests (`#[cfg(test)] mod tests` in `src/types.rs`) — each asserts behaviour
(validate outcomes, error `code`, `expected`/`received`), not mere existence.
Builders are converted via `let s: Schema = <builder>.into();` then
`s.validate(&value)`; the `val(...)` helper and the `Blob` byte-emitting test
type (from `src/value.rs`'s test module — replicate a local `Blob` that calls
`serialize_bytes`) are used to produce `Bytes`.

1. **`buffer` accepts native bytes.** A local `Blob(vec![1,2,3])` whose
   `Serialize` calls `serialize_bytes` validates against `buffer()` → `Ok`;
   the output is `ZerxValue::Bytes(vec![1,2,3])`.
2. **`buffer` rejects non-bytes (C2 no-coercion).** `buffer()` validating
   `&vec![1u8,2,3]` (a number array, **not** emitted via `serialize_bytes`) →
   `Err(TYPE_MISMATCH)` with `expected == Some("buffer")`, `received ==
   Some("array")`. `buffer()` validating `"hi"` → `TYPE_MISMATCH` (`received ==
   Some("string")`); validating `&7i32` → `received == Some("number")`.
3. **`buffer().mime(...)` stores the MIME on the modifier.** `buffer().mime(
   "image/png")` — white-box: the builder's `modifiers.mime == Some("image/png")`;
   a `Blob` still validates `Ok`. (`mime` and `mime_format` write the same field:
   `buffer().mime_format("image/png")` yields the same `modifiers.mime`.)
4. **`uri` happy + sad.** `uri()` accepts `"https://example.com"`,
   `"mailto:a@b.co"`, `"urn:isbn:123"`; rejects `"not a uri"` and `"://nohost"`
   and `":path"` → `Err(INVALID_URI)`; rejects a non-string (`&7i32`) →
   `TYPE_MISMATCH` with `expected == Some("uri")`.
5. **`url` happy + sad.** `url()` accepts `"http://example.com"`,
   `"https://example.com:8080/path?q=1#frag"`, `"http://localhost"`; rejects
   `"ftp://example.com"` (wrong scheme), `"http://exa..mple.com"` (consecutive
   dots), `"http://nodot"` (no dot, not localhost), `"http://example.com:99999"`
   (port out of range) → `Err(INVALID_URL)`; rejects a non-string (`&true`) →
   `TYPE_MISMATCH` with `expected == Some("url")`.
6. **`json` accepts anything serde-bridgeable, identity parse.** `json()`
   validates `&7i32`, `"hi"`, `&true`, `&()`, `&vec![1i64,2,3]`, and a struct →
   each `Ok`, with the output equal to `ZerxValue::from_serialize(&input)`. A
   `Blob` (bytes) also validates `Ok` to `ZerxValue::Bytes`.
7. **`jsonschema` accepts anything serde-bridgeable.** `jsonschema()` validates
   an object (`{ "type": "string" }` built via serde) and a bare `&true` (a valid
   2020-12 schema) → `Ok`, identity output. (No structural JSON-Schema validation
   is asserted — documenting the v1 accept-all behaviour.)
8. **Negative compile-guarantee.** The `compile_fail` doctests on `UriSchema`
   (`uri().min(3)`), `UrlSchema`, `BufferSchema`, `JsonSchema`,
   `JsonschemaSchema` fail to compile (no string/number validator methods leak).
9. **Modifiers compose via the blanket trait.** `buffer().optional()`,
   `uri().describe("d")`, `json().default(serde_json::json!({}))` etc. type-check
   and carry the modifier (white-box on `modifiers`); a missing optional `buffer`
   field inside an `object` is omitted from output (delegates to `parse_field`).
10. **`SchemaKind` dispatch + Debug.** `Schema`'s hand-written `Debug` renders
    `"Buffer"`, `"Uri"`, `"Url"`, `"Json"`, `"JsonSchema"` for the five builders,
    proving each type added exactly one variant + one arm per dispatch method.
11. **Composition.** An `object` mixing a special type with complex types — e.g.
    `object([("avatar", buffer().mime("image/png").optional().into()),
    ("home", url().into()), ("meta", record(json()).into())])` — validates a
    well-formed input (avatar omitted when absent; a `Blob` when present) and
    reports a deep error with the full path (e.g. a malformed `home` URL →
    `Err(INVALID_URL)` with `path == ["home"]`).
12. **Clone immutability.** From `let base = buffer();`,
    `base.clone().mime("image/png")` carries the MIME while `Schema::from(base)`
    has `modifiers.mime == None` (clone-and-return holds).

Expected outcome: a `zerx` crate exposing the
`buffer`/`uri`/`url`/`json`/`jsonschema` constructors and their typed builders,
the bytes-only buffer (with MIME on the modifier), the structural URI/URL
checks, and the serde-bridgeable `json`/`jsonschema` leaves — all built on F2's
dispatch seam with no new parse-flow control logic. This **completes Phase 2 —
Type catalogue**; the full vision catalogue (excluding the feature-gated
host-opaque types) is implemented, ready for **Phase 3 — JSON Schema roundtrip**
(`PLAN_J1_export`, which reads these variants and `modifiers.mime` to emit the
format markers).

<plan_ready>docs/PLAN_T4_special_types.md</plan_ready>
