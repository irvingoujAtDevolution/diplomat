//! Struct-type codegen.
//!
//! Two outputs per `StructDef`, mirroring the opaque split:
//!
//! 1. **Raw layer** (`Raw<Name>.cs`) — `[StructLayout(Sequential)]` struct
//!    holding the public fields by value, plus `[DllImport]` declarations
//!    for the struct's methods.
//! 2. **Idiomatic layer** (`<Name>.cs`) — public C# `partial struct` with
//!    PascalCase fields and wrapper methods that forward to the raw layer
//!    via `AsFFI()` / `FromFFI(...)` bridge helpers.
//!
//! No `IDisposable`, no `_inner`, no destructor — value type, GC handles
//! cleanup. The bridge methods exist because the raw and idiomatic structs
//! are *separate* C# types (in `Raw.X` vs `X` namespaces) even though they
//! share `[StructLayout(Sequential)]` layout.

use askama::Template;
use diplomat_core::hir::{self, IdentBuf, StructDef};

use crate::dotnet::r#gen::method::{self, MethodInfo, PropertyInfo};
use crate::dotnet::r#gen::{DotnetPrimitives, ItemGenContext};

// ─────────────────────────────────────────────────────────────────────────────
// Shared field view (used by both raw and idiomatic templates)
// ─────────────────────────────────────────────────────────────────────────────

struct StructField {
    /// PascalCase C# field name (e.g. `X`, `Y`), already run through the
    /// formatter so the template can drop it in verbatim.
    name: String,
    primitive_type: DotnetPrimitives,
}

// ─────────────────────────────────────────────────────────────────────────────
// Templates
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "dotnet/struct.raw.cs.jinja", escape = "none")]
struct RawStructTemplate<'ctx, 'tcx> {
    name: &'tcx IdentBuf,
    fields: Vec<StructField>,
    methods: Vec<MethodInfo<'tcx>>,
    dylib_name: &'ctx str,
    namespace: &'ctx str,
}

#[derive(Template)]
#[template(path = "dotnet/struct.impl.cs.jinja", escape = "none")]
struct ImplStructTemplate<'ctx, 'tcx> {
    name: &'tcx IdentBuf,
    namespace: &'ctx str,
    fields: Vec<StructField>,
    methods: Vec<MethodInfo<'tcx>>,
    properties: Vec<PropertyInfo>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Codegen entry points
// ─────────────────────────────────────────────────────────────────────────────

impl<'ctx, 'tcx> ItemGenContext<'ctx, 'tcx> {
    pub(crate) fn gen_struct_raw(&self, struct_def: &'tcx StructDef) -> Option<String> {
        let fields = lower_fields(struct_def, self);
        let methods: Vec<MethodInfo<'tcx>> = struct_def
            .methods
            .iter()
            .map(|m| self.build_method_info(m))
            .collect();

        Some(
            RawStructTemplate {
                dylib_name: self.dylib_name,
                namespace: self.namespace,
                name: &struct_def.name,
                fields,
                methods,
            }
            .render()
            .expect("Failed to render struct raw template"),
        )
    }

    pub(crate) fn gen_struct_impl(&self, struct_def: &'tcx StructDef) -> String {
        let fields = lower_fields(struct_def, self);
        let methods: Vec<MethodInfo<'tcx>> = struct_def
            .methods
            .iter()
            .map(|m| self.build_method_info(m))
            .collect();
        let properties = method::collect_properties(&methods);

        ImplStructTemplate {
            name: &struct_def.name,
            namespace: self.namespace,
            fields,
            methods,
            properties,
        }
        .render()
        .expect("Failed to render struct impl template")
    }
}

/// Lower a struct's fields into the `StructField` view used by both raw and
/// idiomatic templates. Shared so the two builders can't drift.
fn lower_fields<'ctx, 'tcx>(
    struct_def: &'tcx StructDef,
    ctx: &ItemGenContext<'ctx, 'tcx>,
) -> Vec<StructField> {
    struct_def
        .fields
        .iter()
        .map(|field| {
            let primitive_type = match &field.ty {
                hir::Type::Primitive(p) => DotnetPrimitives::from(p),
                _ => panic!("Only primitive fields are supported in structs: {field:?}"),
            };
            StructField {
                name: ctx.formatter.fmt_field_name(field.name.as_str()).into_owned(),
                primitive_type,
            }
        })
        .collect()
}
