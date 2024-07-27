use std::ffi::CString;

use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::LitCStr;

pub(crate) struct GenerateModule {
}

impl GenerateModule {
    /// Creates a new instance of the `GenerateModule` struct.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Generates the code for the `module` attribute macro.
    pub(crate) fn generate(&mut self, _attr: TokenStream, item: TokenStream) -> TokenStream {
        let input = syn::parse_macro_input!(item as syn::ItemStruct);
        let module_ident = input.ident.clone();
        let module_name = input.ident
            .to_string()
            .trim_end_matches("Module")
            .from_case(Case::UpperCamel)
            .to_case(Case::Snake);
        let module_ident_name = module_name.from_case(Case::Snake).to_case(Case::UpperSnake);
        let reg_var_ident = format_ident!("__{}_MODULE", module_ident_name);
        let module_name_ident = format_ident!("__{}_MODULE_NAME", module_ident_name);
        let module_name_cstr = CString::new(module_name).unwrap();
        let module_name_lit = LitCStr::new(&module_name_cstr, module_ident.span());

        let output = quote!{
            #input

            static #reg_var_ident: ::std::sync::OnceLock<::spdk::bdev::Module<#module_ident>> = ::std::sync::OnceLock::new();

            const #module_name_ident: &::std::ffi::CStr = #module_name_lit;

            #[static_init::constructor]
            extern "C" fn register() {
                #reg_var_ident.set(::spdk::bdev::Module::new(#module_name_ident)).unwrap();

                #reg_var_ident.get().unwrap().register();
            }

            impl ::spdk::bdev::ModuleInstance<#module_ident> for #module_ident {
                fn instance() -> &'static #module_ident {
                    &#reg_var_ident.get().unwrap().instance
                }

                fn module() -> *const ::spdk_sys::spdk_bdev_module {
                    &#reg_var_ident.get().unwrap().module as *const _
                }

                fn new_bdev<B>(name: &::std::ffi::CStr, ctx: B) -> Box<::spdk::bdev::BDevImpl<B>>
                where
                    B: ::spdk::bdev::BDevOps
                {
                    ::spdk::bdev::BDevImpl::new(name, Self::module(), ctx)
                }
            }
        };

        output.into()
    }
}
