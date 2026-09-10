use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_macro_input, parse_quote, Attribute, Error, FnArg, GenericArgument,
    ImplItem, ImplItemFn, ItemImpl, LitStr, PathArguments, Result, ReturnType, Type, Visibility,
};

/// Exposes public instance methods in an implementation block as SliM fixture
/// methods. `#[slim(constructor)]` marks associated constructors and
/// `#[slim(sut)]` marks the zero-argument System Under Test accessor.
#[proc_macro_attribute]
pub fn fixture(args: TokenStream, input: TokenStream) -> TokenStream {
    let impl_fixture = parse_macro_input!(input as ItemImpl);
    match expand_fixture(args, impl_fixture) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_fixture(args: TokenStream, mut item: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if item.trait_.is_some() {
        return Err(Error::new_spanned(
            &item,
            "fixture must be applied to an inherent impl block",
        ));
    }
    let ty = &item.self_ty;
    let class_path = get_class_path(args, ty)?;
    let (constructors, sut) = extract_markers(&mut item.items)?;
    let methods = impl_slim_fixture_methods(ty, &class_path, &item.items, sut)?;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    let constructor = impl_constructor(ty, &item.generics, constructors)?;

    Ok(quote! {
        #item
        impl #impl_generics ::rust_slim::SlimFixture for #ty #where_clause { #methods }
        impl #impl_generics ::rust_slim::ClassPath for #ty #where_clause {
            fn class_path() -> String { #class_path }
        }
        #constructor
    })
}

fn extract_markers(items: &mut [ImplItem]) -> Result<(Vec<ImplItemFn>, Option<ImplItemFn>)> {
    let mut constructors = Vec::new();
    let mut sut = None;
    for item in items {
        let ImplItem::Fn(method) = item else { continue };
        let mut retained = Vec::new();
        let mut marker = None;
        for attribute in std::mem::take(&mut method.attrs) {
            if attribute.path().is_ident("slim") {
                let name = slim_marker(&attribute)?;
                if marker.replace(name).is_some() {
                    return Err(Error::new_spanned(
                        attribute,
                        "a fixture method may carry only one `#[slim(...)]` marker",
                    ));
                }
            } else {
                retained.push(attribute);
            }
        }
        method.attrs = retained;
        match marker.as_deref() {
            Some("constructor") => constructors.push(method.clone()),
            Some("sut") if sut.replace(method.clone()).is_some() => {
                return Err(Error::new_spanned(
                    method,
                    "a fixture may declare only one `sut` method",
                ));
            }
            Some("sut") => {}
            Some(name) => {
                return Err(Error::new_spanned(
                    method,
                    format!("unknown SliM fixture marker `{name}`"),
                ));
            }
            None => {}
        }
    }
    Ok((constructors, sut))
}

fn slim_marker(attribute: &Attribute) -> Result<String> {
    Ok(attribute.parse_args::<syn::Ident>()?.to_string())
}

fn validate_signature(method: &ImplItemFn) -> Result<()> {
    if method.sig.constness.is_some()
        || method.sig.asyncness.is_some()
        || method.sig.unsafety.is_some()
        || method.sig.abi.is_some()
        || method.sig.variadic.is_some()
    {
        return Err(Error::new_spanned(
            &method.sig,
            "const, async, unsafe, extern, and variadic fixture methods are not supported",
        ));
    }
    if !method.sig.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &method.sig.generics,
            "generic fixture methods are not supported",
        ));
    }
    Ok(())
}

