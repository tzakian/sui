// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    VARIANT_TAG_MAX_VALUE,
    account_address::AccountAddress,
    annotated_visitor::{Error as VError, ValueDriver, Visitor, visit_struct, visit_value},
    identifier::Identifier,
    language_storage::{StructTag, TypeTag},
    runtime_value::{self as R, MOVE_STRUCT_FIELDS, MOVE_STRUCT_TYPE},
    u256,
};
use anyhow::Result as AResult;
use serde::{
    Deserialize, Serialize,
    de::Error as DeError,
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
};
use std::{
    collections::BTreeMap,
    fmt::{self, Debug},
    io::Cursor,
};

/// In the `WithTypes` configuration, a Move struct gets serialized into a Serde struct with this name
pub const MOVE_STRUCT_NAME: &str = "struct";

/// In the `WithTypes` configuration, a Move enum/struct gets serialized into a Serde struct with this as the first field
pub const MOVE_DATA_TYPE: &str = "type";

/// In the `WithTypes` configuration, a Move struct gets serialized into a Serde struct with this as the second field
pub const MOVE_DATA_FIELDS: &str = "fields";

/// In the `WithTypes` configuration, a Move enum gets serialized into a Serde struct with this as the second field
/// In the `WithFields` configuration, this is the first field of the serialized enum
pub const MOVE_VARIANT_NAME: &str = "variant_name";

/// Field name for the tag of the variant
pub const MOVE_VARIANT_TAG_NAME: &str = "variant_tag";

