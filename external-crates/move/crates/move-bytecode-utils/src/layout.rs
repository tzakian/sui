// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::module_cache::GetModule;
use anyhow::{Result, bail};
use move_binary_format::{
    CompiledModule,
    file_format::{
        DatatypeHandleIndex, DatatypeTyParameter, EnumDefinition, SignatureToken, StructDefinition,
        StructFieldInformation,
    },
    normalized::{self, Datatype, RcIdentifier, RcPool},
};
use move_core_types::{
    account_address::AccountAddress,
    annotated_value::{self as A, compressed_layouts as AC},
    identifier::{IdentStr, Identifier},
    language_storage::{ModuleId, StructTag, TypeTag},
};
use serde_reflection::{ContainerFormat, Format, Named, Registry, VariantFormat};
use std::{borrow::Borrow, collections::BTreeMap, fmt::Write, rc::Rc};

/// Name of the Move `address` type in the serde registry
const ADDRESS: &str = "AccountAddress";

/// Name of the Move `signer` type in the serde registry
const SIGNER: &str = "Signer";

/// Name of the Move `u256` type in the serde registry
const U256_SERDE_NAME: &str = "u256";

/// The maximal value depth that we allow creating a layout for.
const MAX_VALUE_DEPTH: u64 = 128;

macro_rules! check_depth {
    ($depth:expr) => {
        if $depth > MAX_VALUE_DEPTH {
            anyhow::bail!("Exceeded max value recursion depth when creating struct layout")
        }
    };
}

type Struct = normalized::Struct<RcIdentifier>;
type Enum = normalized::Enum<RcIdentifier>;
type Module = normalized::Module<RcIdentifier>;
type Type = normalized::Type<RcIdentifier>;

/// Build a `StructTag` for a datatype defined in module `m` with the given handle and type params.
fn struct_tag_for_datatype(
    m: &CompiledModule,
    handle: DatatypeHandleIndex,
    type_params: Vec<TypeTag>,
) -> StructTag {
    let dh = m.datatype_handle_at(handle);
    let mid = m.self_id();
    StructTag {
        address: *mid.address(),
        module: mid.name().to_owned(),
        name: m.identifier_at(dh.name).to_owned(),
        type_params,
    }
}

/// Convert a `SignatureToken` to a `TypeTag`. Used to populate `StructTag::type_params`
/// when building layouts from bytecode signature tokens (which don't carry TypeTags).
fn signature_token_to_type_tag(m: &CompiledModule, token: &SignatureToken) -> Result<TypeTag> {
    use SignatureToken::*;
    Ok(match token {
        Bool => TypeTag::Bool,
        U8 => TypeTag::U8,
        U16 => TypeTag::U16,
        U32 => TypeTag::U32,
        U64 => TypeTag::U64,
        U128 => TypeTag::U128,
        U256 => TypeTag::U256,
        Address => TypeTag::Address,
        Signer => TypeTag::Signer,
        Vector(inner) => {
            TypeTag::Vector(Box::new(signature_token_to_type_tag(m, inner)?))
        }
        Datatype(shi) => {
            let handle = m.datatype_handle_at(*shi);
            let module_handle = m.module_handle_at(handle.module);
            TypeTag::Struct(Box::new(StructTag {
                address: *m.address_identifier_at(module_handle.address),
                module: m.identifier_at(module_handle.name).to_owned(),
                name: m.identifier_at(handle.name).to_owned(),
                type_params: vec![],
            }))
        }
        DatatypeInstantiation(inst) => {
            let (shi, type_actuals) = &**inst;
            let handle = m.datatype_handle_at(*shi);
            let module_handle = m.module_handle_at(handle.module);
            let type_params = type_actuals
                .iter()
                .map(|t| signature_token_to_type_tag(m, t))
                .collect::<Result<Vec<_>>>()?;
            TypeTag::Struct(Box::new(StructTag {
                address: *m.address_identifier_at(module_handle.address),
                module: m.identifier_at(module_handle.name).to_owned(),
                name: m.identifier_at(handle.name).to_owned(),
                type_params,
            }))
        }
        TypeParameter(_) | Reference(_) | MutableReference(_) => {
            bail!("Cannot convert {:?} to TypeTag", token)
        }
    })
}

enum Container {
    Struct(Rc<Struct>),
    Enum(Rc<Enum>),
}

