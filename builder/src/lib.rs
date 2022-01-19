use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, AngleBracketedGenericArguments, Data, DataStruct, DeriveInput,
    GenericArgument, Ident, Path, PathArguments, Type, TypePath,
};

enum FieldWrapperType<'a> {
    None,
    Option(&'a Type),
    Vec(&'a Type),
}

#[proc_macro_derive(Builder)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let st_ident = input.ident;
    let builder_ident = Ident::new(&format!("{}Builder", st_ident), Span::call_site());

    let st_fields = match input.data {
        Data::Struct(DataStruct { ref fields, .. }) => fields,
        _ => panic!("Builder is only support for structs"),
    }
    .into_iter()
    .map(|f| {
        let segments = match f.ty {
            Type::Path(TypePath {
                qself: None,
                path: Path { ref segments, .. },
            }) => segments,
            _ => unimplemented!(),
        };

        let seg = &segments[0];
        let segi = &seg.ident;
        let ty_name = segi.to_string();
        let wrapper_ty = match &ty_name[..] {
            "Option" | "Vec" => {
                // I really wish we had some type info before macro expansion
                let inner_ty = match seg.arguments {
                    PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        ref args,
                        ..
                    }) => match &args[0] {
                        GenericArgument::Type(ty) => ty,
                        _ => unimplemented!(),
                    },
                    _ => unimplemented!(),
                };

                if &ty_name[..] == "Option" {
                    FieldWrapperType::Option(inner_ty)
                } else {
                    FieldWrapperType::Vec(inner_ty)
                }
            }
            _ => FieldWrapperType::None,
        };
        let opt = segi == &Ident::new("Option", segi.span());

        (f, wrapper_ty)
    })
    .collect::<Vec<_>>();

    let fields = st_fields
        .iter()
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            let fty = &f.ty;

            match wrapper_ty {
                FieldWrapperType::Option(_) => quote! { #fi: #fty},
                _ => quote! { #fi: Option<#fty> },
            }
        })
        .collect::<Vec<_>>();

    let field_defaults = st_fields
        .iter()
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            quote! {
                #fi: None
            }
        })
        .collect::<Vec<_>>();

    let field_copies = st_fields
        .iter()
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            match wrapper_ty {
                FieldWrapperType::Option(_) => quote! { #fi: self.#fi.clone() },
                _ => quote! { #fi: self.#fi.clone().ok_or("No value for field")? },
            }
        })
        .collect::<Vec<_>>();

    let methods = st_fields
        .iter()
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            let fty = &f.ty;

            let inner_ty = match wrapper_ty {
                FieldWrapperType::Option(ty) => ty,
                _ => fty,
            };

            quote! {
                pub fn #fi(&mut self, val: #inner_ty) -> &mut Self {
                    self.#fi = std::option::Option::Some(val);
                    self
                }
            }
        })
        .collect::<Vec<_>>();

    let output = quote! {
        impl #st_ident {
            pub fn builder() -> #builder_ident {
                #builder_ident {
                    #(#field_defaults,)*
                }
            }
        }

        pub struct #builder_ident {
            #(#fields,)*
        }

        impl #builder_ident {
            #(#methods)*

            pub fn build(&self) -> Result<#st_ident, Box<dyn std::error::Error>> {
                Ok(#st_ident {
                    #(#field_copies,)*
                })
            }
        }
    };

    output.into()
}