/// In the `WithTypes` configuration, a Move enum gets serialized into a Serde struct with this name
pub const MOVE_ENUM_NAME: &str = "enum";

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct MoveStruct {
    pub type_: StructTag,
    pub fields: Vec<(Identifier, MoveValue)>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct MoveVariant {
    pub type_: StructTag,
    pub variant_name: Identifier,
    pub tag: u16,
    pub fields: Vec<(Identifier, MoveValue)>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum MoveValue {
    U8(u8),
    U64(u64),
    U128(u128),
    Bool(bool),
    Address(AccountAddress),
    Vector(Vec<MoveValue>),
    Struct(MoveStruct),
    Signer(AccountAddress),
    // NOTE: Added in bytecode version v6, do not reorder!
    U16(u16),
    U32(u32),
    U256(u256::U256),
    Variant(MoveVariant),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveFieldLayout {
    pub name: Identifier,
    pub layout: MoveTypeLayout,
}

impl MoveFieldLayout {
    pub fn new(name: Identifier, layout: MoveTypeLayout) -> Self {
        Self { name, layout }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveStructLayout {
    /// An decorated representation with both types and human-readable field names
    pub type_: StructTag,
    pub fields: Vec<MoveFieldLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveEnumLayout {
    pub type_: StructTag,
    pub variants: BTreeMap<(Identifier, u16), Vec<MoveFieldLayout>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MoveDatatypeLayout {
    Struct(Box<MoveStructLayout>),
    Enum(Box<MoveEnumLayout>),
}

impl MoveDatatypeLayout {
    pub fn into_layout(self) -> MoveTypeLayout {
        match self {
            Self::Struct(s) => MoveTypeLayout::Struct(s),
            Self::Enum(e) => MoveTypeLayout::Enum(e),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MoveTypeLayout {
    #[serde(rename(serialize = "bool", deserialize = "bool"))]
    Bool,
    #[serde(rename(serialize = "u8", deserialize = "u8"))]
    U8,
    #[serde(rename(serialize = "u64", deserialize = "u64"))]
    U64,
    #[serde(rename(serialize = "u128", deserialize = "u128"))]
    U128,
    #[serde(rename(serialize = "address", deserialize = "address"))]
    Address,
    #[serde(rename(serialize = "vector", deserialize = "vector"))]
    Vector(Box<MoveTypeLayout>),
    #[serde(rename(serialize = "struct", deserialize = "struct"))]
    Struct(Box<MoveStructLayout>),
    #[serde(rename(serialize = "signer", deserialize = "signer"))]
    Signer,

    // NOTE: Added in bytecode version v6, do not reorder!
    #[serde(rename(serialize = "u16", deserialize = "u16"))]
    U16,
    #[serde(rename(serialize = "u32", deserialize = "u32"))]
    U32,
    #[serde(rename(serialize = "u256", deserialize = "u256"))]
    U256,
    #[serde(rename(serialize = "enum", deserialize = "enum"))]
    Enum(Box<MoveEnumLayout>),
}

impl MoveStructLayout {
    /// Returns `true` if and only if the layout is for `type_`.
    pub fn is_type(&self, type_: &StructTag) -> bool {
        self.type_ == *type_
    }
}

impl MoveEnumLayout {
    /// Returns `true` if and only if the layout is for `type_`.
    pub fn is_type(&self, type_: &StructTag) -> bool {
        self.type_ == *type_
    }
}

impl MoveTypeLayout {
    /// Returns `true` if and only if the layout is for `type_`.
    pub fn is_type(&self, type_: &TypeTag) -> bool {
        use MoveTypeLayout as L;
        use TypeTag as T;

        match self {
            L::Bool => matches!(type_, T::Bool),
            L::U8 => matches!(type_, T::U8),
            L::U16 => matches!(type_, T::U16),
            L::U32 => matches!(type_, T::U32),
            L::U64 => matches!(type_, T::U64),
            L::U128 => matches!(type_, T::U128),
            L::U256 => matches!(type_, T::U256),
            L::Address => matches!(type_, T::Address),
            L::Signer => matches!(type_, T::Signer),
            L::Vector(l) => matches!(type_, T::Vector(t) if l.is_type(t)),
            L::Struct(l) => matches!(type_, T::Struct(t) if l.is_type(t)),
            L::Enum(l) => matches!(type_, T::Struct(t) if l.is_type(t)),
        }
    }
}

impl MoveValue {
    /// TODO (annotated-visitor): Port legacy uses of this method to `BoundedVisitor`.
    pub fn simple_deserialize(blob: &[u8], ty: &MoveTypeLayout) -> AResult<Self> {
        Ok(bcs::from_bytes_seed(ty, blob)?)
    }

    /// Deserialize a BCS-encoded blob using a compressed annotated type layout.
    pub fn simple_deserialize_compressed(
        blob: &[u8],
        layout: &compressed_layouts::MoveTypeLayout,
    ) -> AResult<Self> {
        Ok(bcs::from_bytes_seed(layout.as_view(), blob)?)
    }

    /// Deserialize `blob` as a Move value with the given `ty`-pe layout, and visit its
    /// sub-structure with the given `visitor`. The visitor dictates the return value that is built
    /// up during deserialization.
    ///
    /// # Nested deserialization
    ///
    /// Vectors and structs are nested structures that can be met during deserialization. Visitors
    /// are passed a driver (`VecDriver` or `StructDriver` correspondingly) which controls how
    /// nested elements or fields are visited including whether a given nested element/field is
    /// explored, which visitor to use (the visitor can pass `self` to recursively explore them) and
    /// whether a given element is visited or skipped.
    ///
    /// The visitor may leave elements unvisited at the end of the vector or struct, which
    /// implicitly skips them.
    ///
    /// # Errors
    ///
    /// Deserialization can fail because of an issue in the serialized format (data doesn't match
    /// layout, unexpected bytes or trailing bytes), or a custom error expressed by the visitor.
    pub fn visit_deserialize<'b, 'l, V: Visitor<'b, 'l>>(
        blob: &'b [u8],
        ty: &'l MoveTypeLayout,
        visitor: &mut V,
    ) -> Result<V::Value, V::Error>
    where
        V::Error: std::error::Error + Send + Sync + 'static,
    {
        // TODO: Don't simplify error to anyhow::Error
        let mut bytes = Cursor::new(blob);
        let res = visit_value(&mut bytes, ty, visitor)?;
        if bytes.position() as usize == blob.len() {
            Ok(res)
        } else {
            let remaining = blob.len() - bytes.position() as usize;
            Err(VError::TrailingBytes(remaining).into())
        }
    }

    pub fn simple_serialize(&self) -> Option<Vec<u8>> {
        bcs::to_bytes(self).ok()
    }

    pub fn undecorate(self) -> R::MoveValue {
        match self {
            Self::Struct(s) => R::MoveValue::Struct(s.undecorate()),
            Self::Variant(v) => R::MoveValue::Variant(v.undecorate()),
            Self::Vector(vals) => {
                R::MoveValue::Vector(vals.into_iter().map(MoveValue::undecorate).collect())
            }
            MoveValue::U8(u) => R::MoveValue::U8(u),
            MoveValue::U64(u) => R::MoveValue::U64(u),
            MoveValue::U128(u) => R::MoveValue::U128(u),
            MoveValue::Bool(b) => R::MoveValue::Bool(b),
            MoveValue::Address(a) => R::MoveValue::Address(a),
            MoveValue::Signer(s) => R::MoveValue::Signer(s),
            MoveValue::U16(u) => R::MoveValue::U16(u),
            MoveValue::U32(u) => R::MoveValue::U32(u),
            MoveValue::U256(u) => R::MoveValue::U256(u),
        }
    }
}

pub fn serialize_values<'a, I>(vals: I) -> Vec<Vec<u8>>
where
    I: IntoIterator<Item = &'a MoveValue>,
{
    vals.into_iter()
        .map(|val| {
            val.simple_serialize()
                .expect("serialization should succeed")
        })
        .collect()
}

impl MoveStruct {
    pub fn new(type_: StructTag, fields: Vec<(Identifier, MoveValue)>) -> Self {
        Self { type_, fields }
    }

    /// TODO (annotated-visitor): Port legacy uses of this method to `BoundedVisitor`.
    pub fn simple_deserialize(blob: &[u8], ty: &MoveStructLayout) -> AResult<Self> {
        Ok(bcs::from_bytes_seed(ty, blob)?)
    }

    /// Like `MoveValue::visit_deserialize` (see for details), but specialized to visiting a struct
    /// (the `blob` is known to be a serialized Move struct, and the layout is a
    /// `MoveStructLayout`).
    pub fn visit_deserialize<'b, 'l, V: Visitor<'b, 'l>>(
        blob: &'b [u8],
        ty: &'l MoveStructLayout,
        visitor: &mut V,
    ) -> Result<V::Value, V::Error>
    where
        V::Error: std::error::Error + Send + Sync + 'static,
    {
        let mut bytes = Cursor::new(blob);
        let driver = ValueDriver::new(&mut bytes, None);
        let res = visit_struct(driver, ty, visitor)?;
        if bytes.position() as usize == blob.len() {
            Ok(res)
        } else {
            let remaining = blob.len() - bytes.position() as usize;
            Err(VError::TrailingBytes(remaining).into())
        }
    }

    pub fn into_fields(self) -> Vec<MoveValue> {
        self.fields.into_iter().map(|(_, v)| v).collect()
    }

    pub fn undecorate(self) -> R::MoveStruct {
        R::MoveStruct(
            self.into_fields()
                .into_iter()
                .map(MoveValue::undecorate)
                .collect(),
        )
    }
}

impl MoveVariant {
    pub fn new(
        type_: StructTag,
        variant_name: Identifier,
        tag: u16,
        fields: Vec<(Identifier, MoveValue)>,
    ) -> Self {
        Self {
            type_,
            variant_name,
            tag,
            fields,
        }
    }

    pub fn simple_deserialize(blob: &[u8], ty: &MoveEnumLayout) -> AResult<Self> {
        Ok(bcs::from_bytes_seed(ty, blob)?)
    }

    pub fn into_fields(self) -> Vec<MoveValue> {
        self.fields.into_iter().map(|(_, v)| v).collect()
    }

    pub fn undecorate(self) -> R::MoveVariant {
        R::MoveVariant {
            tag: self.tag,
            fields: self
                .into_fields()
                .into_iter()
                .map(MoveValue::undecorate)
                .collect(),
        }
    }
}

impl MoveStructLayout {
    pub fn new(type_: StructTag, fields: Vec<MoveFieldLayout>) -> Self {
        Self { type_, fields }
    }

    pub fn into_fields(self) -> Vec<MoveTypeLayout> {
        self.fields.into_iter().map(|f| f.layout).collect()
    }
}

impl<'d> serde::de::DeserializeSeed<'d> for &MoveTypeLayout {
    type Value = MoveValue;
    fn deserialize<D: serde::de::Deserializer<'d>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        match self {
            MoveTypeLayout::Bool => bool::deserialize(deserializer).map(MoveValue::Bool),
            MoveTypeLayout::U8 => u8::deserialize(deserializer).map(MoveValue::U8),
            MoveTypeLayout::U16 => u16::deserialize(deserializer).map(MoveValue::U16),
            MoveTypeLayout::U32 => u32::deserialize(deserializer).map(MoveValue::U32),
            MoveTypeLayout::U64 => u64::deserialize(deserializer).map(MoveValue::U64),
            MoveTypeLayout::U128 => u128::deserialize(deserializer).map(MoveValue::U128),
            MoveTypeLayout::U256 => u256::U256::deserialize(deserializer).map(MoveValue::U256),
            MoveTypeLayout::Address => {
                AccountAddress::deserialize(deserializer).map(MoveValue::Address)
            }
            MoveTypeLayout::Signer => {
                AccountAddress::deserialize(deserializer).map(MoveValue::Signer)
            }
            MoveTypeLayout::Struct(ty) => Ok(MoveValue::Struct(ty.deserialize(deserializer)?)),
            MoveTypeLayout::Enum(ty) => Ok(MoveValue::Variant(ty.deserialize(deserializer)?)),
            MoveTypeLayout::Vector(layout) => Ok(MoveValue::Vector(
                deserializer.deserialize_seq(VectorElementVisitor(layout))?,
            )),
        }
    }
}

struct VectorElementVisitor<'a>(&'a MoveTypeLayout);

impl<'d> serde::de::Visitor<'d> for VectorElementVisitor<'_> {
    type Value = Vec<MoveValue>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Vector")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'d>,
    {
        let mut vals = Vec::new();
        while let Some(elem) = seq.next_element_seed(self.0)? {
            vals.push(elem)
        }
        Ok(vals)
    }
}

struct DecoratedStructFieldVisitor<'a>(&'a [MoveFieldLayout]);

impl<'d> serde::de::Visitor<'d> for DecoratedStructFieldVisitor<'_> {
    type Value = Vec<(Identifier, MoveValue)>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Struct")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'d>,
    {
        let mut vals = Vec::new();
        for (i, layout) in self.0.iter().enumerate() {
            match seq.next_element_seed(layout)? {
                Some(elem) => vals.push(elem),
                None => return Err(A::Error::invalid_length(i, &self)),
            }
        }
        Ok(vals)
    }
}

impl<'d> serde::de::DeserializeSeed<'d> for &MoveFieldLayout {
    type Value = (Identifier, MoveValue);

    fn deserialize<D: serde::de::Deserializer<'d>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        Ok((self.name.clone(), self.layout.deserialize(deserializer)?))
    }
}

impl<'d> serde::de::DeserializeSeed<'d> for &MoveStructLayout {
    type Value = MoveStruct;

    fn deserialize<D: serde::de::Deserializer<'d>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        let fields = deserializer
            .deserialize_tuple(self.fields.len(), DecoratedStructFieldVisitor(&self.fields))?;
        Ok(MoveStruct {
            type_: self.type_.clone(),
            fields,
        })
    }
}

impl<'d> serde::de::DeserializeSeed<'d> for &MoveEnumLayout {
    type Value = MoveVariant;
    fn deserialize<D: serde::de::Deserializer<'d>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        let (variant_name, tag, fields) =
            deserializer.deserialize_tuple(2, DecoratedEnumFieldVisitor(&self.variants))?;
        Ok(MoveVariant {
            type_: self.type_.clone(),
            variant_name,
            tag,
            fields,
        })
    }
}

struct DecoratedEnumFieldVisitor<'a>(&'a BTreeMap<(Identifier, u16), Vec<MoveFieldLayout>>);

