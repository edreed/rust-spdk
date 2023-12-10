use std::{
    cell::Cell,
    os::raw::c_int,
};

use proc_macro2::TokenStream;
use quote::{
    format_ident,
    quote,
    quote_spanned,
};
use syn::{
    Data::Struct,
    Expr,
    Field,
    Fields::Named,
    Ident,
    Lit::{
        self,
        Char,
        Str,
    },
    spanned::Spanned,
    Type,
    visit::{
        self,
        Visit,
    },
};
use ternary_rs::if_else;

/// Indicates that the command-line option is standalone and does not take an
/// argument (e.g. `-O`, `--option`).
const NO_ARGUMENT: c_int = 0;
/// Indicates that the command-line option requires an argument (e.g.
/// `-O=value`, `--option=value`).
const REQUIRED_ARGUMENT: c_int = 1;
/// Indicates that the command-line option takes an optional argument (e.g.
/// `-O`, `-O=value`, `--option`, `--option=value`).
const OPTIONAL_ARGUMENT: c_int = 2;

static mut CURRENT_OPT_VALUE: c_int = 1024;

#[derive(Default)]
struct Arg {
    ident: Option<Ident>,
    r#type: Option<Type>,
    has_arg: c_int,
    help: Option<String>,
    short_opt: Option<char>,
    long_opt: Option<String>,
    default: Option<TokenStream>,
    value_name: Option<String>,
    opt_val: Cell<Option<c_int>>,
    errors: Vec<TokenStream>,
}

impl Arg {
    /// Creates a new `Arg` initialized from a [`Field`].
    fn from_field(field: &Field) -> Self {
        let mut arg = Self { ..Default::default() };

        arg.visit_field(field);

        arg
    }

    /// Returns a `TokenStream` representing the initialization of a field in
    /// the derived struct
    fn field_init(&self) -> TokenStream {
        let ident = self.ident.as_ref().unwrap();
        let default = self.default.clone().unwrap_or_else(|| {
            quote_spanned!(self.r#type.span()=> std::default::Default::default())
        });

        quote!{
            #ident: #default
        }
    }

    /// Returns the value of the `val` field of the [`option`] struct.
    /// 
    /// [`option`]: ../../spdk_sys/struct.option.html
    fn opt_val(&self) -> c_int {
        if let Some(v) = self.opt_val.get() {
            return v
        }
        
        let opt_val = if let Some(c) = self.short_opt {
            c as c_int
        }
        else {
            let v = unsafe { CURRENT_OPT_VALUE + 1 };

            unsafe { CURRENT_OPT_VALUE = v };

            v
        };

        self.opt_val.set(Some(opt_val));

        opt_val
    }

    /// Returns a `TokenStream` representing the [`option`] struct for a field
    /// in the derived struct.
    /// 
    /// [`option`]: ../../spdk_sys/struct.option.html
    fn long_opt(&self) -> TokenStream {
        let ident = self.ident.as_ref().unwrap();
        let long_opt = self.long_opt.clone().unwrap_or_else(|| {
            ident.to_string().replace("_", "-")
        });
        let has_arg = self.has_arg;
        let opt_val = self.opt_val();

        quote!{
            spdk_sys::option {
                name: byte_strings::c_str!(#long_opt).as_ptr(),
                has_arg: #has_arg,
                flag: std::ptr::null_mut(),
                val: #opt_val as std::os::raw::c_int,
            }
        }
    }

    /// Returns a `TokenStream` representing the `match` arm in the argument
    /// parsing callback invoked by [`spdk_app_parse_args`] that converts the
    /// string argument into the field value.
    /// 
    /// [`spdk_app_parse_args`]: ../../spdk_sys/fn.spdk_app_parse_args.html
    fn parse_option(&self) -> TokenStream {
        let ident = self.ident.as_ref().unwrap();
        let opt_val = self.opt_val();

        match self.has_arg {
            NO_ARGUMENT => {
                quote_spanned!{self.r#type.as_ref().unwrap().span()=>
                    #opt_val => {
                        cur_value.#ident = true;
                        return 0;
                    },
                }
            },
            OPTIONAL_ARGUMENT => {
                quote_spanned!{self.r#type.as_ref().unwrap().span()=>
                    #opt_val => {
                        cur_value.#ident = match optarg {
                            Some(v) => v.parse().unwrap(),
                            None => true,
                        };
                        return 0;
                    },
                }
            },
            REQUIRED_ARGUMENT => {
                quote_spanned!{self.r#type.as_ref().unwrap().span()=>
                    #opt_val => {
                        cur_value.#ident = optarg.unwrap().parse().unwrap();
                        return 0;
                    },
                }
            },
            _ => unreachable!("unexpected has_arg value: {}", self.has_arg),
        }
    }

    /// Returns a `String` containg the short option string for a field in the
    /// derived struct.
    fn getopts_str(&self) -> String {
        let mut res = String::new();

        if let Some(short_opt) = self.short_opt.as_ref() {
            if short_opt.is_ascii_alphabetic() {
                res.push(*short_opt);
                for _ in 0..self.has_arg {
                    res.push(':');
                }
            }
        }

        res
    }

    /// Returns a `Vec<String>` containing the help text for a field in the
    /// derived struct.
    fn help(&self) -> Vec<String> {
        let ident = self.ident.as_ref().unwrap();
        let long_opt = self.long_opt.clone().unwrap_or_else(|| {
            ident.to_string().replace("_", "-")
        });
        let opt_val = self.opt_val();
        let value_name = self.value_name.clone().unwrap_or_else(|| {
            ident.to_string().to_uppercase()
        });

        let mut opt_text = if_else!(
            opt_val < 128,
            format!("-{}, --{}", opt_val as u8 as char, long_opt),
            format!("    --{}", long_opt));
        opt_text = match self.has_arg {
            NO_ARGUMENT => opt_text,
            OPTIONAL_ARGUMENT => {
                format!("{} [{}]", opt_text, value_name)
            },
            REQUIRED_ARGUMENT => {
                format!("{} <{}>", opt_text, value_name)
            }
            _ => unreachable!("unexpected has_arg value: {}", self.has_arg),
        };

        if opt_text.len() < 25 {
            vec![format!("  {:25}{}", opt_text, self.help.clone().unwrap_or_default())]
        }
        else {
            vec![
                format!("  {}", opt_text),
                format!("  {:>25}{}", "", self.help.clone().unwrap_or_default())
            ]
        }
    }

    fn errors(&self) -> Vec<TokenStream> {
        self.errors.clone()
    }
}

