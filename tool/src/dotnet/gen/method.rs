//! Method composition vocabulary — how a single HIR `Method` turns into
//! the C# fragments that templates consume.
//!
//! Both inputs and outputs are layer-agnostic at the data level — variants
//! name the *kind* of type ("Opaque", "Struct", "Primitive", …) and the raw
//! vs idiomatic C# spellings come from view methods. Body-shape decisions
//! (the disposed check, the `new T(...)` wrap, the `FromFFI` bridge) live
//! in the templates, where the C# code naturally lives.
//!
//! ## What lives here
//!
//! * [`DotnetReturnType`] — return-side vocabulary. Predicates +
//!   `as_raw` / `as_idiomatic` let templates pick the right spelling and
//!   render the right body shape.
//! * [`DotnetInputs`] — input-side vocabulary. Three precomputed,
//!   comma-joined strings: one for the raw extern decl, one for the
//!   idiomatic method decl (no self), one for the raw call args.
//! * [`MethodInfo`] — one method's render data; consumed by every template.

use std::{
    collections::BTreeMap,
    fmt::{self, Display},
};

use diplomat_core::hir::{self, MaybeOwn, Method};

use crate::dotnet::r#gen::fillable::{
    DotnetErrorType, DotnetOption, DotnetResult, ErrorInfo, OptionInfo,
};

use super::{callback::DotnetCallback, DotnetPrimitives, ItemGenContext};

/// Context from the type-level codegen call site that a bare HIR method
/// does not carry by itself.
///
/// This matters for callback helper naming: `hir::Type::Callback` describes
/// the callback signature, but not the owner type whose method contains it.
#[derive(Clone, Copy)]
pub(super) struct StructMethodContext<'ctx> {
    method: &'ctx Method,
}

impl<'ctx> StructMethodContext<'ctx> {
    pub(super) fn new(method: &'ctx Method) -> Self {
        Self { method }
    }

    pub(super) fn method(&self) -> &'ctx Method {
        self.method
    }

    pub(super) fn method_abi_name(&self) -> &str {
        self.method.abi_name.as_str()
    }
}

pub(super) struct MethodInputContext<'ctx> {
    method: StructMethodContext<'ctx>,
    param: &'ctx hir::Param,
    param_index: usize,
    arg_name: String,
}

impl<'ctx> MethodInputContext<'ctx> {
    fn new(
        method: StructMethodContext<'ctx>,
        param_index: usize,
        param: &'ctx hir::Param,
        arg_name: String,
    ) -> Self {
        Self {
            method,
            param,
            param_index,
            arg_name,
        }
    }

    pub(super) fn method(&self) -> StructMethodContext<'ctx> {
        self.method
    }

    fn param(&self) -> &'ctx hir::Param {
        self.param
    }

    pub(super) fn param_index(&self) -> usize {
        self.param_index
    }

    pub(super) fn arg_name(&self) -> &str {
        &self.arg_name
    }

    pub(super) fn param_ident(&self) -> &str {
        self.param.name.as_str()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Return type
// ─────────────────────────────────────────────────────────────────────────────

/// The return type of a method, expressed once. The variant names the
/// *kind*; [`Display`] writes the bare C# name; templates branch on kind
/// via `is_*` predicates and supply the kind-specific bits (the `*` for
/// opaque in raw externs, the `new T(...)` wrap, the `T.FromFFI(...)` bridge).
#[derive(Debug, Clone)]
pub(crate) enum DotnetReturnType {
    Primitive(DotnetPrimitives),
    /// `Box<T>` / `&T` / `&mut T` opaque return. Carries the bare name
    /// (`"Color"`). Raw externs append `*`; idiomatic wrappers don't.
    Opaque(String),
    /// Returnable struct (by-value). Carries the bare name (`"Point2D"`).
    Struct(String),
    /// Enum return by value. Carries the bare name (`"ShaVariant"`). Same
    /// shape as a primitive at the C ABI (an integer discriminant) —
    /// neither raw extern nor idiomatic surface needs marshalling glue.
    Enum(String),
    /// `DiplomatWrite` writer. Not yet emitted; preserves prior behavior.
    Write,
    Unit,
}

impl Display for DotnetReturnType {
    /// Writes the bare C# type name (e.g. `byte`, `Color`, `Point2D`, `void`).
    /// Used directly in idiomatic method signatures and as the prefix in
    /// raw externs (which append `*` for opaque).
    ///
    /// Struct returns inside the `Raw.<Name>` partial-struct resolve to the
    /// enclosing type via C# name lookup — no explicit `Raw.` prefix needed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(p) => write!(f, "{p}"),
            Self::Opaque(name) | Self::Struct(name) | Self::Enum(name) => write!(f, "{name}"),
            // Benoit-compat: Write methods surface as `void M(args,
            // DiplomatWriteable writeable)` on the idiomatic side, with
            // the caller managing writer lifecycle. The `string`-returning
            // convenience form would have been a more modern idiom but
            // would not be source-compatible with picky's hand-written C#.
            Self::Write | Self::Unit => write!(f, "void"),
        }
    }
}

