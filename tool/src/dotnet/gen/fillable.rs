use std::fmt::Display;

use askama::Template;
use diplomat_core::hir::{OutputOnly, Type};

use crate::dotnet::r#gen::{DotnetPrimitives, ItemGenContext, method::DotnetReturnType};

#[derive(Template)]
#[template(path = "dotnet/result.raw.cs.jinja", escape = "none")]
pub(crate) struct DotnetResult {
    pub(crate) namespace: String,
    pub(crate) result_struct_name: DotnetResultName,
    pub(crate) exception_name: String,
    pub(crate) ok_result: DotnetReturnType,
    pub(crate) error: DotnetErrorType,
}

/// Exception class — generated once per unique error type encountered in
/// any `Result<T, E>` return. Catchable by name in user code
/// (`catch (ColorErrorException ex) { ... ex.Inner ... }`).
#[derive(Template)]
#[template(path = "dotnet/exception.cs.jinja", escape = "none")]
pub(crate) struct DotnetException {
    pub(crate) namespace: String,
    pub(crate) error: DotnetErrorType,
    pub(crate) exception_name: String,
    pub(crate) message_method: Option<String>,
}

/// Runtime helper for `Option<value-type>` — tagged struct on the wire,
/// parallel to `DotnetResult` but with unit error (i.e. `IsSome` instead
/// of `IsOk`, no `Err` payload). One emitted per unique inner type.
#[derive(Template)]
#[template(path = "dotnet/option.raw.cs.jinja", escape = "none")]
pub(crate) struct DotnetOption {
    pub(crate) namespace: String,
    pub(crate) option_struct_name: DotnetOptionName,
    pub(crate) inner: DotnetReturnType,
}

#[derive(Debug, Clone)]
pub(crate) struct DotnetOptionName(String);

impl Display for DotnetOptionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl DotnetOption {
    pub(crate) fn new(namespace: String, inner: DotnetReturnType) -> Self {
        Self {
            namespace,
            option_struct_name: DotnetOptionName(format!("DiplomatOption{}", inner)),
            inner,
        }
    }

    pub(crate) fn key(&self) -> String {
        format!("option:{}", self.inner)
    }

    pub(crate) fn option_info(&self) -> OptionInfo {
        OptionInfo {
            raw_option_type: Some(self.option_struct_name.clone()),
        }
    }
}

/// Carried on `MethodInfo` when the return is `Option<T>`. Templates branch:
/// - `raw_option_type == None` → pointer-nullable (`Option<Box<T>>`). The
///   inner opaque return carries null directly; idiomatic body is
///   `result == null ? null : new T(result)`.
/// - `raw_option_type == Some(name)` → tagged struct (`Option<value-type>`).
///   Idiomatic body is `result.IsSome ? result.Value : (T?)null`.
#[derive(Debug, Clone)]
pub(crate) struct OptionInfo {
    pub(crate) raw_option_type: Option<DotnetOptionName>,
}

impl OptionInfo {
    /// Pointer-nullable Option — no runtime helper struct needed.
    pub(crate) fn nullable_pointer() -> Self {
        Self {
            raw_option_type: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DotnetResultName(String);

impl Display for DotnetResultName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}


impl DotnetResult {
    pub(crate) fn new(
        namespace: String,
        ok_result: DotnetReturnType,
        error: DotnetErrorType,
        exception_name: String,
    ) -> Self {
        Self {
            namespace,
            result_struct_name: DotnetResultName(format!("DiplomatResult{}{}", ok_result, error)),
            exception_name,
            ok_result,
            error,
        }
    }

    pub(crate) fn key(&self) -> String {
        format!("{}|{}", self.ok_result, self.error)
    }

    pub(crate) fn error_info(&self) -> ErrorInfo {
        ErrorInfo {
            error: self.error.clone(),
            exception_name: self.exception_name.clone(),
            raw_return_type: self.result_struct_name.clone(),
        }
    }

}

#[derive(Debug, Clone)]
pub(crate) enum DotnetErrorType {
    // Indicates Optional values
    Primitive(DotnetPrimitives),
    Opaque(String),
    Enum(String),
    Struct(String),
}

pub(crate) struct ErrorInfo {
       pub(crate) error : DotnetErrorType,
       pub(crate) exception_name: String,
       pub(crate) raw_return_type: DotnetResultName,
}

impl DotnetErrorType {
    pub(crate) fn new(value: &Type<OutputOnly>, ctx:&ItemGenContext ) -> Self {
        match value {
            Type::Primitive(primitive_type) => DotnetErrorType::Primitive(DotnetPrimitives::from(primitive_type)),
            Type::Opaque(opaque_path) =>  {
                let opaque_name = ctx.opaque_name(opaque_path);
                DotnetErrorType::Opaque(opaque_name)
            }
            Type::Enum(enum_path) => {
                let enum_name = ctx.enum_name(enum_path);
                DotnetErrorType::Enum(enum_name)
            }
            Type::Struct(struct_path) => {
                let struct_name = ctx.returnable_struct_name(struct_path);
                DotnetErrorType::Struct(struct_name)
            }
            _ => unimplemented!("unsupported error type: {:?}", value),
        }
    }

    pub(crate) fn raw(&self) -> String {
        match self {
            DotnetErrorType::Opaque(name) => format!("{name}*"),
            _ => self.to_string(),
        }
    }

    pub(crate) fn is_opaque(&self) -> bool {
        matches!(self, DotnetErrorType::Opaque(_))
    }

    pub(crate) fn exception_name(&self, trim_suffix: Option<&str>) -> String {
        let mut name = self.to_string();
        if let Some(trim_suffix) = trim_suffix {
            if let Some(trimmed) = name.strip_suffix(trim_suffix) {
                name = trimmed.to_string();
            }
        }
        format!("{name}Exception")
    }
}

impl Display for DotnetErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DotnetErrorType::Primitive(p) => write!(f, "{}", p),
            DotnetErrorType::Opaque(name) | DotnetErrorType::Enum(name) | DotnetErrorType::Struct(name) => write!(f, "{}", name),
        }
    }
}
