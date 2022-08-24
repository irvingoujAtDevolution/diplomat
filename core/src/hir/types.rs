//! Types that can be exposed in Diplomat APIs.

use super::{
    EnumPath, MaybeOwn, NonOptional, OpaquePath, Optional, PrimitiveType, ReturnableStructPath,
    StructPath, TypeContext, TypeLifetime,
};
use crate::ast;

/// Type that can only be used as an output.
pub enum OutType {
    Primitive(PrimitiveType),
    Opaque(OpaquePath<Optional, MaybeOwn>),
    Struct(ReturnableStructPath),
    Enum(EnumPath),
    Slice(Slice),
}

/// Type that may be used as input or output.
pub enum Type {
    Primitive(PrimitiveType),
    Opaque(OpaquePath<Optional, Borrow>),
    Struct(StructPath),
    Enum(EnumPath),
    Slice(Slice),
}

/// Type that can appear in the `self` position.
pub enum SelfType {
    Opaque(OpaquePath<NonOptional, Borrow>),
    Struct(StructPath),
    Enum(EnumPath),
}

#[derive(Copy, Clone)]
pub enum Slice {
    /// A string slice, e.g. `&str`.
    Str(TypeLifetime),

    /// A primitive slice, e.g. `&mut [u8]`.
    Primitive(Borrow, PrimitiveType),
}

pub use ast::Mutability;

// For now, the lifetime in not optional. This is because when you have references
// as fields of structs, the lifetime must always be present, and we want to uphold
// this invariant at the type level within the HIR.
//
// The only time when a lifetime is optional in Rust code is in function signatures,
// where implicit lifetimes are allowed. Getting this to all fit together will
// involve getting the implicit lifetime thing to be understood by Diplomat, but
// should be doable.
#[derive(Copy, Clone)]
pub struct Borrow {
    pub lifetime: TypeLifetime,
    pub mutability: Mutability,
}

impl Type {
    /// Return the number of fields and leaves that will show up in the [`LifetimeTree`]
    /// returned by [`Param::lifetime_tree`] and [`ParamSelf::lifetime_tree`].
    ///
    /// This method is used to calculate how much space to allocate upfront.
    pub(super) fn field_leaf_lifetime_counts(&self, tcx: &TypeContext) -> (usize, usize) {
        match self {
            Type::Struct(ty) => ty.resolve(tcx).fields.iter().fold((1, 0), |acc, field| {
                let inner = field.ty.field_leaf_lifetime_counts(tcx);
                (acc.0 + inner.0, acc.1 + inner.1)
            }),
            Type::Opaque(_) | Type::Slice(_) => (0, 1),
            Type::Primitive(_) | Type::Enum(_) => (0, 0),
        }
    }
}

impl Slice {
    /// Returns the [`TypeLifetime`] contained in either the `Str` or `Primitive`
    /// variant.
    pub fn lifetime(&self) -> &TypeLifetime {
        match self {
            Slice::Str(lifetime) => lifetime,
            Slice::Primitive(reference, _) => &reference.lifetime,
        }
    }
}

impl Borrow {
    pub(super) fn from_ast(
        parent_lifetimes: &ast::LifetimeEnv,
        lifetime: &ast::Lifetime,
        mutability: Mutability,
    ) -> Self {
        Borrow {
            lifetime: TypeLifetime::from_ast(parent_lifetimes, lifetime),
            mutability,
        }
    }
}