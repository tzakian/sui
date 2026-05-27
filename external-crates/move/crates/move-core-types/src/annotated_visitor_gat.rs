// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Visitor over the backend-abstracted compressed annotated layout
//! ([`crate::compressed::gat`], vendored from PR #26798).
//!
//! Structurally identical to [`crate::annotated_visitor_ref`], but every
//! driver is additionally generic over the storage backend `T: TypeLayout`,
//! so the same visitor drives any node-table backend (`ArcPool`, `BoxPool`,
//! …). Kept here purely for benchmarking the abstracted layout against the
//! hand-written `ref`/`exp` variants.

use std::io::{Cursor, Read};

use crate::{
    VARIANT_TAG_MAX_VALUE,
    account_address::AccountAddress,
    annotated_visitor::Error,
    compressed::gat::annotated::{
        MoveEnumLayout, MoveFieldsLayout, MoveLayoutView, MoveStructLayout, MoveTypeLayoutRef,
        TypeLayout, VariantLayout,
    },
    identifier::{IdentStr, Identifier},
    u256::U256,
};

/// A borrowed (name, layout) pair, as returned by [`StructDriver::peek_field`].
pub type Field<'a, T> = (&'a Identifier, MoveTypeLayoutRef<'a, T>);

/// Visitors can be used for building values out of a serialized Move struct or value.
pub trait Visitor<'a, 'b, T: TypeLayout> {
    type Value;
    type Error: From<Error>;

