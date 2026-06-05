# PLAN_T2_complex_types

## Context & Goal

Second plan of **Phase 2 — Type catalogue** in the `CONCEPT_zerx_foundation`
execution. Phase 1 (`PLAN_F3_error_model`, `PLAN_F1_value_model`,
`PLAN_F2_schema_core`) and the first Phase-2 plan (`PLAN_T1_basic_types`) are
complete and archived. F2 delivered the `schema-core` spine — the erased
`Schema` carrier, the `SchemaKind` container with its per-kind dispatch seam
(`check_type` / `parse_inner`), the blanket `Modify` trait, the parse flow, the
`Validator` storage contract, and `parse_field` (the missing/present field
wrapper). T1 delivered the five basic typed builders, the first concrete
validators, the `type_tag`/`numeric_as_f64` helpers, and proved the
type-specific-method composition and the negative compile-guarantee.

T2 delivers the **complex (container) types** and the first **recursive parse
descent** through the F2 dispatch seam:

- `object` (ordered shape) with the three unknown-key modes
  (`strict` / `passthrough` / `strip`), strict being the default (candidate C4).
- `array` (homogeneous element schema) with `min` / `max` length validators.
- `record` (open object, all values share one schema).
- `tuple` (fixed-length, positional element schemas).
- `union` (first-match across variants, aggregated error).
- `discriminated_union` (O(1) variant lookup by a discriminator key).
- `literal` (a single constant value).

T2 is the plan that first makes the parse flow **recurse**: container
`parse_inner` delegates descend into child schemas via the F2-provided
`Schema::parse_present` / `Schema::parse_field`, and attach **path segments**
(object key, array/tuple index) to child errors as they unwind. The leaf layer
(T1) deliberately left `path == []`; T2 is where paths are built.

T2 extends `SchemaKind` with seven variants and one delegating arm per variant
in each dispatch method, introducing **no new parse-flow control logic** in
`schema-core` (concept C1 / `schema-core.md` constraints). It fills the `types`
concern's complex-type Constraints at Doc Update.

### Boundary (`docs/architecture/types.md`, `docs/architecture/schema-core.md`)

- `types` owns the type catalogue and the pluggable validators. T2 adds seven
  complex-type variants, their typed builders, their per-kind delegate functions
  (`check_*` + `parse_*`), and the array-length validators.
- `types` does **NOT** define the core `Schema` representation, the parse flow,
  the depth/cycle guard, `parse_field`, or the `Validator` **trait** — those are
  `schema-core` (F2). T2 only adds variants + delegating dispatch arms and
  consumes `parse_present` / `parse_field`.
- `types` does **NOT** own JSON Schema export/import. T2 fills the
  `json_schema()` fragment of the two new array validators only
  (`minItems`/`maxItems`); the **type-level** JSON Schema export of objects,
  arrays, unions, discriminators, etc. is `PLAN_J1_export`. T2 implements no
  `to_json_schema` for any type.
- `types` does **NOT** own the host-opaque `function`/`tvalue` types — see
  `mlua`.
- The object utilities (`partial`, `extend`, `omit*`, `strip*`) are **NOT** in
  T2 scope — they are `PLAN_T3_object_utilities`. T2's `ObjectBody` carries only
  `shape` + `mode`; T3 extends it with the `all_optional` flag and the prestrip
  fields. T2 leaves a coherent object type without them.
- The special types (`buffer`, `uri`, `url`, `json`, `jsonschema`) are **NOT**
  in T2 scope — they are `PLAN_T4_special_types`.

### Governing candidate decisions (`CONCEPT_zerx_foundation.md`)

- **C1 — hybrid enum-core + typed builders.** Each complex type is one
  `SchemaKind` variant fronted by one typed builder; single-child constructors
  (`array`, `record`) accept `impl Into<Schema>`, and multi-child constructors
  (`object`, `tuple`, `union`, `discriminated_union`) take erased `Schema`
  children (each composed via `.into()` — see *Child-collection construction*
  under Design). T2 adds the variants syntactically inside the
  `schema-core`-owned container.
- **C4 — strict by default.** Object validation rejects unknown properties by
  default (`unknown_property`); `.passthrough()` / `.strip()` relax it
  explicitly. The runtime mode is the single source of truth; the
  `additionalProperties` export sync is J1's concern (T2 only stores the mode).
  **T2 is the plan that substantiates C4.**
- **C7 — single `Result`-returning API, no throwing or dual parse.** Builder
  construction stays infallible. `discriminated_union(...)` returns its builder
  even when the variants are mis-configured (a non-object variant; a missing,
  non-literal, optional/defaulted/nullable, or non-keyable discriminator field;
  a duplicate discriminator value); such a structural defect is remembered in
  `DiscriminatorState::Invalid` and surfaced as
  `Err(INVALID_DISCRIMINATED_UNION)` at `validate` time — never as a panic, and
  raised in `check_discriminated_union` (the first parse-flow step) so it wins
  over `TYPE_MISMATCH` for any input. This is required by the API shape:
  `discriminated_union(...)` is used as an erased `Schema` child (via `.into()`)
  inside `object([...])` (see `docs/vision.md:195-206`), and `.into()` cannot be
  called on a `Result`. It mirrors the T1 invalid-regex precedent
  (`PatternState::Invalid`).
- **C8 — English-only, machine-readable errors.** Each new failure mode carries
  a distinct `code`; `expected`/`received` are descriptor strings (a type tag or
  an allowed-value list), never raw `ZerxValue`s. Union failures aggregate
  per-variant errors via the existing `ZerxError::union` helper.

