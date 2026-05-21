use askama::Template;

use crate::dotnet::gen::method::{DotnetReturnType, MethodInputContext};

#[derive(Template)]
#[template(path = "dotnet/callback.cs.jinja", escape = "none")]
pub struct DotnetCallback {
    pub namespace: String,
    pub name: String,
    pub return_type: DotnetReturnType,
    pub args: String,
    pub callback_args: String,
    pub callback_arg_names: String,
    pub idiomatic_type: String,
}

impl DotnetCallback {
    pub(super) fn new(
        namespace: String,
        input_context: &MethodInputContext<'_>,
        return_type: DotnetReturnType,
        args: String,
        callback_args: String,
        callback_arg_names: String,
        idiomatic_type: String,
    ) -> Self {
        let method_context = input_context.method();
        let method_abi_name = method_context.method_abi_name();
        let param_name = if input_context.param_ident().is_empty() {
            format!("arg{}", input_context.param_index())
        } else {
            input_context.param_ident().to_string()
        };
        let name = format!("DiplomatCallback_{method_abi_name}_{param_name}");

        Self {
            namespace,
            name,
            return_type,
            args,
            callback_args,
            callback_arg_names,
            idiomatic_type,
        }
    }

    pub(super) fn run_delegate_name(&self) -> String {
        format!("{}_Run", self.name)
    }
}
