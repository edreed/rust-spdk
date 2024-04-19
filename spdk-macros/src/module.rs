use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::quote;

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
        let module = input.ident.clone();
        let module_name = input.ident.to_string().trim_end_matches("Module").from_case(Case::UpperCamel).to_case(Case::Snake);
        let reg_var_name = module_name.from_case(Case::Snake).to_case(Case::UpperSnake);
        let reg_var = syn::Ident::new(&reg_var_name, module.span());

        let output = quote!{
            #input

            static #reg_var: ::std::sync::OnceLock<::spdk::bdev::Module<#module>> = ::std::sync::OnceLock::new();

            #[static_init::constructor]
            extern "C" fn register() {
                #reg_var.set(::spdk::bdev::Module::new(::byte_strings::c_str!(#module_name))).unwrap();

                #reg_var.get().unwrap().register();
            }

            impl ::spdk::bdev::ModuleInstance<#module> for #module {
                fn instance() -> &'static #module {
                    &#reg_var.get().unwrap().instance
                }

                fn module() -> *const ::spdk_sys::spdk_bdev_module {
                    &#reg_var.get().unwrap().module as *const _
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