### Architect rulings to encode

None pending. T2 introduces **no new dependency** (no entry under Hard Rule 11);
all container logic is pure over the existing `serde`/`serde_json`/`std` set plus
the `regex` crate already present (unused by T2). The discriminator lookup uses
`std::collections::HashMap`.

## Breaking Changes

**No.** T2 is additive: new builder types, constructors, validators, and
delegate functions in `src/types.rs`; new re-exports from `src/lib.rs`; and seven
new `SchemaKind` variants plus one delegating arm per variant in `check_type`,
`parse_inner`, and the `Debug` match in `src/schema.rs`. It changes no existing
public signature and does not edit `src/error.rs` or `src/value.rs`. The one
non-additive edit is renaming the currently-unused `_ctx` parameter of
`SchemaKind::parse_inner` to `ctx` (T2 is the first caller to use it for
recursion) — an internal `pub(crate)` change with no API impact.

## Dependencies

**None added.** No new crate. `Cargo.toml` is untouched.

## Reference Patterns

- `src/types.rs` (T1) — the authoritative pattern for everything T2 does:
  - the builder newtype + `BuilderInner` impl (grants blanket `Modify`) +
    `From<Builder> for Schema` + constructor, repeated per type,
  - the `check_*` delegate functions called from `SchemaKind::check_type`,
  - the concrete `Validator` impls (`validate` + `json_schema`),
  - the open `ErrorCode` catalogue block (`impl ErrorCode { pub const … }` in
    `src/types.rs`; **no edit to `src/error.rs`**),
  - the `type_tag` helper (reused for `received` descriptors),
  - the `#[cfg(test)] mod tests` layout and the `val(builder, &value)` helper.
- `src/schema.rs` (F2) — the dispatch seam and recursion primitives T2 builds on:
  - `SchemaKind::check_type` / `parse_inner` as a single `match self` with one
    arm per kind (T2 adds seven arms to each, plus the `Debug` match),
  - `Schema::parse_present(&self, value, ctx)` — recurse into a present child,
  - `Schema::parse_field(&self, Option<&value>, ctx)` — handle a possibly-missing
    object field (default → optional-omit → required-error), used by `object`,
  - `ParseContext` — the depth guard already wraps every `parse_present`; T2
    container recursion accumulates frames automatically.
- `src/error.rs` (F3) — `ZerxError::new(code, msg).expected(…).received(…)`; the
  public `path: Vec<String>` field (T2 prepends segments directly:
  `e.path.insert(0, segment)`); the seeded `ErrorCode::TYPE_MISMATCH` and
  `ErrorCode::UNION_MISMATCH`; and the `ZerxError::union(path, inner)` aggregator
  (used by `union`).
- `src/value.rs` (F1) — `ZerxValue::Object(Map)` / `Array(Vec)`; `Map` (ordered,
  insertion-order, `insert`/`get`/`iter`/`len`); accessors `as_object`,
  `as_array`; `From<&str>`/`From<i64>`/… for ergonomic `literal(...)`.
- **Zex semantics** (`/Users/martinschlott/Documents/MyProjects/zex`):
  - `src/zex/complex-types/object.ts` — modes, unknown-key handling, output key
    ordering (shape order, then passthrough keys in input order), per-field
    default/optional/required (the parts T2 needs; `partial`/`omit*`/`strip*`
    belong to T3 and are out of scope here).
  - `src/zex/complex-types/array.ts` — element-wise validation, `min`/`max`.
  - `src/zex/complex-types/record.ts` — every value validated against one schema.
  - `src/zex/complex-types/tuple.ts` — fixed length, positional validation.
  - `src/zex/complex-types/literal.ts` — strict equality against the const.
  - `src/zex/unions.ts` — `ZexUnion` (first-match, aggregated error) and
    `ZexDiscriminatedUnion` (build a value→variant map; require each variant to
    be an object whose discriminator field is a required `const`; reject
    duplicates).

## Assumptions & Risks

- **Recursion lives in `parse_inner`, not `check_type`.** `check_type` only
  verifies the container's outer kind (object/array). The structural descent
  (per-field, per-element, variant matching) happens in `parse_inner`, which
  receives `&mut ParseContext` and recurses via `parse_present`/`parse_field`.
  This keeps the F2 invariant: each type adds exactly one arm to each dispatch
  method and no new control logic enters the shared flow.
- **Path is built on the way up.** A child error returned from
  `parse_present`/`parse_field` carries the child's own (shorter) path; the
  container prepends its segment via `e.path.insert(0, segment)` before
  re-returning. Indices render as decimal strings (`"0"`, `"1"`), matching Zex's
  path keys and the existing `ZerxError` display (`a/b/0`).
- **Depth guard composes for free.** Every `parse_present` call enters/exits the
  depth counter (and exits even on `Err`), so container recursion accumulates
  frames correctly and sequential `union`/`discriminated_union` variant attempts
  do **not** falsely accumulate depth (each attempt fully unwinds before the
  next). No T2 code touches `ParseContext`.
- **`discriminated_union` defers config errors to validate-time (C7), checked
  first.** The constructor is infallible and builds the lookup eagerly into a
  `DiscriminatorState`: `Ok { map, allowed }` on success, `Invalid(message)` on
  any structural defect (non-object variant; missing/non-literal/
  optional/defaulted/nullable/non-keyable discriminator field; duplicate
  discriminant). The `Invalid` state is raised in `check_discriminated_union` —
  the **first** step of the fixed `check_type → validators → parse_inner` flow —
  so the schema-defect error wins over `TYPE_MISMATCH` even when the validated
  value is a non-object. This is forced by the erased-`Schema` child API (a
  `Result` cannot be `.into()`-ed into an `object([...])` field) and mirrors the
  T1 `Pattern` precedent. **Flagged for Reviewer** as a deliberate design choice
  over a fallible constructor.
