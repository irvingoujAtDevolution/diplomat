//! .NET backend for Diplomat.
//!
//! Generates C# bindings that call into the Diplomat-generated C ABI via
//! P/Invoke (`LibraryImport` source generators on .NET 7+). Opaque Rust
//! handles map to a `SafeHandle`-based wrapper plus `IDisposable`; slices
//! and strings copy across the boundary; callbacks are pinned via
//! `GCHandle` on the managed side.
//!
//! This file is the entry point that the Diplomat CLI dispatches to. Codegen
//! itself lives in [`gen`] and naming/type-formatting concerns live in
//! [`formatter`].

use askama::Template;
use diplomat_core::hir::{BackendAttrSupport, DocsUrlGenerator, TypeContext};
use heck::ToUpperCamelCase;
use serde::{Deserialize, Serialize};

use crate::{dotnet::formatter::DotnetFormatter, Config, ErrorStore, FileMap};

mod formatter;
mod gen;

// ─────────────────────────────────────────────────────────────────────────────
// Runtime helpers — emitted once per generation run, independent of HIR.
// ─────────────────────────────────────────────────────────────────────────────

/// `DiplomatSliceU8` — the `repr(C)` fat pointer that crosses the FFI
/// boundary for every `&DiplomatStr` / `&[u8]` param. Namespace is
/// project-specific, so this is templated rather than `include_str!`'d.
#[derive(Template)]
#[template(path = "dotnet/DiplomatSliceU8.cs.jinja", escape = "none")]
struct DiplomatSliceU8Template<'a> {
    namespace: &'a str,
}

/// `DiplomatSliceMutU8` — the mutable counterpart, used for `&mut [u8]`
/// params. Same layout as `DiplomatSliceU8`; the distinct C# type keeps
/// the binding's intent (read-only vs writeable) clear at the call site.
#[derive(Template)]
#[template(path = "dotnet/DiplomatSliceMutU8.cs.jinja", escape = "none")]
struct DiplomatSliceMutU8Template<'a> {
    namespace: &'a str,
}

#[derive(Template)]
#[template(path = "dotnet/DiplomatSliceU32.cs.jinja", escape = "none")]
struct DiplomatSliceU32Template<'a> {
    namespace: &'a str,
}

#[derive(Template)]
#[template(path = "dotnet/DiplomatSliceMutU32.cs.jinja", escape = "none")]
struct DiplomatSliceMutU32Template<'a> {
    namespace: &'a str,
}

/// `DiplomatWriteable` — caller-provided buffer Rust appends UTF-8 bytes
/// into. Carries function pointers for `flush` and `grow` callbacks so
/// Rust can ask C# to enlarge the buffer when it runs out. Used for
/// every "return string" API on the Rust side (`fn foo(&self, write: &mut DiplomatWrite)`).
#[derive(Template)]
#[template(path = "dotnet/DiplomatWriteable.cs.jinja", escape = "none")]
struct DiplomatWriteableTemplate<'a> {
    namespace: &'a str,
}