    fn visit_u8(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: u8,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_u16(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: u16,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_u32(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: u32,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_u64(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: u64,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_u128(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: u128,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_u256(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: U256,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_bool(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: bool,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_address(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: AccountAddress,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_signer(
        &mut self,
        driver: &ValueDriver<'_, 'a, 'b, T>,
        value: AccountAddress,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_vector(
        &mut self,
        driver: &mut VecDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_struct(
        &mut self,
        driver: &mut StructDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error>;

    fn visit_variant(
        &mut self,
        driver: &mut VariantDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error>;
}

/// Side-effect-only counterpart of [`Visitor`].
pub trait Traversal<'a, 'b, T: TypeLayout> {
    type Error: From<Error>;

    fn traverse_u8(&mut self, _: &ValueDriver<'_, 'a, 'b, T>, _: u8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_u16(&mut self, _: &ValueDriver<'_, 'a, 'b, T>, _: u16) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_u32(&mut self, _: &ValueDriver<'_, 'a, 'b, T>, _: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_u64(&mut self, _: &ValueDriver<'_, 'a, 'b, T>, _: u64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_u128(
        &mut self,
        _: &ValueDriver<'_, 'a, 'b, T>,
        _: u128,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_u256(
        &mut self,
        _: &ValueDriver<'_, 'a, 'b, T>,
        _: U256,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_bool(
        &mut self,
        _: &ValueDriver<'_, 'a, 'b, T>,
        _: bool,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_address(
        &mut self,
        _: &ValueDriver<'_, 'a, 'b, T>,
        _: AccountAddress,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn traverse_signer(
        &mut self,
        _: &ValueDriver<'_, 'a, 'b, T>,
        _: AccountAddress,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn traverse_vector(
        &mut self,
        driver: &mut VecDriver<'_, 'a, 'b, T>,
    ) -> Result<(), Self::Error> {
        while driver.next_element(self)?.is_some() {}
        Ok(())
    }

    fn traverse_struct(
        &mut self,
        driver: &mut StructDriver<'_, 'a, 'b, T>,
    ) -> Result<(), Self::Error> {
        while driver.next_field(self)?.is_some() {}
        Ok(())
    }

    fn traverse_variant(
        &mut self,
        driver: &mut VariantDriver<'_, 'a, 'b, T>,
    ) -> Result<(), Self::Error> {
        while driver.next_field(self)?.is_some() {}
        Ok(())
    }
}

impl<'a, 'b, T: TypeLayout, S: Traversal<'a, 'b, T> + ?Sized> Visitor<'a, 'b, T> for S {
    type Value = ();
    type Error = S::Error;

    fn visit_u8(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: u8,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u8(d, v)
    }
    fn visit_u16(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: u16,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u16(d, v)
    }
    fn visit_u32(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: u32,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u32(d, v)
    }
    fn visit_u64(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: u64,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u64(d, v)
    }
    fn visit_u128(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: u128,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u128(d, v)
    }
    fn visit_u256(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: U256,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_u256(d, v)
    }
    fn visit_bool(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: bool,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_bool(d, v)
    }
    fn visit_address(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: AccountAddress,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_address(d, v)
    }
    fn visit_signer(
        &mut self,
        d: &ValueDriver<'_, 'a, 'b, T>,
        v: AccountAddress,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_signer(d, v)
    }
    fn visit_vector(
        &mut self,
        d: &mut VecDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_vector(d)
    }
    fn visit_struct(
        &mut self,
        d: &mut StructDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_struct(d)
    }
    fn visit_variant(
        &mut self,
        d: &mut VariantDriver<'_, 'a, 'b, T>,
    ) -> Result<Self::Value, Self::Error> {
        self.traverse_variant(d)
    }
}

/// Drives a single value visit. Carries an optional borrowed layout — `None`
/// when constructed by an outer caller for a struct field.
pub struct ValueDriver<'c, 'a, 'b, T: TypeLayout> {
    bytes: &'c mut Cursor<&'b [u8]>,
    layout: Option<MoveTypeLayoutRef<'a, T>>,
    start: usize,
}

pub struct VecDriver<'c, 'a, 'b, T: TypeLayout> {
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveTypeLayoutRef<'a, T>,
    len: u64,
    off: u64,
}

pub struct StructDriver<'c, 'a, 'b, T: TypeLayout> {
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveStructLayout<'a, T>,
    off: u64,
}

pub struct VariantDriver<'c, 'a, 'b, T: TypeLayout> {
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveEnumLayout<'a, T>,
    tag: u16,
    variant_name: &'a Identifier,
    variant_layout: MoveFieldsLayout<'a, T>,
    off: u64,
}

/// Null traversal that reads every byte but produces no value.
pub struct NullTraversal;

impl<T: TypeLayout> Traversal<'_, '_, T> for NullTraversal {
    type Error = Error;
}

impl<'c, 'a, 'b, T: TypeLayout> ValueDriver<'c, 'a, 'b, T> {
    pub(crate) fn new(
        bytes: &'c mut Cursor<&'b [u8]>,
        layout: Option<MoveTypeLayoutRef<'a, T>>,
    ) -> Self {
        let start = bytes.position() as usize;
        Self {
            bytes,
            layout,
            start,
        }
    }

    pub fn start(&self) -> usize {
        self.start
    }
    pub fn position(&self) -> usize {
        self.bytes.position() as usize
    }
    pub fn bytes(&self) -> &'b [u8] {
        self.bytes.get_ref()
    }
    pub fn remaining_bytes(&self) -> &'b [u8] {
        &self.bytes.get_ref()[self.position()..]
    }

    pub fn layout(&self) -> Result<MoveTypeLayoutRef<'a, T>, Error> {
        self.layout.ok_or(Error::NoValueLayout)
    }

    fn read_exact<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut buf = [0u8; N];
        self.bytes
            .read_exact(&mut buf)
            .map_err(|_| Error::UnexpectedEof)?;
        Ok(buf)
    }

    fn read_leb128(&mut self) -> Result<u64, Error> {
        leb128::read::unsigned(self.bytes).map_err(|_| Error::UnexpectedEof)
    }
}

#[allow(clippy::len_without_is_empty)]
impl<'c, 'a, 'b, T: TypeLayout> VecDriver<'c, 'a, 'b, T> {
    fn new(inner: ValueDriver<'c, 'a, 'b, T>, layout: MoveTypeLayoutRef<'a, T>, len: u64) -> Self {
        Self {
            inner,
            layout,
            len,
            off: 0,
        }
    }

    pub fn start(&self) -> usize {
        self.inner.start()
    }
    pub fn position(&self) -> usize {
        self.inner.position()
    }
    pub fn bytes(&self) -> &'b [u8] {
        self.inner.bytes()
    }
    pub fn remaining_bytes(&self) -> &'b [u8] {
        self.inner.remaining_bytes()
    }

    pub fn layout(&self) -> Result<MoveTypeLayoutRef<'a, T>, Error> {
        self.inner.layout()
    }
    pub fn element_layout(&self) -> MoveTypeLayoutRef<'a, T> {
        self.layout
    }

    pub fn off(&self) -> u64 {
        self.off
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn has_element(&self) -> bool {
        self.off < self.len
    }

    pub fn next_element<V: Visitor<'a, 'b, T> + ?Sized>(
        &mut self,
        visitor: &mut V,
    ) -> Result<Option<V::Value>, V::Error> {
        Ok(if self.off >= self.len {
            None
        } else {
            let res = visit_value(self.inner.bytes, self.layout, visitor)?;
            self.off += 1;
            Some(res)
        })
    }

    pub fn skip_element(&mut self) -> Result<bool, Error> {
        self.next_element(&mut NullTraversal).map(|v| v.is_some())
    }
}

impl<'c, 'a, 'b, T: TypeLayout> StructDriver<'c, 'a, 'b, T> {
    fn new(inner: ValueDriver<'c, 'a, 'b, T>, layout: MoveStructLayout<'a, T>) -> Self {
        Self {
            inner,
            layout,
            off: 0,
        }
    }

    pub fn start(&self) -> usize {
        self.inner.start()
    }
    pub fn position(&self) -> usize {
        self.inner.position()
    }
    pub fn bytes(&self) -> &'b [u8] {
        self.inner.bytes()
    }
    pub fn remaining_bytes(&self) -> &'b [u8] {
        self.inner.remaining_bytes()
    }

    pub fn layout(&self) -> Result<MoveTypeLayoutRef<'a, T>, Error> {
        self.inner.layout()
    }

    pub fn struct_layout(&self) -> MoveStructLayout<'a, T> {
        self.layout
    }

    pub fn off(&self) -> u64 {
        self.off
    }

    /// The next field's borrowed name + borrowed layout, or `None` if exhausted.
    pub fn peek_field(&self) -> Option<Field<'a, T>> {
        self.layout.field(self.off as usize)
    }

    pub fn next_field<V: Visitor<'a, 'b, T> + ?Sized>(
        &mut self,
        visitor: &mut V,
    ) -> Result<Option<(Field<'a, T>, V::Value)>, V::Error> {
        let Some((name, field_layout)) = self.peek_field() else {
            return Ok(None);
        };
        let res = visit_value(self.inner.bytes, field_layout, visitor)?;
        self.off += 1;
        Ok(Some(((name, field_layout), res)))
    }

    pub fn skip_field(&mut self) -> Result<Option<Field<'a, T>>, Error> {
        self.next_field(&mut NullTraversal)
            .map(|res| res.map(|(f, _)| f))
    }
}

impl<'c, 'a, 'b, T: TypeLayout> VariantDriver<'c, 'a, 'b, T> {
    fn new(
        inner: ValueDriver<'c, 'a, 'b, T>,
        layout: MoveEnumLayout<'a, T>,
        variant_layout: MoveFieldsLayout<'a, T>,
        variant_name: &'a Identifier,
        tag: u16,
    ) -> Self {
        Self {
            inner,
            layout,
            tag,
            variant_name,
            variant_layout,
            off: 0,
        }
    }