- **Discriminator values are restricted to a hashable subset.** The O(1) lookup
  keys on a `DiscriminantKey` derived from the variant's literal const: `Bool`,
  integer (all of `I64`/`U64`/`I128`/`U128` normalised to `i128` via
  `i128::try_from`, falling back to "not keyable" on `u128` overflow), and
  `String`. A discriminator literal that is a float, null, array, object, or
  bytes is "not keyable" → the constructor records `Invalid`. In practice
  discriminators are strings (see `docs/vision.md:195-206`). **Flagged for
  Reviewer.**
- **`literal` uses exact `ZerxValue` equality.** `ZerxValue` is `PartialEq`, so
  `literal("human")` matches only `ZerxValue::String("human")`. Numeric literals
  are subject to the cross-variant footgun: `literal(5i64)` stores `I64(5)` and
  does **not** match an input that serialised to `U64(5)` (e.g. a `u64` field).
  String and boolean literals — which cover every discriminator use in the
  vision — are exact and unaffected. Numeric-aware literal coercion is **out of
  scope** for v1 (YAGNI); **flagged for Reviewer**, consistent with T1's
  accepted `f64` bound-comparison precision note.
- **No object-level utilities or special types here.** `ObjectBody` is
  intentionally minimal (`shape` + `mode`); `partial`/`extend`/`omit*`/`strip*`
  (T3) and `buffer`/`uri`/`url`/`json`/`jsonschema` (T4) are out of scope.
  Adding a field to `ObjectBody` in T3 is additive and stays within the object
  type's own delegate — it does not reopen the shared parse flow.
- **Validators run before recursion.** The fixed flow is
  `check_type → validators → parse_inner`. For `array`, the `min`/`max` length
  validators therefore run on the raw array **before** element descent, matching
  Zex. Containers have no other validators in T2.
- **Defensive non-matching value in `parse_*` delegates.** `check_type` gates the
  kind before `parse_inner`, so a container `parse_*` delegate can assume the
  right `ZerxValue` shape; the unreachable mismatch branch returns `value.clone()`
  (no panic, consistent with the T1 validator pass-through convention).

## Design

### Module layout

- `src/schema.rs`: two changes only.
  - **(a)** Extend `SchemaKind` with the seven complex variants and add one
    delegating arm per variant in `check_type`, `parse_inner`, and the `Debug`
    match (per *SchemaKind extension* below).
  - **(b)** Rename `parse_inner`'s `_ctx` parameter to `ctx` (it is now used for
    recursion). `check_type`'s `_ctx` stays unused.
  - No other change. `Schema::parse_present`, `Schema::parse_field`,
    `Schema::new`, the `Schema` fields, `BuilderInner` (already `pub(crate)`),
    and the blanket `Modify` are reused unchanged.
- `src/types.rs`: add (appended to the existing file)
  - the T2 `impl ErrorCode { … }` catalogue block,
  - the supporting types: `ObjectMode`, `ObjectBody`, `DiscriminantKey`,
    `DiscriminatorState`, `DiscriminatedUnionBody`,
  - the per-kind delegates: `check_object`/`parse_object`,
    `check_array`/`parse_array`, `check_record`/`parse_record`,
    `check_tuple`/`parse_tuple`, `parse_union` (+ a no-op `check_union`),
    `check_discriminated_union`/`parse_discriminated_union`, `check_literal`,
  - the array-length validators `ArrayMinLength`/`ArrayMaxLength`,
  - the seven builder newtypes (`ObjectSchema`, `ArraySchema`, `RecordSchema`,
    `TupleSchema`, `UnionSchema`, `DiscriminatedUnionSchema`, `LiteralSchema`)
    with `BuilderInner`, `From<…> for Schema`, inherent methods, and
    constructors,
  - the path-prefix helper `fn prefix_path(e: ZerxError, segment: impl Into<String>) -> ZerxError`,
  - new `#[cfg(test)] mod tests` cases (appended).
- `src/lib.rs`: extend the `pub use types::{…}` line with the new constructors
  and builder types:
  `object, array, record, tuple, union, discriminated_union, literal,
  ObjectSchema, ArraySchema, RecordSchema, TupleSchema, UnionSchema,
  DiscriminatedUnionSchema, LiteralSchema`. Delegates, body types, validators,
  and `DiscriminantKey`/`DiscriminatorState` stay `pub(crate)`/private (not
  public API).

### Child-collection construction — erased `Schema` items (API decision)

Rust array literals are **homogeneous**: `[string(), number()]` cannot form,
because `StringSchema` and `NumberSchema` are distinct types, and a per-element
`S: Into<Schema>` bound on the *iterator* does not erase them inside the literal
(it leaves the element type ambiguous). Therefore the **multi-child**
constructors take **already-erased `Schema` items**, and each child is erased at
the call site with `.into()`:

- `object([("a", string().into()), ("b", number().into())])`
- `tuple([string().into(), number().into()])`
- `union([number().into(), string().into()])`
- `discriminated_union("kind", [variant_a.into(), variant_b.into()])`