pub(crate) fn attr_support() -> BackendAttrSupport {
    let mut a = BackendAttrSupport::default();

    // Conservative defaults — flip to `true` as features land in `gen`.
    a.namespacing = true;
    a.memory_sharing = false;
    a.non_exhaustive_structs = true;
    a.method_overloading = true;
    a.utf8_strings = true;
    a.utf16_strings = false;
    a.static_slices = true;
    a.option = true;

    a.mutable_slices = true;
    a.static_slices = true;
    a.owned_slices = true;

    a.constructors = false;
    a.named_constructors = false;
    a.fallible_constructors = false;
    a.accessors = false;
    a.static_accessors = false;
    a.stringifiers = false;
    a.comparators = false;
    a.iterators = false;
    a.iterables = false;
    a.indexing = false;
    a.callbacks = false;
    a.traits = false;
    a.custom_errors = false;
    a.traits_are_send = false;
    a.traits_are_sync = false;
    a.generate_mocking_interface = false;

    a
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DotnetConfig {
    /// Root .NET namespace for the generated bindings (e.g. `Devolutions.Icu4x`).
    pub namespace: Option<String>,
    /// The native library name passed to `LibraryImport`. Defaults to the
    /// crate's `lib_name`.
    pub dylib_name: Option<String>,
    /// Suffix trimmed when generating exception names from error types.
    /// Existing Devolutions bindings use `IronRdpError` -> `IronRdpException`.
    pub exception_trim_suffix: Option<String>,
    /// Error method used for exception messages, e.g. `ToDisplay`.
    pub exception_message_method: Option<String>,
    /// Prefix identifying property getters, e.g. `get_`.
    pub getters_prefix: Option<String>,
    /// Prefix identifying property setters, e.g. `set_`.
    pub setters_prefix: Option<String>,
    /// If `true`, emit a `.csproj` scaffold next to the generated sources.
    pub scaffold: Option<bool>,
}

impl DotnetConfig {
    pub fn set(&mut self, key: &str, value: toml::Value) {
        match key {
            "namespace" if value.is_str() => {
                self.namespace = value.as_str().map(str::to_string);
            }
            "dylib_name" | "native_lib" if value.is_str() => {
                self.dylib_name = value.as_str().map(str::to_string);
            }
            "exception_trim_suffix" | "exceptions.trim_suffix" if value.is_str() => {
                self.exception_trim_suffix = value.as_str().map(str::to_string);
            }
            "exception_message_method" | "exceptions.error_message_method" if value.is_str() => {
                self.exception_message_method = value.as_str().map(str::to_string);
            }
            "getters_prefix" | "properties.getters_prefix" if value.is_str() => {
                self.getters_prefix = value.as_str().map(str::to_string);
            }
            "setters_prefix" | "properties.setters_prefix" if value.is_str() => {
                self.setters_prefix = value.as_str().map(str::to_string);
            }
            "scaffold" => {
                self.scaffold = value
                    .as_bool()
                    .or_else(|| value.as_str().map(|v| v == "true"));
            }
            _ => {}
        }
    }
}

pub(crate) fn run<'tcx>(
    tcx: &'tcx TypeContext,
    config: &'tcx Config,
    docs_url_gen: &'tcx DocsUrlGenerator,
) -> (FileMap, ErrorStore<'tcx, String>) {
    let files = FileMap::default();
    let errors: ErrorStore<'tcx, String> = ErrorStore::default();
    let formatter = DotnetFormatter::new(tcx, &config, docs_url_gen);

    let lib_name = config
        .shared_config
        .lib_name
        .clone()
        .or_else(|| config.dotnet_config.dylib_name.clone())
        .expect("Missing required field `lib_name` in [shared] or `native_lib`/`dylib_name` in .NET config");

    let dylib_name = config
        .dotnet_config
        .dylib_name
        .clone()
        .unwrap_or_else(|| lib_name.clone());

    let namespace = config
        .dotnet_config
        .namespace
        .clone()
        .unwrap_or_else(|| lib_name.to_upper_camel_case());

    let ctx = gen::ItemGenContext {
        tcx,
        formatter: &formatter,
        errors: &errors,
        docs_url_gen,
        lib_name: &lib_name,
        dylib_name: &dylib_name,
        namespace: &namespace,
        exception_trim_suffix: config.dotnet_config.exception_trim_suffix.as_deref(),
        exception_message_method: config.dotnet_config.exception_message_method.as_deref(),
        getters_prefix: config.dotnet_config.getters_prefix.as_deref(),
        setters_prefix: config.dotnet_config.setters_prefix.as_deref(),
        result_struct_registry: std::cell::RefCell::new(std::collections::HashMap::new()),
        option_struct_registry: std::cell::RefCell::new(std::collections::HashMap::new()),
    };

    for (id, ty) in tcx.all_types() {
        if ty.attrs().disable {
            continue;
        }

        let _guard = errors.set_context_ty(ty.name().as_str().into());

        /*
         * Raw represents the layer of C# that directly manipulates the C ABI. It is expected to be unsafe and low-level, and is not intended for direct consumption by end-users.
         * The content layer represents the safe, idiomatic C# API that end-users will interact with.
         * It may wrap or compose multiple raw items, and should prioritize usability and safety.
         */
        let (raw, content) = match ty {
            diplomat_core::hir::TypeDef::Struct(struct_def) => ctx.gen_struct(struct_def),
            diplomat_core::hir::TypeDef::OutStruct(struct_def) => ctx.gen_out_struct(struct_def),
            diplomat_core::hir::TypeDef::Opaque(opaque_def) => ctx.gen_opaque(opaque_def),
            diplomat_core::hir::TypeDef::Enum(enum_def) => ctx.gen_enum(enum_def),
            _ => {
                // No other type variants are expected to be emitted as top-level items, but
                // if we add any in the future, this will catch them and prevent silent
                // omissions.
                panic!("unexpected type variant: {id:?}");
            }
        };

        let file_name = format!("{}.cs", ty.name());
        if let Some(raw) = raw {
            let raw_file_name = format!("Raw{}.cs", ty.name());
            files.add_file(raw_file_name, raw);
        }
        files.add_file(file_name, content);
    }

    // Emit result structs + their exception classes. One exception per
    // unique error type, dedup'd via a HashSet on the way through.
    let mut emitted_exceptions: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for result_struct in ctx.result_struct_registry.into_inner().into_values() {
        let error_name = result_struct.error.to_string();
        if emitted_exceptions.insert(error_name.clone()) {
            let exception = gen::fillable::DotnetException {
                namespace: namespace.clone(),
                error: result_struct.error.clone(),
                exception_name: result_struct.exception_name.clone(),
                message_method: config.dotnet_config.exception_message_method.clone(),
            };
            files.add_file(
                format!("{}.cs", result_struct.exception_name),
                exception
                    .render()
                    .expect("DotnetException template render failed"),
            );
        }

        let file_name = format!("{}.cs", result_struct.result_struct_name);
        files.add_file(file_name, result_struct.render().unwrap());
    }

    // Emit option structs — one per unique inner type encountered in any
    // Option<value-type> return. Pointer-nullable Options (Option<Box<T>>)
    // don't register anything; the inner opaque pointer carries null
    // natively and needs no wrapper.
    for option_struct in ctx.option_struct_registry.into_inner().into_values() {
        let file_name = format!("{}.cs", option_struct.option_struct_name);
        files.add_file(
            file_name,
            option_struct
                .render()
                .expect("DotnetOption template render failed"),
        );
    }

    // Runtime helpers — emit once, independent of which types exist.
    files.add_file(
        "DiplomatSliceU8.cs".to_string(),
        DiplomatSliceU8Template {
            namespace: &namespace,
        }
        .render()
        .expect("DiplomatSliceU8 template render failed"),
    );
    files.add_file(
        "DiplomatSliceMutU8.cs".to_string(),
        DiplomatSliceMutU8Template {
            namespace: &namespace,
        }
        .render()
        .expect("DiplomatSliceMutU8 template render failed"),
    );
    files.add_file(
        "DiplomatSliceU32.cs".to_string(),
        DiplomatSliceU32Template {
            namespace: &namespace,
        }
        .render()
        .expect("DiplomatSliceU32 template render failed"),
    );
    files.add_file(
        "DiplomatSliceMutU32.cs".to_string(),
        DiplomatSliceMutU32Template {
            namespace: &namespace,
        }
        .render()
        .expect("DiplomatSliceMutU32 template render failed"),
    );
    files.add_file(
        "DiplomatWriteable.cs".to_string(),
        DiplomatWriteableTemplate {
            namespace: &namespace,
        }
        .render()
        .expect("DiplomatWriteable template render failed"),
    );

    (files, errors)
}