impl<'ast> Visit<'ast> for Arg {
    fn visit_ident(&mut self, ident: &'ast Ident) {
        self.ident = Some(ident.clone());
    }

    fn visit_type(&mut self, ty: &'ast Type) {
        self.r#type = Some(ty.clone());

        self.has_arg = match ty {
            syn::Type::Path(ref p) => {
                if_else!(p.path.is_ident("bool"), OPTIONAL_ARGUMENT, REQUIRED_ARGUMENT)
            },
            _ => REQUIRED_ARGUMENT,
        };
    }

    fn visit_meta_name_value(&mut self, name_value: &'ast syn::MetaNameValue) {
        if name_value.path.is_ident("doc") {
            if let Expr::Lit(s) = &name_value.value {
                if let Lit::Str(s) = &s.lit {
                    self.help = Some(s.value().trim().to_string());
                }
            };

            visit::visit_meta_name_value(self, name_value);
        }
    }

    fn visit_meta_list(&mut self, list: &'ast syn::MetaList) {
        if list.path.is_ident("spdk_arg") {
            let res = list.parse_nested_meta(|meta| {
                if meta.path.is_ident("short") {
                    if let Char(c) = meta.value()?.parse()? {
                        self.short_opt = Some(c.value());
                    }
                }
                else if meta.path.is_ident("long") {
                    if let Str(s) = meta.value()?.parse()? {
                        self.long_opt = Some(s.value());
                    }
                }
                else if meta.path.is_ident("default") {
                    let val: Expr = meta.value()?.parse()?;

                    self.default = Some(quote_spanned!(val.span()=> #val));
                }
                else if meta.path.is_ident("value_name") {
                    if let Str(s) = meta.value()?.parse()? {
                        self.value_name = Some(s.value());
                    }
                }
                else {
                    return Err(meta.error(format!("unexpected attribute metadata: {}", meta.path.get_ident().unwrap())));
                }

                Ok(())
            });

            if let Err(e) = res {
                self.errors.push(e.into_compile_error());
            }
        }

        visit::visit_meta_list(self, list)
    }
}

/// Orchestrates the derivation of the [`Parser`] trait for a struct annotated
/// with the `derive(Parser)` attribute.
/// 
/// [`Parser`]: ../../spdk/cli/trait.Parser.html
pub(crate) struct DeriveParser {
}

impl DeriveParser {
    /// Creates a new `DeriveParser`.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Derives the [`Parser`] trait for a struct annotated with the
    /// `derive(Parser)` attribute.
    /// 
    /// [`Parser`]: ../../spdk/cli/trait.Parser.html
    pub(crate) fn derive(&mut self, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
        let input =  syn::parse_macro_input!(input as syn::DeriveInput);

        let fields = match input.data {
            Struct(ref s) => {
                match &s.fields {
                    Named(f) => f.named.iter(),
                    _ => {
                        return syn::Error::new(input.span(), "cannot derive macro `Parser` for tuple or unit struct types")
                            .into_compile_error()
                            .into()
                    },
                }
            },
            _ => {
                return syn::Error::new(input.span(), "cannot derive macro `Parser` for non-struct types")
                    .into_compile_error()
                    .into()
            },
        };

        let args: Vec<_> = fields
            .map(Arg::from_field)
            .collect();

        let type_ident = input.ident;
        let uppername = type_ident.to_string().to_uppercase();
        let current_value_ident = format_ident!("{}_CURRENT_VALUE", uppername);

        let field_inits = args.iter().map(Arg::field_init);
        let parse_opts = args.iter().map(Arg::parse_option);
        let long_opt = args.iter().map(Arg::long_opt);
        let long_opts_count = long_opt.len();
        let getopts_str = args
            .iter()
            .map(Arg::getopts_str)
            .fold(String::new(), |mut acc, s| {
                acc.push_str(&s);

                acc
            });
        let help = args.iter().flat_map(Arg::help);
        let errors = args.iter().flat_map(Arg::errors);

        let output = quote! {
            #(#errors)*

            static ARGV_CSTRING: std::sync::OnceLock<Vec<std::ffi::CString>> = std::sync::OnceLock::new();

            static mut #current_value_ident: std::sync::OnceLock<#type_ident> = std::sync::OnceLock::new();

            impl #type_ident {
                fn new() -> Self {
                    Self {
                        #(#field_inits),*
                    }
                }

                unsafe extern "C" fn parse_option(ch: std::os::raw::c_int, optarg: *mut std::os::raw::c_char) -> std::os::raw::c_int {
                    let optarg = if !optarg.is_null() {
                        Some(std::ffi::CStr::from_ptr(optarg).to_str().unwrap())
                    } else {
                        None
                    };
                    let mut cur_value = #current_value_ident.get_mut().unwrap();

                    match ch {
                        #(#parse_opts)*
                        _ => {},
                    }

                    -libc::EINVAL
                }

                unsafe extern "C" fn app_usage() {
                    #(println!(#help);)*
                }
            }

            impl spdk::cli::Parser for #type_ident {
                fn parse() -> Result<spdk::runtime::Builder, spdk::errors::Errno> {
                    unsafe {
                        if let Err(_) = #current_value_ident.set(Self::new()) {
                            return Err(spdk::errors::EALREADY);
                        }
                    }

                    let argv = ARGV_CSTRING.get_or_init(|| {
                        std::env::args()
                            .map(|a| std::ffi::CString::new(a).unwrap())
                            .collect()
                    });
                    let argv: std::vec::Vec<*const std::os::raw::c_char> = argv
                        .iter()
                        .map(|a| a.as_ptr())
                        .collect();

                    let custom_opts: [spdk_sys::option; #long_opts_count] = [
                        #(#long_opt),*
                    ];

                    unsafe {
                        let mut app_opts = std::mem::MaybeUninit::<spdk_sys::spdk_app_opts>::uninit();

                        spdk_sys::spdk_app_opts_init(
                            app_opts.as_mut_ptr(),
                            std::mem::size_of::<spdk_sys::spdk_app_opts>()
                        );

                        let rc = spdk_sys::spdk_app_parse_args(
                            argv.len() as std::os::raw::c_int,
                            argv.as_ptr() as *mut *mut std::os::raw::c_char,
                            app_opts.as_mut_ptr(),
                            byte_strings::c_str!(#getopts_str).as_ptr(),
                            &custom_opts as *const _ as *mut spdk_sys::option,
                            Some(Self::parse_option),
                            Some(Self::app_usage)
                        );

                        if rc != spdk_sys::SPDK_APP_PARSE_ARGS_SUCCESS {
                            return Err(spdk::errors::EINVAL)
                        }

                        Ok(spdk::runtime::Builder::from_options(app_opts.assume_init()))
                    }
                }

                fn get() -> &'static Self {
                    use std::ops::Deref;

                    unsafe {
                        #current_value_ident.get().unwrap()
                    }
                }
            }
        };

        output.into()
    }
}
