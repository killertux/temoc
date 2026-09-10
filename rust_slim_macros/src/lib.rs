use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_macro_input, Error, FnArg, ImplItem, ItemImpl, LitStr, Result, Type,
    Visibility,
};

/// Exposes the public instance methods in an implementation block as SliM
/// fixture methods.
///
/// Unsupported signatures produce compile errors at the method declaration.
#[proc_macro_attribute]
pub fn fixture(args: TokenStream, input: TokenStream) -> TokenStream {
    let impl_fixture = parse_macro_input!(input as ItemImpl);
    match expand_fixture(args, impl_fixture) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_fixture(args: TokenStream, impl_fixture: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if impl_fixture.trait_.is_some() {
        return Err(Error::new_spanned(
            &impl_fixture,
            "fixture must be applied to an inherent impl block",
        ));
    }
    let (impl_generics, _type_generics, where_clause) = impl_fixture.generics.split_for_impl();
    let ty = &impl_fixture.self_ty;
    let class_path = get_class_path(args, ty)?;
    let impl_methods = impl_slim_fixture_methods(ty, &class_path, &impl_fixture.items)?;

    Ok(quote! {
        #impl_fixture

        impl #impl_generics ::rust_slim::SlimFixture for #ty #where_clause {
            #impl_methods
        }

        impl #impl_generics ::rust_slim::ClassPath for #ty #where_clause {
            fn class_path() -> String {
                #class_path
            }
        }
    })
}

fn impl_slim_fixture_methods(
    ty: &Type,
    class_path: &proc_macro2::TokenStream,
    items: &[ImplItem],
) -> Result<proc_macro2::TokenStream> {
    let mut methods = Vec::new();
    for item in items {
        let ImplItem::Fn(impl_fn) = item else {
            continue;
        };
        if !matches!(impl_fn.vis, Visibility::Public(_)) {
            continue;
        }

        if impl_fn.sig.constness.is_some()
            || impl_fn.sig.asyncness.is_some()
            || impl_fn.sig.unsafety.is_some()
            || impl_fn.sig.abi.is_some()
            || impl_fn.sig.variadic.is_some()
        {
            return Err(Error::new_spanned(
                &impl_fn.sig,
                "const, async, unsafe, extern, and variadic fixture methods are not supported",
            ));
        }

        let Some(FnArg::Receiver(receiver)) = impl_fn.sig.inputs.first() else {
            // Associated functions are not callable SliM methods. Constructor
            // annotations will process them separately when constructor
            // support is added.
            continue;
        };
        if receiver.reference.is_none() || receiver.colon_token.is_some() {
            return Err(Error::new_spanned(
                receiver,
                "fixture methods must use an &self or &mut self receiver",
            ));
        }
        if !impl_fn.sig.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &impl_fn.sig.generics,
                "generic fixture methods are not supported",
            ));
        }

        let arity = impl_fn.sig.inputs.len() - 1;
        let method = impl_fn.sig.ident.to_string();
        let ident = &impl_fn.sig.ident;
        let arguments = impl_fn
            .sig
            .inputs
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, argument)| {
                let FnArg::Typed(argument) = argument else {
                    unreachable!("only the first fixture argument can be a receiver")
                };
                let ty = &argument.ty;
                quote! {
                    <#ty as ::rust_slim::FromSlimValue>::from_slim_value(
                        supplied_args.next().expect("argument count checked before conversion"),
                    ).map_err(|error| error.for_argument(#index))?
                }
            });
        methods.push(quote! {
            #method => {
                if args.len() != #arity {
                    return Err(::rust_slim::ExecuteMethodError::MethodNotFound {
                        method: method.to_string(),
                        class: #class_path,
                    });
                }
                let mut supplied_args = args.into_iter();
                ::rust_slim::IntoSlimValue::into_slim_value(<#ty>::#ident(self, #(#arguments),*))
            }
        });
    }
    Ok(quote! {
        fn execute_method(
            &mut self,
            method: &str,
            args: ::std::vec::Vec<::rust_slim::SlimValue>,
        ) -> ::std::result::Result<::rust_slim::SlimValue, ::rust_slim::ExecuteMethodError> {
            match method {
                #(#methods,)*
                _ => Err(::rust_slim::ExecuteMethodError::MethodNotFound {
                    method: method.to_string(),
                    class: #class_path,
                }),
            }
        }
    })
}

fn get_class_path(args: TokenStream, ty: &Type) -> Result<proc_macro2::TokenStream> {
    let parser = syn::punctuated::Punctuated::<LitStr, syn::Token![,]>::parse_terminated;
    let args = parser.parse(args)?;
    if args.len() > 1 {
        return Err(Error::new_spanned(
            args,
            "expected at most one fixture class path",
        ));
    }
    if let Some(path) = args.first() {
        return Ok(quote! { #path.into() });
    }

    let Type::Path(path) = ty else {
        return Err(Error::new_spanned(
            ty,
            "the fixture implementation must target a named type",
        ));
    };
    let Some(ident) = path.path.segments.last().map(|segment| &segment.ident) else {
        return Err(Error::new_spanned(
            ty,
            "the fixture implementation must target a named type",
        ));
    };
    let path = ident.to_string();
    Ok(quote! {
        ::rust_slim::from_rust_module_path_to_class_path(&format!("{}::{}", module_path!(), #path))
    })
}