    pub fn start(&self) -> usize {
        self.inner.start()
    }
    pub fn position(&self) -> usize {
        self.inner.position()
    }
    pub fn bytes(&self) -> &'b [u8] {
        self.inner.bytes()
    }
    pub fn remaining_bytes(&self) -> &'b [u8] {
        self.inner.remaining_bytes()
    }

    pub fn layout(&self) -> Result<MoveTypeLayoutRef<'a, T>, Error> {
        self.inner.layout()
    }

    pub fn enum_layout(&self) -> MoveEnumLayout<'a, T> {
        self.layout
    }

    pub fn variant_layout(&self) -> MoveFieldsLayout<'a, T> {
        self.variant_layout
    }

    pub fn tag(&self) -> u16 {
        self.tag
    }

    pub fn variant_name(&self) -> &'a IdentStr {
        self.variant_name.as_ident_str()
    }

    pub fn off(&self) -> u64 {
        self.off
    }

    pub fn peek_field(&self) -> Option<Field<'a, T>> {
        self.variant_layout.field(self.off as usize)
    }

    pub fn next_field<V: Visitor<'a, 'b, T> + ?Sized>(
        &mut self,
        visitor: &mut V,
    ) -> Result<Option<(Field<'a, T>, V::Value)>, V::Error> {
        let Some((name, field_layout)) = self.peek_field() else {
            return Ok(None);
        };
        let res = visit_value(self.inner.bytes, field_layout, visitor)?;
        self.off += 1;
        Ok(Some(((name, field_layout), res)))
    }

    pub fn skip_field(&mut self) -> Result<Option<Field<'a, T>>, Error> {
        self.next_field(&mut NullTraversal)
            .map(|res| res.map(|(f, _)| f))
    }
}

