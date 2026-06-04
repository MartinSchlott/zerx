# PLAN_F1_value_model

## Context & Goal

Second plan of the `CONCEPT_zerx_foundation` execution (Phase 1 — Foundation).
It implements the `value-model` concern: `ZerxValue`, the validated dynamic value
that spans serde's *full* data model (including bytes) plus a host-opaque layer,
and the **serde bridge** in both directions.

This plan delivers the value type and its serde bridge only. It does **not**
deliver validation, schema shape, or JSON Schema representation — those are
`schema-core` (`PLAN_F2_schema_core`) and `json-schema` respectively. The serde
bridge is the primitive on which `Schema::validate<T: Serialize>` (F2) is later
built: F1 turns any `T: Serialize` into a `ZerxValue`; F2 walks a `Schema` and
produces the validated `ZerxValue`.

The concern boundary is fixed in `docs/architecture/value-model.md`:

- This concern owns `ZerxValue` and the two-layer value model (serde-bridgeable
  + host-opaque).
- This concern owns the serde bridge — accepting any `T: Serialize` (serialise
  **in**) and serialising any `ZerxValue` to any serde format (serialise **out**).
- This concern does **NOT** own validation rules or schema shape (`schema-core`),
  nor JSON Schema representation (`json-schema`).

Three candidate decisions of `CONCEPT_zerx_foundation.md` govern this plan:

- **C5 — `ZerxValue` spans the full serde data model.** Two layers:
  serde-bridgeable (full roundtrip through any serde format) and host-opaque
  (validatable, not serde-roundtrip-capable). This decision is the umbrella over
  C2 and C3.
