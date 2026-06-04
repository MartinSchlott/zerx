// ZerxError is intentionally structured (144 B) for rich diagnostics; boxing
// it everywhere would worsen ergonomics for callers who always handle the Ok path.
#![allow(clippy::result_large_err)]

use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Serialize, Serializer};

use crate::{ErrorCode, ZerxError};

// ---------------------------------------------------------------------------
// Open error-code catalogue entry for this module (no edit to error.rs)
// ---------------------------------------------------------------------------

impl ErrorCode {
    pub const SERIALIZATION_FAILED: ErrorCode = ErrorCode::new("serialization_failed");
}

// ---------------------------------------------------------------------------
// serde::ser::Error for ZerxError
// ---------------------------------------------------------------------------

impl serde::ser::Error for ZerxError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        ZerxError::new(ErrorCode::SERIALIZATION_FAILED, msg.to_string())
    }
}

// ---------------------------------------------------------------------------
// Map — ordered object map (insertion-order preserved, last-write-wins)
// ---------------------------------------------------------------------------

/// Ordered key-value map used for `ZerxValue::Object`. Preserves insertion
/// order; duplicate keys are overwritten in place (last-write-wins, order of
/// first occurrence kept).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Map(Vec<(String, ZerxValue)>);

impl Map {
    pub fn new() -> Self {
        Map(Vec::new())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: ZerxValue) {
        let key = key.into();
        if let Some(entry) = self.0.iter_mut().find(|(k, _)| k == &key) {
            entry.1 = value;
        } else {
            self.0.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&ZerxValue> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, ZerxValue)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, ZerxValue)> for Map {
    fn from_iter<I: IntoIterator<Item = (String, ZerxValue)>>(iter: I) -> Self {
        let mut map = Map::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Host-opaque placeholder (feature-gated, uninhabited)
// ---------------------------------------------------------------------------

/// Uninhabited placeholder for the host-opaque variant. Replaced by
/// `PLAN_M1_host_opaque` with the real mlua-backed type.
#[cfg(feature = "mlua")]
#[derive(Debug, Clone, PartialEq)]
pub enum HostOpaque {}

// ---------------------------------------------------------------------------
// ZerxValue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum ZerxValue {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    I128(i128),
    U128(u128),
    F64(f64),
    String(std::string::String),
    /// First-class buffer variant. Only produced by `serialize_bytes` (C2).
    Bytes(Vec<u8>),
    Array(Vec<ZerxValue>),
    Object(Map),
    /// Uninhabited host-opaque placeholder (C3). Realised by `PLAN_M1_host_opaque`.
    #[cfg(feature = "mlua")]
    HostOpaque(HostOpaque),
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

impl ZerxValue {
    pub fn is_null(&self) -> bool {
        matches!(self, ZerxValue::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ZerxValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ZerxValue::I64(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            ZerxValue::U64(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i128(&self) -> Option<i128> {
        match self {
            ZerxValue::I128(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u128(&self) -> Option<u128> {
        match self {
            ZerxValue::U128(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ZerxValue::F64(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ZerxValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            ZerxValue::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[ZerxValue]> {
        match self {
            ZerxValue::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            ZerxValue::Object(m) => Some(m),
            _ => None,
        }
    }

    /// Object key lookup. Returns `None` for non-objects.
    pub fn get(&self, key: &str) -> Option<&ZerxValue> {
        self.as_object()?.get(key)
    }

    /// Array index lookup. Returns `None` for non-arrays or out-of-range.
    pub fn get_index(&self, index: usize) -> Option<&ZerxValue> {
        self.as_array()?.get(index)
    }
}

// ---------------------------------------------------------------------------
// From impls (ergonomic construction, no From<Vec<u8>> per C2)
// ---------------------------------------------------------------------------

impl From<bool> for ZerxValue {
    fn from(v: bool) -> Self {
        ZerxValue::Bool(v)
    }
}
impl From<i64> for ZerxValue {
    fn from(v: i64) -> Self {
        ZerxValue::I64(v)
    }
}
impl From<u64> for ZerxValue {
    fn from(v: u64) -> Self {
        ZerxValue::U64(v)
    }
}
impl From<i128> for ZerxValue {
    fn from(v: i128) -> Self {
        ZerxValue::I128(v)
    }
}
impl From<u128> for ZerxValue {
    fn from(v: u128) -> Self {
        ZerxValue::U128(v)
    }
}
impl From<f64> for ZerxValue {
    fn from(v: f64) -> Self {
        ZerxValue::F64(v)
    }
}
impl From<std::string::String> for ZerxValue {
    fn from(v: std::string::String) -> Self {
        ZerxValue::String(v)
    }
}
impl From<&str> for ZerxValue {
    fn from(v: &str) -> Self {
        ZerxValue::String(v.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Serialize for ZerxValue (serde out)
// ---------------------------------------------------------------------------

impl Serialize for ZerxValue {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            ZerxValue::Null => ser.serialize_unit(),
            ZerxValue::Bool(b) => ser.serialize_bool(*b),
            ZerxValue::I64(n) => ser.serialize_i64(*n),
            ZerxValue::U64(n) => ser.serialize_u64(*n),
            ZerxValue::I128(n) => ser.serialize_i128(*n),
            ZerxValue::U128(n) => ser.serialize_u128(*n),
            ZerxValue::F64(n) => ser.serialize_f64(*n),
            ZerxValue::String(s) => ser.serialize_str(s),
            // Preserves bytes on byte-aware formats; degrades to number array on JSON (C2).
            ZerxValue::Bytes(b) => ser.serialize_bytes(b),
            ZerxValue::Array(arr) => {
                let mut seq = ser.serialize_seq(Some(arr.len()))?;
                for item in arr {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            ZerxValue::Object(map) => {
                let mut m = ser.serialize_map(Some(map.len()))?;
                for (k, v) in map.iter() {
                    m.serialize_entry(k, v)?;
                }
                m.end()
            }
            // Uninhabited placeholder: statically unreachable in F1. When M1 realises
            // this variant with a real mlua type it MUST produce a serialisation error
            // rather than a silent roundtrip (C3/C5).
            #[cfg(feature = "mlua")]
            ZerxValue::HostOpaque(h) => match *h {},
        }
    }
}

// ---------------------------------------------------------------------------
// Serde-in bridge: ZerxValue::from_serialize
// ---------------------------------------------------------------------------

impl ZerxValue {
    pub fn from_serialize<T: serde::Serialize + ?Sized>(
        value: &T,
    ) -> Result<ZerxValue, ZerxError> {
        value.serialize(ValueSerializer)
    }
}

// --- map key serializer (mirrors serde_json MapKeySerializer) ---

struct MapKeySerializer;

impl Serializer for MapKeySerializer {
    type Ok = std::string::String;
    type Error = ZerxError;

    type SerializeSeq = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeTuple = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeTupleStruct = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeTupleVariant = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeMap = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeStruct = serde::ser::Impossible<std::string::String, ZerxError>;
    type SerializeStructVariant = serde::ser::Impossible<std::string::String, ZerxError>;

    fn serialize_str(self, v: &str) -> Result<std::string::String, ZerxError> {
        Ok(v.to_owned())
    }
    fn serialize_bool(self, v: bool) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_i8(self, v: i8) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_i128(self, v: i128) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_u128(self, v: u128) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_f32(self, v: f32) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_f64(self, v: f64) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_char(self, v: char) -> Result<std::string::String, ZerxError> {
        Ok(v.to_string())
    }
    fn serialize_unit(self) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom("unit cannot be a map key"))
    }
    fn serialize_unit_struct(
        self,
        _name: &'static str,
    ) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom("unit struct cannot be a map key"))
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<std::string::String, ZerxError> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<std::string::String, ZerxError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom(
            "newtype variant cannot be a map key",
        ))
    }
    fn serialize_none(self) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom("None cannot be a map key"))
    }
    fn serialize_some<T: Serialize + ?Sized>(
        self,
        _value: &T,
    ) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom("Some cannot be a map key"))
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<std::string::String, ZerxError> {
        Err(serde::ser::Error::custom("bytes cannot be a map key"))
    }
    fn serialize_seq(
        self,
        _len: Option<usize>,
    ) -> Result<Self::SerializeSeq, ZerxError> {
        Err(serde::ser::Error::custom("sequence cannot be a map key"))
    }
    fn serialize_tuple(
        self,
        _len: usize,
    ) -> Result<Self::SerializeTuple, ZerxError> {
        Err(serde::ser::Error::custom("tuple cannot be a map key"))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, ZerxError> {
        Err(serde::ser::Error::custom("tuple struct cannot be a map key"))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, ZerxError> {
        Err(serde::ser::Error::custom(
            "tuple variant cannot be a map key",
        ))
    }
    fn serialize_map(
        self,
        _len: Option<usize>,
    ) -> Result<Self::SerializeMap, ZerxError> {
        Err(serde::ser::Error::custom("map cannot be a map key"))
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, ZerxError> {
        Err(serde::ser::Error::custom("struct cannot be a map key"))
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, ZerxError> {
        Err(serde::ser::Error::custom(
            "struct variant cannot be a map key",
        ))
    }
}

// --- main Serializer ---

struct ValueSerializer;

impl Serializer for ValueSerializer {
    type Ok = ZerxValue;
    type Error = ZerxError;

    type SerializeSeq = SeqBuilder;
    type SerializeTuple = SeqBuilder;
    type SerializeTupleStruct = SeqBuilder;
    type SerializeTupleVariant = TupleVariantBuilder;
    type SerializeMap = MapBuilder;
    type SerializeStruct = MapBuilder;
    type SerializeStructVariant = StructVariantBuilder;

    fn serialize_bool(self, v: bool) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::I64(v as i64))
    }
    fn serialize_i16(self, v: i16) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::I64(v as i64))
    }
    fn serialize_i32(self, v: i32) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::I64(v as i64))
    }
    fn serialize_i64(self, v: i64) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::I64(v))
    }
    fn serialize_i128(self, v: i128) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::I128(v))
    }
    fn serialize_u8(self, v: u8) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::U64(v as u64))
    }
    fn serialize_u16(self, v: u16) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::U64(v as u64))
    }
    fn serialize_u32(self, v: u32) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::U64(v as u64))
    }
    fn serialize_u64(self, v: u64) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::U64(v))
    }
    fn serialize_u128(self, v: u128) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::U128(v))
    }
    fn serialize_f32(self, v: f32) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::F64(v as f64))
    }
    fn serialize_f64(self, v: f64) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::F64(v))
    }
    fn serialize_char(self, v: char) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::String(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::String(v.to_owned()))
    }
    // C2: native serialize_bytes → first-class Bytes variant.
    fn serialize_bytes(self, v: &[u8]) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Bytes(v.to_vec()))
    }
    fn serialize_unit(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Null)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Null)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::String(variant.to_owned()))
    }
    fn serialize_none(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(
        self,
        value: &T,
    ) -> Result<ZerxValue, ZerxError> {
        value.serialize(self)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<ZerxValue, ZerxError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<ZerxValue, ZerxError> {
        let inner = value.serialize(ValueSerializer)?;
        let mut map = Map::new();
        map.insert(variant, inner);
        Ok(ZerxValue::Object(map))
    }
    fn serialize_seq(
        self,
        _len: Option<usize>,
    ) -> Result<Self::SerializeSeq, ZerxError> {
        Ok(SeqBuilder(Vec::new()))
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, ZerxError> {
        Ok(SeqBuilder(Vec::new()))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, ZerxError> {
        Ok(SeqBuilder(Vec::new()))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, ZerxError> {
        Ok(TupleVariantBuilder {
            variant: variant.to_owned(),
            items: Vec::new(),
        })
    }
    fn serialize_map(
        self,
        _len: Option<usize>,
    ) -> Result<Self::SerializeMap, ZerxError> {
        Ok(MapBuilder {
            map: Map::new(),
            next_key: None,
        })
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, ZerxError> {
        Ok(MapBuilder {
            map: Map::new(),
            next_key: None,
        })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, ZerxError> {
        Ok(StructVariantBuilder {
            variant: variant.to_owned(),
            map: Map::new(),
        })
    }
}

// --- sequence helper ---

struct SeqBuilder(Vec<ZerxValue>);

impl SerializeSeq for SeqBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ZerxError> {
        self.0.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Array(self.0))
    }
}

impl SerializeTuple for SeqBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ZerxError> {
        self.0.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Array(self.0))
    }
}

impl SerializeTupleStruct for SeqBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ZerxError> {
        self.0.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Array(self.0))
    }
}

