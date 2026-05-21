use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use diplomat_core::hir::{
    self, CallbackInstantiationFunctionality, MaybeOwn, OpaqueOwner, ReturnType, StructPathLike,
    SuccessType, TraitIdGetter, TyPosition, Type,
};
use diplomat_tool::config::Config;

#[derive(Clone, Debug, ValueEnum)]
enum DumpFormat {
    Human,
    Debug,
}

/// Dump Diplomat HIR for a bridge entry file.
#[derive(Debug, Parser)]
#[clap(
    name = "diplomat-hir-dump",
    about = "Lower a Diplomat bridge entry file and dump the HIR TypeContext"
)]
struct Opt {
    /// The backend whose feature support should be used during HIR validation.
    #[clap(short, long, default_value = "c")]
    target_language: String,

    /// The path to the lib.rs file.
    #[clap(short, long, value_parser, default_value = "src/lib.rs")]
    entry: PathBuf,

    /// The path to an optional config file to override lowering defaults.
    #[clap(short, long, value_parser, default_value = "config.toml")]
    config_file: PathBuf,

    /// Inline config settings, in the form `key=value`.
    #[arg(long, value_parser, action=clap::ArgAction::Append)]
    config: Vec<String>,

    /// What features (`#[diplomat::attr(feature=)]`) are enabled.
    #[arg(long, value_parser, action=clap::ArgAction::Append)]
    features_enabled: Vec<String>,

    /// Optional output path. If omitted, prints to stdout.
    #[clap(short, long, value_parser)]
    output: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = DumpFormat::Human)]
    format: DumpFormat,
}

fn main() -> std::io::Result<()> {
    let opt = Opt::parse();

    let mut config = Config::default();
    config.read_file(&opt.config_file).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Error loading config: {e}"),
        )
    })?;
    config.read_cli_settings(opt.config);
    config.shared_config.features_enabled = opt.features_enabled.iter().cloned().collect();

    let tcx = diplomat_tool::lower_to_hir(&opt.entry, &opt.target_language, config)
        .map_err(std::io::Error::other)?;

    let dump = match opt.format {
        DumpFormat::Human => HumanFormatter::new(&tcx).render(),
        DumpFormat::Debug => format!("{tcx:#?}\n"),
    };
    if let Some(output) = opt.output {
        std::fs::write(output, dump)
    } else {
        print!("{dump}");
        Ok(())
    }
}

struct HumanFormatter<'tcx> {
    tcx: &'tcx hir::TypeContext,
}

impl<'tcx> HumanFormatter<'tcx> {
    fn new(tcx: &'tcx hir::TypeContext) -> Self {
        Self { tcx }
    }

    fn render(&self) -> String {
        let mut out = String::new();

        self.line(&mut out, "Diplomat HIR Summary");
        self.line(&mut out, "====================");
        self.line(&mut out, "");
        self.line(&mut out, "Types");
        self.line(&mut out, "-----");

        let mut wrote_type = false;
        for (id, ty) in self.tcx.all_types() {
            if ty.attrs().disable {
                continue;
            }
            wrote_type = true;
            match ty {
                hir::TypeDef::Struct(def) => {
                    self.line(&mut out, &format!("struct {}", def.name));
                    self.render_attrs(&mut out, &def.attrs, 2);
                    self.render_fields(&mut out, &def.fields, 2);
                    self.render_methods(&mut out, &def.methods, 2);
                }
                hir::TypeDef::OutStruct(def) => {
                    self.line(&mut out, &format!("out struct {}", def.name));
                    self.render_attrs(&mut out, &def.attrs, 2);
                    self.render_fields(&mut out, &def.fields, 2);
                    self.render_methods(&mut out, &def.methods, 2);
                }
                hir::TypeDef::Opaque(def) => {
                    self.line(&mut out, &format!("opaque {}", def.name));
                    self.line(&mut out, &format!("  destructor: {}", def.dtor_abi_name));
                    self.render_attrs(&mut out, &def.attrs, 2);
                    self.render_methods(&mut out, &def.methods, 2);
                }
                hir::TypeDef::Enum(def) => {
                    self.line(&mut out, &format!("enum {}", def.name));
                    self.render_attrs(&mut out, &def.attrs, 2);
                    if def.variants.is_empty() {
                        self.line(&mut out, "  variants: none");
                    } else {
                        self.line(&mut out, "  variants:");
                        for variant in &def.variants {
                            self.line(
                                &mut out,
                                &format!("    {} = {}", variant.name, variant.discriminant),
                            );
                        }
                    }
                    self.render_methods(&mut out, &def.methods, 2);
                }
                _ => {
                    self.line(&mut out, "unknown type");
                }
            }
            self.line(&mut out, &format!("  type id: {}", self.format_type_id(id)));
            self.line(&mut out, "");
        }

        if !wrote_type {
            self.line(&mut out, "  none");
            self.line(&mut out, "");
        }

        self.line(&mut out, "Traits");
        self.line(&mut out, "------");
        let mut wrote_trait = false;
        for (id, trt) in self.tcx.all_traits() {
            if trt.attrs.disable {
                continue;
            }
            wrote_trait = true;
            self.line(&mut out, &format!("trait {}", trt.name));
            self.line(&mut out, &format!("  trait id: {id:?}"));
            if trt.methods.is_empty() {
                self.line(&mut out, "  callbacks: none");
            } else {
                self.line(&mut out, "  callbacks:");
                for callback in &trt.methods {
                    self.render_callback(&mut out, callback, 4);
                }
            }
            self.line(&mut out, "");
        }
        if !wrote_trait {
            self.line(&mut out, "  none");
            self.line(&mut out, "");
        }

        self.line(&mut out, "Free Functions");
        self.line(&mut out, "--------------");
        let mut wrote_function = false;
        for (id, method) in self.tcx.all_free_functions() {
            if method.attrs.disable {
                continue;
            }
            wrote_function = true;
            self.line(&mut out, &format!("function {} ({id:?})", method.name));
            self.render_method_body(&mut out, method, 2);
            self.line(&mut out, "");
        }
        if !wrote_function {
            self.line(&mut out, "  none");
        }

        out
    }