impl<'d> serde::de::Visitor<'d> for DecoratedEnumFieldVisitor<'_> {
    type Value = (Identifier, u16, Vec<(Identifier, MoveValue)>);

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Enum")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'d>,
    {
        let tag = match seq.next_element_seed(&MoveTypeLayout::U8)? {
            Some(MoveValue::U8(tag)) if tag as u64 <= VARIANT_TAG_MAX_VALUE => tag as u16,
            Some(MoveValue::U8(tag)) => return Err(A::Error::invalid_length(tag as usize, &self)),
            Some(val) => {
                return Err(A::Error::invalid_type(
                    serde::de::Unexpected::Other(&format!("{val:?}")),
                    &self,
                ));
            }
            None => return Err(A::Error::invalid_length(0, &self)),
        };

        let Some(((variant_name, _), variant_layout)) =
            self.0.iter().find(|((_, v_tag), _)| *v_tag == tag)
        else {
            return Err(A::Error::invalid_length(tag as usize, &self));
        };

        let Some(fields) = seq.next_element_seed(&DecoratedVariantFieldLayout(variant_layout))?
        else {
            return Err(A::Error::invalid_length(1, &self));
        };

        Ok((variant_name.clone(), tag, fields))
    }
}

struct DecoratedVariantFieldLayout<'a>(&'a Vec<MoveFieldLayout>);

impl<'d> serde::de::DeserializeSeed<'d> for &DecoratedVariantFieldLayout<'_> {
    type Value = Vec<(Identifier, MoveValue)>;

    fn deserialize<D: serde::de::Deserializer<'d>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_tuple(self.0.len(), DecoratedStructFieldVisitor(self.0))
    }
}

impl serde::Serialize for MoveValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MoveValue::Struct(s) => s.serialize(serializer),
            MoveValue::Variant(v) => v.serialize(serializer),
            MoveValue::Bool(b) => serializer.serialize_bool(*b),
            MoveValue::U8(i) => serializer.serialize_u8(*i),
            MoveValue::U16(i) => serializer.serialize_u16(*i),
            MoveValue::U32(i) => serializer.serialize_u32(*i),
            MoveValue::U64(i) => serializer.serialize_u64(*i),
            MoveValue::U128(i) => serializer.serialize_u128(*i),
            MoveValue::U256(i) => i.serialize(serializer),
            MoveValue::Address(a) => a.serialize(serializer),
            MoveValue::Signer(a) => a.serialize(serializer),
            MoveValue::Vector(v) => {
                let mut t = serializer.serialize_seq(Some(v.len()))?;
                for val in v {
                    t.serialize_element(val)?;
                }
                t.end()
            }
        }
    }
}

struct MoveFields<'a>(&'a [(Identifier, MoveValue)]);

impl serde::Serialize for MoveFields<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut t = serializer.serialize_map(Some(self.0.len()))?;
        for (f, v) in self.0.iter() {
            t.serialize_entry(f, v)?;
        }
        t.end()
    }
}