/// Visit a serialized Move value (`bytes`) against the borrowed `layout`.
pub fn visit_value<'c, 'a, 'b, T: TypeLayout, V: Visitor<'a, 'b, T> + ?Sized>(
    bytes: &'c mut Cursor<&'b [u8]>,
    layout: MoveTypeLayoutRef<'a, T>,
    visitor: &mut V,
) -> Result<V::Value, V::Error> {
    let view = layout.as_view();
    let mut driver = ValueDriver::new(bytes, Some(layout));
    match view {
        MoveLayoutView::Bool => match driver.read_exact()? {
            [0] => visitor.visit_bool(&driver, false),
            [1] => visitor.visit_bool(&driver, true),
            [b] => Err(Error::UnexpectedByte(b).into()),
        },
        MoveLayoutView::U8 => {
            let v = u8::from_le_bytes(driver.read_exact()?);
            visitor.visit_u8(&driver, v)
        }
        MoveLayoutView::U16 => {
            let v = u16::from_le_bytes(driver.read_exact()?);
            visitor.visit_u16(&driver, v)
        }
        MoveLayoutView::U32 => {
            let v = u32::from_le_bytes(driver.read_exact()?);
            visitor.visit_u32(&driver, v)
        }
        MoveLayoutView::U64 => {
            let v = u64::from_le_bytes(driver.read_exact()?);
            visitor.visit_u64(&driver, v)
        }
        MoveLayoutView::U128 => {
            let v = u128::from_le_bytes(driver.read_exact()?);
            visitor.visit_u128(&driver, v)
        }
        MoveLayoutView::U256 => {
            let v = U256::from_le_bytes(&driver.read_exact()?);
            visitor.visit_u256(&driver, v)
        }
        MoveLayoutView::Address => {
            let v = AccountAddress::new(driver.read_exact()?);
            visitor.visit_address(&driver, v)
        }
        MoveLayoutView::Signer => {
            let v = AccountAddress::new(driver.read_exact()?);
            visitor.visit_signer(&driver, v)
        }
        MoveLayoutView::Vector(inner) => visit_vector(driver, inner, visitor),
        MoveLayoutView::Struct(s) => visit_struct(driver, s, visitor),
        MoveLayoutView::Enum(e) => visit_variant(driver, e, visitor),
    }
}

fn visit_vector<'c, 'a, 'b, T: TypeLayout, V: Visitor<'a, 'b, T> + ?Sized>(
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveTypeLayoutRef<'a, T>,
    visitor: &mut V,
) -> Result<V::Value, V::Error> {
    let mut inner = inner;
    let len = inner.read_leb128()?;
    let mut driver = VecDriver::new(inner, layout, len);
    let res = visitor.visit_vector(&mut driver)?;
    while driver.skip_element()? {}
    Ok(res)
}

pub(crate) fn visit_struct<'c, 'a, 'b, T: TypeLayout, V: Visitor<'a, 'b, T> + ?Sized>(
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveStructLayout<'a, T>,
    visitor: &mut V,
) -> Result<V::Value, V::Error> {
    let mut driver = StructDriver::new(inner, layout);
    let res = visitor.visit_struct(&mut driver)?;
    while driver.skip_field()?.is_some() {}
    Ok(res)
}

fn visit_variant<'c, 'a, 'b, T: TypeLayout, V: Visitor<'a, 'b, T> + ?Sized>(
    inner: ValueDriver<'c, 'a, 'b, T>,
    layout: MoveEnumLayout<'a, T>,
    visitor: &mut V,
) -> Result<V::Value, V::Error> {
    let mut inner = inner;
    let [tag] = inner.read_exact()?;
    if tag > VARIANT_TAG_MAX_VALUE as u8 {
        return Err(Error::UnexpectedVariantTag(tag as usize).into());
    }
    let variant = layout
        .variant_by_tag(tag as u16)
        .ok_or(Error::UnexpectedVariantTag(tag as usize))?;
    let (variant_name, variant_layout) = match variant {
        VariantLayout::Known { name, fields, .. } => (name, fields),
        VariantLayout::Unknown { .. } => return Err(Error::NoValueLayout.into()),
    };

    let mut driver = VariantDriver::new(inner, layout, variant_layout, variant_name, tag as u16);
    let res = visitor.visit_variant(&mut driver)?;
    while driver.skip_field()?.is_some() {}
    Ok(res)
}