This honours C1 — each child still converts through `Into<Schema>`, and `Schema:
Into<Schema>` holds by the identity `From` impl — while respecting Rust's type
system. The vision's no-`.into()` snippets (`docs/vision.md`) are **illustrative,
not normative** (concept: "Code snippets … are illustrative, not normative"); the
`.into()` per child is the honest, compile-correct surface, and `S: Into<Schema>`
for the whole collection is **rejected** because it does not compile for
heterogeneous children. The **single-child** constructors (`array`, `record`,
`literal`) keep the ergonomic `impl Into<Schema>` / `impl Into<ZerxValue>`
argument (no literal, no homogeneity problem). A future ergonomic macro
(`zerx::object!{ … }`) is possible but out of T2 scope (YAGNI).

### `SchemaKind` extension (in `src/schema.rs`)

Add variants (T2-owned, living syntactically in the container per C1). `Array`
and `Record` box their child to keep `SchemaKind` sized:

```rust
pub(crate) enum SchemaKind {
    Any,                                  // F2
    Lazy(Lazy),                           // F2
    String, Number, Boolean,             // T1
    Enum(Vec<String>), Null,             // T1
    Object(crate::types::ObjectBody),                       // T2
    Array(Box<Schema>),                                     // T2
    Record(Box<Schema>),                                   // T2
    Tuple(Vec<Schema>),                                    // T2
    Union(Vec<Schema>),                                    // T2
    DiscriminatedUnion(crate::types::DiscriminatedUnionBody), // T2
    Literal(ZerxValue),                                    // T2
}
```

`SchemaKind` must stay `Clone`; every payload above is `Clone` (`ObjectBody`,
`DiscriminatedUnionBody`, `Box<Schema>`, `Vec<Schema>`, `ZerxValue`).

Dispatch arms:

- `check_type` (kind check only; no recursion — `_ctx` unused):
  - `Object(_) => crate::types::check_object(value)`
  - `Array(_) => crate::types::check_array(value)`
  - `Record(_) => crate::types::check_record(value)`
  - `Tuple(_) => crate::types::check_tuple(value)`
  - `Union(_) => crate::types::check_union()` (always `Ok(())`; matching happens
    in `parse_inner`)
  - `DiscriminatedUnion(body) => crate::types::check_discriminated_union(value, body)`
  - `Literal(c) => crate::types::check_literal(value, c)`
- `parse_inner` (structural step; recurses via `ctx`):
  - `Object(body) => crate::types::parse_object(body, value, ctx)`
  - `Array(item) => crate::types::parse_array(item, value, ctx)`
  - `Record(vs) => crate::types::parse_record(vs, value, ctx)`
  - `Tuple(items) => crate::types::parse_tuple(items, value, ctx)`
  - `Union(variants) => crate::types::parse_union(variants, value, ctx)`
  - `DiscriminatedUnion(body) => crate::types::parse_discriminated_union(body, value, ctx)`
  - `Literal(_) => Ok(value.clone())` (the const equality is the type check; the
    structural step is identity — may share the leaf arm)
- `Debug` match: render `"Object"`, `"Array"`, `"Record"`, `"Tuple"`, `"Union"`,
  `"DiscriminatedUnion"`, `"Literal"`.

### Type checks (`check_*` in `src/types.rs`)

Each returns `Result<(), ZerxError>`; on outer-kind mismatch,
`ErrorCode::TYPE_MISMATCH` with `.expected(<requirement>).received(type_tag(value))`.

- `check_object` — accept `ZerxValue::Object`; else `TYPE_MISMATCH`, expected
  `"object"`.
- `check_array` — accept `ZerxValue::Array`; else `TYPE_MISMATCH`, expected
  `"array"`.
- `check_record` — accept `ZerxValue::Object`; else `TYPE_MISMATCH`, expected
  `"object"`.
- `check_tuple` — accept `ZerxValue::Array` (length is checked in
  `parse_tuple`); else `TYPE_MISMATCH`, expected `"array"`.
- `check_union() -> Ok(())` — unconditional; union matching is in `parse_union`.
- `check_discriminated_union(value, body)` — **the config-defect check runs
  first**: if `body.state` is `Invalid(msg)`, return
  `Err(ZerxError::new(INVALID_DISCRIMINATED_UNION, msg.clone()))` regardless of
  the input value. Only when `state` is `Ok` does it then check the outer kind:
  accept `ZerxValue::Object`; else `TYPE_MISMATCH`, expected `"object"`. Because
  `check_type` is the first step of the fixed parse flow
  (`check_type → validators → parse_inner`), this guarantees the schema-defect
  error wins over `TYPE_MISMATCH` for **any** input (including a non-object) —
  resolving the ordering hazard. `parse_discriminated_union` therefore runs only
  with `state == Ok`.
- `check_literal(value, const)` — accept iff `value == const`
  (`ZerxValue` `PartialEq`); else `ErrorCode::INVALID_LITERAL` with
  `expected = literal_descriptor(const)` and `received = type_tag(value)`.
  `literal_descriptor` is a private helper rendering a compact form of the const
  (e.g. its `type_tag`, or for strings/bools/ints a short value form); it MUST
  NOT embed the raw `ZerxValue` in a non-string field (C8).

### Path-prefix helper

```rust
fn prefix_path(mut e: ZerxError, segment: impl Into<String>) -> ZerxError {
    e.path.insert(0, segment.into());
    e
}
```

Used by every container `parse_*` to attach its segment to a child error as the
stack unwinds. No edit to `src/error.rs` (operates on the public `path` field).

### Structural parse delegates (`parse_*` in `src/types.rs`)

All assume `check_type` already gated the outer kind; the unreachable
wrong-kind branch returns `Ok(value.clone())`.

