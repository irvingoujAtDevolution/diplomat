//! Selective borrow-source classification.
//!
//! The .NET backend used to give every opaque wrapper its own
//! reference-counted native handle (`RustHandleState<T>` / universal RC),
//! whether or not anything ever actually borrowed from it. This module is the
//! seam that instead classifies each opaque type, based only on the borrow
//! edges its methods actually produce, into one of four roles:
//!
//! * **neither** — an ordinary opaque nothing borrows from and that borrows
//!   nothing itself. Gets the plain, zero-allocation `RustHandle<T>` lane
//!   (see `RustHandle.cs.jinja`) — the same shape upstream #1244 used before
//!   the universal RC redesign.
//! * **dependent only** — borrows from another opaque (or pins one of its own
//!   input buffers) but nothing borrows from *it*. Still uses the plain
//!   `RustHandle<T>` for its own pointer, plus a couple of extra fields to
//!   hold/release what it borrowed — but never allocates its own RC state,
//!   since nothing needs to retain a reference to *this* wrapper.
//! * **source only** — nothing it returns borrows from anything, but at
//!   least one *other* wrapper borrows from it. Needs the reference-counted
//!   `RcRustHandle<T>` lane so a dependent can outlive this wrapper's own
//!   managed lifetime.
//! * **both** — e.g. a self-referential borrowed view (`fn view(&self) ->
//!   &Self`): the type is simultaneously a source (something borrows from
//!   it — potentially itself) and a dependent (it holds a borrow of its
//!   own). Gets the `RcRustHandle<T>` lane, exactly like source-only.
//!
//! [`LifetimePlan`] is folded once, run-level, over every method's already-
//! computed keep-alive data (see `gen::method::output_keep_alive_edges`) —
//! it does not re-walk HIR lifetimes itself. Classification only looks at
//! `OpaqueParam` edges that already made it into a method's structured
//! [`OpaqueBorrowSource`] list; an edge kind the walker can't yet support
//! still fails with a diagnostic there, before this module ever sees it.

use std::collections::HashMap;

use diplomat_core::hir;

/// One opaque-typed borrow source contributing to a method's keep-alive set:
/// the source's own `hir::OpaqueId` (for role classification) alongside the
/// C# expression that names it at the call site (`"this"` or a parameter's
/// local name — see `gen::method::dependencies_array_expr`).
///
/// Carrying the id alongside the expression (rather than just the bare
/// string the pre-selective design used) is what lets [`LifetimePlan`]
/// classify the *referenced* opaque type without re-deriving it from the
/// expression text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpaqueBorrowSource {
    pub(crate) opaque_id: hir::OpaqueId,
    pub(crate) expression: String,
}

/// An opaque type's classified role in the run's borrow graph. `is_source`
/// and `is_dependent` are independent bits — see the module doc for the four
/// combinations. `has_pins` is an implementation extension beyond the two
/// RC-relevant bits: pins are emitted independent of RC role (a dependent-only
/// *or* an ordinary-looking-at-RC-but-pinning type still needs somewhere to
/// hold its own pinned input buffers), so it is tracked separately rather
/// than folded into `is_dependent`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct OpaqueLifetimeRole {
    /// At least one *other* method (of any type in the run) returns a value
    /// that retains a borrow of this opaque — this type needs the
    /// reference-counted `RcRustHandle<T>` lane.
    pub(super) is_source: bool,
    /// At least one of this type's own methods returns/constructs a value of
    /// this same type that borrows from something else — this type needs to
    /// hold (and release, after its own Rust destructor runs) one or more
    /// `IRustHandleDependency` tokens.
    pub(super) is_dependent: bool,
    /// At least one of this type's own methods pins one of its own input
    /// buffers for the returned value's lifetime.
    pub(super) has_pins: bool,
}

impl OpaqueLifetimeRole {
    /// True iff this role needs the RC-capable `RcRustHandle<T>` lane. Kept
    /// as a named predicate (rather than reading `is_source` directly at
    /// call sites) so the "what decides the runtime lane" question has
    /// exactly one place to change.
    pub(super) fn needs_rc(self) -> bool {
        self.is_source
    }

    /// True iff this role needs the plain-`RustHandle<T>`-plus-extra-fields
    /// lane (dependent-only, possibly also pinning, but never retained by
    /// anything else).
    pub(super) fn needs_dependent_fields(self) -> bool {
        !self.is_source && (self.is_dependent || self.has_pins)
    }
}

/// Run-level fold of every method's keep-alive data into a per-opaque role
/// map. Built once, after every type's methods have been lowered (so it sees
/// the whole run's borrow graph) and before any opaque template renders (so
/// a method's own return statement can pick the right handle type for
/// whatever *other* type it constructs) — see `mod.rs::render_all_types`.
#[derive(Debug, Default)]
pub(super) struct LifetimePlan {
    roles: HashMap<hir::OpaqueId, OpaqueLifetimeRole>,
}

impl LifetimePlan {
    /// The classified role for an opaque type, defaulting to "neither" (the
    /// plain, non-RC, no-extra-fields lane) for a type nothing ever
    /// contributes a role for — most opaques in a typical run.
    pub(super) fn role(&self, id: hir::OpaqueId) -> OpaqueLifetimeRole {
        self.roles.get(&id).copied().unwrap_or_default()
    }

    fn role_mut(&mut self, id: hir::OpaqueId) -> &mut OpaqueLifetimeRole {
        self.roles.entry(id).or_default()
    }

    /// Fold one method's Ok-arm or Err-arm keep-alive data into the run-level
    /// map. `dependent_opaque_id` is the opaque type that would hold
    /// `sources` (the returned opaque for the Ok arm, the inner error opaque
    /// for the Err arm) — `None` when the arm's value isn't itself an opaque
    /// wrapper (e.g. a borrowed-span return: its sources still need
    /// `is_source`, but there is no generated opaque wrapper to mark
    /// `is_dependent` on). `has_pins` only ever applies to the Ok arm (the
    /// error arm structurally never pins — see
    /// `gen::method::borrowed_output_keep_alive_edges`).
    pub(super) fn record_output(
        &mut self,
        dependent_opaque_id: Option<hir::OpaqueId>,
        sources: &[OpaqueBorrowSource],
        has_pins: bool,
    ) {
        for source in sources {
            self.role_mut(source.opaque_id).is_source = true;
        }
        if let Some(id) = dependent_opaque_id {
            let role = self.role_mut(id);
            if !sources.is_empty() {
                role.is_dependent = true;
            }
            if has_pins {
                role.has_pins = true;
            }
        }
    }
}