// --- tuple variant helper ---

struct TupleVariantBuilder {
    variant: std::string::String,
    items: Vec<ZerxValue>,
}

impl SerializeTupleVariant for TupleVariantBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ZerxError> {
        self.items.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        let mut map = Map::new();
        map.insert(self.variant, ZerxValue::Array(self.items));
        Ok(ZerxValue::Object(map))
    }
}

// --- map/struct helper ---

struct MapBuilder {
    map: Map,
    next_key: Option<std::string::String>,
}

impl SerializeMap for MapBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), ZerxError> {
        self.next_key = Some(key.serialize(MapKeySerializer)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ZerxError> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| serde::ser::Error::custom("map value without preceding key"))?;
        self.map.insert(key, value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Object(self.map))
    }
}

impl SerializeStruct for MapBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ZerxError> {
        self.map.insert(key, value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        Ok(ZerxValue::Object(self.map))
    }
}

// --- struct variant helper ---

struct StructVariantBuilder {
    variant: std::string::String,
    map: Map,
}

impl SerializeStructVariant for StructVariantBuilder {
    type Ok = ZerxValue;
    type Error = ZerxError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ZerxError> {
        self.map.insert(key, value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<ZerxValue, ZerxError> {
        let mut outer = Map::new();
        outer.insert(self.variant, ZerxValue::Object(self.map));
        Ok(ZerxValue::Object(outer))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    // Test-only blob type whose Serialize calls serialize_bytes (no serde_bytes dep).
    struct Blob(Vec<u8>);
    impl Serialize for Blob {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            ser.serialize_bytes(&self.0)
        }
    }

    // Test-only type whose Serialize always returns a custom error.
    struct Bad;
    impl Serialize for Bad {
        fn serialize<S: Serializer>(&self, _ser: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("boom"))
        }
    }

    #[derive(Serialize)]
    struct S {
        a: i64,
        b: i64,
        c: i64,
    }

    #[derive(Serialize)]
    enum E {
        Unit,
        Newtype(i64),
    }

    // 1. Scalars in
    #[test]
    fn scalars_in() {
        assert_eq!(ZerxValue::from_serialize(&true).unwrap(), ZerxValue::Bool(true));
        assert_eq!(ZerxValue::from_serialize(&7i32).unwrap(), ZerxValue::I64(7));
        assert_eq!(ZerxValue::from_serialize(&7u8).unwrap(), ZerxValue::U64(7));
        assert_eq!(ZerxValue::from_serialize(&2.5f64).unwrap(), ZerxValue::F64(2.5));
        assert_eq!(
            ZerxValue::from_serialize(&'x').unwrap(),
            ZerxValue::String("x".into())
        );
        assert_eq!(
            ZerxValue::from_serialize("hi").unwrap(),
            ZerxValue::String("hi".into())
        );
        assert_eq!(ZerxValue::from_serialize(&()).unwrap(), ZerxValue::Null);
        assert_eq!(
            ZerxValue::from_serialize(&None::<i32>).unwrap(),
            ZerxValue::Null
        );
        assert_eq!(
            ZerxValue::from_serialize(&Some(5i64)).unwrap(),
            ZerxValue::I64(5)
        );
    }

    // 1b. 128-bit integers (C5)
    #[test]
    fn ints_128bit() {
        let i = i128::MAX;
        let u = u128::MAX;
        assert_eq!(ZerxValue::from_serialize(&i).unwrap(), ZerxValue::I128(i128::MAX));
        assert_eq!(ZerxValue::from_serialize(&u).unwrap(), ZerxValue::U128(u128::MAX));
        // Self-roundtrip via serialize out → in
        assert_eq!(
            ZerxValue::from_serialize(&ZerxValue::I128(i128::MAX)).unwrap(),
            ZerxValue::I128(i128::MAX)
        );
        assert_eq!(
            ZerxValue::from_serialize(&ZerxValue::U128(u128::MAX)).unwrap(),
            ZerxValue::U128(u128::MAX)
        );
    }

    // 2. Bytes fidelity — buffer side (C2)
    #[test]
    fn bytes_fidelity_buffer() {
        let v = ZerxValue::from_serialize(&Blob(vec![1, 2, 3])).unwrap();
        assert_eq!(v, ZerxValue::Bytes(vec![1, 2, 3]));
    }

    // 3. Bytes footgun — array side (C2)
    #[test]
    fn bytes_footgun_array() {
        let v = ZerxValue::from_serialize(&vec![1u8, 2, 3]).unwrap();
        assert_eq!(
            v,
            ZerxValue::Array(vec![
                ZerxValue::U64(1),
                ZerxValue::U64(2),
                ZerxValue::U64(3)
            ])
        );
    }

    // 4. Object order preserved
    #[test]
    fn object_order_preserved() {
        let v = ZerxValue::from_serialize(&S { a: 1, b: 2, c: 3 }).unwrap();
        let keys: Vec<&str> = v
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    // 5. Externally-tagged enum
    #[test]
    fn externally_tagged_enum() {
        let unit = ZerxValue::from_serialize(&E::Unit).unwrap();
        assert_eq!(unit, ZerxValue::String("Unit".into()));

        let newtype = ZerxValue::from_serialize(&E::Newtype(42)).unwrap();
        let obj = newtype.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert_eq!(obj.get("Newtype"), Some(&ZerxValue::I64(42)));
    }

    // 6. Serialize out → serde_json
    #[test]
    fn serialize_out_to_json() {
        let mut map = Map::new();
        map.insert("k", ZerxValue::Bool(true));
        let v = serde_json::to_value(ZerxValue::Object(map)).unwrap();
        assert_eq!(v["k"], serde_json::json!(true));

        assert_eq!(
            serde_json::to_value(ZerxValue::I64(7)).unwrap(),
            serde_json::json!(7)
        );
        assert_eq!(
            serde_json::to_value(ZerxValue::String("hi".into())).unwrap(),
            serde_json::json!("hi")
        );
        assert_eq!(
            serde_json::to_value(ZerxValue::Bool(false)).unwrap(),
            serde_json::json!(false)
        );
        assert_eq!(
            serde_json::to_value(ZerxValue::Array(vec![ZerxValue::U64(1)])).unwrap(),
            serde_json::json!([1])
        );
        // Bytes degrade to number array on JSON (C2 documented degradation)
        assert_eq!(
            serde_json::to_value(ZerxValue::Bytes(vec![1, 2, 3])).unwrap(),
            serde_json::json!([1, 2, 3])
        );
    }

    // 7. Bytes self-roundtrip (out → in)
    #[test]
    fn bytes_self_roundtrip() {
        let original = ZerxValue::Bytes(vec![9, 8, 7]);
        let roundtripped = ZerxValue::from_serialize(&original).unwrap();
        assert_eq!(roundtripped, ZerxValue::Bytes(vec![9, 8, 7]));
    }

    // 8. Accessors
    #[test]
    fn accessors() {
        assert!(ZerxValue::Null.is_null());
        assert!(!ZerxValue::Bool(true).is_null());

        assert_eq!(ZerxValue::Bool(true).as_bool(), Some(true));
        assert_eq!(ZerxValue::I64(1).as_bool(), None);

        assert_eq!(ZerxValue::I64(-1).as_i64(), Some(-1));
        assert_eq!(ZerxValue::U64(1).as_i64(), None);

        assert_eq!(ZerxValue::U64(5).as_u64(), Some(5));
        assert_eq!(ZerxValue::I64(5).as_u64(), None);

        assert_eq!(ZerxValue::F64(1.5).as_f64(), Some(1.5));
        assert_eq!(ZerxValue::I64(1).as_f64(), None);

        assert_eq!(
            ZerxValue::String("s".into()).as_str(),
            Some("s")
        );
        assert_eq!(ZerxValue::I64(1).as_str(), None);

        assert_eq!(
            ZerxValue::Bytes(vec![0]).as_bytes(),
            Some([0u8].as_slice())
        );
        assert_eq!(ZerxValue::I64(1).as_bytes(), None);

        let arr = ZerxValue::Array(vec![ZerxValue::Null]);
        assert!(arr.as_array().is_some());
        assert_eq!(ZerxValue::Null.as_array(), None);

        let mut m = Map::new();
        m.insert("k", ZerxValue::Bool(true));
        let obj = ZerxValue::Object(m);
        assert!(obj.as_object().is_some());
        assert_eq!(ZerxValue::Null.as_object(), None);

        assert_eq!(obj.get("k"), Some(&ZerxValue::Bool(true)));
        assert_eq!(obj.get("x"), None);
        assert_eq!(ZerxValue::Null.get("k"), None);

        let arr2 = ZerxValue::Array(vec![ZerxValue::I64(10), ZerxValue::I64(20)]);
        assert_eq!(arr2.get_index(0), Some(&ZerxValue::I64(10)));
        assert_eq!(arr2.get_index(5), None);
        assert_eq!(ZerxValue::Null.get_index(0), None);
    }

    // 9. Map semantics
    #[test]
    fn map_semantics() {
        let mut m = Map::new();
        m.insert("x", ZerxValue::I64(1));
        m.insert("y", ZerxValue::I64(2));
        m.insert("x", ZerxValue::I64(99)); // overwrite in place
        assert_eq!(m.len(), 2);
        assert_eq!(m.get("x"), Some(&ZerxValue::I64(99)));
        // Order of first occurrence preserved: x then y
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["x", "y"]);
        assert_eq!(m.get("absent"), None);
    }

    // 10. Serialisation error → ZerxError
    #[test]
    fn serialization_error() {
        let result = ZerxValue::from_serialize(&Bad);
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::SERIALIZATION_FAILED);
        assert!(err.message.contains("boom"));
    }
}
