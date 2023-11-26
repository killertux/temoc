use convert_case::{Case, Casing};
use proc_macro::{TokenStream, TokenTree};
use quote::quote;
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, Type, Visibility};

#[proc_macro_attribute]
pub fn fixture(args: TokenStream, input: TokenStream) -> TokenStream {
    let impl_fixture = parse_macro_input!(input as ItemImpl);
    let generics = &impl_fixture.generics;
    let ty = &impl_fixture.self_ty;

    let class_path = get_class_path(args, ty);
    let impl_methods = impl_slim_fixture_methods(ty, &impl_fixture.items);

    quote! {
        #impl_fixture

        impl #generics ::rust_slim::SlimFixture for #ty {
            #impl_methods
        }

        impl #generics ::rust_slim::ClassPath for #ty {
            fn class_path() -> String {
                #class_path
            }
        }
    }
    .into()
}

fn impl_slim_fixture_methods(ty: &Type, items: &[ImplItem]) -> proc_macro2::TokenStream {
    let mut methods = Vec::new();
    for item in items {
        match item {
            ImplItem::Fn(impl_fn) => {
                if !matches!(impl_fn.vis, Visibility::Public(_)) {
                    continue;
                }
                let has_receiver = impl_fn.sig.inputs.iter().any(|param| match param {
                    FnArg::Receiver(_) => true,
                    _ => false,
                });
                if !has_receiver {
                    continue;
                }
                let _n_args = impl_fn.sig.inputs.len() - 1;
                let method = impl_fn.sig.ident.to_string();
                let ident = &impl_fn.sig.ident;
                let args = impl_fn.sig.inputs.iter().skip(1).enumerate().map(|(i, fn_arg)| {
                    let FnArg::Typed(typed_arg) = fn_arg else {
                        panic!("Expected a typed arg")
                    };
                    let ty = &typed_arg.ty;
                    quote!{args[#i].parse::<#ty>().map_err(|e| ::rust_slim::ExecuteMethodError::ArgumentParsingError(e.to_string()))?}
                });
                methods.push(quote! {
                    #method => ::rust_slim::ToSlimResultString::to_slim_result_string(#ty::#ident(self,#(#args),*))
                });
            }
            _ => {}
        }
    }
    quote! {
        fn execute_method(&mut self, method: &str, args: ::std::vec::Vec<::std::string::String>) -> ::std::result::Result<::std::string::String, ::rust_slim::ExecuteMethodError> {
            match method {
                #(#methods,)*
                _ => Err(::rust_slim::ExecuteMethodError::ExecutionError(method.to_string())),
            }
        }
    }
}

fn get_class_path(args: TokenStream, ty: &Type) -> proc_macro2::TokenStream {
    let args: Vec<String> = args
        .into_iter()
        .map(|t| match t {
            TokenTree::Literal(literal) => literal.to_string(),
            _ => panic!("Expected a string path"),
        })
        .collect();
    if !args.is_empty() {
        let mut path: Vec<String> = args[0].split('.').map(|s| s.to_case(Case::Snake)).collect();
        path.last_mut()
            .map(|last| *last = last.to_case(Case::Pascal));
        let path = path.join("::");
        quote! {
            #path.into()
        }
    } else {
        let path = match ty {
            Type::Path(path) => path.path.get_ident().unwrap().to_string(),
            _ => todo!(),
        };
        quote! {
            format!("{}::{}", module_path!(), #path)
        }
    }
}
