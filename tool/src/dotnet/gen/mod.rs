//! C# code generation for the .NET backend.
//!
//! Skeleton only: most paths are `todo!()` / `unimplemented!()` and meant to be
//! filled in incrementally — opaques first, then primitives, then slices,
//! strings, results, options, and finally callbacks.
//!
//! Module layout:
//!
//! * [`opaque`] — `Raw[T].cs` `[DllImport]` declarations + the idiomatic
//!   `IDisposable`-shaped wrapper class. Self-contained for a single
//!   `OpaqueDef`.
//! * [`lower`] — pure type-leaf lowering shared across opaque / struct /
//!   enum: primitives → C# keywords, opaque paths → `T*`. New backends
//!   (struct, slice) reuse these directly.
//! * `mod.rs` (this file) — [`ItemGenContext`] context struct, the enum
//!   template, and the public dispatch entry points (`gen_enum`,
//!   `gen_opaque`) the parent module routes types to.

use std::{
    cell::RefCell,
    collections::HashMap,
    fmt::{self, Display},
};

use askama::Template;
use diplomat_core::hir::{
    self, DocsUrlGenerator, EnumDef, OpaqueDef, OutStructDef, StructDef, TypeContext,
};
use heck::ToUpperCamelCase;

use crate::dotnet::r#gen::callback::DotnetCallback;
use crate::{dotnet::gen::fillable::DotnetResult, ErrorStore};

use super::formatter::DotnetFormatter;

mod callback;
pub(super) mod fillable;
mod impl_struct;
mod lower;
mod method;
mod opaque;

// ─────────────────────────────────────────────────────────────────────────────
// Codegen context
// ─────────────────────────────────────────────────────────────────────────────

/// Carries everything `gen_*` methods need to render a single type.
///
/// Mirrors the role of `kotlin::ItemGenContext` / `cpp::ItemGenContext`. Built
/// once in `mod.rs::run` and reused across every type in the `TypeContext`.
#[allow(dead_code)] // fields will be used as gen_* methods are filled in
pub(super) struct ItemGenContext<'ctx, 'tcx> {
    pub tcx: &'tcx TypeContext,
    pub formatter: &'ctx DotnetFormatter<'tcx>,
    pub errors: &'ctx ErrorStore<'tcx, String>,
    pub docs_url_gen: &'ctx DocsUrlGenerator,
    /// Crate-style library name (e.g. `dotnet_smoke`). From `[shared] lib_name`
    /// in the config; used for naming and as the default for `dylib_name`.
    pub lib_name: &'ctx str,
    /// The native cdylib name used in `[DllImport("...")]`. Defaults to
    /// `lib_name` unless `[dotnet] dylib_name` overrides it.
    pub dylib_name: &'ctx str,
    /// The C# namespace generated files declare. Defaults to `lib_name`
    /// in `UpperCamelCase` unless `[dotnet] namespace` overrides it.
    pub namespace: &'ctx str,
    pub exception_trim_suffix: Option<&'ctx str>,
    pub exception_message_method: Option<&'ctx str>,
    pub getters_prefix: Option<&'ctx str>,
    pub setters_prefix: Option<&'ctx str>,

    pub result_struct_registry: RefCell<HashMap<String, DotnetResult>>,
    pub option_struct_registry: RefCell<HashMap<String, fillable::DotnetOption>>,
    pub callback_struct_registry: RefCell<HashMap<String, DotnetCallback>>,
}

#[derive(Template)]
#[template(path = "dotnet/enum.cs.jinja", escape = "none")]
struct EnumTemplate<'ctx> {
    namespace: &'ctx str,
    name: String,
    variants: Vec<EnumVariantInfo>,
}

