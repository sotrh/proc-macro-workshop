use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parser, parse_macro_input, spanned::Spanned, DeriveInput};

#[proc_macro_derive(CustomDebug, attributes(debug))]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let output = compose_debug(input)
        .map_err(|e| e.to_compile_error())
        .unwrap();
    output.into()
}

fn compose_debug(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let struct_span = input.span();
    let struct_ident = input.ident;
    let struct_name = format!("{}", struct_ident);
    let struct_generics = input.generics;

    let fields = match input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(syn::FieldsNamed { named: fields, .. }),
            ..
        }) => fields,
        _ => {
            return Err(syn::Error::new(
                struct_span,
                "CustomDebug is only implemented for structs",
            ))
        }
    };

    let mut generics_with_bounds = struct_generics.clone();
    let mut associated_types = Vec::new();
    generics_with_bounds.params.iter_mut().for_each(|p| {
        match p {
            syn::GenericParam::Type(tp) => {
                let g_ident = &tp.ident;
                let mut skip_bounds = false;

                // This implementation is really bad and I hate it 
                for f in &fields {
                    let ty = &f.ty;
                    match ty {
                        syn::Type::Path(syn::TypePath { path, .. }) => {
                            let first = path.segments.iter().next().unwrap();
                            match &first.arguments {
                                syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                                    args,
                                    ..
                                }) => {
                                    for a in args {
                                        match a {
                                            syn::GenericArgument::Type(syn::Type::Path(tp)) => {
                                                let mut iter = tp.path.segments.iter();
                                                if let Some(first) = iter.next() {
                                                    if &first.ident == g_ident && iter.next().is_some() {
                                                        associated_types.push(tp);
                                                        skip_bounds = true;
                                                    }
                                                }
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                _ => ()
                            }

                        }
                        _ => ()
                    }

                    let expected = quote! {PhantomData<#g_ident>}.to_string();
                    let actual = quote! {#ty}.to_string();
                    // TODO: this is wrong. I'm only considering the case where
                    // T is only used once.
                    if expected == actual {
                        skip_bounds = true;
                        break;
                    }
                }

                if !skip_bounds {
                    let parser = syn::Path::parse_mod_style;
                    tp.bounds.push(syn::TypeParamBound::Trait(syn::TraitBound {
                        paren_token: None,
                        modifier: syn::TraitBoundModifier::None,
                        lifetimes: None,
                        path: parser.parse_str("std::fmt::Debug").unwrap(),
                    }));
                }
            }
            _ => todo!(),
        }
    });

    let mut field_formatters = Vec::with_capacity(fields.len());
    let mut field_names = Vec::with_capacity(fields.len());
    let mut field_errors = Vec::new();

    fields.iter().for_each(|f| {
        let ident = f.ident.clone().unwrap();

        let attrs: Vec<String> = f
            .attrs
            .iter()
            .map(|a| {
                let meta = a.parse_meta()?;
                let span = meta.span();

                match meta {
                    syn::Meta::NameValue(syn::MetaNameValue {
                        path,
                        lit: syn::Lit::Str(lit),
                        ..
                    }) if path_to_string(&path) == "debug" => Ok(lit.value()),
                    _ => Err(syn::Error::new(span, "Unsuported attribute")),
                }
            })
            .filter_map(|a| match a {
                Ok(a) => Some(a),
                Err(e) => {
                    field_errors.push(e);
                    None
                }
            })
            .collect();

        let name = format!("{}", ident);
        let formatter = if let Some(debug) = attrs.first() {
            quote! { &format_args!(#debug, &self.#ident) }
        } else {
            quote! { &self.#ident }
        };

        field_formatters.push(formatter);
        field_names.push(name);
    });

    let sanitized_generics = struct_generics.params.iter().map(|p| {
        match p {
            syn::GenericParam::Type(syn::TypeParam {
                ident,
                ..
            }) => quote! { #ident },
            _ => quote! { #p },
        }
    }).collect::<Vec<_>>();

    let where_clause = if associated_types.len() > 0 {
        quote! {
            where
                #(#associated_types: Debug,)*
        }
    } else {
        quote! {}
    };

    let errors = field_errors
        .into_iter()
        .map(|e| e.to_compile_error())
        .collect::<Vec<_>>();

    let output = quote! {
        impl #generics_with_bounds std::fmt::Debug for #struct_ident <#(#sanitized_generics),*>
            #where_clause
        {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                fmt.debug_struct(#struct_name)
                    #(.field(#field_names, #field_formatters))*
                    .finish()
            }
        }

        #(#errors)*
    };
    Ok(output)
}

fn path_to_string(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}