impl Container {
    fn type_parameters(&self) -> &[DatatypeTyParameter] {
        match self {
            Container::Struct(s) => &s.type_parameters,
            Container::Enum(e) => &e.type_parameters,
        }
    }
}

/// Type for building a registry of serde-reflection friendly struct layouts for Move types.
/// The layouts created by this type are intended to be passed to the serde-generate tool to create
/// struct bindings for Move types in source languages that use Move-based services.
pub struct SerdeLayoutBuilder<'a, T> {
    pool: RcPool,
    modules: BTreeMap<ModuleId, Module>,
    registry: Registry,
    module_resolver: &'a T,
    config: SerdeLayoutConfig,
}

#[derive(Default)]
pub struct SerdeLayoutConfig {
    /// If separator is Some, replace all Move source syntax separators ("::" for address/struct/module name
    /// separation, "<", ">", and "," for generics separation) with this string.
    /// If separator is None, use the same syntax as Move source
    pub separator: Option<String>,
    /// If true, do not include addresses in fully qualified type names.
    /// If there is a name conflict (e.g., the registry we're building has both
    /// 0x1::M::T and 0x2::M::T), layout generation will fail when this option is true.
    pub omit_addresses: bool,
    /// If true, do not include phantom types in fully qualified type names, since they do not contribute to the layout
    /// E.g., if we have `struct S<phantom T> { u: 64 }` and try to generate bindings for this struct with `T = u8`,
    /// the name for `S` in the registry will be `S<u64>` when this option is false, and `S` when this option is true
    pub ignore_phantom_types: bool,
    /// The LayoutBuilder can operate in two modes: "deep" and "shallow".
    /// In shallow mode, generate a single layout for the struct or type passed in by the user
    /// (under the assumption that layouts for dependencies have been generated previously).
    /// In deep mode, it generate layouts for all of the (transitive) dependencies of the type passed
    /// in, as well as layouts for the Move ground types like `address` and `signer`. The result is a
    /// self-contained registry with no unbound typenames
    pub shallow: bool,
}

impl<'a, T: GetModule> SerdeLayoutBuilder<'a, T> {
    /// Create a `LayoutBuilder` with an empty registry and deep layout resolution
    pub fn new(module_resolver: &'a T) -> Self {
        Self {
            pool: RcPool::new(),
            modules: BTreeMap::new(),
            registry: Self::default_registry(),
            module_resolver,
            config: SerdeLayoutConfig::default(),
        }
    }

    /// Create a `LayoutBuilder` with an empty registry and shallow layout resolution
    pub fn new_with_config(module_resolver: &'a T, config: SerdeLayoutConfig) -> Self {
        Self {
            pool: RcPool::new(),
            modules: BTreeMap::new(),
            registry: Self::default_registry(),
            module_resolver,
            config,
        }
    }

    /// Return a registry containing layouts for all the Move ground types (e.g., address)
    pub fn default_registry() -> Registry {
        let mut registry = BTreeMap::new();
        // add Move ground types to registry (address, signer)
        let address_layout = Box::new(Format::TupleArray {
            content: Box::new(Format::U8),
            size: AccountAddress::LENGTH,
        });
        registry.insert(
            ADDRESS.to_string(),
            ContainerFormat::NewTypeStruct(address_layout.clone()),
        );
        registry.insert(
            SIGNER.to_string(),
            ContainerFormat::NewTypeStruct(address_layout),
        );

        registry
    }

    /// Get the registry of layouts generated so far
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Get the registry of layouts generated so far
    pub fn into_registry(self) -> Registry {
        self.registry
    }

    /// Add layouts for all types used in `t` to the registry
    pub fn build_type_layout(&mut self, t: &TypeTag) -> Result<Format> {
        let ty = Type::from_type_tag(&mut self.pool, t);
        self.build_normalized_type_layout(&ty, &Vec::new(), 0)
    }

    /// Add layouts for all types used in `t` to the registry
    pub fn build_data_layout(&mut self, s: &StructTag) -> Result<Format> {
        let serde_type_args = s
            .type_params
            .iter()
            .map(|t| self.build_type_layout(t))
            .collect::<Result<Vec<Format>>>()?;
        self.build_data_layout_(&s.module_id(), &s.name, &serde_type_args, 0)
    }

