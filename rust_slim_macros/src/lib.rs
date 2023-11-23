use proc_macro::TokenStream;
use quote::quote;
use syn::{ ItemImpl, parse_macro_input};

#[proc_macro_attribute]
pub fn fixture(_metadata: TokenStream, input: TokenStream) -> TokenStream {
    let inputc = input.clone();
    let impl_fixture = dbg!(parse_macro_input!(input as ItemImpl));
    inputc
}