- **`parse_object(body, value, ctx)`** — `body: &ObjectBody { shape, mode }`.
  1. Read the input `Map` (`value.as_object()`).
  2. **Strict unknown-key check** (only when `mode == Strict`): for each input
     key not present in `shape`, return
     `Err(prefix_path(ZerxError::new(UNKNOWN_PROPERTY, "unknown property '<key>'").expected("property not in schema").received(type_tag(<that value>)), <key>))`.
     The first unknown key wins (Zex order: unknown check precedes field
     validation).
  3. **Field validation**, in `shape` order: for each `(key, field_schema)`,
     call `field_schema.parse_field(input.get(key), ctx)`. On `Ok(Some(v))`
     insert `(key, v)` into the output map; on `Ok(None)` omit the field; on
     `Err(e)` return `Err(prefix_path(e, key))`.
  4. **Mode tail:** if `mode == Passthrough`, append every input key not in
     `shape` (in input order) to the output map verbatim; if `mode == Strip`,
     drop them (no-op); `Strict` already errored in step 2.
  5. Return `Ok(ZerxValue::Object(output))`. Output key order = `shape` order,
     then passthrough keys in input order (matches Zex).
- **`parse_array(item, value, ctx)`** — for each element at `index`, call
  `item.parse_present(elem, ctx)`; on `Err(e)` return
  `Err(prefix_path(e, index.to_string()))`; collect successes into
  `ZerxValue::Array`.
- **`parse_record(value_schema, value, ctx)`** — for each `(key, val)` in the
  input `Map`, call `value_schema.parse_present(val, ctx)`; on `Err(e)` return
  `Err(prefix_path(e, key))`; collect into a new `Map` preserving input order.
- **`parse_tuple(items, value, ctx)`** — read the input array; if
  `array.len() != items.len()`, return
  `Err(ZerxError::new(TUPLE_LENGTH_MISMATCH, …).expected("array of length <n>").received("array of length <m>"))`;
  else validate each position `i` via `items[i].parse_present(array[i], ctx)`,
  prefixing `i.to_string()` on error; collect into `ZerxValue::Array`.
- **`parse_union(variants, value, ctx)`** — try each `variant.parse_present(value, ctx)`
  in order; return the first `Ok`. If all fail, collect the per-variant errors
  and return `Err(ZerxError::union(Vec::new(), inner_errors))` (the seeded
  aggregator; `UNION_MISMATCH` code, `expected = "one of the union variants"`,
  the variant errors as `inner_errors`). Path stays empty at this level; a parent
  container prefixes its own segment as usual.
- **`parse_discriminated_union(body, value, ctx)`** —
  `body: &DiscriminatedUnionBody { key, variants, state }`.
  1. `state` is `Ok` here (the `Invalid` config defect is already caught in
     `check_discriminated_union`, which runs first); a defensive `Invalid` arm
     returns `Err(INVALID_DISCRIMINATED_UNION)` and is unreachable in practice.
  2. Read the input object; read the discriminator field `input.get(key)`. If
     absent, return `Err(INVALID_DISCRIMINANT)` with
     `expected = <allowed values joined>`, message naming the missing key.
  3. Compute `DiscriminantKey` of the discriminator value; if not keyable or not
     in the map, return `Err(INVALID_DISCRIMINANT)` with
     `expected = <allowed values joined>`, `received = type_tag(<disc value>)`.
  4. Otherwise `variants[index].parse_present(value, ctx)` on the **whole**
     object and return its result as-is (the variant — itself an object schema —
     re-validates the discriminator literal field; no extra path segment, no
     double prefix). The matched variant's own field errors keep their paths.

### Array-length validators (concrete `Validator` impls)

- `ArrayMinLength(usize)` — fail if `arr.len() < n` → `ARRAY_TOO_SHORT`.
  Fragment `{"minItems": n}`. Defensive non-array → pass.
- `ArrayMaxLength(usize)` — fail if `arr.len() > n` → `ARRAY_TOO_LONG`.
  Fragment `{"maxItems": n}`. Defensive non-array → pass.

These mirror T1's `MinLength`/`MaxLength` (which count string scalars); array
length counts elements.

### Supporting types

```rust
#[derive(Clone)]
pub(crate) enum ObjectMode { Strict, Passthrough, Strip }   // default Strict

#[derive(Clone)]
pub(crate) struct ObjectBody {
    pub(crate) shape: Vec<(String, Schema)>,   // ordered; key uniqueness on construction
    pub(crate) mode: ObjectMode,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum DiscriminantKey { Bool(bool), Int(i128), Str(String) }

#[derive(Clone)]
pub(crate) enum DiscriminatorState {
    Ok { map: std::collections::HashMap<DiscriminantKey, usize>, allowed: Vec<String> },
    Invalid(String),
}

#[derive(Clone)]
pub(crate) struct DiscriminatedUnionBody {
    pub(crate) key: String,
    pub(crate) variants: Vec<Schema>,
    pub(crate) state: DiscriminatorState,
}
```

`discriminant_key(value: &ZerxValue) -> Option<DiscriminantKey>` maps
`Bool`/`String` and the four integer variants (via `i128::try_from` where
needed) to `Some`; everything else to `None`.

### Builders, methods, constructors