    fn build_normalized_type_layout(
        &mut self,
        t: &Type,
        input_type_args: &[Format],
        depth: u64,
    ) -> Result<Format> {
        use Type as T;
        check_depth!(depth);
        Ok(match t {
            T::Bool => Format::Bool,
            T::U8 => Format::U8,
            T::U16 => Format::U16,
            T::U32 => Format::U32,
            T::U64 => Format::U64,
            T::U128 => Format::U128,
            T::U256 => Format::TypeName(U256_SERDE_NAME.to_string()),
            T::Address => Format::TypeName(ADDRESS.to_string()),
            T::Signer => Format::TypeName(SIGNER.to_string()),
            T::Datatype(dt) => {
                let Datatype {
                    module,
                    name,
                    type_arguments,
                } = &**dt;
                let serde_type_args = type_arguments
                    .iter()
                    .map(|t| self.build_normalized_type_layout(t, input_type_args, depth + 1))
                    .collect::<Result<Vec<Format>>>()?;
                let declaring_module = module.to_core_module_id(&self.pool);
                self.build_data_layout_(&declaring_module, name, &serde_type_args, depth + 1)?
            }
            T::Vector(inner_t) => {
                if matches!(inner_t.as_ref(), T::U8) {
                    // specialize vector<u8> as bytes
                    Format::Bytes
                } else {
                    Format::Seq(Box::new(self.build_normalized_type_layout(
                        inner_t,
                        input_type_args,
                        depth + 1,
                    )?))
                }
            }
            T::TypeParameter(i) => input_type_args[*i as usize].clone(),
            T::Reference(_, _) => unreachable!(), // structs cannot store references
        })
    }

    fn build_data_layout_(
        &mut self,
        module_id: &ModuleId,
        name: &Identifier,
        type_arguments: &[Format],
        depth: u64,
    ) -> Result<Format> {
        check_depth!(depth);

        // build a human-readable name for the struct type. this should do the same thing as
        // StructTag::display(), but it's not easy to use that code here

        let declaring_module = self
            .module_resolver
            .get_module_by_id(module_id)
            .map_err(|e| anyhow::format_err!("{:?}", e))?
            .expect("Failed to resolve module");
        let normalized = self.normalized_module(declaring_module.borrow());
        let def = match (
            normalized.structs.get(name.as_ident_str()),
            normalized.enums.get(name.as_ident_str()),
        ) {
            (Some(s), None) => Container::Struct(s.clone()),
            (None, Some(e)) => Container::Enum(e.clone()),
            (Some(_), Some(_)) => bail!("Found both struct and enum with name {}", name),
            (None, None) => bail!(
                "Could not find datatype named {} in module {}",
                name,
                declaring_module.borrow().name()
            ),
        };
        assert_eq!(
            def.type_parameters().len(),
            type_arguments.len(),
            "Wrong number of type arguments for struct"
        );

        let generics: Vec<String> = type_arguments
            .iter()
            .zip(def.type_parameters().iter())
            .filter(|(_, type_param)| {
                // do not include phantom type arguments in the struct key, since they do not affect the struct layout
                !(self.config.ignore_phantom_types && type_param.is_phantom)
            })
            .map(|(type_arg, _)| print_format_type(type_arg, depth))
            .collect::<Result<_>>()?;
        let mut type_key = String::new();
        if !self.config.omit_addresses {
            write!(
                type_key,
                "{}{}",
                module_id.address(),
                self.config.separator.as_deref().unwrap_or("::")
            )
            .unwrap();
        }
        write!(
            type_key,
            "{}{}{}",
            module_id.name(),
            self.config.separator.as_deref().unwrap_or("::"),
            name
        )
        .unwrap();
        if !generics.is_empty() {
            write!(
                type_key,
                "{}{}{}",
                self.config.separator.as_deref().unwrap_or("<"),
                generics.join(self.config.separator.as_deref().unwrap_or(",")),
                self.config.separator.as_deref().unwrap_or(">")
            )
            .unwrap()
        }
        if self.config.shallow {
            return Ok(Format::TypeName(type_key));
        }

        if let Some(old_datatype) = self.registry.get(&type_key) {
            if self.config.omit_addresses || self.config.separator.is_some() {
                // check for conflicts (e.g., 0x1::M::T and 0x2::M::T that both get stripped to M::T because
                // omit_addresses is on)
                if old_datatype.clone()
                    != self.generate_serde_container(def, type_arguments, depth)?
                {
                    bail!(
                        "Name conflict: multiple structs with name {}, but different addresses",
                        type_key
                    )
                }
            }
        } else {
            // not found--generate and update registry
            let serde_data = self.generate_serde_container(def, type_arguments, depth)?;
            self.registry.insert(type_key.clone(), serde_data);
        }

        Ok(Format::TypeName(type_key))
    }

