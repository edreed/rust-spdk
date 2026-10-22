use std::ffi::CString;

use convert_case::{
    Case::{Snake, UpperCamel, UpperSnake},
    Casing,
};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{FnArg, Ident, ItemFn, Lifetime, LitCStr, Type, spanned::Spanned, visit::Visit};

/// Represents a field in the generated `RpcCall` structure.
///
/// A field is generated for each argument in the RPC call's function signature.
#[derive(Default)]
struct Field {
    ident: Option<Ident>,
    type_: Option<Type>,
    errors: Vec<proc_macro2::TokenStream>,
}

impl Field {
    /// Creates a new [`Field`] for a function argument.
    fn from_fn_arg(fn_arg: &FnArg) -> Self {
        let mut this: Self = Default::default();

        this.visit_fn_arg(fn_arg);

        this
    }

    /// Returns the error found while parsing the function argument.
    fn errors(&self) -> Vec<proc_macro2::TokenStream> {
        self.errors.clone()
    }

    /// Returns the field declarations to used used in the generated `RpcCall` structure.
    fn declaration(&self) -> proc_macro2::TokenStream {
        if let (Some(ident), Some(type_)) = (self.ident.as_ref(), self.type_.as_ref()) {
            return quote!(#ident: #type_);
        }

        syn::Error::new(Span::call_site(), "missing argument name or type").into_compile_error()
    }

    /// Returns the arguments to pass to the RPC call function in the `RcpCall::invoke` implementation.
    fn to_call_arg(&self) -> proc_macro2::TokenStream {
        if let (Some(ident), Some(type_)) = (self.ident.as_ref(), self.type_.as_ref()) {
            match type_ {
                Type::Reference(_) => return quote!(&self.#ident),
                _ => return quote!(self.#ident),
            }
        }

        syn::Error::new(Span::call_site(), "missing argument name or type").into_compile_error()
    }
}

impl<'ast> Visit<'ast> for Field {
    fn visit_receiver(&mut self, receiver: &'ast syn::Receiver) {
        self.errors.push(
            syn::Error::new(
                receiver.span(),
                "the `jsonrpc` attribute cannot be applied to a function with a receiver",
            )
            .into_compile_error(),
        );
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        self.errors.push(
            syn::Error::new(
                attr.span(),
                "attributes on function arguments are not supported by the `jsonrpc` attribute",
            )
            .into_compile_error(),
        );
    }

    fn visit_pat_ident(&mut self, arg_name: &'ast syn::PatIdent) {
        self.ident = Some(arg_name.ident.clone())
    }

    fn visit_type(&mut self, arg_type: &'ast syn::Type) {
        if let Type::Reference(reference) = arg_type {
            let mut field_ref = reference.clone();

            field_ref.lifetime = Some(Lifetime::new("'call", Span::call_site()));

            self.type_ = Some(Type::Reference(field_ref));
            return;
        }

        self.type_ = Some(arg_type.clone());
    }
}

/// Represents the return type of the RPC call's function signature.
#[derive(Default)]
struct ReturnType {
    is_result: bool,
    type_: Option<proc_macro2::TokenStream>,
}

impl ReturnType {
    /// Creates a [`ReturnType`] from the RPC call's function signature.
    fn from_item_fn(item_fn: &syn::ItemFn) -> Self {
        let mut this: Self = Default::default();

        this.visit_return_type(&item_fn.sig.output);

        this
    }

    /// Returns whether the return type is a [`std::result::Result`] and the token stream describing
    /// the return value's type.
    ///
    /// This instance is consumed in the process.
    fn information(mut self) -> (bool, proc_macro2::TokenStream) {
        (
            self.is_result,
            self.type_.take().unwrap_or_else(|| {
                syn::Error::new(
                    Span::call_site(),
                    "function signature is missing return type",
                )
                .into_compile_error()
            }),
        )
    }
}

impl<'ast> Visit<'ast> for ReturnType {
    fn visit_return_type(&mut self, ret_type: &'ast syn::ReturnType) {
        self.type_ = match ret_type {
            syn::ReturnType::Default => Some(quote!(())),
            syn::ReturnType::Type(_, type_) => {
                self.visit_type(type_.as_ref());

                Some(quote!(#type_))
            }
        }
    }

    fn visit_type_path(&mut self, type_path: &'ast syn::TypePath) {
        if let Some(segment) = type_path.path.segments.last()
            && segment.ident.eq("Result")
        {
            self.is_result = true;
        }
    }
}

/// Generates the code for the [`jsonrpc`] attribute.
///
/// [`jsonrpc`]: macro@crate::jsonrpc
pub(crate) fn generate_rpc_call(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let fun = syn::parse_macro_input!(item as ItemFn);
    let fun_ident = fun.sig.ident.clone();
    let fun_name = fun_ident.to_string();

    let call_struct_ident =
        format_ident!("__{}RpcCall", fun_name.from_case(Snake).to_case(UpperCamel));

    let fun_name_constant_ident = format_ident!(
        "__{}_RPC_NAME",
        fun_name.from_case(Snake).to_case(UpperSnake)
    );
    let fun_name_constant_value =
        LitCStr::new(&CString::new(fun_name.clone()).unwrap(), fun_ident.span());
    let fun_register_ident = format_ident!("__register_{}_rpc_call", fun_name);

    let fields: Vec<_> = fun.sig.inputs.iter().map(Field::from_fn_arg).collect();

    let errors = fields.iter().flat_map(Field::errors);
    let field_decls = fields.iter().map(Field::declaration);
    let call_args = fields.iter().map(Field::to_call_arg);

    let (fun_return_is_result, fun_return_type) = ReturnType::from_item_fn(&fun).information();

    let output_type = if fun_return_is_result {
        fun_return_type
    } else {
        quote!(std::result::Result<#fun_return_type, spdk::json::RpcInfallible>)
    };

    let mut fun_call: proc_macro2::TokenStream = if fun.sig.asyncness.is_none() {
        quote!(#fun_ident(#(#call_args),*))
    } else {
        quote!(#fun_ident(#(#call_args),*).await)
    };

    if !fun_return_is_result {
        fun_call = quote!(Ok(#fun_call))
    }

    let output = quote! {
        #(#errors)*

        #fun

        #[derive(serde::Deserialize)]
        struct #call_struct_ident<'call> {
            #[serde(skip)]
            _unused: std::marker::PhantomData<&'call ()>,
            #(#field_decls),*
        }

        impl<'call> spdk::json::RpcCall<'call> for #call_struct_ident<'call> {
            type Output = #output_type;

            async fn invoke(self) -> Self::Output {
                #fun_call
            }
        }

        const #fun_name_constant_ident: &std::ffi::CStr = #fun_name_constant_value;

        #[static_init::constructor]
        extern "C" fn #fun_register_ident() {
            unsafe {
                spdk_sys::spdk_rpc_register_method(
                    #fun_name_constant_ident.as_ptr(),
                    Some(spdk::json::rpc_call::<'_, #call_struct_ident<'_>>),
                    spdk_sys::SPDK_RPC_RUNTIME,
                );
            }
        }
    };

    output.into()
}
