use std::ffi::CStr;

use quote::{quote, quote_spanned};
use syn::{
    Expr, ExprAssign, Token,
    parse::Parse,
    parse_macro_input,
    punctuated::{IntoIter, Punctuated},
    spanned::Spanned,
};

struct ErrorList(Punctuated<ExprAssign, Token![,]>);

impl IntoIterator for ErrorList {
    type Item = ExprAssign;

    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Parse for ErrorList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self(Punctuated::<ExprAssign, Token![,]>::parse_terminated(
            input,
        )?))
    }
}

pub(crate) struct DefineError {}

impl DefineError {
    /// Creates a new instance of `DefineError`.
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Generates the `Error` constant definitions from a comma-delimited list of simple assignment
    /// expressions in the form given by the regular expression, `((?P<name>\w+)\s+=\s+(?P<value>\d+),?)*`.
    pub(crate) fn generate(&mut self, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
        let input = parse_macro_input!(item as ErrorList);
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
                    Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
                        if let Expr::Lit(l) = &*u.expr
                            && let syn::Lit::Int(l) = &l.lit
                        {
                            -l.base10_parse()
                                .map_err(|e| syn::Error::to_compile_error(&e))?
                        } else {
                            return Err(syn::Error::new(
                                e.right.span(),
                                "expected a negative decimal value",
                            )
                            .to_compile_error());
                        }
                    }
                    _ => {
                        return Err(syn::Error::new(
                            e.right.span(),
                            "expected a negative unary value",
                        )
                        .to_compile_error());
                    }
                };

                let msg = unsafe { &CStr::from_ptr(libc::gai_strerror(val)).to_string_lossy() };

                Ok(quote_spanned! {e.span()=>
                    #[doc = #msg]
                    pub const #name: Error = Error(#val);

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
