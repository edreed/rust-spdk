use std::ffi::CString;

use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{LitCStr, Meta, spanned::Spanned};

pub(crate) struct GenerateModule {}

impl GenerateModule {
    /// Creates a new instance of the `GenerateModule` struct.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Generates the code for the `module` attribute macro.
    pub(crate) fn generate(&mut self, attr: TokenStream, item: TokenStream) -> TokenStream {
        let input = syn::parse_macro_input!(item as syn::ItemStruct);
        let module_ident = input.ident.clone();
        let module_basename = input
            .ident
            .to_string()
            .trim_end_matches("Module")
            .to_string();
        let module_name = module_basename
            .from_case(Case::UpperCamel)
            .to_case(Case::Snake);
        let product_name = format!(
            "{} Disk",
            module_basename
                .from_case(Case::UpperCamel)
                .to_case(Case::Title)
        );
        let mut product_name_cstr = CString::new(product_name).unwrap();
        let module_ident_name = module_name.from_case(Case::Snake).to_case(Case::UpperSnake);
        let reg_var_ident = format_ident!("__{}_MODULE", module_ident_name);
        let module_name_ident = format_ident!("__{}_MODULE_NAME", module_ident_name);
        let module_name_cstr = CString::new(module_name).unwrap();
        let module_name_lit = LitCStr::new(&module_name_cstr, module_ident.span());

        if !attr.is_empty() {
            let meta = syn::parse_macro_input!(attr as Meta);

            match &meta {
                Meta::NameValue(nv) if nv.path.is_ident("product_name") => {
                    let expr = &nv.value;

                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit_str),
                        ..
                    }) = expr
                    {
                        product_name_cstr = CString::new(lit_str.value()).unwrap();
                    } else {
                        return syn::Error::new_spanned(expr, "expected string literal")
                            .into_compile_error()
                            .into();
                    }
                }
                _ => {
                    let message = format!(
                        "unexpected attribute metadata: {}`",
                        meta.span().source_text().unwrap()
                    );

                    return syn::Error::new(meta.span(), message)
                        .into_compile_error()
                        .into();
                }
            }
        }

        let product_name_lit = LitCStr::new(&product_name_cstr, module_ident.span());

        let output = quote! {
            #input

            static #reg_var_ident: ::std::sync::OnceLock<::spdk::bdev::Module<#module_ident>> = ::std::sync::OnceLock::new();

            const #module_name_ident: &::std::ffi::CStr = #module_name_lit;

            #[static_init::constructor]
            extern "C" fn register() {
                #reg_var_ident.set(::spdk::bdev::Module::new(#module_name_ident)).unwrap();

                #reg_var_ident.get().unwrap().register();
            }

            impl ::spdk::bdev::ModuleInstance<#module_ident> for #module_ident {
                fn instance() -> &'static ::spdk::bdev::Module<#module_ident> {
                    &#reg_var_ident.get().unwrap()
                }

                fn product_name() -> &'static ::std::ffi::CStr {
                    #product_name_lit
                }
            }
        };

        output.into()
    }
}