    fn generate_serde_container(
        &mut self,
        container: Container,
        type_arguments: &[Format],
        depth: u64,
    ) -> Result<ContainerFormat> {
        match container {
            Container::Struct(s) => self.generate_serde_struct(&s, type_arguments, depth),
            Container::Enum(e) => self.generate_serde_enum(&e, type_arguments, depth),
        }
    }

    fn generate_serde_struct(
        &mut self,
        normalized_struct: &Struct,
        type_arguments: &[Format],
        depth: u64,
    ) -> Result<ContainerFormat> {
        check_depth!(depth);
        let fields = normalized_struct
            .fields
            .0
            .values()
            .map(|f| {
                self.build_normalized_type_layout(&f.type_, type_arguments, depth)
                    .map(|value| Named {
                        name: f.name.to_string(),
                        value,
                    })
            })
            .collect::<Result<Vec<Named<Format>>>>()?;
        Ok(ContainerFormat::Struct(fields))
    }

    fn generate_serde_enum(
        &mut self,
        normalized_enum: &Enum,
        type_arguments: &[Format],
        depth: u64,
    ) -> Result<ContainerFormat> {
        check_depth!(depth);
        let variants = normalized_enum
            .variants
            .values()
            .enumerate()
            .map(|(i, v)| {
                let fields = v
                    .fields
                    .0
                    .values()
                    .map(|f| {
                        self.build_normalized_type_layout(&f.type_, type_arguments, depth)
                            .map(|value| Named {
                                name: f.name.to_string(),
                                value,
                            })
                    })
                    .collect::<Result<Vec<Named<Format>>>>();
                fields.map(|fields| {
                    if fields.is_empty() {
                        (
                            i as u32,
                            Named {
                                name: v.name.to_string(),
                                value: VariantFormat::Unit,
                            },
                        )
                    } else {
                        (
                            i as u32,
                            Named {
                                name: v.name.to_string(),
                                value: VariantFormat::Struct(fields),
                            },
                        )
                    }
                })
            })
            .collect::<Result<BTreeMap<u32, Named<VariantFormat>>>>()?;
        Ok(ContainerFormat::Enum(variants))
    }

    fn normalized_module(&mut self, module: &CompiledModule) -> &Module {
        let id = module.self_id();
        if !self.modules.contains_key(&id) {
            let normalized = Module::new(&mut self.pool, module, /* include code */ false);
            self.modules.insert(id.clone(), normalized);
        }
        self.modules.get(&id).unwrap()
    }
}

fn print_format_type(t: &Format, depth: u64) -> Result<String> {
    check_depth!(depth);
    Ok(match t {
        Format::TypeName(s) => s.to_string(),
        Format::Bool => "bool".to_string(),
        Format::U8 => "u8".to_string(),
        Format::U16 => "u16".to_string(),
        Format::U32 => "u32".to_string(),
        Format::U64 => "u64".to_string(),
        Format::U128 => "u128".to_string(),
        Format::Bytes => "vector<u8>".to_string(),
        Format::Seq(inner) => format!("vector<{}>", print_format_type(inner, depth + 1)?),
        v => unimplemented!("Printing format value {:?}", v),
    })
}

pub enum TypeLayoutBuilder {}
pub enum DatatypeLayoutBuilder {}

impl TypeLayoutBuilder {
    /// Construct a compressed annotated type layout from a `TypeTag`.
    pub fn build_with_types(
        t: &TypeTag,
        resolver: &impl GetModule,
    ) -> Result<AC::MoveTypeLayout> {
        let mut builder = AC::MoveTypeLayoutBuilder::new();
        let root = Self::build_impl(t, resolver, 0, &mut builder)?;
        Ok(builder.build(root))
    }