impl DotnetReturnType {
    /// Bare name + `*` for opaque, bare name otherwise. The "raw FFI
    /// surface" form — what the type looks like at the C ABI boundary.
    ///
    /// Use this where the C# extern declares the return type, where a
    /// union arm declares an opaque-payload field, or where the idiomatic
    /// body binds the raw result (`Raw.{{ x.raw() }} result = ...`).
    /// Templates avoid carrying the inline `{% if is_opaque() %}*{% endif %}`
    /// micro-conditional.
    pub(super) fn raw(&self) -> String {
        if self.is_opaque() {
            format!("{self}*")
        } else {
            self.to_string()
        }
    }

    pub(super) fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque(_))
    }

    pub(super) fn is_struct(&self) -> bool {
        matches!(self, Self::Struct(_))
    }

    pub(super) fn is_void(&self) -> bool {
        // Both `Unit` (no return) and `Write` (writer-out-param) have no
        // value in the success arm of a Result struct — neither belongs
        // in the ok union slot. Display is also `void` for both.
        matches!(self, Self::Unit | Self::Write)
    }

    /// True for `DiplomatWrite` returns. The idiomatic wrapper renders
    /// these as `string`-returning methods (auto-allocates writer);
    /// the raw extern declares them as `void` + writer-pointer param.
    pub(super) fn is_write(&self) -> bool {
        matches!(self, Self::Write)
    }

    pub(super) fn is_bool(&self) -> bool {
        matches!(self, Self::Primitive(DotnetPrimitives::Bool))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inputs
// ─────────────────────────────────────────────────────────────────────────────

/// One HIR input (param or self) lowered to the three rendered surfaces.
///
/// Self produces an empty `idiomatic_param` (since `this` is implicit) and
/// uses kind-specific call args (`"_inner"` for opaque, `"this.AsFFI()"` for
/// struct).
#[derive(Debug, Default)]
struct InputLowering {
    /// C# decl for the raw `[DllImport]` extern: `"Color* handle"`, `"byte v"`.
    raw_param: String,
    /// C# decl for the idiomatic wrapper signature: `"Color name"`, `"byte v"`.
    /// Empty for self — `this` is implicit.
    idiomatic_param: String,
    /// Expression passed to the raw call from the idiomatic body:
    /// `"_inner"` / `"this.AsFFI()"` for self, `"name._inner"` for an opaque
    /// param, `"v"` for a primitive.
    raw_call_arg: String,
    validation_statement: Option<String>,

    /// Statements that must run calling into the raw layer — e.g. the DiplomatStr
    fix_statement: Option<String>,

    /// For inputs that need to convert from string to pointer, basically the DiplomatStr only
    to_bytes_statement: Option<String>,
    idiomatic_param_type: Option<String>,
}

/// All of a method's inputs, joined for template substitution.
///
/// Self's quirks (empty idiomatic decl, kind-specific call arg) are absorbed
/// by the builder — the finished aggregate has no "self is special" surface.
#[derive(Debug, Default)]
pub(super) struct DotnetInputs {
    /// Raw `[DllImport]` decl: `"Color* handle, byte value"`.
    pub(super) raw_params: String,
    /// Idiomatic wrapper decl (no self): `"byte value"`.
    pub(super) idiomatic_params: String,
    /// Bare param names joined with `, ` — useful for forwarding the
    /// idiomatic params to another C# overload (e.g. the convenience
    /// `string M(args)` form calling the primary `void M(args, writer)`).
    pub(super) idiomatic_call_args: String,
    /// Raw call args from the idiomatic body: `"_inner, value"`.
    pub(super) raw_call_args: String,
    pub(super) validation_statements: Vec<String>,
    pub(super) fix_statements: Vec<String>,
    pub(super) to_bytes_statements: Vec<String>,
    pub(super) first_param_type: Option<String>,
    pub(super) param_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Return lowering — one struct out, two fields, no special-case predicates
// ─────────────────────────────────────────────────────────────────────────────

/// One method's lowered return — `return_type` is always present;
/// `error_info` is `Some` iff the Rust side returns `Result<T, E>` with a
/// concrete error type; `option_info` is `Some` iff the Rust side returns
/// `Option<T>`. The three are mutually exclusive at the HIR level.
/// Consumers pattern-match these Option fields directly; no separate
/// is_fallible / is_optional predicates exist.
pub(super) struct ReturnLowering {
    pub(super) return_type: DotnetReturnType,
    pub(super) error_info: Option<ErrorInfo>,
    pub(super) option_info: Option<OptionInfo>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Method info
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-processed view of a single HIR `Method`. Carries both raw-layer and
/// idiomatic-layer render data — templates pick the side they want.
pub(super) struct MethodInfo<'ctx> {
    /// `extern "C"` symbol name (e.g. `Color_brightness`).
    pub(super) abi_name: &'ctx str,
    /// C# method name (PascalCase, e.g. `Brightness`).
    pub(super) name: String,
    /// `"static "` for static methods, `""` for instance. Renders directly
    /// before the return type in the idiomatic method declaration.
    pub(super) static_kw: &'static str,
    pub(super) inputs: DotnetInputs,
    pub(super) return_type: DotnetReturnType,
    /// `Some` iff this method returns `Result<T, E>` with a concrete `E`.
    /// Templates branch on `{% if let Some(info) = method.error_info %}` —
    /// no separate `is_fallible()` predicate needed.
    pub(super) error_info: Option<ErrorInfo>,
    /// `Some` iff this method returns `Option<T>`. Templates branch on
    /// `{% if let Some(opt) = method.option_info %}` to render the
    /// nullable C# return type + null/IsSome check. Mutually exclusive
    /// with `error_info`.
    pub(super) option_info: Option<OptionInfo>,
    pub(super) property_accessor: Option<PropertyAccessor>,
}

pub(super) struct PropertyInfo {
    pub(super) name: String,
    pub(super) property_type: String,
    pub(super) getter: Option<String>,
    pub(super) setter: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) enum PropertyAccessorKind {
    Getter,
    Setter,
}

#[derive(Clone, Debug)]
pub(super) struct PropertyAccessor {
    pub(super) name: String,
    pub(super) kind: PropertyAccessorKind,
    pub(super) property_type: String,
}

impl MethodInfo<'_> {
    /// True for methods with a `self` receiver. Drives the disposed-check
    /// emission in `opaque.impl.cs.jinja` (every opaque instance method
    /// must validate `_inner` before calling into the raw layer).
    pub(super) fn is_instance(&self) -> bool {
        self.static_kw.is_empty()
    }

    /// C# fragment for the idiomatic method signature's return type —
    /// `Color` for plain returns, `Color?` for `Option<T>` returns. The
    /// `?` suffix tells C# 8+ "this might be null" and triggers
    /// `Nullable<T>` for value types.
    pub(super) fn idiomatic_return_type(&self) -> String {
        if self.option_info.is_some() {
            format!("{}?", self.return_type)
        } else {
            self.return_type.to_string()
        }
    }

    /// Raw `[DllImport]` extern param list. For `DiplomatWrite` returns,
    /// the writer pointer is an implicit trailing parameter not present
    /// in the Rust signature's user-facing params — appended here so the
    /// raw template doesn't have to know about it.
    pub(super) fn raw_params_with_writer(&self) -> String {
        if !self.return_type.is_write() {
            return self.inputs.raw_params.clone();
        }
        if self.inputs.raw_params.is_empty() {
            "DiplomatWriteable* writeable".to_string()
        } else {
            format!("{}, DiplomatWriteable* writeable", self.inputs.raw_params)
        }
    }

    /// Raw call arg list. Mirror of `raw_params_with_writer`: appends
    /// `&writeable` for `DiplomatWrite` returns so the idiomatic body's
    /// raw-call line doesn't have to special-case it.
    pub(super) fn raw_call_args_with_writer(&self) -> String {
        if !self.return_type.is_write() {
            return self.inputs.raw_call_args.clone();
        }
        if self.inputs.raw_call_args.is_empty() {
            "&writeable".to_string()
        } else {
            format!("{}, &writeable", self.inputs.raw_call_args)
        }
    }

    /// Idiomatic-side param list. For `DiplomatWrite` returns the caller
    /// supplies the writer (matches Benoit's surface). For everything
    /// else this is just `inputs.idiomatic_params` verbatim.
    pub(super) fn idiomatic_params_with_writer(&self) -> String {
        if !self.return_type.is_write() {
            return self.inputs.idiomatic_params.clone();
        }
        if self.inputs.idiomatic_params.is_empty() {
            "DiplomatWriteable writeable".to_string()
        } else {
            format!("{}, DiplomatWriteable writeable", self.inputs.idiomatic_params)
        }
    }
}

pub(super) fn collect_properties(methods: &[MethodInfo<'_>]) -> Vec<PropertyInfo> {
    let mut properties = BTreeMap::<String, PropertyInfo>::new();

    for method in methods {
        let Some(accessor) = &method.property_accessor else {
            continue;
        };
        let property = properties
            .entry(accessor.name.clone())
            .or_insert_with(|| PropertyInfo {
                name: accessor.name.clone(),
                property_type: accessor.property_type.clone(),
                getter: None,
                setter: None,
            });

        match accessor.kind {
            PropertyAccessorKind::Getter => {
                property.property_type = accessor.property_type.clone();
                property.getter = Some(method.name.clone());
            }
            PropertyAccessorKind::Setter => {
                property.setter = Some(method.name.clone());
            }
        }
    }

    properties
        .into_values()
        .filter(|property| property.getter.is_some())
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-method builders
// ─────────────────────────────────────────────────────────────────────────────

impl<'ctx, 'tcx> ItemGenContext<'ctx, 'tcx> {
    pub(super) fn build_method_info(
        &self,
        method_context: StructMethodContext<'tcx>,
    ) -> MethodInfo<'tcx> {
        let method = method_context.method();
        let static_kw = if method.param_self.is_some() {
            ""
        } else {
            "static "
        };
        let ReturnLowering {
            return_type,
            error_info,
            option_info,
        } = self.lower_return(&method.output);

        let inputs = self.lower_inputs(method_context);
        let return_type_name = if option_info.is_some() {
            format!("{return_type}?")
        } else {
            return_type.to_string()
        };
        let property_accessor =
            self.property_accessor(method, &return_type, &return_type_name, &inputs);

        MethodInfo {
            abi_name: method.abi_name.as_str(),
            name: self.formatter.fmt_method_name(method).into_owned(),
            static_kw,
            inputs,
            return_type,
            error_info,
            option_info,
            property_accessor,
        }
    }

    fn property_accessor(
        &self,
        method: &'tcx Method,
        return_type: &DotnetReturnType,
        return_type_name: &str,
        inputs: &DotnetInputs,
    ) -> Option<PropertyAccessor> {
        if method.param_self.is_none() {
            return None;
        }

        if let Some(prefix) = self.getters_prefix {
            if inputs.param_count == 0 {
                if let Some(name) = method.name.as_str().strip_prefix(prefix) {
                    // Write-returning getters surface as `string PropName`
                    // properties (using the convenience overload), not
                    // `void` — `void` is illegal as a property type and
                    // wouldn't match what callers expect anyway.
                    let property_type = if return_type.is_write() {
                        "string".to_string()
                    } else {
                        return_type_name.to_string()
                    };
                    return Some(PropertyAccessor {
                        name: self.formatter.fmt_property_name(name).into_owned(),
                        kind: PropertyAccessorKind::Getter,
                        property_type,
                    });
                }
            }
        }

        if let Some(prefix) = self.setters_prefix {
            if inputs.param_count == 1 && return_type.is_void() {
                if let (Some(name), Some(param_type)) = (
                    method.name.as_str().strip_prefix(prefix),
                    inputs.first_param_type.clone(),
                ) {
                    return Some(PropertyAccessor {
                        name: self.formatter.fmt_property_name(name).into_owned(),
                        kind: PropertyAccessorKind::Setter,
                        property_type: param_type,
                    });
                }
            }
        }

        None
    }

    /// Lower a method's [`hir::ReturnType`] to a [`ReturnLowering`].
    ///
    /// Three sequential bindings, top to bottom — read the whole story without
    /// jumping. No nested cartesian-product matching between Fallible /
    /// Infallible / Nullable and the success type; each HIR variant appears
    /// in exactly one arm.
    pub(super) fn lower_return(&self, output: &hir::ReturnType) -> ReturnLowering {
        // 1. Decompose the HIR shape into the three orthogonal axes:
        //    success type, optional error, optional "wrap in Option".
        //
        //    Note: Option<Box<T>> hits the Infallible arm too — HIR encodes
        //    nullability on the opaque path itself via `is_optional()`,
        //    not in the ReturnType variant, because the pointer carries
        //    null natively (no wrapper struct needed on the wire).
        let (success, failed, is_nullable_path) = match output {
            hir::ReturnType::Infallible(s) => (s, None, false),
            hir::ReturnType::Fallible(s, Some(err)) => (s, Some(err), false),
            hir::ReturnType::Nullable(s) => (s, None, true),
            hir::ReturnType::Fallible(_, None) => {
                unimplemented!("Result<T, ()> — unit error not yet supported")
            }
        };

        // 2. Lower the success side. Detect "this opaque return was
        //    originally Option<Box<T>>" via the Optional marker on the
        //    opaque path — this is Path A (pointer-nullable) for Option.
        let mut pointer_nullable = false;
        let return_type = match success {
            hir::SuccessType::Unit => DotnetReturnType::Unit,
            hir::SuccessType::Write => DotnetReturnType::Write,
            hir::SuccessType::OutType(hir::Type::Primitive(p)) => {
                DotnetReturnType::Primitive(DotnetPrimitives::from(p))
            }
            hir::SuccessType::OutType(hir::Type::Opaque(p)) => {
                if p.is_optional() {
                    pointer_nullable = true;
                }
                DotnetReturnType::Opaque(self.opaque_name(p))
            }
            hir::SuccessType::OutType(hir::Type::Struct(p)) => {
                DotnetReturnType::Struct(self.returnable_struct_name(p))
            }
            hir::SuccessType::OutType(hir::Type::Enum(p)) => {
                DotnetReturnType::Enum(self.enum_name(p))
            }
            other => unimplemented!("success type {:?}", other),
        };

        // 3. If there's an error type: register the (Ok, Err) pair and
        //    build the ErrorInfo for the throw site.
        let error_info = failed.map(|err| {
            let error_type = DotnetErrorType::new(err, self);
            let exception_name = error_type.exception_name(self.exception_trim_suffix);
            let result = DotnetResult::new(
                self.namespace.to_string(),
                return_type.clone(),
                error_type,
                exception_name,
            );
            let info = result.error_info();
            self.result_struct_registry
                .borrow_mut()
                .insert(result.key(), result);
            info
        });

        // 4. If the return is wrapped in Option: either Path A (pointer
        //    null carries the None) or Path B (DiplomatOption<T> tagged
        //    struct on the wire). Path B registers a runtime helper struct.
        let option_info = if pointer_nullable {
            Some(OptionInfo::nullable_pointer())
        } else if is_nullable_path {
            let option = DotnetOption::new(self.namespace.to_string(), return_type.clone());
            let info = option.option_info();
            self.option_struct_registry
                .borrow_mut()
                .insert(option.key(), option);
            Some(info)
        } else {
            None
        };

        ReturnLowering {
            return_type,
            error_info,
            option_info,
        }
    }

    /// Lower `param_self` + user `params` into the joined-string surfaces
    /// templates consume.
    pub(super) fn lower_inputs(&self, method_context: StructMethodContext<'tcx>) -> DotnetInputs {
        let method = method_context.method();
        let self_lowering = method.param_self.as_ref().map(|s| self.lower_self(s));
        let param_lowerings: Vec<InputLowering> = method
            .params
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let arg_name = self.formatter.fmt_param_name(p.name.as_str()).into_owned();
                self.lower_input(MethodInputContext::new(method_context, index, p, arg_name))
            })
            .collect();

        let mut raw_params = Vec::new();
        let mut idiomatic_params = Vec::new();
        let mut call_args = Vec::new();
        let mut validation_statements = Vec::new();
        let mut fix_statements = Vec::new();
        let mut to_bytes_statements = Vec::new();
        let mut first_param_type = None;
        let mut param_count = 0;

        if let Some(s) = &self_lowering {
            raw_params.push(s.raw_param.as_str());
            call_args.push(s.raw_call_arg.as_str());
            // self contributes nothing to the idiomatic decl — `this` is implicit.
        }

        for p in &param_lowerings {
            raw_params.push(p.raw_param.as_str());
            idiomatic_params.push(p.idiomatic_param.as_str());
            call_args.push(p.raw_call_arg.as_str());
            param_count += 1;
            if first_param_type.is_none() {
                first_param_type = p.idiomatic_param_type.clone();
            }
            if let Some(validation) = &p.validation_statement {
                validation_statements.push(validation.clone());
            }
            if let Some(fix) = &p.fix_statement {
                fix_statements.push(fix.clone());
            }
            if let Some(to_bytes) = &p.to_bytes_statement {
                to_bytes_statements.push(to_bytes.clone());
            }
        }

        DotnetInputs {
            raw_params: raw_params.join(", "),
            idiomatic_params: idiomatic_params.join(", "),
            // Bare names extracted from each `"Type name"` decl. C# allows
            // arbitrary whitespace in decls but the codegen emits a single
            // space, so splitting on whitespace and taking the last token
            // gives the bare param name. `byte[] der` → `der`.
            idiomatic_call_args: idiomatic_params
                .iter()
                .map(|p| p.split_whitespace().last().unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join(", "),
            raw_call_args: call_args.join(", "),
            validation_statements,
            fix_statements,
            to_bytes_statements,
            first_param_type,
            param_count,
        }
    }

    fn lower_self(&self, this: &hir::ParamSelf) -> InputLowering {
        match &this.ty {
            hir::SelfType::Opaque(p) => {
                let name = self.opaque_name_borrowed(p);
                InputLowering {
                    raw_param: format!("{name}* handle"),
                    idiomatic_param: String::new(),
                    raw_call_arg: "_inner".into(),
                    ..Default::default()
                }
            }
            hir::SelfType::Struct(p) => {
                let name = self.struct_name(p);
                InputLowering {
                    raw_param: format!("{name} self"),
                    idiomatic_param: String::new(),
                    raw_call_arg: "this.AsFFI()".into(),
                    ..Default::default()
                }
            }
            hir::SelfType::Enum(_) => unimplemented!("self type enum"),
            _ => todo!(),
        }
    }

    fn lower_input(&self, input_context: MethodInputContext<'tcx>) -> InputLowering {
        let arg_name = input_context.arg_name();
        match &input_context.param().ty {
            hir::Type::Primitive(p) => {
                let primitive = DotnetPrimitives::from(p);
                let ty = primitive.to_string();
                let raw_ty = if matches!(primitive, DotnetPrimitives::Bool) {
                    "[MarshalAs(UnmanagedType.U1)] bool".to_string()
                } else {
                    ty.clone()
                };
                InputLowering {
                    raw_param: format!("{raw_ty} {arg_name}"),
                    idiomatic_param: format!("{ty} {arg_name}"),
                    raw_call_arg: arg_name.to_string(),
                    idiomatic_param_type: Some(ty),
                    ..Default::default()
                }
            }
            hir::Type::Opaque(p) => {
                let ty = self.opaque_name_borrowed(p);
                let optional = p.is_optional();
                let idiomatic_ty = if optional {
                    format!("{ty}?")
                } else {
                    ty.clone()
                };
                let raw_call_arg = if optional {
                    format!("{arg_name} == null ? null : {arg_name}.AsFFI()")
                } else {
                    format!("{arg_name}.AsFFI()")
                };
                let validation_statement = if optional {
                    Some(format!(
                        "if ({arg_name} != null && {arg_name}.AsFFI() == null) throw new ObjectDisposedException(nameof({ty}));"
                    ))
                } else {
                    Some(format!(
                        "if ({arg_name}.AsFFI() == null) throw new ObjectDisposedException(nameof({ty}));"
                    ))
                };
                InputLowering {
                    raw_param: format!("{ty}* {arg_name}"),
                    idiomatic_param: format!("{idiomatic_ty} {arg_name}"),
                    raw_call_arg,
                    validation_statement,
                    idiomatic_param_type: Some(idiomatic_ty),
                    ..Default::default()
                }
            }
            hir::Type::Slice(slice) => match slice {
                hir::Slice::Str(maybe_static, _string_encoding) => match maybe_static {
                    Some(lifetime) => match lifetime {
                        hir::MaybeStatic::Static => todo!(),
                        hir::MaybeStatic::NonStatic(_) => {
                            let ptr = name_ptr(&arg_name);
                            let bytes = name_bytes(&arg_name);
                            return InputLowering {
                                raw_param: format!("DiplomatSliceU8 {arg_name}"),
                                idiomatic_param: format!("string {arg_name}"),
                                raw_call_arg: format!(r#"new DiplomatSliceU8
                                {{
                                    Ptr = {ptr},
                                    Len = (nuint){bytes}.Length
                                }}
                                "#),
                                to_bytes_statement: Some(format!(
                                    "byte[] {bytes} = System.Text.Encoding.UTF8.GetBytes({arg_name});"
                                )),
                                fix_statement: Some(format!(
                                    "fixed (byte* {ptr} = {bytes})"
                                )),
                                idiomatic_param_type: Some("string".to_string()),
                                ..Default::default()
                            };
                        }
                    },
                    None => {
                        todo!();
                    }
                },
                hir::Slice::Primitive(maybe_own, primitive_type) => match primitive_type {
                    hir::PrimitiveType::Int(int_type) => match int_type {
                        hir::IntType::U8 | hir::IntType::U32 => {
                            let ptr = name_ptr(&arg_name);
                            let MaybeOwn::Borrow(borrow) = maybe_own else {
                                unimplemented!(
                                    "owned primitive slice of {:?} : {:?}",
                                    int_type,
                                    maybe_own
                                );
                            };

                            let (element_type, ptr_type, immutable_class, mutable_class) =
                                match int_type {
                                    hir::IntType::U8 => {
                                        ("byte", "byte", "DiplomatSliceU8", "DiplomatSliceMutU8")
                                    }
                                    hir::IntType::U32 => {
                                        ("uint", "uint", "DiplomatSliceU32", "DiplomatSliceMutU32")
                                    }
                                    _ => unreachable!(),
                                };

                            let slice_class = match borrow.mutability {
                                hir::Mutability::Mutable => mutable_class,
                                hir::Mutability::Immutable => immutable_class,
                            };

                            return InputLowering {
                                raw_param: format!("{slice_class} {arg_name}"),
                                idiomatic_param: format!("{element_type}[] {arg_name}"),
                                raw_call_arg: format!(
                                    r#"new {slice_class}
                                            {{
                                                Ptr = {ptr},
                                                Len = (nuint){arg_name}.Length  
                                            }}
                                            "#
                                ),
                                fix_statement: Some(format!(
                                    "fixed ({ptr_type}* {ptr} = {arg_name})"
                                )),
                                to_bytes_statement: None,
                                idiomatic_param_type: Some(format!("{element_type}[]")),
                                ..Default::default()
                            };
                        }
                        _ => unimplemented!("primitive slice of {:?} : {:?}", int_type, maybe_own),
                    },
                    other => unimplemented!("primitive type of {:?} : {:?}", other, maybe_own),
                },
                hir::Slice::Strs(_string_encoding) => todo!(),
                hir::Slice::Struct(_maybe_own, _) => todo!(),
                _ => todo!(),
            },
            hir::Type::Callback(callback) => self.lower_callback_input(input_context, callback),
            hir::Type::Enum(enum_path) => {
                // Enums cross the FFI boundary by value as their underlying
                // integer discriminant. The raw extern and the idiomatic
                // surface both take the enum type directly; no marshalling
                // glue needed.
                let ty = self.enum_name(enum_path);
                InputLowering {
                    raw_param: format!("{ty} {arg_name}"),
                    idiomatic_param: format!("{ty} {arg_name}"),
                    raw_call_arg: arg_name.to_string(),
                    idiomatic_param_type: Some(ty),
                    ..Default::default()
                }
            }
            other => unimplemented!("input type {:?}", other),
        }
    }

    fn lower_callback_input(
        &self,
        input_context: MethodInputContext<'tcx>,
        callback: &hir::Callback,
    ) -> InputLowering {
        let arg_name = input_context.arg_name().to_string();
        let return_type = self.lower_callback_return_type(&callback.output);
        let mut callback_param_types = Vec::new();
        let mut callback_param_decls = Vec::new();
        let mut callback_param_names = Vec::new();

        for (index, param) in callback.params.iter().enumerate() {
            let param_type = self.lower_callback_param_type(&param.ty);
            let param_name = param
                .name
                .as_ref()
                .map(|name| self.formatter.fmt_param_name(name.as_str()).into_owned())
                .unwrap_or_else(|| format!("arg{index}"));

            callback_param_decls.push(format!("{param_type} {param_name}"));
            callback_param_types.push(param_type);
            callback_param_names.push(param_name);
        }

        let mut delegate_args = vec!["IntPtr callbackHandle".to_string()];
        delegate_args.extend(callback_param_decls.iter().cloned());
        let idiomatic_type = callback_idiomatic_type(&callback_param_types, &return_type);
        let callback = DotnetCallback::new(
            self.namespace.to_string(),
            &input_context,
            return_type,
            delegate_args.join(", "),
            callback_param_decls.join(", "),
            callback_param_names.join(", "),
            idiomatic_type.clone(),
        );
        let callback_name = callback.name.clone();
        self.callback_struct_registry
            .borrow_mut()
            .insert(callback_name.clone(), callback);

        InputLowering {
            raw_param: format!("{callback_name} {arg_name}"),
            idiomatic_param: format!("{idiomatic_type} {arg_name}"),
            raw_call_arg: format!("{callback_name}.FromDelegate({arg_name})"),
            validation_statement: Some(format!(
                "if ({arg_name} == null) throw new ArgumentNullException(nameof({arg_name}));"
            )),
            idiomatic_param_type: Some(idiomatic_type),
            ..Default::default()
        }
    }

    fn lower_callback_return_type(
        &self,
        output: &hir::ReturnType<hir::InputOnly>,
    ) -> DotnetReturnType {
        match output {
            hir::ReturnType::Infallible(hir::SuccessType::Unit) => DotnetReturnType::Unit,
            hir::ReturnType::Infallible(hir::SuccessType::OutType(hir::Type::Primitive(p))) => {
                DotnetReturnType::Primitive(DotnetPrimitives::from(p))
            }
            other => unimplemented!("callback return type {:?}", other),
        }
    }

    fn lower_callback_param_type(&self, ty: &hir::Type<hir::OutputOnly>) -> String {
        match ty {
            hir::Type::Primitive(p) => DotnetPrimitives::from(p).to_string(),
            other => unimplemented!("callback parameter type {:?}", other),
        }
    }
}

fn callback_idiomatic_type(param_types: &[String], return_type: &DotnetReturnType) -> String {
    if return_type.is_void() {
        if param_types.is_empty() {
            "Action".to_string()
        } else {
            format!("Action<{}>", param_types.join(", "))
        }
    } else {
        let mut types = param_types.to_vec();
        types.push(return_type.to_string());
        format!("Func<{}>", types.join(", "))
    }
}

fn name_ptr(ty: &str) -> String {
    format!("{ty}Ptr")
}

fn name_bytes(ty: &str) -> String {
    format!("{ty}Bytes")
}