fn impl_slim_fixture_methods(
    ty: &Type,
    class_path: &proc_macro2::TokenStream,
    items: &[ImplItem],
    sut: Option<ImplItemFn>,
) -> Result<proc_macro2::TokenStream> {
    let sut_name = sut.as_ref().map(|method| method.sig.ident.to_string());
    let mut methods = Vec::new();
    for item in items {
        let ImplItem::Fn(method) = item else { continue };
        if !matches!(method.vis, Visibility::Public(_))
            || sut_name.as_deref() == Some(&method.sig.ident.to_string())
        {
            continue;
        }
        let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
            continue;
        };
        validate_signature(method)?;
        if receiver.reference.is_none() || receiver.colon_token.is_some() {
            return Err(Error::new_spanned(
                receiver,
                "fixture methods must use an &self or &mut self receiver",
            ));
        }
        methods.push(invocation_arm(ty, class_path, method));
    }
    let sut_method = if let Some(sut) = sut {
        validate_signature(&sut)?;
        let Some(FnArg::Receiver(receiver)) = sut.sig.inputs.first() else {
            return Err(Error::new_spanned(
                &sut.sig,
                "a `#[slim(sut)]` method must have an &mut self receiver",
            ));
        };
        if receiver.reference.is_none()
            || receiver.mutability.is_none()
            || receiver.colon_token.is_some()
            || sut.sig.inputs.len() != 1
        {
            return Err(Error::new_spanned(
                receiver,
                "a `#[slim(sut)]` method must have an &mut self receiver and no arguments",
            ));
        }
        if !matches!(&sut.sig.output, ReturnType::Type(_, output) if matches!(output.as_ref(), Type::Reference(reference) if reference.mutability.is_some()))
        {
            return Err(Error::new_spanned(
                &sut.sig.output,
                "a `#[slim(sut)]` method must return a mutable fixture reference",
            ));
        }
        let ident = &sut.sig.ident;
        quote! {
            fn execute_system_under_test(&mut self, method: &str, args: ::std::vec::Vec<::rust_slim::SlimValue>)
                -> ::std::result::Result<::rust_slim::SlimValue, ::rust_slim::ExecuteMethodError> {
                ::rust_slim::SlimFixture::execute_method(<#ty>::#ident(self), method, args)
            }
        }
    } else {
        quote! {}
    };
    Ok(quote! {
        fn execute_method(&mut self, method: &str, args: ::std::vec::Vec<::rust_slim::SlimValue>)
            -> ::std::result::Result<::rust_slim::SlimValue, ::rust_slim::ExecuteMethodError> {
            match method {
                #(#methods,)*
                _ => Err(::rust_slim::ExecuteMethodError::MethodNotFound { method: method.to_string(), class: #class_path }),
            }
        }
        #sut_method
    })
}

fn invocation_arm(
    ty: &Type,
    class_path: &proc_macro2::TokenStream,
    method: &ImplItemFn,
) -> proc_macro2::TokenStream {
    let arity = method.sig.inputs.len() - 1;
    let name = method.sig.ident.to_string();
    let ident = &method.sig.ident;
    let arguments = method.sig.inputs.iter().skip(1).map(|argument| {
        let FnArg::Typed(argument) = argument else { unreachable!() };
        let ty = &argument.ty;
        quote! {
            <#ty as ::rust_slim::FromSlimValue>::from_slim_value(supplied.next().expect("argument count checked"))
                .map_err(|error| match error {
                    ::rust_slim::ExecuteMethodError::ArgumentParsingError(_) =>
                        ::rust_slim::ExecuteMethodError::ArgumentParsingError(stringify!(#ty).into()),
                    error => error,
                })?
        }
    });
    let invoke = quote! { <#ty>::#ident(self, #(#arguments),*) };
    let result = if returns_slim_control_result(&method.sig.output) {
        quote! {
            match #invoke {
                Ok(value) => ::rust_slim::IntoSlimValue::into_slim_value(value),
                Err(control) => Err(::rust_slim::ExecuteMethodError::Control(control)),
            }
        }
    } else {
        quote! { ::rust_slim::IntoSlimValue::into_slim_value(#invoke) }
    };
    quote! {
        #name => {
            if args.len() != #arity {
                return Err(::rust_slim::ExecuteMethodError::MethodNotFound { method: method.to_string(), class: #class_path });
            }
            let mut supplied = args.into_iter();
            #result
        }
    }
}

fn impl_constructor(
    ty: &Type,
    impl_generics: &syn::Generics,
    constructors: Vec<ImplItemFn>,
) -> Result<proc_macro2::TokenStream> {
    if constructors.is_empty() {
        let mut default_generics = impl_generics.clone();
        default_generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ty: ::std::default::Default));
        let (impl_generics, _, where_clause) = default_generics.split_for_impl();
        return Ok(quote! {
            impl #impl_generics ::rust_slim::Constructor for #ty #where_clause {
                fn construct(args: ::std::vec::Vec<::rust_slim::SlimValue>)
                    -> ::std::result::Result<Self, ::rust_slim::ConstructorError> {
                    if args.is_empty() { Ok(::std::default::Default::default()) }
                    else { Err(::rust_slim::ConstructorError::NoConstructor) }
                }
            }
        });
    }

    let mut arities = std::collections::BTreeSet::new();
    let mut arms = Vec::new();
    for method in constructors {
        validate_signature(&method)?;
        if method
            .sig
            .inputs
            .first()
            .is_some_and(|argument| matches!(argument, FnArg::Receiver(_)))
        {
            return Err(Error::new_spanned(
                &method.sig,
                "a `#[slim(constructor)]` method must be an associated function",
            ));
        }
        if !constructor_returns_fixture(&method.sig.output, ty) {
            return Err(Error::new_spanned(
                &method.sig.output,
                "a `#[slim(constructor)]` method must return `Self` or `Result<Self, E>`",
            ));
        }
        let arity = method.sig.inputs.len();
        if !arities.insert(arity) {
            return Err(Error::new_spanned(
                &method.sig,
                format!("a fixture may declare only one constructor with {arity} arguments"),
            ));
        }
        arms.push(constructor_arm(ty, &method));
    }
    let (impl_generics, _, where_clause) = impl_generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::rust_slim::Constructor for #ty #where_clause {
            fn construct(args: ::std::vec::Vec<::rust_slim::SlimValue>)
                -> ::std::result::Result<Self, ::rust_slim::ConstructorError> {
                match args.len() {
                    #(#arms,)*
                    _ => Err(::rust_slim::ConstructorError::NoConstructor),
                }
            }
        }
    })
}

