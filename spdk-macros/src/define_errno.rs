use std::ffi::CStr;

use quote::{quote, quote_spanned};
use syn::{
    parse::Parse,
    parse_macro_input,
    punctuated::{IntoIter, Punctuated},
    spanned::Spanned,
    Expr, ExprAssign, Token,
};

struct ErrnoList(Punctuated<ExprAssign, Token![,]>);

impl IntoIterator for ErrnoList {
    type Item = ExprAssign;

    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Parse for ErrnoList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self(Punctuated::<ExprAssign, Token![,]>::parse_terminated(
            input,
        )?))
    }
}

pub(crate) struct DefineErrno {}

impl DefineErrno {
    /// Creates a new instance of `DefineErrno`.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Generates the `Errno` constant definitions from a comma-delimited list of simple assignment
    /// expressions in the form given by the regular expression, `((?P<name>\w+)\s+=\s+(?P<value>\d+),?)*`.
    pub(crate) fn generate(&mut self, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
        let input = parse_macro_input!(item as ErrnoList);
        let (defs, errors): (_, _) = input
            .into_iter()
            .map(|e| {
                let name = match &*e.left {
                    Expr::Path(p) if p.path.segments.len() == 1 => {
                        p.path.segments.first().unwrap().ident.clone()
                    }
                    _ => {
                        return Err(
                            syn::Error::new(e.left.span(), "expected a plain identifier")
                                .to_compile_error(),
                        );
                    }
                };

                let val: i32 = match &*e.right {
                    Expr::Lit(l) if let syn::Lit::Int(l) = &l.lit => l
                        .base10_parse()
                        .map_err(|e| syn::Error::to_compile_error(&e))?,
                    _ => {
                        return Err(
                            syn::Error::new(e.right.span(), "expected a numeric literal")
                                .to_compile_error(),
                        )
                    }
                };

                let msg = unsafe { &CStr::from_ptr(libc::strerror(val)).to_string_lossy() };

                Ok(quote_spanned! {e.span()=>
                    #[doc = #msg]
                    pub const #name: Errno = Errno(#val);

                })
            })
            .partition::<Vec<_>, _>(Result::is_ok);

        let defs = defs.into_iter().map(Result::ok);
        let errors = errors.into_iter().map(Result::err);

        let output = quote! {
            #(#errors)*
            #(#defs)*
        };

        output.into()
    }
}