- **C2 — Buffer fidelity via serde bytes, no coercion.** Bytes are a first-class
  `ZerxValue` variant fed by serde's native `serialize_bytes` path and stored as
  `Vec<u8>`. zerx does **not** coerce JSON-style number arrays into buffers, and
  does **not** depend on `serde_bytes`. A plain `Vec<u8>` field arrives as a
  number array (a documented caller-side contract); only a source emitting
  `serialize_bytes` (e.g. `serde_bytes::ByteBuf` on the caller's own types)
  produces a buffer.
- **C3 — Host-opaque values are borrowed handles, feature-gated.** The
  host-opaque variant is behind the optional `mlua` feature and absent from the
  default build. It never enters the serde roundtrip. Per C3, *"the precise
  handle lifetime mechanism does not block the foundation plans —
  `PLAN_F1_value_model` defines the gated variant as an opaque placeholder."*
  `PLAN_M1_host_opaque` (Phase 5) realises it.

`PLAN_F3_error_model` is complete and archived; its `ZerxError` and `ErrorCode`
(with the **open code catalogue**) are present in the crate. F1 builds on them:
the serde-in bridge returns `Result<ZerxValue, ZerxError>`, and a single new
serialisation error code is added via the open-catalogue mechanism (an
`impl ErrorCode` block in the new module — no edit to `src/error.rs`).

## Breaking Changes

**No.** This plan adds a new module (`src/value.rs`) and re-exports; it does not
change the existing public API and does not modify `src/error.rs`. It adds one
new, empty Cargo feature (`mlua`) that is off by default.

## Dependencies

No new crates. Per candidate decision C9 (*Built on serde, lean beyond it*), this
plan uses only the dependencies already declared in `Cargo.toml`:

- `serde` (with `derive`) — the `Serializer`/`Serialize` traits for the bridge.
- `serde_json` — used only in tests (asserting serialise-out shape).

In particular **no `serde_bytes`** (C2/C9) and **no `mlua`** (C9 — `mlua` is the
optional dependency wired by `PLAN_M1_host_opaque` in Phase 5). This plan adds an
**empty** Cargo feature `mlua = []` so the gated placeholder variant compiles
under `--features mlua`; `PLAN_M1_host_opaque` later changes this entry to enable
the optional `mlua` dependency. Declaring an empty feature here is a build-config
change, not a new package, and is within scope.

## Reference Patterns

- **`serde_json::value::Serializer` and `serde_json::Value`** (the
  `serde_json::to_value` machinery) — the template for both halves of the bridge:
  the `Serializer` that builds a value tree, and the hand-written `Serialize`
  impl on the value enum. zerx mirrors this design structurally but deviates in
  **two** places, both mandated by candidate decisions:
  - `serialize_bytes` produces a first-class `Bytes` variant instead of being
    mapped to a sequence of integers (C2).
  - `serialize_i128`/`serialize_u128` are overridden to produce first-class
    `I128`/`U128` variants instead of `serde_json`'s erroring default methods, so
    the full serde integer range is carried (C5; see Assumptions).
- `src/error.rs` — the open-`ErrorCode`-catalogue pattern and the `ZerxError`
  builder style; F1 adds its serialisation code the same way (`impl ErrorCode`
  block in the new module).
- `docs/vision.md` lines 76–97 (the "bastard extension of serde" two-layer model)
  and `README.md` line 45 ("serde in, serde out"); accessor naming (`as_str`,
  `as_u64`, `get`) follows the vision's `refine` example (`docs/vision.md:213`).

## Assumptions & Risks

- **Number model carries the full serde integer range (C5).** Signed integer
  types `i8..i64` funnel to `I64` and `i128` to a dedicated `I128`; unsigned
  `u8..u64` funnel to `U64` and `u128` to `U128`; floats (`f32`,`f64`) to `F64`;
  `char` to a one-character `String`. 128-bit integers are first-class variants,
  **not** a serialisation error: C5 binds `ZerxValue` to serde's *full* data
  model, and serde's model includes `i128`/`u128`; mapping them to an error would
  narrow `ZerxValue` to "`serde_json::Value` + bytes", which is exactly the
  narrower model C5 rejects. How 128-bit integers degrade on the JSON projection
  is a `json-schema` concern (a later plan), not a reason to drop them from the
  value model here.
- **Object key order is preserved** via a small ordered `Map` newtype over
  `Vec<(String, ZerxValue)>` (no `indexmap` dependency, per C9). Insertion order
  is preserved; duplicate keys are last-write-wins (JSON object semantics).
  `get` is O(n) over entries; per the concept, micro-optimisation is out of scope
  for v1 (object sizes are small; the concept's "O(1) lookup" requirement is
  schema-level discriminator/key lookup, not value-level). Rationale: a sorted
  `BTreeMap` would reorder fields and break the natural correspondence between
  serialise-in field order (and later, schema field order) and output order,
  hurting roundtrip fidelity.
- **Bytes fidelity is a caller-side contract (C2).** A plain `Vec<u8>` arrives as
  a number array; only `serialize_bytes` yields `Bytes`. This is a known footgun;
  tests assert **both** sides so the failure mode is legible.
- **Host-opaque is an uninhabited placeholder in F1.** The gated variant exists so
  the value model is structurally complete, but its inner type is an
  unconstructable placeholder — no host-opaque `ZerxValue` can be built in F1, in
  either the default or the `--features mlua` build. This matches C3 (*"In the
  default build no host-opaque values exist"*) and leaves the handle mechanism to
  `PLAN_M1_host_opaque`, which replaces the placeholder with the real
  `mlua`-backed type.

## Design

### Module layout

- New file `src/value.rs` declaring `ZerxValue`, the ordered `Map`, the serde
  bridge, the new error code, and the `serde::ser::Error` impl.
- `src/lib.rs` gains `mod value;` and `pub use value::{ZerxValue, Map};`. No
  other change to `lib.rs`; `src/error.rs` is untouched.

### `ZerxValue`

An enum over the serde data model (collapsed the way `serde_json::Value` collapses
it) **plus** bytes, **plus** the gated host-opaque layer:

- `Null`
- `Bool(bool)`
- `I64(i64)`
- `U64(u64)`
- `I128(i128)`
- `U128(u128)`
- `F64(f64)`
- `String(String)`
- `Bytes(Vec<u8>)` — the C2 first-class buffer variant.
- `Array(Vec<ZerxValue>)`
- `Object(Map)`
- `#[cfg(feature = "mlua")] HostOpaque(HostOpaque)` — gated placeholder (below).

Derives: `Debug`, `Clone`, `PartialEq`. **Not** `Eq` (holds `f64`). **No**
`Serialize` derive (the impl is hand-written, below) and **no** `Deserialize`
(YAGNI for v1, mirroring `ZerxError`).

### `Map` — ordered object map

A public newtype `Map(Vec<(String, ZerxValue)>)` providing:

- `new()` / `Default`.
- `insert(key: impl Into<String>, value: ZerxValue)` — appends, or overwrites the
  existing entry with that key in place (last-write-wins, order of first
  occurrence preserved).
- `get(&self, key: &str) -> Option<&ZerxValue>` — linear scan.
- `iter()` — entries in insertion order.
- `len()`, `is_empty()`.
- `FromIterator<(String, ZerxValue)>` for ergonomic construction.

Derives: `Debug`, `Clone`, `PartialEq`, `Default`.

### Accessors (read API of `ZerxValue`)

Borrowing accessors, each returning `Option`/`bool`, one per inspectable variant
plus container navigation:

- `is_null(&self) -> bool`
- `as_bool(&self) -> Option<bool>`
- `as_i64(&self) -> Option<i64>`
- `as_u64(&self) -> Option<u64>`
- `as_i128(&self) -> Option<i128>`
- `as_u128(&self) -> Option<u128>`
- `as_f64(&self) -> Option<f64>`
- `as_str(&self) -> Option<&str>`
- `as_bytes(&self) -> Option<&[u8]>`
- `as_array(&self) -> Option<&[ZerxValue]>`
- `as_object(&self) -> Option<&Map>`
- `get(&self, key: &str) -> Option<&ZerxValue>` — object key lookup (`None` for
  non-objects).
- `get_index(&self, index: usize) -> Option<&ZerxValue>` — array index lookup
  (`None` for non-arrays / out of range).

These are the value type's fundamental read surface, owned by `value-model` and
relied on by later plans (e.g. `refine` closures in `schema-core`). The
host-opaque variant (when present) returns `None`/`false` from every accessor.

### `From` impls (minimal, ergonomic construction)

- `From<bool>`, `From<i64>`, `From<u64>`, `From<i128>`, `From<u128>`,
  `From<f64>`, `From<String>`, `From<&str>`.
- **No** `From<Vec<u8>>` — it would invite exactly the array/bytes ambiguity C2
  forbids. `Bytes` is constructed explicitly via the variant. Arrays/objects are
  constructed via their variants / `Map`.

### Serde bridge — **in** (`T: Serialize` → `ZerxValue`)

- Public entry point: `ZerxValue::from_serialize<T>(value: &T) -> Result<ZerxValue, ZerxError>`
  where `T: serde::Serialize + ?Sized`.
- Internally, a private zero-sized `Serializer` whose associated `Error` is
  `ZerxError`, with the standard `SerializeSeq`/`SerializeTuple`/
  `SerializeTupleStruct`/`SerializeTupleVariant`/`SerializeMap`/`SerializeStruct`/
  `SerializeStructVariant` helper types building `Array`/`Object`. This mirrors
  `serde_json::value::Serializer` exactly, with these fixed mappings:
  - **`serialize_bytes(v)` → `ZerxValue::Bytes(v.to_vec())`** — the C2 deviation
    from `serde_json` (which would map bytes to a sequence).
  - Signed ints `i8..i64` → `I64`; `serialize_i128` → `I128`; unsigned `u8..u64`
    → `U64`; `serialize_u128` → `U128`; floats (`f32`,`f64`) → `F64`; `char` →
    one-character `String`. `serialize_i128`/`serialize_u128` are overridden (not
    left to serde's erroring defaults) so the full serde integer range is carried
    per C5.
  - `unit`, `unit_struct`, `None` → `Null`. `Some(x)` and `newtype_struct(x)` →
    the serialised inner value.
  - Enums mirror `serde_json`'s externally-tagged form: `unit_variant` → the
    variant-name `String`; `newtype_variant`/`tuple_variant`/`struct_variant` →
    a single-key `Object` mapping the variant name to the inner value/array/object.
  - Map keys go through a key serializer that mirrors `serde_json`'s
    `MapKeySerializer`: string and stringifiable-primitive keys become the object
    key string; compound keys produce a serialisation error.

### Serde bridge — **out** (`impl Serialize for ZerxValue`)

Hand-written (not derived) so bytes route through `serialize_bytes`:

- `Null` → `serialize_unit`; `Bool`/`I64`/`U64`/`F64` → the matching scalar
  method; `I128` → `serialize_i128`; `U128` → `serialize_u128`; `String` →
  `serialize_str`; `Array` → `serialize_seq`; `Object` → `serialize_map`
  (entries in stored order).
- **`Bytes(v)` → `serializer.serialize_bytes(v)`** — preserves bytes on
  byte-aware formats (msgpack, bincode) and degrades to a number array on JSON
  (C2/C5).
- `#[cfg(feature = "mlua")]` `HostOpaque` arm: host-opaque values are **not**
  serde-roundtrip-capable (C3/C5). Because the F1 placeholder is uninhabited the
  arm is statically unreachable (`match` on the never value); it carries a doc
  comment recording that, if the realised type ever reaches here, it MUST be a
  serialisation error rather than a silent roundtrip. `PLAN_M1_host_opaque` makes
  this concrete when it realises the variant.

### `serde::ser::Error for ZerxError`

To let the serde-in bridge surface failures as `ZerxError`, implement
`serde::ser::Error for ZerxError` in `src/value.rs`: `custom(msg)` →
`ZerxError::new(ErrorCode::SERIALIZATION_FAILED, msg.to_string())`. (`ZerxError`
already implements `Display` + `std::error::Error`, the other `ser::Error`
bounds.)

Add the code via the open catalogue (no edit to `src/error.rs`):

```rust
impl ErrorCode {
    pub const SERIALIZATION_FAILED: ErrorCode = ErrorCode::new("serialization_failed");
}
```

Placement rationale: the serde bridge is owned by `value-model`, so the trait
impl and its code live with the bridge, not in the `errors` concern's file. This
respects F3's open-catalogue contract (each module declares its own codes) and
keeps the completed `error.rs` untouched.

### `mlua` host-opaque placeholder

- `Cargo.toml` gains `[features]` with `mlua = []` (empty; no optional dependency
  yet).
- Under `#[cfg(feature = "mlua")]`, declare a private, **uninhabited** placeholder
  type (e.g. an empty enum) and the `ZerxValue::HostOpaque(HostOpaque)` variant
  wrapping it. Uninhabited means no host-opaque value can be constructed in F1,
  so every `match` arm handling it is statically unreachable and the default and
  `--features mlua` builds behave identically at runtime.
- `PLAN_M1_host_opaque` (Phase 5) replaces the placeholder with the real
  `mlua`-backed type, wires `mlua = ["dep:mlua"]`, and per C3 adds only the gated
  leaf-kind revalidation and the descend-into-leaf error. The `replace()`
  contract itself is **already fixed by C3** (replacement value is `T: Serialize`
  and thus never host-opaque; a JSON Pointer path MUST NOT descend into a
  host-opaque leaf; host-opaque leaves elsewhere are revalidated by kind on full
  root revalidation) — M1 does not re-decide it, and in the default build
  `replace()` is fully defined by `PLAN_D1_delta_replace`. F1 fixes none of the
  handle mechanism.

## Steps

1. `src/lib.rs`: add `mod value;` and `pub use value::{ZerxValue, Map};`.
2. `src/value.rs`: implement the ordered `Map` newtype (constructors, `insert`
   with last-write-wins, `get`, `iter`, `len`, `is_empty`, `FromIterator`,
   derives).
3. Implement `ZerxValue`: the enum (incl. the `#[cfg(feature = "mlua")]` gated
   placeholder variant and its uninhabited inner type), derives, accessors, and
   the `From` impls.
4. Implement the hand-written `impl Serialize for ZerxValue` (serialise out),
   including the gated unreachable host-opaque arm and the `serialize_bytes`
   routing.
5. Implement the serde-in bridge: the private `Serializer` and the
   `SerializeSeq`/`SerializeTuple`/`SerializeTupleStruct`/`SerializeTupleVariant`/
   `SerializeMap`/`SerializeStruct`/`SerializeStructVariant` helpers and the map
   key serializer, mirroring `serde_json::value::Serializer` with the C2
   `serialize_bytes` → `Bytes` deviation, plus the public
   `ZerxValue::from_serialize`.
6. Implement `serde::ser::Error for ZerxError` and add the
   `ErrorCode::SERIALIZATION_FAILED` associated constant (open catalogue) in
   `src/value.rs`.
7. `Cargo.toml`: add `[features]` with `mlua = []`.
8. Add unit tests (`#[cfg(test)] mod tests` in `src/value.rs`) per Verification.
9. Run `cargo build`, `cargo build --features mlua`, `cargo test`,
   `cargo test --features mlua`, `cargo clippy --all-targets`, and
   `cargo clippy --all-targets --features mlua`; resolve all warnings.

**Doc Update (workflow step 6, performed by the Coder after Validation — not
now):** fill `docs/architecture/value-model.md` Constraints with the
review-hardened normative contract delivered here (the variant set incl. bytes;
two-layer model; bytes fidelity via native `serialize_bytes` with no coercion and
no `serde_bytes` dep; host-opaque is feature-gated and never serde-roundtrips;
the serde bridge accepts any `T: Serialize` and serialises any `ZerxValue` out;
object key order is preserved). Per this concept's Decision-Reference Discipline,
this Doc Update fills **Constraints only** and MUST NOT add any `D-<slug>`
reference to `value-model.md`; the C1–C9 migration and all slug references are
routed to Concept Closeout. The binding candidate decisions for this plan during
execution are C5, C2, C3 in `CONCEPT_zerx_foundation.md`.

## Verification

Build & lint (all must be warning-free):

- `cargo build`
- `cargo build --features mlua`
- `cargo test`
- `cargo test --features mlua`
- `cargo clippy --all-targets`
- `cargo clippy --all-targets --features mlua`

Unit tests (each asserts behaviour, not mere existence). A test-only `Blob`
type with a hand-written `Serialize` that calls `serialize_bytes`, and a
test-only serde-derived `struct`, stand in for the caller-side contracts (no
`serde_bytes` dependency is added):

1. **Scalars in.** `from_serialize` of `true`, `7i32`, `7u8`, `2.5f64`, `'x'`,
   `"hi"`, `()`, `None::<i32>`, `Some(5i64)` yields `Bool(true)`, `I64(7)`,
   `U64(7)`, `F64(2.5)`, `String("x")`, `String("hi")`, `Null`, `Null`, `I64(5)`
   respectively.
   **128-bit (C5):** `from_serialize(&(i128::MAX))` yields `I128(i128::MAX)` and
   `from_serialize(&(u128::MAX))` yields `U128(u128::MAX)` — both carried, neither
   an error — and they self-roundtrip (`from_serialize(&ZerxValue::I128(i128::MAX))
   == ZerxValue::I128(i128::MAX)`, likewise for `U128`), exercising the
   `serialize_i128`/`serialize_u128` overrides on both bridge halves.
2. **Bytes fidelity (the buffer side, C2).** `from_serialize(&Blob(vec![1,2,3]))`
   — where `Blob`'s `Serialize` calls `serialize_bytes` — yields
   `ZerxValue::Bytes(vec![1,2,3])`.
3. **Bytes footgun (the array side, C2).** `from_serialize(&vec![1u8,2,3])` yields
   `Array([U64(1),U64(2),U64(3)])`, **not** `Bytes` — proving plain `Vec<u8>` is
   not coerced.
4. **Object order preserved.** `from_serialize` of a derived
   `struct S { a: i64, b: i64, c: i64 }` yields an `Object` whose `iter()` keys
   are exactly `["a","b","c"]` in that order.
5. **Externally-tagged enum.** `from_serialize` of a unit variant yields the
   variant-name `String`; of a newtype variant `V(x)` yields a single-key
   `Object {"V": <x>}` (locks the serde_json-parity enum mapping that later plans
   depend on).
6. **Serialise out → `serde_json`.** `serde_json::to_value(&ZerxValue::Object(..))`
   for a small object yields the expected `serde_json::Value`; an `I64`/`String`/
   `Bool`/`Array` produce their JSON counterparts; `ZerxValue::Bytes(vec![1,2,3])`
   produces the JSON array `[1,2,3]` (JSON has no bytes — the documented C2
   degradation).
7. **Bytes self-roundtrip (out → in).** `from_serialize(&ZerxValue::Bytes(vec![9,8,7]))`
   returns `ZerxValue::Bytes(vec![9,8,7])` — exercising the hand-written
   `Serialize` (which calls `serialize_bytes`) against the bridge's
   `serialize_bytes` (which builds `Bytes`), proving byte fidelity survives a
   full bytes-aware roundtrip using only zerx's own code.
8. **Accessors.** On representative values: `as_i64`/`as_u64`/`as_f64`/`as_bool`/
   `as_str`/`as_bytes`/`as_array`/`as_object` return `Some(..)` for the matching
   variant and `None` for a mismatching one; `is_null` is `true` only for `Null`;
   `get("k")` returns the entry for an object and `None` for a non-object;
   `get_index(0)` returns the element for an array and `None` for a non-array /
   out-of-range index.
9. **`Map` semantics.** `insert` twice with the same key overwrites in place
   (`len() == 1`, order of first occurrence kept); distinct keys preserve
   insertion order; `get` returns the stored value; `get` of an absent key is
   `None`.
10. **Serialisation error → `ZerxError`.** `from_serialize(&Bad)` — where `Bad`'s
    `Serialize` returns `Err(serde::ser::Error::custom("boom"))` — returns
    `Err(e)` with `e.code == ErrorCode::SERIALIZATION_FAILED` and `e.message`
    containing `"boom"`.

Expected outcome: a `zerx` crate exposing `ZerxValue` and `Map`, with a complete
serde bridge in both directions (bytes first-class per C2), a feature-gated
uninhabited host-opaque placeholder per C3, and the value model the full serde
data model spans per C5 — ready for `PLAN_F2_schema_core` to build the parse flow
on top.