Per builder (mirrors T1's newtype pattern; relies on the `pub(crate)
BuilderInner` trait for the blanket `Modify`):

```rust
#[derive(Clone)] pub struct ObjectSchema(Schema);
impl BuilderInner for ObjectSchema { fn schema_mut(&mut self) -> &mut Schema { &mut self.0 } }
impl From<ObjectSchema> for Schema { fn from(b: ObjectSchema) -> Schema { b.0 } }
```

Same for `ArraySchema`, `RecordSchema`, `TupleSchema`, `UnionSchema`,
`DiscriminatedUnionSchema`, `LiteralSchema`.

Inherent methods (T2):

- `ObjectSchema`: `strict(self)`, `passthrough(self)`, `strip(self)` — set
  `body.mode` and return `Self`. (`partial`/`extend`/`omit*`/`strip*` → T3.)
- `ArraySchema`: `min(usize)`, `max(usize)` — push `ArrayMinLength`/`ArrayMaxLength`.
- `RecordSchema`, `TupleSchema`, `UnionSchema`, `DiscriminatedUnionSchema`,
  `LiteralSchema`: no inherent validators.

Constructors (see *Child-collection construction* above — multi-child
constructors take erased `Schema` items; single-child take `impl Into<…>`):

- `object<I, K>(fields: I) -> ObjectSchema where I: IntoIterator<Item = (K, Schema)>, K: Into<String>`
  — collect into `Vec<(String, Schema)>` (last-write-wins on duplicate keys,
  preserving first-occurrence order, matching `Map::insert` semantics); default
  `mode = Strict`. Call form: `object([("street", string().min(1).into()), …])`.
- `array(item: impl Into<Schema>) -> ArraySchema` — wrap `Box::new(item.into())`.
- `record(value: impl Into<Schema>) -> RecordSchema` — wrap `Box::new(value.into())`.
- `tuple<I>(items: I) -> TupleSchema where I: IntoIterator<Item = Schema>`
  — collect into `Vec<Schema>`.
- `union<I>(variants: I) -> UnionSchema where I: IntoIterator<Item = Schema>`
  — collect into `Vec<Schema>`.
- `discriminated_union<K, I>(key: K, variants: I) -> DiscriminatedUnionSchema
  where K: Into<String>, I: IntoIterator<Item = Schema>` — collect variants into
  `Vec<Schema>`, then **build `DiscriminatorState`** by inspecting each variant.
  A variant is well-formed **only if**:
  - its kind is `Object(body)`; else `Invalid`,
  - `body.shape` contains `(key, field_schema)`; else `Invalid` (missing
    discriminator field),
  - `field_schema.kind` is `Literal(const)`; else `Invalid`,
  - the discriminator field is a **required, non-nullable const**:
    `field_schema.modifiers.optional == false`,
    `field_schema.modifiers.default.is_none()`, and
    `field_schema.modifiers.nullable == false`; if any modifier makes the const
    non-required or nullable → `Invalid` (matches Zex's "discriminator must be a
    required const", `unions.ts:145-151`),
  - `discriminant_key(const)` is `Some`; else `Invalid` (non-keyable const),
  - the discriminant key is unique across variants; a duplicate → `Invalid`.
  On success, `state = Ok { map, allowed }` where `allowed` holds the per-variant
  discriminator display strings (for error `expected`). The first defect's
  message is recorded in `Invalid`.
- `literal(value: impl Into<ZerxValue>) -> LiteralSchema` — wrap
  `SchemaKind::Literal(value.into())`. (`ZerxValue` already has `From<&str>`,
  `From<bool>`, integer/float `From`s for ergonomic literals.)

### Error-code catalogue (open, in `src/types.rs`, no edit to `error.rs`)

```rust
impl ErrorCode {
    pub const UNKNOWN_PROPERTY: ErrorCode             = ErrorCode::new("unknown_property");
    pub const ARRAY_TOO_SHORT: ErrorCode              = ErrorCode::new("array_too_short");
    pub const ARRAY_TOO_LONG: ErrorCode               = ErrorCode::new("array_too_long");
    pub const TUPLE_LENGTH_MISMATCH: ErrorCode        = ErrorCode::new("tuple_length_mismatch");
    pub const INVALID_LITERAL: ErrorCode              = ErrorCode::new("invalid_literal");
    pub const INVALID_DISCRIMINANT: ErrorCode         = ErrorCode::new("invalid_discriminant");
    pub const INVALID_DISCRIMINATED_UNION: ErrorCode  = ErrorCode::new("invalid_discriminated_union");
}
```

`TYPE_MISMATCH` and `UNION_MISMATCH` (seeded in `errors`) are reused; T1's codes
are reused where relevant (e.g. nested string/number validator failures bubble up
unchanged). T2 adds **no** edit to `src/error.rs`.

## Steps

1. `src/schema.rs`: add the seven `SchemaKind` variants (`Object(ObjectBody)`,
   `Array(Box<Schema>)`, `Record(Box<Schema>)`, `Tuple(Vec<Schema>)`,
   `Union(Vec<Schema>)`, `DiscriminatedUnion(DiscriminatedUnionBody)`,
   `Literal(ZerxValue)`) and one delegating arm per variant in `check_type`,
   `parse_inner`, and the `Debug` match (per Design). Rename `parse_inner`'s
   `_ctx` to `ctx`. No other change.
2. `src/types.rs`: add the T2 `impl ErrorCode { … }` catalogue block.
3. `src/types.rs`: add the supporting types (`ObjectMode`, `ObjectBody`,
   `DiscriminantKey`, `DiscriminatorState`, `DiscriminatedUnionBody`), the
   `discriminant_key` and `literal_descriptor` helpers, and the `prefix_path`
   helper.
4. `src/types.rs`: implement the `check_*` delegate functions
   (`check_object`/`check_array`/`check_record`/`check_tuple`/`check_union`/
   `check_discriminated_union`/`check_literal`).
5. `src/types.rs`: implement the `parse_*` structural delegates
   (`parse_object`/`parse_array`/`parse_record`/`parse_tuple`/`parse_union`/
   `parse_discriminated_union`).
6. `src/types.rs`: implement `ArrayMinLength`/`ArrayMaxLength`
   (`validate` + `json_schema`).
7. `src/types.rs`: implement the seven builder newtypes (`BuilderInner`,
   `From<…> for Schema`, inherent methods) and the constructors (`object`,
   `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`),
   including the `DiscriminatorState` build in `discriminated_union`.
8. `src/lib.rs`: extend the `pub use types::{…}` line with the new constructors
   and builder types.
9. Add the `#[cfg(test)] mod tests` cases per Verification (appended to the
   existing T1 test module or a sibling module).
