use std::collections::{BTreeMap, BTreeSet};

use diplomat_core::hir::{OpaqueId, TypeContext, TypeDef, TypeId};

use crate::{dotnet::formatter::DotnetFormatter, ErrorStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReturnArm {
    Ok,
    Err,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BorrowSource {
    Receiver,
    Parameter(String),
}

impl BorrowSource {
    fn new(name: &str) -> Self {
        if name == "this" {
            Self::Receiver
        } else {
            Self::Parameter(name.to_string())
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Receiver => "the receiver".to_string(),
            Self::Parameter(name) => format!("parameter `{name}`"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TriggerKind {
    MutableBorrow,
    OwnedBorrow(BorrowSource),
    Retains {
        source: OpaqueId,
        root: OpaqueId,
        source_is_marked: bool,
    },
    PinnedSlice(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Trigger {
    method: String,
    arm: ReturnArm,
    kind: TriggerKind,
}

#[derive(Clone, Debug)]
struct Retain {
    dependent: OpaqueId,
    source: OpaqueId,
    method: String,
    arm: ReturnArm,
}

#[derive(Default)]
pub(in crate::dotnet) struct DisposalRequirements {
    required: BTreeMap<OpaqueId, Vec<Trigger>>,
    retains: Vec<Retain>,
}

impl DisposalRequirements {
    fn add_trigger(&mut self, opaque: OpaqueId, trigger: Trigger) {
        let triggers = self.required.entry(opaque).or_default();
        if !triggers.contains(&trigger) {
            triggers.push(trigger);
        }
    }

    pub(super) fn record_mutable_borrow(&mut self, opaque: OpaqueId, method: &str, arm: ReturnArm) {
        self.add_trigger(
            opaque,
            Trigger {
                method: method.to_string(),
                arm,
                kind: TriggerKind::MutableBorrow,
            },
        );
    }

    pub(super) fn record_owned_borrow(
        &mut self,
        opaque: OpaqueId,
        source: &str,
        method: &str,
        arm: ReturnArm,
    ) {
        self.add_trigger(
            opaque,
            Trigger {
                method: method.to_string(),
                arm,
                kind: TriggerKind::OwnedBorrow(BorrowSource::new(source)),
            },
        );
    }

    pub(super) fn record_pin(
        &mut self,
        opaque: OpaqueId,
        parameter: &str,
        method: &str,
        arm: ReturnArm,
    ) {
        self.add_trigger(
            opaque,
            Trigger {
                method: method.to_string(),
                arm,
                kind: TriggerKind::PinnedSlice(parameter.to_string()),
            },
        );
    }

    pub(super) fn record_retain(
        &mut self,
        dependent: OpaqueId,
        source: OpaqueId,
        method: &str,
        arm: ReturnArm,
    ) {
        self.retains.push(Retain {
            dependent,
            source,
            method: method.to_string(),
            arm,
        });
    }

    pub(super) fn finish<'tcx>(
        &self,
        tcx: &'tcx TypeContext,
        errors: &ErrorStore<'tcx, String>,
        formatter: &DotnetFormatter<'tcx>,
    ) {
        let marked: BTreeSet<OpaqueId> = tcx
            .all_types()
            .filter_map(|(id, ty)| match (id, ty) {
                (TypeId::Opaque(id), TypeDef::Opaque(def))
                    if !def.attrs.disable && def.attrs.manually_disposable =>
                {
                    Some(id)
                }
                _ => None,
            })
            .collect();
        let mut required: BTreeSet<OpaqueId> = self.required.keys().copied().collect();
        let mut roots: BTreeMap<OpaqueId, OpaqueId> = marked
            .iter()
            .chain(required.iter())
            .map(|id| (*id, *id))
            .collect();

        loop {
            let mut changed = false;
            for retain in &self.retains {
                if (marked.contains(&retain.source) || required.contains(&retain.source))
                    && required.insert(retain.dependent)
                {
                    let root = roots.get(&retain.source).copied().unwrap_or(retain.source);
                    roots.insert(retain.dependent, root);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let mut triggers = self.required.clone();
        for retain in &self.retains {
            if !marked.contains(&retain.source) && !required.contains(&retain.source) {
                continue;
            }
            let trigger = Trigger {
                method: retain.method.clone(),
                arm: retain.arm,
                kind: TriggerKind::Retains {
                    source: retain.source,
                    root: roots.get(&retain.source).copied().unwrap_or(retain.source),
                    source_is_marked: marked.contains(&retain.source),
                },
            };
            let dependent_triggers = triggers.entry(retain.dependent).or_default();
            if !dependent_triggers.contains(&trigger) {
                dependent_triggers.push(trigger);
            }
        }

        let mut missing: Vec<(String, OpaqueId)> = required
            .difference(&marked)
            .filter(|id| !tcx.resolve_opaque(**id).attrs.disable)
            .map(|id| (formatter.fmt_type_name((*id).into()).into_owned(), *id))
            .collect();
        missing.sort_by(|a, b| a.0.cmp(&b.0));

        for (display_name, id) in missing {
            let mut type_triggers = triggers.remove(&id).unwrap_or_default();
            type_triggers.sort_by(|a, b| {
                a.method
                    .cmp(&b.method)
                    .then_with(|| trigger_rule(&a.kind).cmp(&trigger_rule(&b.kind)))
            });
            let mut message = format!(
                "[.NET backend] `{display_name}` must be `#[diplomat::attr(dotnet, manually_disposable)]`:"
            );
            for trigger in &type_triggers {
                message.push_str("\n  - ");
                message.push_str(&trigger.describe(formatter));
            }
            let _guard = errors.set_context_ty(display_name.into());
            errors.push_error(message);
        }
    }
}

fn trigger_rule(kind: &TriggerKind) -> u8 {
    match kind {
        TriggerKind::MutableBorrow => 1,
        TriggerKind::OwnedBorrow(_) => 2,
        TriggerKind::Retains { .. } => 3,
        TriggerKind::PinnedSlice(_) => 4,
    }
}

impl Trigger {
    fn describe(&self, formatter: &DotnetFormatter<'_>) -> String {
        let error_arm = if self.arm == ReturnArm::Err {
            " from the error arm"
        } else {
            ""
        };
        match &self.kind {
            TriggerKind::MutableBorrow => format!(
                "`{}` returns it as a mutable borrow (rule 1: only Dispose() ends the exclusive borrow)",
                self.method
            ),
            TriggerKind::OwnedBorrow(source) => format!(
                "`{}` returns it{error_arm} as an owned value that borrows from {} (rule 2: the wrapper holds a borrow it must be able to release)",
                self.method,
                source.describe()
            ),
            TriggerKind::Retains {
                source,
                root,
                source_is_marked,
            } => {
                let source_name = formatter.fmt_type_name((*source).into());
                let relation = if *source_is_marked {
                    "which is manually_disposable".to_string()
                } else if source == root {
                    "which must be manually_disposable".to_string()
                } else {
                    let root_name = formatter.fmt_type_name((*root).into());
                    format!("which retains the manually_disposable `{root_name}`")
                };
                format!(
                    "`{}` returns it{error_arm} borrowing from `{source_name}`, {relation} (rule 3: a view must not defer a disposable source's release)",
                    self.method
                )
            }
            TriggerKind::PinnedSlice(parameter) => format!(
                "`{}` returns it borrowing from slice parameter `{parameter}` (rule 4: the wrapper pins managed memory it must be able to unpin)",
                self.method
            ),
        }
    }
}
