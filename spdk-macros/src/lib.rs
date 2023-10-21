//! Procedural macros for the spdk crate.
use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

/// Marks the main entry point of an application using the SPDK Application
/// Framework.
/// 
/// NOTE: This macro is for applications that do not require a complex setup.
/// Consider using [`Builder`](../spdk/runtime/struct.Builder.html) to create a
/// [`Runtime`](../spdk/runtime/struct.Runtime.html) directly if this macro
/// does not meet your needs.
/// 
/// The [`Runtime`](../spdk/runtime/struct.Runtime.html) created by this macro
/// is initialized from the command line arguments supported by the
/// [`spdk_app_parse_args`](../spdk_sys/fn.spdk_app_parse_args.html) function.
/// 
/// # Usage
/// 
/// ```no_run
/// #[spdk::main]
/// async fn main() {
///     println!("Hello, World!");
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let fun: ItemFn = syn::parse(item).unwrap();

    if format!("{}", fun.sig.ident) != "main" || fun.sig.asyncness.is_none() {
        panic!("expected `async fn main()`");
    }

    let block = fun.block;
    let expanded = quote!{
        fn main() {
            let rt = spdk::runtime::Runtime::from_cmdline().unwrap();

            rt.block_on(async #block);
        }
    };

    expanded.into()
}
