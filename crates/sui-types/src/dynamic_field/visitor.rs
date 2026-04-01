// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_core_types::{
    account_address::AccountAddress,
    annotated_value::{self as A, compressed_layouts as AC},
    annotated_visitor::{self, StructDriver, Visitor},
    language_storage::TypeTag,
    u256::U256,
    visitor_default,
};

use crate::{base_types::ObjectID, id::UID};

use super::{DynamicFieldInfo, DynamicFieldType};

/// Visitor to deserialize the outer structure of a `0x2::dynamic_field::Field` while leaving its
/// name and value untouched.
pub struct FieldVisitor;

#[derive(Debug, Clone)]
pub struct Field<'b, 'l> {
    pub id: ObjectID,
    pub kind: DynamicFieldType,
    pub name_layout: AC::MoveLayoutView<'l>,
    pub name_bytes: &'b [u8],
    pub value_layout: AC::MoveLayoutView<'l>,
    pub value_bytes: &'b [u8],
}

pub enum ValueMetadata {
    DynamicField(TypeTag),
    DynamicObjectField(ObjectID),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Not a dynamic field")]
    NotADynamicField,

    #[error("Not a dynamic object field")]
    NotADynamicObjectField,

    #[error("{0}")]
    Visitor(#[from] annotated_visitor::Error),
}

impl FieldVisitor {
    /// Deserialize the top-level structure from a dynamic field's `0x2::dynamic_field::Field`
    /// without having to fully deserialize its name or value.
    pub fn deserialize<'b, 'l>(
        bytes: &'b [u8],
        layout: &'l AC::MoveTypeLayout,
    ) -> Result<Field<'b, 'l>, Error> {
        Self::deserialize_view(bytes, layout.as_view())
    }

    /// Like [`deserialize`](Self::deserialize) but accepts a pre-resolved layout view.
    pub fn deserialize_view<'b, 'l>(
        bytes: &'b [u8],
        view: AC::MoveLayoutView<'l>,
    ) -> Result<Field<'b, 'l>, Error> {
        A::MoveValue::visit_deserialize(bytes, view, &mut FieldVisitor)
    }
}

impl Field<'_, '_> {
    /// If this field is a dynamic field, returns its value's type. If it is a dynamic object
    /// field, it returns the ID of the object the value points to (which must be fetched to
    /// extract its type).
    pub fn value_metadata(&self) -> Result<ValueMetadata, Error> {
        match self.kind {
            DynamicFieldType::DynamicField => Ok(ValueMetadata::DynamicField(TypeTag::from(
                self.value_layout,
            ))),

            DynamicFieldType::DynamicObject => {
                let id: ObjectID =
                    bcs::from_bytes(self.value_bytes).map_err(|_| Error::NotADynamicObjectField)?;
                Ok(ValueMetadata::DynamicObjectField(id))
            }
        }
    }
}

impl<'b, 'l> Visitor<'b, 'l> for FieldVisitor {
    type Value = Field<'b, 'l>;
    type Error = Error;

    // === Empty/default casees ===
    //
    // A dynamic field must be a struct, so if the visitor is fed anything else, it complains.
    visitor_default! { <'b, 'l> u8, u16, u32, u64, u128, u256 = Err(Error::NotADynamicField) }
    visitor_default! { <'b, 'l> bool, address, signer, vector, variant = Err(Error::NotADynamicField) }