    fn render_fields<P: TyPosition>(
        &self,
        out: &mut String,
        fields: &[hir::StructField<P>],
        indent: usize,
    ) {
        if fields.is_empty() {
            self.indented(out, indent, "fields: none");
            return;
        }

        self.indented(out, indent, "fields:");
        for field in fields {
            self.indented(
                out,
                indent + 2,
                &format!("{}: {}", field.name, self.format_type(&field.ty)),
            );
        }
    }

    fn render_methods(&self, out: &mut String, methods: &[hir::Method], indent: usize) {
        if methods.is_empty() {
            self.indented(out, indent, "methods: none");
            return;
        }

        self.indented(out, indent, "methods:");
        for method in methods {
            self.indented(out, indent + 2, &format!("method {}", method.name));
            self.render_method_body(out, method, indent + 4);
        }
    }

    fn render_method_body(&self, out: &mut String, method: &hir::Method, indent: usize) {
        self.indented(out, indent, &format!("abi: {}", method.abi_name));
        if let Some(special_method) = method.attrs.special_method.as_ref() {
            self.indented(out, indent, &format!("special: {special_method:?}"));
        }

        match &method.param_self {
            Some(param_self) => self.indented(
                out,
                indent,
                &format!("self: {}", self.format_self(&param_self.ty)),
            ),
            None => self.indented(out, indent, "self: none"),
        }

        if method.params.is_empty() {
            self.indented(out, indent, "params: none");
        } else {
            self.indented(out, indent, "params:");
            for param in &method.params {
                self.indented(
                    out,
                    indent + 2,
                    &format!("{}: {}", param.name, self.format_type(&param.ty)),
                );
            }
        }

        self.indented(
            out,
            indent,
            &format!("return: {}", self.format_return(&method.output)),
        );
    }

    fn render_callback(&self, out: &mut String, callback: &hir::Callback, indent: usize) {
        let name = callback
            .name
            .as_ref()
            .map(|name| name.as_str())
            .unwrap_or("<anonymous>");
        self.indented(out, indent, &format!("callback {name}"));
        if callback.params.is_empty() {
            self.indented(out, indent + 2, "params: none");
        } else {
            self.indented(out, indent + 2, "params:");
            for (index, param) in callback.params.iter().enumerate() {
                let name = param
                    .name
                    .as_ref()
                    .map(|name| name.as_str().to_string())
                    .unwrap_or_else(|| format!("arg{index}"));
                self.indented(
                    out,
                    indent + 4,
                    &format!("{name}: {}", self.format_type(&param.ty)),
                );
            }
        }
        self.indented(
            out,
            indent + 2,
            &format!("return: {}", self.format_return(&callback.output)),
        );
    }

    fn render_attrs(&self, out: &mut String, attrs: &hir::Attrs, indent: usize) {
        if let Some(namespace) = attrs.namespace.as_ref() {
            self.indented(out, indent, &format!("namespace: {namespace}"));
        }
        if let Some(deprecated) = attrs.deprecated.as_ref() {
            self.indented(out, indent, &format!("deprecated: {deprecated}"));
        }
    }

    fn format_return<P: TyPosition>(&self, ret: &ReturnType<P>) -> String {
        match ret {
            ReturnType::Infallible(ok) => self.format_success(ok),
            ReturnType::Fallible(ok, None) => format!("Result<{}, ()>", self.format_success(ok)),
            ReturnType::Fallible(ok, Some(err)) => {
                format!(
                    "Result<{}, {}>",
                    self.format_success(ok),
                    self.format_type(err)
                )
            }
            ReturnType::Nullable(ok) => format!("Option<{}>", self.format_success(ok)),
        }
    }