impl serde::Serialize for MoveStruct {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize a Move struct as Serde struct type named `struct `with two fields named `type` and `fields`.
        // `fields` will get serialized as a Serde map.
        // Unfortunately, we can't serialize this in the logical way: as a Serde struct named `type` with a field for
        // each of `fields` because serde insists that struct and field names be `'static &str`'s
        let mut t = serializer.serialize_struct(MOVE_STRUCT_NAME, 2)?;
        // serialize type as string (e.g., 0x0::ModuleName::StructName<TypeArg1,TypeArg2>) instead of (e.g.
        // { address: 0x0...0, module: ModuleName, name: StructName, type_args: [TypeArg1, TypeArg2]})
        t.serialize_field(MOVE_STRUCT_TYPE, &self.type_.to_string())?;
        t.serialize_field(MOVE_STRUCT_FIELDS, &MoveFields(&self.fields))?;
        t.end()
    }
}

impl serde::Serialize for MoveVariant {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize an enum as:
        // enum { "type": 0xC::module::enum_type, "variant_name": name, "variant_tag": tag, "fields": { ... } }
        let mut t = serializer.serialize_struct(MOVE_ENUM_NAME, 4)?;
        t.serialize_field(MOVE_DATA_TYPE, &self.type_.to_string())?;
        t.serialize_field(MOVE_VARIANT_NAME, &self.variant_name.to_string())?;
        t.serialize_field(MOVE_VARIANT_TAG_NAME, &MoveValue::U16(self.tag))?;
        t.serialize_field(MOVE_DATA_FIELDS, &MoveFields(&self.fields))?;
        t.end()
    }
}

impl fmt::Display for MoveTypeLayout {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        use MoveTypeLayout::*;
        match self {
            Bool => write!(f, "bool"),
            U8 => write!(f, "u8"),
            U16 => write!(f, "u16"),
            U32 => write!(f, "u32"),
            U64 => write!(f, "u64"),
            U128 => write!(f, "u128"),
            U256 => write!(f, "u256"),
            Address => write!(f, "address"),
            Signer => write!(f, "signer"),
            Vector(typ) if f.alternate() => write!(f, "vector<{typ:#}>"),
            Vector(typ) => write!(f, "vector<{typ}>"),
            Struct(s) if f.alternate() => write!(f, "{s:#}"),
            Struct(s) => write!(f, "{s}"),
            Enum(e) if f.alternate() => write!(f, "{e:#}"),
            Enum(e) => write!(f, "enum {}", e),
        }
    }
}

/// Helper type that uses `T`'s `Display` implementation as its own `Debug` implementation, to allow
/// other `Display` implementations in this module to take advantage of the structured formatting
/// helpers that Rust uses for its own debug types.
pub struct DebugAsDisplay<'a, T>(pub &'a T);
impl<T: fmt::Display> fmt::Debug for DebugAsDisplay<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{:#}", self.0)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl fmt::Display for MoveStructLayout {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        use DebugAsDisplay as DD;
        write!(f, "struct ")?;
        write!(f, "{} ", self.type_)?;
        let mut map = f.debug_map();
        for field in &*self.fields {
            map.entry(&DD(&field.name), &DD(&field.layout));
        }
        map.finish()
    }
}

impl fmt::Display for MoveEnumLayout {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        use DebugAsDisplay as DD;
        write!(f, "enum {} ", self.type_)?;
        let mut vmap = f.debug_set();
        for ((variant_name, _), fields) in self.variants.iter() {
            vmap.entry(&DD(&MoveVariantDisplay(variant_name.as_str(), fields)));
        }
        vmap.finish()
    }
}

struct MoveVariantDisplay<'a>(&'a str, &'a [MoveFieldLayout]);

impl fmt::Display for MoveVariantDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
        use DebugAsDisplay as DD;
        let mut map = f.debug_struct(self.0);
        for field in self.1 {
            map.field(field.name.as_str(), &DD(&field.layout));
        }
        map.finish()
    }
}

impl From<&MoveTypeLayout> for TypeTag {
    fn from(val: &MoveTypeLayout) -> TypeTag {
        match val {
            MoveTypeLayout::Address => TypeTag::Address,
            MoveTypeLayout::Bool => TypeTag::Bool,
            MoveTypeLayout::U8 => TypeTag::U8,
            MoveTypeLayout::U16 => TypeTag::U16,
            MoveTypeLayout::U32 => TypeTag::U32,
            MoveTypeLayout::U64 => TypeTag::U64,
            MoveTypeLayout::U128 => TypeTag::U128,
            MoveTypeLayout::U256 => TypeTag::U256,
            MoveTypeLayout::Signer => TypeTag::Signer,
            MoveTypeLayout::Vector(v) => {
                let inner_type = &**v;
                TypeTag::Vector(Box::new(inner_type.into()))
            }
            MoveTypeLayout::Struct(v) => TypeTag::Struct(Box::new(v.as_ref().into())),
            MoveTypeLayout::Enum(e) => TypeTag::Struct(Box::new(e.as_ref().into())),
        }
    }
}

impl From<&MoveStructLayout> for StructTag {
    fn from(val: &MoveStructLayout) -> StructTag {
        val.type_.clone()
    }
}

impl From<&MoveEnumLayout> for StructTag {
    fn from(val: &MoveEnumLayout) -> StructTag {
        val.type_.clone()
    }
}

impl fmt::Display for MoveValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoveValue::U8(u) => write!(f, "{}u8", u),
            MoveValue::U16(u) => write!(f, "{}u16", u),
            MoveValue::U32(u) => write!(f, "{}u32", u),
            MoveValue::U64(u) => write!(f, "{}u64", u),
            MoveValue::U128(u) => write!(f, "{}u128", u),
            MoveValue::U256(u) => write!(f, "{}u256", u),
            MoveValue::Bool(false) => write!(f, "false"),
            MoveValue::Bool(true) => write!(f, "true"),
            MoveValue::Address(a) => write!(f, "{}", a.to_hex_literal()),
            MoveValue::Signer(a) => write!(f, "signer({})", a.to_hex_literal()),
            MoveValue::Vector(v) => {
                use DebugAsDisplay as DD;
                write!(f, "vector")?;
                let mut list = f.debug_list();
                for val in v {
                    list.entry(&DD(val));
                }
                list.finish()
            }
            MoveValue::Struct(s) => fmt::Display::fmt(s, f),
            MoveValue::Variant(v) => fmt::Display::fmt(v, f),
        }
    }
}