    fn visit_struct(
        &mut self,
        driver: &mut StructDriver<'_, 'b, 'l>,
    ) -> Result<Self::Value, Error> {
        if !DynamicFieldInfo::is_dynamic_field(driver.struct_layout().type_()) {
            return Err(Error::NotADynamicField);
        }

        // Set-up optionals to fill while visiting fields -- all of them must be filled by the end
        // to successfully return a `Field`.
        let mut id = None;
        let mut name_parts = None;
        let mut value_parts = None;

        while let Some(field) = driver.peek_field() {
            match field.name().as_str() {
                "id" => {
                    let lo = driver.position();
                    driver.skip_field()?;
                    let hi = driver.position();

                    let layout = field.layout();
                    if !matches!(layout, AC::MoveLayoutView::Struct(sv) if *sv.type_() == UID::layout().type_)
                    {
                        return Err(Error::NotADynamicField);
                    }

                    // HACK: Bypassing `id`'s layout to deserialize its bytes as a Rust type.
                    let bytes = &driver.bytes()[lo..hi];
                    id = Some(ObjectID::from_bytes(bytes).map_err(|_| Error::NotADynamicField)?);
                }

                "name" => {
                    let lo = driver.position();
                    driver.skip_field()?;
                    let hi = driver.position();

                    let layout = field.layout();
                    let (kind, layout) = extract_name_layout(layout)?;
                    name_parts = Some((&driver.bytes()[lo..hi], layout, kind));
                }

                "value" => {
                    let lo = driver.position();
                    driver.skip_field()?;
                    let hi = driver.position();
                    value_parts = Some((&driver.bytes()[lo..hi], field.layout()));
                }

                _ => {
                    return Err(Error::NotADynamicField);
                }
            }
        }

        let (Some(id), Some((name_bytes, name_layout, kind)), Some((value_bytes, value_layout))) =
            (id, name_parts, value_parts)
        else {
            return Err(Error::NotADynamicField);
        };

        Ok(Field {
            id,
            kind,
            name_layout,
            name_bytes,
            value_layout,
            value_bytes,
        })
    }
}

/// Extract the type and layout of a dynamic field name, from the layout of its `Field.name`.
fn extract_name_layout<'l>(
    layout: AC::MoveLayoutView<'l>,
) -> Result<(DynamicFieldType, AC::MoveLayoutView<'l>), Error> {
    let AC::MoveLayoutView::Struct(sv) = layout else {
        return Ok((DynamicFieldType::DynamicField, layout));
    };

    if !DynamicFieldInfo::is_dynamic_object_field_wrapper(sv.type_()) {
        return Ok((DynamicFieldType::DynamicField, layout));
    }

    // Wrapper contains just one field
    if sv.field_count() != 1 {
        return Err(Error::NotADynamicField);
    }

    let Some((name, inner_layout)) = sv.field(0) else {
        return Err(Error::NotADynamicField);
    };

    // ...called `name`
    if name.as_str() != "name" {
        return Err(Error::NotADynamicField);
    }

    Ok((DynamicFieldType::DynamicObject, inner_layout))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use move_core_types::{
        account_address::AccountAddress, annotated_value as A, language_storage::TypeTag,
    };

    use crate::{
        base_types::ObjectID,
        dynamic_field,
        id::UID,
        object::bounded_visitor::tests::{enum_, layout_, value_, variant_},
    };

    use super::*;

    #[test]
    fn test_dynamic_field_name() {
        for (name, name_layout, name_bcs) in fixtures() {
            for (value, value_layout, value_bcs) in fixtures() {
                let df = serialized_df("0x264", name.clone(), value.clone());
                let df_layout = df_layout(name_layout.clone(), value_layout.clone());
                let compressed = AC::MoveTypeLayout::from(&df_layout);

                let field = FieldVisitor::deserialize(&df, &compressed).unwrap();

                assert_eq!(field.name_bytes, name_bcs);
                assert_eq!(field.value_bytes, value_bcs);

                assert_eq!(
                    TypeTag::from(field.name_layout),
                    TypeTag::from(&name_layout),
                );

                assert_eq!(
                    TypeTag::from(field.value_layout),
                    TypeTag::from(&value_layout),
                );
            }
        }
    }

    #[test]
    fn test_dynamic_object_field_name() {
        for (name, name_layout, name_bcs) in fixtures() {
            let df = serialized_dof("0x264", name.clone());
            let df_layout = dof_layout(name_layout.clone());
            let compressed = AC::MoveTypeLayout::from(&df_layout);

            let field = FieldVisitor::deserialize(&df, &compressed).unwrap();

            assert_eq!(field.name_bytes, name_bcs);
            assert_eq!(
                TypeTag::from(field.name_layout),
                TypeTag::from(&name_layout),
            );

            assert_eq!(
                TypeTag::from(field.value_layout),
                TypeTag::Address,
            );

            assert_eq!(field.kind, DynamicFieldType::DynamicObject);
        }
    }

    #[test]
    fn test_dynamic_field_enum_name() {
        let name = enum_("0x5::m::Foo", "V", 0, vec![("a", A::MoveValue::U64(42))]);
        let name_layout = A::MoveTypeLayout::Enum(Box::new(A::MoveEnumLayout {
            type_: "0x5::m::Foo".parse().unwrap(),
            variants: [(
                ("V".parse().unwrap(), 0),
                vec![A::MoveFieldLayout::new("a".parse().unwrap(), A::MoveTypeLayout::U64)],
            )]
            .into_iter()
            .collect(),
        }));
        let name_bcs = bcs::to_bytes(&name.clone().undecorate()).unwrap();

        let value = A::MoveValue::U64(100);
        let value_layout = A::MoveTypeLayout::U64;
        let value_bcs = bcs::to_bytes(&value.clone().undecorate()).unwrap();

        let df = serialized_df("0x264", name, value);
        let df_layout = df_layout(name_layout.clone(), value_layout.clone());
        let compressed = AC::MoveTypeLayout::from(&df_layout);

        let field = FieldVisitor::deserialize(&df, &compressed).unwrap();

        assert_eq!(field.name_bytes, name_bcs);
        assert_eq!(field.value_bytes, value_bcs);

        assert_eq!(
            TypeTag::from(field.name_layout),
            TypeTag::from(&name_layout),
        );

        assert_eq!(
            TypeTag::from(field.value_layout),
            TypeTag::from(&value_layout),
        );
    }

    #[test]
    fn test_dynamic_field_enum_value() {
        let name = A::MoveValue::U64(42);
        let name_layout = A::MoveTypeLayout::U64;
        let name_bcs = bcs::to_bytes(&name.clone().undecorate()).unwrap();

        let value = enum_("0x5::m::Bar", "W", 0, vec![("b", A::MoveValue::Bool(true))]);
        let value_layout = A::MoveTypeLayout::Enum(Box::new(A::MoveEnumLayout {
            type_: "0x5::m::Bar".parse().unwrap(),
            variants: [(
                ("W".parse().unwrap(), 0),
                vec![A::MoveFieldLayout::new("b".parse().unwrap(), A::MoveTypeLayout::Bool)],
            )]
            .into_iter()
            .collect(),
        }));
        let value_bcs = bcs::to_bytes(&value.clone().undecorate()).unwrap();

        let df = serialized_df("0x264", name, value);
        let df_layout = df_layout(name_layout.clone(), value_layout.clone());
        let compressed = AC::MoveTypeLayout::from(&df_layout);

        let field = FieldVisitor::deserialize(&df, &compressed).unwrap();

        assert_eq!(field.name_bytes, name_bcs);
        assert_eq!(field.value_bytes, value_bcs);

        assert_eq!(
            TypeTag::from(field.name_layout),
            TypeTag::from(&name_layout),
        );

        assert_eq!(
            TypeTag::from(field.value_layout),
            TypeTag::from(&value_layout),
        );
    }

    fn fixtures() -> Vec<(A::MoveValue, A::MoveTypeLayout, Vec<u8>)> {
        vec![
            (
                A::MoveValue::U64(42),
                A::MoveTypeLayout::U64,
                bcs::to_bytes(&42u64).unwrap(),
            ),
            (
                A::MoveValue::Bool(true),
                A::MoveTypeLayout::Bool,
                bcs::to_bytes(&true).unwrap(),
            ),
            (
                A::MoveValue::Address(AccountAddress::TWO),
                A::MoveTypeLayout::Address,
                bcs::to_bytes(&AccountAddress::TWO).unwrap(),
            ),
        ]
    }

    fn serialized_df(id: &str, name: A::MoveValue, value: A::MoveValue) -> Vec<u8> {
        let df = dynamic_field::Field {
            id: ObjectID::from_str(id).unwrap().into(),
            name: name.undecorate(),
            value: value.undecorate(),
        };
        bcs::to_bytes(&df).unwrap()
    }

    fn serialized_dof(id: &str, name: A::MoveValue) -> Vec<u8> {
        serialized_df(id, value_("0x2::object::Wrapper", vec![("name", name)]), addr("0x42"))
    }

    fn df_layout(name: A::MoveTypeLayout, value: A::MoveTypeLayout) -> A::MoveTypeLayout {
        layout_(
            "0x2::dynamic_field::Field<$0, $1>",
            name.clone(),
            value.clone(),
            vec![
                (
                    "id",
                    A::MoveTypeLayout::Struct(Box::new(UID::layout())),
                ),
                ("name", name),
                ("value", value),
            ],
        )
    }

    fn dof_layout(name: A::MoveTypeLayout) -> A::MoveTypeLayout {
        let wrapper = layout_(
            "0x2::object::Wrapper<$0>",
            name.clone(),
            A::MoveTypeLayout::U8, // placeholder
            vec![("name", name)],
        );

        df_layout(wrapper, A::MoveTypeLayout::Address)
    }

    fn addr(a: &str) -> A::MoveValue {
        A::MoveValue::Address(AccountAddress::from_str(a).unwrap())
    }

    fn value_(rep: &str, fields: Vec<(&str, A::MoveValue)>) -> A::MoveValue {
        let type_ = rep.parse().unwrap();
        let fields = fields
            .into_iter()
            .map(|(name, value)| {
                (
                    move_core_types::identifier::Identifier::new(name).unwrap(),
                    value,
                )
            })
            .collect();

        A::MoveValue::Struct(A::MoveStruct::new(type_, fields))
    }

    fn layout_(
        rep: &str,
        type_param_0: A::MoveTypeLayout,
        type_param_1: A::MoveTypeLayout,
        fields: Vec<(&str, A::MoveTypeLayout)>,
    ) -> A::MoveTypeLayout {
        use move_core_types::identifier::Identifier;

        let rep = rep
            .replace("$0", &TypeTag::from(&type_param_0).to_string())
            .replace("$1", &TypeTag::from(&type_param_1).to_string());
        let type_ = rep.parse().unwrap();
        let fields = fields
            .into_iter()
            .map(|(name, layout)| A::MoveFieldLayout::new(Identifier::new(name).unwrap(), layout))
            .collect();

        A::MoveTypeLayout::Struct(Box::new(A::MoveStructLayout { type_, fields }))
    }
}