fn constructor_arm(ty: &Type, method: &ImplItemFn) -> proc_macro2::TokenStream {
    let ident = &method.sig.ident;
    let arity = method.sig.inputs.len();
    let arguments = method.sig.inputs.iter().map(|argument| {
        let FnArg::Typed(argument) = argument else { unreachable!() };
        let ty = &argument.ty;
        quote! {
            <#ty as ::rust_slim::FromSlimValue>::from_slim_value(supplied.next().expect("argument count checked"))
                .map_err(|error| match error {
                    ::rust_slim::ExecuteMethodError::ArgumentParsingError(_) =>
                        ::rust_slim::ConstructorError::ArgumentParsingError(stringify!(#ty).into()),
                    error => ::rust_slim::ConstructorError::CouldNotInvoke(error.to_string()),
                })?
        }
    });
    let invoke = quote! { <#ty>::#ident(#(#arguments),*) };
    let result = if returns_slim_control_result(&method.sig.output) {
        quote! { #invoke.map_err(::rust_slim::ConstructorError::Control) }
    } else if returns_result(&method.sig.output) {
        quote! { #invoke.map_err(|error| ::rust_slim::ConstructorError::CouldNotInvoke(error.to_string())) }
    } else {
        quote! { Ok(#invoke) }
    };
    quote! {
        #arity => {
            let mut supplied = args.into_iter();
            #result
        }
    }
}

fn returns_result(return_type: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = return_type else {
        return false;
    };
    matches!(ty.as_ref(), Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Result"))
}

fn returns_slim_control_result(return_type: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = return_type else {
        return false;
    };
    let Type::Path(path) = ty.as_ref() else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    if segment.ident != "Result" {
        return false;
    }
    matches!(
        arguments.args.iter().nth(1),
        Some(GenericArgument::Type(Type::Path(error)))
            if error.path.segments.last().is_some_and(|segment| segment.ident == "SlimControlException")
    )
}

fn constructor_returns_fixture(return_type: &ReturnType, fixture_type: &Type) -> bool {
    let ReturnType::Type(_, output) = return_type else {
        return false;
    };
    if is_fixture_type(output, fixture_type) {
        return true;
    }
    let Type::Path(path) = output.as_ref() else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Result" {
        return false;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    matches!(arguments.args.first(), Some(GenericArgument::Type(ok)) if is_fixture_type(ok, fixture_type))
}

fn is_fixture_type(candidate: &Type, fixture_type: &Type) -> bool {
    matches!(candidate, Type::Path(path) if path.qself.is_none() && path.path.is_ident("Self"))
        || quote!(#candidate).to_string() == quote!(#fixture_type).to_string()
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
    let name = ident.to_string();
    Ok(
        quote! { ::rust_slim::from_rust_module_path_to_class_path(&format!("{}::{}", module_path!(), #name)) },
    )
}