struct EnumVariantInfo {
    name: String,
    discriminant: isize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Codegen entry points
// ─────────────────────────────────────────────────────────────────────────────

impl<'ctx, 'tcx> ItemGenContext<'ctx, 'tcx> {
    pub(super) fn gen_enum(&self, enum_def: &'tcx EnumDef) -> (Option<String>, String) {
        // Preserve the Rust source's casing verbatim — `RDCleanPathResultType`
        // stays `RDCleanPathResultType`, not the heck-mangled
        // `RdCleanPathResultType`. Matches Benoit's checked-in output and
        // the casing used by method bodies referencing this enum.
        let name = enum_def.name.as_str().to_string();
        let variants = enum_def
            .variants
            .iter()
            .map(|variant| EnumVariantInfo {
                name: self.formatter.fmt_enum_variant(variant).into_owned(),
                discriminant: variant.discriminant,
            })
            .collect();
        (
            None,
            EnumTemplate {
                namespace: self.namespace,
                name,
                variants,
            }
            .render()
            .unwrap(),
        )
    }

    pub(crate) fn gen_opaque(&self, opaque_def: &'tcx OpaqueDef) -> (Option<String>, String) {
        (
            self.gen_opaque_raw(opaque_def),
            self.gen_opaque_impl(opaque_def),
        )
    }

    pub(crate) fn gen_struct(&self, struct_def: &'tcx StructDef) -> (Option<String>, String) {
        (
            self.gen_struct_raw(struct_def),
            self.gen_struct_impl(struct_def),
        )
    }

    pub(crate) fn gen_out_struct(
        &self,
        _out_struct_def: &'tcx OutStructDef,
    ) -> (Option<String>, String) {
        todo!()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Primitive type lowering
// ─────────────────────────────────────────────────────────────────────────────

/// C# primitive-type keywords. Mirror of `hir::PrimitiveType` but in C#'s
/// vocabulary — built so the type knows how to render itself (`Display`)
/// instead of a function returning `&'static str`.
///
/// Unimplemented variants (Char, Byte, Ordering, IntSize, Int128, Float) hit
/// `todo!()` on construction, preserving the existing behavior of
/// `lower::primitives_to_dotnet_type`.
#[derive(Debug, Clone)]
pub(super) enum DotnetPrimitives {
    Bool,
    SByte,
    Short,
    Int,
    Long,
    Byte,
    NInt,
    NUInt,
    UShort,
    UInt,
    ULong,
    Float,
    Double,
}

impl From<&hir::PrimitiveType> for DotnetPrimitives {
    fn from(primitive: &hir::PrimitiveType) -> Self {
        match primitive {
            hir::PrimitiveType::Bool => Self::Bool,
            hir::PrimitiveType::Char => Self::UInt,
            hir::PrimitiveType::Byte => Self::Byte,
            hir::PrimitiveType::Ordering => todo!(),
            hir::PrimitiveType::Int(int_type) => match int_type {
                hir::IntType::I8 => Self::SByte,
                hir::IntType::I16 => Self::Short,
                hir::IntType::I32 => Self::Int,
                hir::IntType::I64 => Self::Long,
                hir::IntType::U8 => Self::Byte,
                hir::IntType::U16 => Self::UShort,
                hir::IntType::U32 => Self::UInt,
                hir::IntType::U64 => Self::ULong,
            },
            hir::PrimitiveType::IntSize(int_size_type) => match int_size_type {
                hir::IntSizeType::Isize => Self::NInt,
                hir::IntSizeType::Usize => Self::NUInt,
            },
            hir::PrimitiveType::Int128(_) => todo!(),
            hir::PrimitiveType::Float(float_type) => match float_type {
                hir::FloatType::F32 => Self::Float,
                hir::FloatType::F64 => Self::Double,
            },
        }
    }
}

impl Display for DotnetPrimitives {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bool => "bool",
            Self::SByte => "sbyte",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
            Self::Byte => "byte",
            Self::NInt => "nint",
            Self::NUInt => "nuint",
            Self::UShort => "ushort",
            Self::UInt => "uint",
            Self::ULong => "ulong",
            Self::Float => "float",
            Self::Double => "double",
        })
    }
}