impl fmt::Display for MoveStruct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DebugAsDisplay as DD;
        fmt::Display::fmt(&self.type_, f)?;
        write!(f, " ")?;
        let mut map = f.debug_map();
        for (field, value) in &self.fields {
            map.entry(&DD(field), &DD(value));
        }
        map.finish()
    }
}

impl fmt::Display for MoveVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DebugAsDisplay as DD;
        let MoveVariant {
            type_,
            variant_name,
            tag: _,
            fields,
        } = self;
        write!(f, "{}::{}", type_, variant_name)?;
        let mut map = f.debug_map();
        for (field, value) in fields {
            map.entry(&DD(field), &DD(value));
        }
        map.finish()
    }
}

pub mod compressed_layouts {
    use super::{
        MoveEnumLayout, MoveFieldLayout, MoveStructLayout, MoveTypeLayout as TreeMoveTypeLayout,
    };
    use crate::identifier::Identifier;
    use crate::language_storage::StructTag;
    use crate::runtime_value::compressed_layouts::{LayoutRef, LeafType, ResolvedRef};
    use indexmap::IndexSet;
    use serde::{Deserialize, Serialize};

    // =============================================================================
    // Compressed (interned) annotated layout types
    // =============================================================================

    /// Index into an [`MoveTypeLayout`]'s strings table.
    pub type StringIdx = u16;

    /// Index into an [`MoveTypeLayout`]'s tags table.
    pub type TagIdx = u16;

    /// A list of (field_name_idx, layout_ref) pairs for struct/enum fields.
    pub type AnnotatedFieldIndices = Box<[(StringIdx, LayoutRef)]>;

    /// A single variant entry: (variant_name_idx, tag, optional field_indices).
    /// `None` field indices means the variant exists but its layout is unknown.
    pub type AnnotatedVariantEntry = (StringIdx, u16, Option<AnnotatedFieldIndices>);

