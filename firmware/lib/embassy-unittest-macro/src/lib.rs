use proc_macro::TokenStream;
use syn::parse_macro_input;

mod test;

#[proc_macro_attribute]
pub fn test(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    crate::test::test_impl(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