10. Run the full build/lint/test matrix below; resolve every warning.

**Doc Update (workflow Step 6, by the Coder after Validation — not now):** fill
`docs/architecture/types.md` with the review-hardened complex-type contract:

- the seven complex-type variants and the one-arm-per-dispatch-method rule
  (no new shared parse-flow control logic);
- each container's accepted outer `ZerxValue` kind and `TYPE_MISMATCH` behaviour;
- `object`: the three modes (`strict` default, `passthrough`, `strip`), the
  unknown-key rule (`UNKNOWN_PROPERTY` in strict; preserved in passthrough;
  dropped in strip), output key ordering (shape order then passthrough keys in
  input order), and per-field delegation to `parse_field` (default → optional-omit
  → `REQUIRED`);
- `array`: element-wise validation with index path segments, and the
  `min`/`max` length validators (`ARRAY_TOO_SHORT`/`ARRAY_TOO_LONG`; fragments
  `minItems`/`maxItems`);
- `record`: every value validated against one schema, keys preserved, key path
  segments;
- `tuple`: fixed length (`TUPLE_LENGTH_MISMATCH`), positional validation, index
  path segments;
- `union`: first-match across variants, aggregated `UNION_MISMATCH` with
  per-variant `inner_errors`;
- `discriminated_union`: O(1) lookup by a `DiscriminantKey` (Bool/Int/Str
  subset); the well-formedness rules for a variant (object kind, with the
  discriminator field present as a **required, non-nullable** `literal` whose
  const is keyable, and unique across variants); the infallible-constructor +
  deferred `INVALID_DISCRIMINATED_UNION` config error checked in the first
  parse-flow step so it wins over `TYPE_MISMATCH` (C7); `INVALID_DISCRIMINANT`
  for a missing/unmatched discriminator;
- `literal`: exact `ZerxValue` equality (`INVALID_LITERAL`), with the documented
  numeric cross-variant caveat;
- the path-construction rule: containers prepend their segment (object key,
  array/tuple index as a decimal string) to child errors as the stack unwinds;
- the T2 error codes block.

The same Doc Update MUST update `types.md` **Related Decisions** to add the
candidate T2 substantiates — **C4** (object strict-by-default and the three
modes) — alongside the existing C1/C7/C8/C9 note, keeping it a **non-slug
candidate reference** (e.g. "candidate C4") per the concept's Decision-Reference
Discipline; **no `D-<slug>` reference** is added (migration is at Concept
Closeout). The `types ↔ schema-core` `Consumes from`/`Provides to` edges already
exist on both sides (`Schema`, `Validator`; the `types → json-schema` validator
fragment edge already exists); **no new cross-concern edge** is opened by this
plan, so no dual-endpoint mirroring is required.

## Verification

Build & lint — all warning-free (the `mlua` runs prove T2 compiles with the
gated `HostOpaque` placeholder present; `type_tag` already covers it):

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets`
- `cargo clippy --all-targets --features mlua`

Unit tests (`#[cfg(test)] mod tests` in `src/types.rs`) — each asserts behaviour
(validate outcomes, error `code`, `path`, `expected`/`received`, and validator
`json_schema()` fragments), not mere existence. Builders are converted via
`let s: Schema = <builder>.into();` then `s.validate(&value)` (the `val(...)`
helper from T1 may be reused).

1. **`object` type check.** `object([("a", number().into())])` rejects a
   non-object (`&7i32`, `"hi"`, `&true`) → `TYPE_MISMATCH`,
   `expected == Some("object")`.
2. **`object` strict (default).** `object([("a", number().into())])` validating
   `{a:1, extra:2}` → `Err(UNKNOWN_PROPERTY)`; the error `path` contains the
   unknown key. `{a:1}` → `Ok` with output `{a:1}`.
3. **`object` passthrough.** `object([("a", number().into())]).passthrough()`
   validating `{a:1, extra:"x"}` → `Ok`; output contains both `a` and `extra`
   (extra verbatim); key order is `["a","extra"]`.
4. **`object` strip.** `object([("a", number().into())]).strip()` validating
   `{a:1, extra:"x"}` → `Ok`; output is `{a:1}` (extra dropped).
5. **`object` field errors carry the field path.**
   `object([("age", number().into())])` validating `{age:"x"}` → `Err`,
   `code == TYPE_MISMATCH`, `path == ["age"]`. Nested
   `object([("user", object([("age", number().into())]).into())])` validating
   `{user:{age:"x"}}` → `path == ["user","age"]`.
6. **`object` optional / default / required.** With
   `object([("a", number().optional().into()), ("b", number().default(5i64).into()), ("c", number().into())])`:
   `{c:1}` → `Ok` with output `{b:5, c:1}` (optional `a` omitted, default `b`
   applied — defaulted field present and non-optional, missing optional omitted,
   not `null`); `{}` → `Err(REQUIRED)` with `path == ["c"]`.