    /// Construct a tree-based annotated type layout (convenience wrapper).
    pub fn build_with_types_tree(
        t: &TypeTag,
        resolver: &impl GetModule,
    ) -> Result<A::MoveTypeLayout> {
        Self::build_with_types(t, resolver)?
            .inflate()
            .map_err(|e| e.into())
    }

    fn build_impl(
        t: &TypeTag,
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        use TypeTag::*;
        check_depth!(depth);
        Ok(match t {
            Bool => builder.bool(),
            U8 => builder.u8(),
            U16 => builder.u16(),
            U32 => builder.u32(),
            U64 => builder.u64(),
            U128 => builder.u128(),
            U256 => builder.u256(),
            Address => builder.address(),
            Signer => bail!("Type layouts cannot contain signer"),
            Vector(elem_t) => {
                let inner = Self::build_impl(elem_t, resolver, depth + 1, builder)?;
                builder.vector(inner)
            }
            Struct(s) => {
                DatatypeLayoutBuilder::build_impl(s, resolver, depth + 1, builder)?
            }
        })
    }

    fn build_from_signature_token(
        m: &CompiledModule,
        s: &SignatureToken,
        type_arguments: &[AC::LayoutHandle],
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        use SignatureToken::*;
        check_depth!(depth);
        Ok(match s {
            Vector(t) => {
                let inner = Self::build_from_signature_token(
                    m, t, type_arguments, resolver, depth + 1, builder,
                )?;
                builder.vector(inner)
            }
            Datatype(shi) => {
                DatatypeLayoutBuilder::build_from_handle_idx(
                    m, *shi, vec![], &[], resolver, depth + 1, builder,
                )?
            }
            DatatypeInstantiation(inst) => {
                let (shi, type_actuals) = &**inst;
                let actual_layouts = type_actuals
                    .iter()
                    .map(|t| {
                        Self::build_from_signature_token(
                            m, t, type_arguments, resolver, depth + 1, builder,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                // Compute TypeTags for the StructTag's type_params.
                let actual_type_tags = type_actuals
                    .iter()
                    .map(|t| signature_token_to_type_tag(m, t))
                    .collect::<Result<Vec<_>>>()?;
                DatatypeLayoutBuilder::build_from_handle_idx(
                    m, *shi, actual_layouts, &actual_type_tags, resolver, depth + 1, builder,
                )?
            }
            TypeParameter(i) => type_arguments[*i as usize],
            Bool => builder.bool(),
            U8 => builder.u8(),
            U16 => builder.u16(),
            U32 => builder.u32(),
            U64 => builder.u64(),
            U128 => builder.u128(),
            U256 => builder.u256(),
            Address => builder.address(),
            Signer => bail!("Type layouts cannot contain signer"),
            Reference(_) | MutableReference(_) => bail!("Type layouts cannot contain references"),
        })
    }
}

impl DatatypeLayoutBuilder {
    fn build_impl(
        s: &StructTag,
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        check_depth!(depth);
        let type_arguments = s
            .type_params
            .iter()
            .map(|t| TypeLayoutBuilder::build_impl(t, resolver, depth, builder))
            .collect::<Result<Vec<AC::LayoutHandle>>>()?;
        Self::build_from_name(
            &s.module_id(),
            &s.name,
            type_arguments,
            &s.type_params,
            resolver,
            depth,
            builder,
        )
    }

    fn build_from_enum_definition(
        m: &CompiledModule,
        e: &EnumDefinition,
        type_arguments: Vec<AC::LayoutHandle>,
        type_params: &[TypeTag],
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        let e_handle = m.datatype_handle_at(e.enum_handle);
        if e_handle.type_parameters.len() != type_arguments.len() {
            bail!("Wrong number of type arguments for enum")
        }
        let type_ = struct_tag_for_datatype(m, e.enum_handle, type_params.to_vec());

        let mut owned_variants: Vec<(Identifier, u16, Vec<(Identifier, AC::LayoutHandle)>)> =
            vec![];
        for (i, variant) in e.variants.iter().enumerate() {
            let variant_name = m.identifier_at(variant.variant_name).to_owned();
            let fields = variant
                .fields
                .iter()
                .map(|field| {
                    let h = TypeLayoutBuilder::build_from_signature_token(
                        m,
                        &field.signature.0,
                        &type_arguments,
                        resolver,
                        depth,
                        builder,
                    )?;
                    let name = m.identifier_at(field.name).to_owned();
                    Ok((name, h))
                })
                .collect::<Result<Vec<_>>>()?;
            owned_variants.push((variant_name, i as u16, fields));
        }

        let field_ref_vecs: Vec<Vec<(&Identifier, AC::LayoutHandle)>> = owned_variants
            .iter()
            .map(|(_, _, fields)| fields.iter().map(|(n, h)| (n, *h)).collect())
            .collect();
        let variant_refs: Vec<(&Identifier, u16, Option<&[(&Identifier, AC::LayoutHandle)]>)> =
            owned_variants
                .iter()
                .zip(field_ref_vecs.iter())
                .map(|((vn, tag, _), fields)| (vn, *tag, Some(fields.as_slice())))
                .collect();
        Ok(builder.enum_layout(&type_, &variant_refs))
    }

    fn build_from_struct_definition(
        m: &CompiledModule,
        s: &StructDefinition,
        type_arguments: Vec<AC::LayoutHandle>,
        type_params: &[TypeTag],
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        check_depth!(depth);
        let s_handle = m.datatype_handle_at(s.struct_handle);
        if s_handle.type_parameters.len() != type_arguments.len() {
            bail!("Wrong number of type arguments for struct")
        }
        match &s.field_information {
            StructFieldInformation::Native => {
                bail!("Can't extract fields for native struct")
            }
            StructFieldInformation::Declared(fields) => {
                let field_handles: Vec<(Identifier, AC::LayoutHandle)> = fields
                    .iter()
                    .map(|f| {
                        let h = TypeLayoutBuilder::build_from_signature_token(
                            m,
                            &f.signature.0,
                            &type_arguments,
                            resolver,
                            depth,
                            builder,
                        )?;
                        let name = m.identifier_at(f.name).to_owned();
                        Ok((name, h))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let type_ = struct_tag_for_datatype(m, s.struct_handle, type_params.to_vec());
                let field_refs: Vec<(&Identifier, AC::LayoutHandle)> =
                    field_handles.iter().map(|(n, h)| (n, *h)).collect();
                Ok(builder.struct_layout(&type_, &field_refs))
            }
        }
    }

    fn build_from_name(
        declaring_module: &ModuleId,
        name: &IdentStr,
        type_arguments: Vec<AC::LayoutHandle>,
        type_params: &[TypeTag],
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        check_depth!(depth);
        let module = match resolver.get_module_by_id(declaring_module) {
            Err(_) | Ok(None) => bail!("Could not find module"),
            Ok(Some(m)) => m,
        };
        match (
            module.borrow().find_struct_def_by_name(name.as_str()),
            module.borrow().find_enum_def_by_name(name.as_str()),
        ) {
            (Some((_, struct_def)), None) => Self::build_from_struct_definition(
                module.borrow(),
                struct_def,
                type_arguments,
                type_params,
                resolver,
                depth,
                builder,
            ),
            (None, Some((_, enum_def))) => Self::build_from_enum_definition(
                module.borrow(),
                enum_def,
                type_arguments,
                type_params,
                resolver,
                depth,
                builder,
            ),
            (Some(_), Some(_)) => bail!("Found both struct and enum with name {}", name),
            (None, None) => bail!(
                "Could not find struct/enum named {} in module {}",
                name,
                module.borrow().name()
            ),
        }
    }

    fn build_from_handle_idx(
        m: &CompiledModule,
        s: DatatypeHandleIndex,
        type_arguments: Vec<AC::LayoutHandle>,
        type_params: &[TypeTag],
        resolver: &impl GetModule,
        depth: u64,
        builder: &mut AC::MoveTypeLayoutBuilder,
    ) -> Result<AC::LayoutHandle> {
        check_depth!(depth);
        if let Some(def) = m.find_struct_def(s) {
            Self::build_from_struct_definition(
                m, def, type_arguments, type_params, resolver, depth, builder,
            )
        } else if let Some(def) = m.find_enum_def(s) {
            Self::build_from_enum_definition(
                m, def, type_arguments, type_params, resolver, depth, builder,
            )
        } else {
            let handle = m.datatype_handle_at(s);
            let name = m.identifier_at(handle.name);
            let declaring_module = m.module_id_for_handle(m.module_handle_at(handle.module));
            Self::build_from_name(
                &declaring_module, name, type_arguments, type_params, resolver, depth, builder,
            )
        }
    }
}
