use proc_macro::TokenStream;
use quote::{
    quote,
    quote_spanned,
};
use syn::{
    ItemFn,
    Meta,
    parse_macro_input,
    spanned::Spanned,
};

/// Generates the `main` function for an application using the SPDK Application
/// Framework.
pub(crate) fn generate_main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let fun = syn::parse_macro_input!(item as ItemFn);

    if format!("{}", fun.sig.ident) != "main" || fun.sig.asyncness.is_none() {
        return syn::Error::new_spanned(fun.sig, "expected `async fn main()`")
            .into_compile_error()
            .into();
    }

    let mut build_rt = quote!{
        match spdk::runtime::Runtime::from_cmdline() {
            Ok(rt) => rt,
            Err(_) => {
                std::process::exit(1);
            },
        }
    };

    if !attr.is_empty() {
        let meta: Meta = parse_macro_input!(attr);

        match &meta {
            Meta::NameValue(nv) if nv.path.is_ident("cli_args") => {
                let expr = &nv.value;

                build_rt = quote_spanned!{expr.span()=>
                    match #expr {
                        Ok(builder) => builder.build(),
                        Err(_) => {
                            std::process::exit(1);
                        },
                    }
                };
            },
            _ => {
                let message = format!(
                    "unexpected attribute metadata: {}`",
                    meta.span().source_text().unwrap()
                );

                return syn::Error::new(meta.span(), message)
                    .into_compile_error()
                    .into();
            },
        }
    }

    let block = fun.block;
    let expanded = quote!{
        fn main() {
            let rt = #build_rt;

            rt.block_on(async #block);
        }
    };

    expanded.into()
}
