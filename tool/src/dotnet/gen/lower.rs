//! Type-leaf lowering.
//!
//! Pure functions that map a single HIR type fragment to its **bare** C#
//! name (e.g. `"Color"`, `"Point2D"`). Callers compose the form they need:
//! append `"*"` for the FFI pointer, prefix `"Raw."` for the raw namespace.
//!
//! Shared across opaque / struct / enum codegen.

use diplomat_core::hir::{Borrow, EnumPath, MaybeOwn, OpaquePath, Optional, ReturnableStructPath, StructPath};

use super::ItemGenContext;

impl<'ctx, 'tcx> ItemGenContext<'ctx, 'tcx> {
    /// Bare C# name of an *owned* opaque (`Box<T>` in return position).
    /// Returns `"Color"`; append `"*"` for the FFI pointer form.
    pub(super) fn opaque_name(&self, opaque_path: &OpaquePath<Optional, MaybeOwn>) -> String {
        opaque_path.resolve(self.tcx).name.as_str().to_string()
    }

    pub(super) fn enum_name(&self, enum_path: &EnumPath) -> String {
        enum_path.resolve(self.tcx).name.as_str().to_string()
    }

    pub(super) fn returnable_struct_name(&self, struct_path: &ReturnableStructPath) -> String {
        match struct_path {
            ReturnableStructPath::Struct(p) => p.resolve(self.tcx).name.as_str().to_string(),
            ReturnableStructPath::OutStruct(_) => todo!("out struct returns not yet supported"),
            _ => todo!("unsupported struct return type"),
        }
    }

    /// Bare C# name of a *borrowed* opaque (`&T` / `&mut T`). Returns `"Color"`;
    /// callers append `"*"` for the FFI pointer form on params and self.
    pub(super) fn opaque_name_borrowed<O>(
        &self,
        opaque_path: &OpaquePath<O, Borrow>,
    ) -> String {
        opaque_path.resolve(self.tcx).name.as_str().to_string()
    }


    /// Bare C# name of a struct path (e.g. `"Point2D"`). Used for struct-self
    /// params; the raw extern takes the struct by value.
    pub(super) fn struct_name(&self, struct_path: &StructPath) -> String {
        struct_path.resolve(self.tcx).name.as_str().to_string()
    }
}
