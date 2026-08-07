use proc_macro::TokenStream;

mod define_error;

/// Generates `Error` constant definitions for the `libanl` crate.
///
/// The macro takes a comma-delimited list of simple expression assignments given by the regular
/// expression, `((?P<name>\w+)\s+=\s+(?P<value>\d+),?)*`, and generates `Error` constant
/// definitions. The definitions include automatically generated documentation by passing the value
/// to the [`gai_strerror`] function.
///
/// <div class="warning">
///
/// **NOTE:** This macro is intended only for use by the `libanl` crate for its `Error` definitions.
/// The macro defintion and usage is subject to change without notice.
///
/// </div>
///
/// [`gai_strerror`]: https://www.man7.org/linux/man-pages/man3/gai_strerror.3.html
#[proc_macro]
pub fn define_error(item: TokenStream) -> TokenStream {
    define_error::DefineError::new().generate(item)
}