    fn format_success<P: TyPosition>(&self, success: &SuccessType<P>) -> String {
        match success {
            SuccessType::Write => "write output".to_string(),
            SuccessType::OutType(ty) => self.format_type(ty),
            SuccessType::Unit => "()".to_string(),
            _ => "<unknown success type>".to_string(),
        }
    }

    fn format_self(&self, ty: &hir::SelfType) -> String {
        match ty {
            hir::SelfType::Opaque(path) => {
                format!(
                    "{} opaque {}",
                    self.format_owner(path.borrowed()),
                    path.resolve(self.tcx).name
                )
            }
            hir::SelfType::Struct(path) => {
                format!(
                    "{} struct {}",
                    self.format_maybe_own(path.owner()),
                    path.resolve(self.tcx).name
                )
            }
            hir::SelfType::Enum(path) => {
                format!("value enum {}", path.resolve(self.tcx).name)
            }
            _ => "<unknown self type>".to_string(),
        }
    }

    fn format_type<P: TyPosition>(&self, ty: &Type<P>) -> String {
        match ty {
            Type::Primitive(primitive) => primitive.as_str().to_string(),
            Type::Opaque(path) => {
                let optional = if path.is_optional() { "optional " } else { "" };
                let owner = self.format_owner(&path.owner);
                format!(
                    "{}{} opaque {}",
                    optional,
                    owner,
                    path.resolve(self.tcx).name
                )
            }
            Type::Struct(path) => {
                let name = self.tcx.resolve_type(path.id()).name();
                format!("{} struct {name}", self.format_maybe_own(path.owner()))
            }
            Type::ImplTrait(path) => {
                format!("impl trait {}", self.tcx.resolve_trait(path.id()).name)
            }
            Type::Enum(path) => format!("enum {}", path.resolve(self.tcx).name),
            Type::Slice(slice) => self.format_slice(slice),
            Type::Callback(callback) => match callback.get_inputs() {
                Ok(params) => {
                    let params = params
                        .iter()
                        .map(|param| self.format_type(&param.ty))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let output = callback
                        .get_output_type()
                        .map(|ret| self.format_return(ret))
                        .unwrap_or_else(|_| "<unknown>".to_string());
                    format!("callback ({params}) -> {output}")
                }
                Err(()) => "callback <not valid in this position>".to_string(),
            },
            Type::DiplomatOption(inner) => format!("DiplomatOption<{}>", self.format_type(inner)),
            _ => "<unknown type>".to_string(),
        }
    }

    fn format_slice<P: TyPosition>(&self, slice: &hir::Slice<P>) -> String {
        match slice {
            hir::Slice::Str(_, encoding) => format!("{encoding:?} string slice"),
            hir::Slice::Primitive(owner, primitive) => {
                format!(
                    "{} slice [{}]",
                    self.format_maybe_own(*owner),
                    primitive.as_str()
                )
            }
            hir::Slice::Strs(encoding) => format!("{encoding:?} string-list slice"),
            hir::Slice::Struct(owner, path) => {
                let name = self.tcx.resolve_type(path.id()).name();
                format!("{} slice [{name}]", self.format_maybe_own(*owner))
            }
            _ => "<unknown slice>".to_string(),
        }
    }

    fn format_owner(&self, owner: &impl OpaqueOwner) -> String {
        if owner.is_owned() {
            "owned".to_string()
        } else if let Some(mutability) = owner.mutability() {
            format!("borrowed {}", self.format_mutability(mutability))
        } else {
            "borrowed".to_string()
        }
    }

    fn format_maybe_own(&self, owner: MaybeOwn) -> String {
        match owner {
            MaybeOwn::Own => "owned".to_string(),
            MaybeOwn::Borrow(borrow) => self.format_owner(&borrow),
        }
    }

    fn format_mutability(&self, mutability: hir::Mutability) -> &'static str {
        match mutability {
            hir::Mutability::Immutable => "immutable",
            hir::Mutability::Mutable => "mutable",
        }
    }

    fn format_type_id(&self, id: hir::TypeId) -> String {
        match id {
            hir::TypeId::Struct(id) => format!("{id:?}"),
            hir::TypeId::OutStruct(id) => format!("{id:?}"),
            hir::TypeId::Opaque(id) => format!("{id:?}"),
            hir::TypeId::Enum(id) => format!("{id:?}"),
            _ => "<unknown type id>".to_string(),
        }
    }

    fn indented(&self, out: &mut String, indent: usize, text: &str) {
        self.line(out, &format!("{}{text}", " ".repeat(indent)));
    }

    fn line(&self, out: &mut String, text: &str) {
        out.push_str(text);
        out.push('\n');
    }
}