    /// Annotated struct layout node: type tag + named fields stored as interned indices.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MoveStructNode {
        pub type_: TagIdx,
        pub fields: AnnotatedFieldIndices,
    }

    /// Annotated enum layout node: type tag + named variants with named fields.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MoveEnumNode {
        pub type_: TagIdx,
        pub variants: Box<[AnnotatedVariantEntry]>,
    }

    /// A compound layout node in the annotated compressed node table.
    /// Leaf types (primitives) are encoded inline in [`LayoutRef`] and never
    /// appear in the table.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum MoveTypeNode {
        Vector(LayoutRef),
        Struct(MoveStructNode),
        Enum(MoveEnumNode),
    }

    /// A deduplicated, flat representation of an annotated [`MoveTypeLayout`] tree.
    /// Strings (field names, variant names) and [`StructTag`]s are interned into
    /// separate side tables.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MoveTypeLayout {
        nodes: Box<[MoveTypeNode]>,
        strings: Box<[Identifier]>,
        tags: Box<[StructTag]>,
        root: LayoutRef,
    }

    impl MoveTypeLayout {
        /// Number of compound nodes in the table (excludes inline leaf types).
        pub fn node_count(&self) -> usize {
            self.nodes.len()
        }

        /// Number of unique interned strings (field/variant names).
        pub fn string_count(&self) -> usize {
            self.strings.len()
        }

        /// Number of unique interned struct tags.
        pub fn tag_count(&self) -> usize {
            self.tags.len()
        }

        /// Create a resolved view for navigating this layout.
        pub fn as_view(&self) -> MoveLayoutView<'_> {
            resolve_ref(&self.nodes, &self.strings, &self.tags, self.root)
        }

        /// Inflate back into a tree-based [`MoveTypeLayout`].
        pub fn inflate(&self) -> AResult<TreeMoveTypeLayout> {
            self.as_view().inflate()
        }
    }

    // =============================================================================
    // View — the primary public API for navigating compressed layouts
    // =============================================================================

    /// Resolve a [`LayoutRef`] against the node and side tables into a
    /// [`MoveLayoutView`] with eagerly resolved type tags and field names.
    ///
    /// Panics if the reference points to an out-of-bounds table index.
    fn resolve_ref<'a>(
        nodes: &'a [MoveTypeNode],
        strings: &'a [Identifier],
        tags: &'a [StructTag],
        r: LayoutRef,
    ) -> MoveLayoutView<'a> {
        match r.resolve() {
            ResolvedRef::Leaf(leaf) => leaf_to_layout_view(leaf),
            ResolvedRef::Index(idx) => match &nodes[idx] {
                MoveTypeNode::Vector(inner) => {
                    MoveLayoutView::Vector(MoveVectorView {
                        nodes,
                        strings,
                        tags,
                        element: *inner,
                    })
                }
                MoveTypeNode::Struct(s) => {
                    let type_ = &tags[s.type_ as usize];
                    MoveLayoutView::Struct {
                        type_,
                        fields: MoveFieldView {
                            nodes,
                            strings,
                            tags,
                            fields: &s.fields,
                        },
                    }
                }
                MoveTypeNode::Enum(e) => {
                    let type_ = &tags[e.type_ as usize];
                    MoveLayoutView::Enum(MoveEnumView {
                        nodes,
                        strings,
                        tags,
                        type_,
                        variants: &e.variants,
                    })
                }
            },
        }
    }

    fn leaf_to_layout_view(leaf: LeafType) -> MoveLayoutView<'static> {
        match leaf {
            LeafType::Bool => MoveLayoutView::Bool,
            LeafType::U8 => MoveLayoutView::U8,
            LeafType::U16 => MoveLayoutView::U16,
            LeafType::U32 => MoveLayoutView::U32,
            LeafType::U64 => MoveLayoutView::U64,
            LeafType::U128 => MoveLayoutView::U128,
            LeafType::U256 => MoveLayoutView::U256,
            LeafType::Address => MoveLayoutView::Address,
            LeafType::Signer => MoveLayoutView::Signer,
        }
    }

    /// A resolved view of an annotated layout node. Compound types contain
    /// further views with eagerly resolved type tags and field names.
    /// Resolution is lazy — only one layer is resolved at a time.
    #[derive(Debug, Clone, Copy)]
    pub enum MoveLayoutView<'a> {
        Bool,
        U8,
        U16,
        U32,
        U64,
        U128,
        U256,
        Address,
        Signer,
        Vector(MoveVectorView<'a>),
        Struct {
            type_: &'a StructTag,
            fields: MoveFieldView<'a>,
        },
        Enum(MoveEnumView<'a>),
    }

    use anyhow::Result as AResult;

    impl<'a> MoveLayoutView<'a> {
        /// Reconstruct the equivalent tree-based layout. Returns an error
        /// if any enum variant has an unknown layout.
        pub fn inflate(&self) -> AResult<TreeMoveTypeLayout> {
            Ok(match self {
                MoveLayoutView::Bool => TreeMoveTypeLayout::Bool,
                MoveLayoutView::U8 => TreeMoveTypeLayout::U8,
                MoveLayoutView::U16 => TreeMoveTypeLayout::U16,
                MoveLayoutView::U32 => TreeMoveTypeLayout::U32,
                MoveLayoutView::U64 => TreeMoveTypeLayout::U64,
                MoveLayoutView::U128 => TreeMoveTypeLayout::U128,
                MoveLayoutView::U256 => TreeMoveTypeLayout::U256,
                MoveLayoutView::Address => TreeMoveTypeLayout::Address,
                MoveLayoutView::Signer => TreeMoveTypeLayout::Signer,
                MoveLayoutView::Vector(vv) => {
                    TreeMoveTypeLayout::Vector(Box::new(vv.element().inflate()?))
                }
                MoveLayoutView::Struct { type_, fields: fv } => {
                    let fields = fv
                        .fields()
                        .map(|(name, fv)| Ok(MoveFieldLayout::new(name.clone(), fv.inflate()?)))
                        .collect::<AResult<_>>()?;
                    TreeMoveTypeLayout::Struct(Box::new(MoveStructLayout {
                        type_: (*type_).clone(),
                        fields,
                    }))
                }
                MoveLayoutView::Enum(ev) => {
                    let variants = ev
                        .variants()
                        .map(|(variant_name, tag, vfv)| match vfv {
                            VariantFieldView::Known(fv) => {
                                let field_layouts = fv
                                    .fields()
                                    .map(|(name, fv)| {
                                        Ok(MoveFieldLayout::new(name.clone(), fv.inflate()?))
                                    })
                                    .collect::<AResult<_>>()?;
                                Ok(((variant_name.clone(), tag), field_layouts))
                            }
                            VariantFieldView::Unknown => {
                                anyhow::bail!("cannot inflate enum with unknown variant layout")
                            }
                        })
                        .collect::<AResult<_>>()?;
                    TreeMoveTypeLayout::Enum(Box::new(MoveEnumLayout {
                        type_: ev.type_().clone(),
                        variants,
                    }))
                }
            })
        }
    }

    /// A lazy view over an annotated vector layout's element type.
    #[derive(Debug, Clone, Copy)]
    pub struct MoveVectorView<'a> {
        nodes: &'a [MoveTypeNode],
        strings: &'a [Identifier],
        tags: &'a [StructTag],
        element: LayoutRef,
    }

    impl<'a> MoveVectorView<'a> {
        /// Resolve the element type.
        pub fn element(&self) -> MoveLayoutView<'a> {
            resolve_ref(self.nodes, self.strings, self.tags, self.element)
        }
    }

    /// A view over a list of named, typed fields (struct fields or enum variant fields).
    #[derive(Debug, Clone, Copy)]
    pub struct MoveFieldView<'a> {
        nodes: &'a [MoveTypeNode],
        strings: &'a [Identifier],
        tags: &'a [StructTag],
        fields: &'a [(StringIdx, LayoutRef)],
    }

    impl<'a> MoveFieldView<'a> {
        /// Number of fields.
        pub fn field_count(&self) -> usize {
            self.fields.len()
        }

        /// Access a field by index, returning `(name, layout_view)`.
        pub fn field(
            &self,
            i: usize,
        ) -> Option<(&'a Identifier, MoveLayoutView<'a>)> {
            self.fields.get(i).map(|(name_idx, layout_ref)| {
                (
                    &self.strings[*name_idx as usize],
                    resolve_ref(self.nodes, self.strings, self.tags, *layout_ref),
                )
            })
        }

        /// Look up a field by name, returning its layout view.
        pub fn field_by_name(&self, name: &str) -> Option<MoveLayoutView<'a>> {
            self.fields
                .iter()
                .find(|(name_idx, _)| self.strings[*name_idx as usize].as_str() == name)
                .map(|(_, layout_ref)| {
                    resolve_ref(self.nodes, self.strings, self.tags, *layout_ref)
                })
        }

        /// Iterate over all fields as `(name, layout_view)` pairs.
        pub fn fields(
            &self,
        ) -> impl ExactSizeIterator<Item = (&'a Identifier, MoveLayoutView<'a>)> + '_ {
            let nodes = self.nodes;
            let strings = self.strings;
            let tags = self.tags;
            self.fields.iter().map(move |(name_idx, layout_ref)| {
                (
                    &strings[*name_idx as usize],
                    resolve_ref(nodes, strings, tags, *layout_ref),
                )
            })
        }
    }

    /// The result of looking up a variant in an annotated enum view.
    #[derive(Debug, Clone, Copy)]
    pub enum VariantFieldView<'a> {
        /// The variant's field layout is known.
        Known(MoveFieldView<'a>),
        /// The variant exists but its field layout is not available.
        Unknown,
    }

    /// A view over an annotated enum layout's variants.
    #[derive(Debug, Clone, Copy)]
    pub struct MoveEnumView<'a> {
        nodes: &'a [MoveTypeNode],
        strings: &'a [Identifier],
        tags: &'a [StructTag],
        type_: &'a StructTag,
        variants: &'a [AnnotatedVariantEntry],
    }

    impl<'a> MoveEnumView<'a> {
        /// The enum's type tag.
        pub fn type_(&self) -> &'a StructTag {
            self.type_
        }

        /// Number of variants.
        pub fn variant_count(&self) -> usize {
            self.variants.len()
        }

        /// Access a variant by position index. Returns `None` if out of bounds.
        pub fn variant(
            &self,
            i: usize,
        ) -> Option<(&'a Identifier, u16, VariantFieldView<'a>)> {
            self.variants.get(i).map(|(name_idx, tag, fields)| {
                let name = &self.strings[*name_idx as usize];
                let vfv = match fields {
                    Some(fields) => VariantFieldView::Known(MoveFieldView {
                        nodes: self.nodes,
                        strings: self.strings,
                        tags: self.tags,
                        fields,
                    }),
                    None => VariantFieldView::Unknown,
                };
                (name, *tag, vfv)
            })
        }

        /// Find a variant by its tag value.
        pub fn variant_by_tag(
            &self,
            tag: u16,
        ) -> Option<(&'a Identifier, VariantFieldView<'a>)> {
            self.variants
                .iter()
                .find(|(_, t, _)| *t == tag)
                .map(|(name_idx, _, fields)| {
                    let name = &self.strings[*name_idx as usize];
                    let vfv = match fields {
                        Some(fields) => VariantFieldView::Known(MoveFieldView {
                            nodes: self.nodes,
                            strings: self.strings,
                            tags: self.tags,
                            fields,
                        }),
                        None => VariantFieldView::Unknown,
                    };
                    (name, vfv)
                })
        }

        /// Iterate over all variants as `(name, tag, field_view)` tuples.
        pub fn variants(
            &self,
        ) -> impl ExactSizeIterator<Item = (&'a Identifier, u16, VariantFieldView<'a>)> + 'a
        {
            let nodes = self.nodes;
            let strings = self.strings;
            let tags = self.tags;
            self.variants.iter().map(move |(name_idx, tag, fields)| {
                let name = &strings[*name_idx as usize];
                let vfv = match fields {
                    Some(fields) => VariantFieldView::Known(MoveFieldView {
                        nodes,
                        strings,
                        tags,
                        fields,
                    }),
                    None => VariantFieldView::Unknown,
                };
                (name, *tag, vfv)
            })
        }
    }

    // =============================================================================
    // Builder
    // =============================================================================

    /// Incrementally builds an annotated [`MoveTypeLayout`] with automatic
    /// deduplication of nodes, field/variant names, and struct tags.
    pub struct MoveTypeLayoutBuilder {
        nodes: IndexSet<MoveTypeNode>,
        strings: IndexSet<Identifier>,
        tags: IndexSet<StructTag>,
    }

    impl MoveTypeLayoutBuilder {
        pub fn new() -> Self {
            Self {
                nodes: IndexSet::new(),
                strings: IndexSet::new(),
                tags: IndexSet::new(),
            }
        }

        fn intern_string(&mut self, s: &Identifier) -> StringIdx {
            let (idx, _) = self.strings.insert_full(s.clone());
            assert!(
                idx <= u16::MAX as usize,
                "string table exceeds u16 capacity"
            );
            idx as u16
        }

        fn intern_tag(&mut self, tag: &StructTag) -> TagIdx {
            let (idx, _) = self.tags.insert_full(tag.clone());
            assert!(idx <= u16::MAX as usize, "tag table exceeds u16 capacity");
            idx as u16
        }

        fn intern(&mut self, node: MoveTypeNode) -> LayoutRef {
            let (idx, _) = self.nodes.insert_full(node);
            LayoutRef::index(idx)
        }

        pub fn bool(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::Bool)
        }
        pub fn u8(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U8)
        }
        pub fn u16(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U16)
        }
        pub fn u32(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U32)
        }
        pub fn u64(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U64)
        }
        pub fn u128(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U128)
        }
        pub fn u256(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::U256)
        }
        pub fn address(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::Address)
        }
        pub fn signer(&mut self) -> LayoutRef {
            LayoutRef::leaf(LeafType::Signer)
        }

        pub fn vector(&mut self, element: LayoutRef) -> LayoutRef {
            self.intern(MoveTypeNode::Vector(element))
        }

        /// Build a struct layout node.
        /// `fields` is a list of (field_name, field_layout) pairs.
        pub fn struct_layout(
            &mut self,
            type_tag: &StructTag,
            fields: &[(&Identifier, LayoutRef)],
        ) -> LayoutRef {
            let tag_idx = self.intern_tag(type_tag);
            let field_indices: AnnotatedFieldIndices = fields
                .iter()
                .map(|(name, r)| (self.intern_string(name), *r))
                .collect();
            self.intern(MoveTypeNode::Struct(MoveStructNode {
                type_: tag_idx,
                fields: field_indices,
            }))
        }

        /// Build an enum layout node.
        /// Each variant is `(variant_name, tag, fields)` where fields is
        /// `None` for unknown layout or `Some(&[(field_name, layout)])` for known.
        pub fn enum_layout(
            &mut self,
            type_tag: &StructTag,
            variants: &[(&Identifier, u16, Option<&[(&Identifier, LayoutRef)]>)],
        ) -> LayoutRef {
            let tag_idx = self.intern_tag(type_tag);
            let variant_entries: Box<[AnnotatedVariantEntry]> = variants
                .iter()
                .map(|(vn, tag, fields)| {
                    let vn_idx = self.intern_string(vn);
                    let field_indices = fields.map(|fields| {
                        fields
                            .iter()
                            .map(|(fn_name, r)| (self.intern_string(fn_name), *r))
                            .collect()
                    });
                    (vn_idx, *tag, field_indices)
                })
                .collect();
            self.intern(MoveTypeNode::Enum(MoveEnumNode {
                type_: tag_idx,
                variants: variant_entries,
            }))
        }

        /// Recursively intern a tree-based annotated layout.
        /// Tree-based enum layouts always have known variants, so all variants
        /// are wrapped in `Some`.
        pub fn intern_tree(&mut self, layout: &TreeMoveTypeLayout) -> LayoutRef {
            match layout {
                TreeMoveTypeLayout::Bool => self.bool(),
                TreeMoveTypeLayout::U8 => self.u8(),
                TreeMoveTypeLayout::U16 => self.u16(),
                TreeMoveTypeLayout::U32 => self.u32(),
                TreeMoveTypeLayout::U64 => self.u64(),
                TreeMoveTypeLayout::U128 => self.u128(),
                TreeMoveTypeLayout::U256 => self.u256(),
                TreeMoveTypeLayout::Address => self.address(),
                TreeMoveTypeLayout::Signer => self.signer(),
                TreeMoveTypeLayout::Vector(inner) => {
                    let inner_ref = self.intern_tree(inner);
                    self.vector(inner_ref)
                }
                TreeMoveTypeLayout::Struct(s) => {
                    let fields: Vec<(&Identifier, LayoutRef)> = s
                        .fields
                        .iter()
                        .map(|f| (&f.name, self.intern_tree(&f.layout)))
                        .collect();
                    self.struct_layout(&s.type_, &fields)
                }
                TreeMoveTypeLayout::Enum(e) => {
                    let variants: Vec<(&Identifier, u16, Vec<(&Identifier, LayoutRef)>)> = e
                        .variants
                        .iter()
                        .map(|((variant_name, tag), field_layouts)| {
                            let fields: Vec<(&Identifier, LayoutRef)> = field_layouts
                                .iter()
                                .map(|f| (&f.name, self.intern_tree(&f.layout)))
                                .collect();
                            (variant_name, *tag, fields)
                        })
                        .collect();
                    let variant_refs: Vec<(
                        &Identifier,
                        u16,
                        Option<&[(&Identifier, LayoutRef)]>,
                    )> = variants
                        .iter()
                        .map(|(vn, tag, fields)| (*vn, *tag, Some(fields.as_slice())))
                        .collect();
                    self.enum_layout(&e.type_, &variant_refs)
                }
            }
        }

        pub fn build(self, root: LayoutRef) -> MoveTypeLayout {
            MoveTypeLayout {
                nodes: self.nodes.into_iter().collect(),
                strings: self.strings.into_iter().collect(),
                tags: self.tags.into_iter().collect(),
                root,
            }
        }
    }

    impl Default for MoveTypeLayoutBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl From<&TreeMoveTypeLayout> for MoveTypeLayout {
        fn from(layout: &TreeMoveTypeLayout) -> Self {
            let mut b = MoveTypeLayoutBuilder::new();
            let root = b.intern_tree(layout);
            b.build(root)
        }
    }

    // -------------------------------------------------------------------------
    // Deserialization — DeserializeSeed for &MoveLayoutView
    // -------------------------------------------------------------------------

    use super::{MoveStruct as AnnStruct, MoveValue as AnnValue, MoveVariant as AnnVariant};
    use crate::{VARIANT_TAG_MAX_VALUE, account_address::AccountAddress, u256};
    use serde::de::Error as _;

    impl<'d> serde::de::DeserializeSeed<'d> for MoveLayoutView<'_> {
        type Value = AnnValue;

        fn deserialize<D: serde::de::Deserializer<'d>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            match self {
                MoveLayoutView::Bool => bool::deserialize(deserializer).map(AnnValue::Bool),
                MoveLayoutView::U8 => u8::deserialize(deserializer).map(AnnValue::U8),
                MoveLayoutView::U16 => u16::deserialize(deserializer).map(AnnValue::U16),
                MoveLayoutView::U32 => u32::deserialize(deserializer).map(AnnValue::U32),
                MoveLayoutView::U64 => u64::deserialize(deserializer).map(AnnValue::U64),
                MoveLayoutView::U128 => u128::deserialize(deserializer).map(AnnValue::U128),
                MoveLayoutView::U256 => u256::U256::deserialize(deserializer).map(AnnValue::U256),
                MoveLayoutView::Address => {
                    AccountAddress::deserialize(deserializer).map(AnnValue::Address)
                }
                MoveLayoutView::Signer => {
                    AccountAddress::deserialize(deserializer).map(AnnValue::Signer)
                }
                MoveLayoutView::Struct { type_, fields: fv } => {
                    let fields = deserializer.deserialize_tuple(
                        fv.field_count(),
                        CompressedStructFieldVisitor(fv),
                    )?;
                    Ok(AnnValue::Struct(AnnStruct {
                        type_: type_.clone(),
                        fields,
                    }))
                }
                MoveLayoutView::Enum(ev) => {
                    let (variant_name, tag, fields) =
                        deserializer.deserialize_tuple(2, CompressedEnumFieldVisitor(ev))?;
                    Ok(AnnValue::Variant(AnnVariant {
                        type_: ev.type_().clone(),
                        variant_name,
                        tag,
                        fields,
                    }))
                }
                MoveLayoutView::Vector(vv) => {
                    let elem = vv.element();
                    Ok(AnnValue::Vector(
                        deserializer.deserialize_seq(CompressedVectorVisitor(elem))?,
                    ))
                }
            }
        }
    }

    struct CompressedVectorVisitor<'a>(MoveLayoutView<'a>);

    impl<'d> serde::de::Visitor<'d> for CompressedVectorVisitor<'_> {
        type Value = Vec<AnnValue>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("Vector")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'d>,
        {
            let mut vals = Vec::new();
            while let Some(elem) = seq.next_element_seed(self.0)? {
                vals.push(elem)
            }
            Ok(vals)
        }
    }

    struct CompressedStructFieldVisitor<'a>(MoveFieldView<'a>);

    impl<'d> serde::de::Visitor<'d> for CompressedStructFieldVisitor<'_> {
        type Value = Vec<(Identifier, AnnValue)>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("Struct")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'d>,
        {
            let mut vals = Vec::new();
            for (i, (name, field_view)) in self.0.fields().enumerate() {
                match seq.next_element_seed(field_view)? {
                    Some(val) => vals.push((name.clone(), val)),
                    None => return Err(A::Error::invalid_length(i, &self)),
                }
            }
            Ok(vals)
        }
    }

    struct CompressedEnumFieldVisitor<'a>(MoveEnumView<'a>);

    impl<'d> serde::de::Visitor<'d> for CompressedEnumFieldVisitor<'_> {
        type Value = (Identifier, u16, Vec<(Identifier, AnnValue)>);

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("Enum")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'d>,
        {
            let tag = match seq.next_element::<u8>()? {
                Some(tag) if tag as u64 <= VARIANT_TAG_MAX_VALUE => tag as u16,
                Some(tag) => return Err(A::Error::invalid_length(tag as usize, &self)),
                None => return Err(A::Error::invalid_length(0, &self)),
            };

            let (variant_name, field_view) = match self.0.variant_by_tag(tag) {
                Some((name, VariantFieldView::Known(fv))) => (name, fv),
                Some((_, VariantFieldView::Unknown)) => {
                    return Err(A::Error::custom(format!(
                        "cannot deserialize variant {tag}: layout unknown"
                    )));
                }
                None => return Err(A::Error::invalid_length(tag as usize, &self)),
            };

            let Some(fields) = seq.next_element_seed(CompressedVariantFieldSeed(field_view))?
            else {
                return Err(A::Error::invalid_length(1, &self));
            };

            Ok((variant_name.clone(), tag, fields))
        }
    }

    struct CompressedVariantFieldSeed<'a>(MoveFieldView<'a>);

    impl<'d> serde::de::DeserializeSeed<'d> for CompressedVariantFieldSeed<'_> {
        type Value = Vec<(Identifier, AnnValue)>;

        fn deserialize<D: serde::de::Deserializer<'d>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer
                .deserialize_tuple(self.0.field_count(), CompressedStructFieldVisitor(self.0))
        }
    }
}
