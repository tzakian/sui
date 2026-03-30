// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::error::{SuiError, SuiErrorKind};
use move_bytecode_utils::{layout::TypeLayoutBuilder, module_cache::GetModule};
use move_core_types::{
    annotated_value::{self as A, compressed_layouts as AC},
    language_storage::{StructTag, TypeTag},
};

pub trait LayoutResolver {
    fn get_annotated_layout(
        &mut self,
        struct_tag: &StructTag,
    ) -> Result<AC::MoveDatatypeLayout, SuiError>;
}

pub fn get_layout_from_struct_tag(
    struct_tag: StructTag,
    resolver: &impl GetModule,
) -> Result<AC::MoveDatatypeLayout, SuiError> {
    let type_ = TypeTag::Struct(Box::new(struct_tag));
    let layout = TypeLayoutBuilder::build_with_types(&type_, resolver).map_err(|e| {
        SuiErrorKind::ObjectSerializationError {
            error: e.to_string(),
        }
    })?;
    AC::MoveDatatypeLayout::new(layout).ok_or_else(|| {
        SuiErrorKind::ObjectSerializationError {
            error: "Expected struct or enum layout".to_string(),
        }
        .into()
    })
}

pub fn into_struct_layout(layout: A::MoveDatatypeLayout) -> Result<A::MoveStructLayout, SuiError> {
    match layout {
        A::MoveDatatypeLayout::Struct(s) => Ok(*s),
        A::MoveDatatypeLayout::Enum(e) => Err(SuiErrorKind::ObjectSerializationError {
            error: format!("Expected struct layout but got an enum {e:?}"),
        }
        .into()),
    }
}