7. **`array`.** `array(number())` accepts `[1,2,3]`; rejects `&7i32` →
   `TYPE_MISMATCH expected "array"`; `[1,"x",3]` → `Err(TYPE_MISMATCH)` with
   `path == ["1"]`.
8. **`array` min/max.** `array(number()).min(2)` accepts `[1,2]`, rejects `[1]`
   → `ARRAY_TOO_SHORT`; `array(number()).max(2)` accepts `[1,2]`, rejects
   `[1,2,3]` → `ARRAY_TOO_LONG`. `ArrayMinLength(2).json_schema() ==
   {"minItems":2}`, `ArrayMaxLength(2).json_schema() == {"maxItems":2}`.
9. **`record`.** `record(number())` accepts `{x:1, y:2}` (output preserves keys);
   rejects `&7i32` → `TYPE_MISMATCH expected "object"`; `{x:1, y:"z"}` →
   `Err(TYPE_MISMATCH)` with `path == ["y"]`.
10. **`tuple`.** `tuple([string().into(), number().into()])` accepts `("a", 1)`
    (a 2-tuple serialises to a 2-element array); rejects a 1- or 3-element array →
    `TUPLE_LENGTH_MISMATCH`; `("a", "b")` → `Err(TYPE_MISMATCH)` with
    `path == ["1"]`; a non-array → `TYPE_MISMATCH expected "array"`.
11. **`union`.** `union([number().into(), string().into()])` accepts `1` and
    `"x"`; rejects `&true` → `Err(UNION_MISMATCH)` with
    `inner_errors.len() == 2`.
12. **`literal`.** `literal("human")` accepts `"human"`; rejects `"tool"` and a
    non-string → `INVALID_LITERAL`. `literal(true)` accepts `true`, rejects
    `false`. (Numeric cross-variant caveat exercised: `literal(5i64)` accepts a
    value built from `5i64`/`5i32` — both `I64(5)` — documenting the exact-match
    behaviour.)
13. **`discriminated_union` happy path.** With
    ```text
    discriminated_union("kind", [
        object([("kind", literal("human").into()), ("session", string().into())]).into(),
        object([("kind", literal("tool").into()),  ("tool", string().into())]).into(),
    ])
    ```
    `{kind:"human", session:"s"}` → `Ok`; `{kind:"tool", tool:"t"}` → `Ok`;
    `{kind:"other", …}` → `Err(INVALID_DISCRIMINANT)` (expected lists
    `human`/`tool`); `{session:"s"}` (missing discriminator) →
    `Err(INVALID_DISCRIMINANT)`; a non-object → `TYPE_MISMATCH`. A field error
    inside the matched variant (`{kind:"human", session:7}`) →
    `Err(TYPE_MISMATCH)` with `path == ["session"]`.
14. **`discriminated_union` config error (deferred, C7, checked first).** Each
    mis-configuration constructs without panic and yields
    `Err(INVALID_DISCRIMINATED_UNION)` at validate time (white-box: `state` is
    `Invalid`): (a) a variant that is not an object; (b) a variant whose `kind`
    field is not a `literal`; (c) a variant whose `kind` field is
    `literal("human").optional()` (or `.default(...)` / `.nullable()`) — the
    non-required/nullable discriminator MUST be rejected; (d) a duplicate
    discriminator value. Additionally, validating a **non-object** (e.g. `&7i32`)
    against a mis-configured DU MUST return `INVALID_DISCRIMINATED_UNION`, **not**
    `TYPE_MISMATCH` — proving the config check runs before the outer-kind check.
15. **Nesting / composition.** A schema mixing the containers — e.g.
    `object([("tags", array(string()).max(2).into()), ("pair", tuple([number().into(), number().into()]).into()), ("meta", record(number()).into())]).strip()`
    — validates a well-formed input to a correctly-shaped output and reports a
    deep error with the full path (e.g. `{tags:["a","b"], pair:[1,"x"], …}` →
    `path == ["pair","1"]`).
16. **`SchemaKind` dispatch + Debug.** `Schema`'s hand-written `Debug` renders
    the correct kind name for each new builder (`"Object"`, `"Array"`,
    `"Record"`, `"Tuple"`, `"Union"`, `"DiscriminatedUnion"`, `"Literal"`),
    proving each type added exactly one variant + one arm per dispatch method.
17. **Depth guard composes.** A container nested ~deep enough to exceed
    `MAX_PARSE_DEPTH` (e.g. an `array(array(array(…)))` chain, or a `lazy`
    self-reference resolved against deep data) yields
    `Err(PARSE_DEPTH_EXCEEDED)`, confirming container recursion accumulates depth
    frames; a shallow parse on the same `ParseContext` afterwards still succeeds
    (context not poisoned).
18. **Clone immutability with modes.** From
    `let base = object([("a", number().into())]);`,
    `base.clone().passthrough()` is passthrough while `Schema::from(base)` stays
    strict (the original schema is untouched — clone-and-return holds for object
    mode).

Expected outcome: a `zerx` crate exposing the
`object`/`array`/`record`/`tuple`/`union`/`discriminated_union`/`literal`
constructors and their typed builders, the three object modes, the array-length
validators, recursive parse descent with correct path construction, union error
aggregation, and O(1) discriminated-union lookup — all built on F2's dispatch
seam with no new parse-flow control logic — ready for
`PLAN_T3_object_utilities` to add `partial`/`extend`/`omit*`/`strip*` and for
`PLAN_T4_special_types` to add `buffer`/`uri`/`url`/`json`/`jsonschema`.

<plan_ready>docs/PLAN_T2_complex_types.md</plan_ready>
