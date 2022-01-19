use anyhow::bail;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, AngleBracketedGenericArguments, Attribute, Data, DataStruct,
    DeriveInput, Field, GenericArgument, Meta, MetaList, MetaNameValue,
    NestedMeta, Path, PathArguments, Type, TypePath, Lit, Ident,
};

enum FieldWrapperType<'a> {
    None,
    Option(&'a Type),
    Vec {
        inner_ty: &'a Type,
        each: Option<String>,
    },
}

fn compare_path_with_str(p: &Path, s: &str) -> bool {
    let parts = p
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>();
    parts.join("::") == s
}

fn find_builder_attrs(f: &Field) -> Vec<&Attribute> {
    f.attrs
        .iter()
        .filter(|a| {
            let segments = &a.path.segments;
            match segments.len() {
                1 => {
                    let ai = &segments[0].ident;
                    &ai.to_string() == "builder"
                }
                _ => false,
            }
        })
        .collect::<Vec<_>>()
}

fn each_from_attribute(a: &Attribute) -> Result<String, anyhow::Error> {
    let meta = a.parse_meta()?;

    let nested = match meta {
        Meta::List(MetaList { path, nested, .. })
            if compare_path_with_str(&path, "builder") && nested.len() == 1 =>
        {
            nested
        }
        _ => bail!("Only builder(each = \"...\") is supported"),
    };

    match &nested[0] {
        NestedMeta::Meta(Meta::NameValue(MetaNameValue {
            path: nested_path,
            lit: Lit::Str(lit),
            ..
        })) if compare_path_with_str(&nested_path, "each") => Ok(lit.value()),
        _ => bail!("Only builder(each = \"...\") is supported"),
    }
}

#[proc_macro_derive(Builder, attributes(builder))]
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
        // Process attributes
        let attrs = find_builder_attrs(f);
        let mut each = None;
        for a in attrs {
            each = Some(each_from_attribute(a).unwrap());
        }

        // Process wrapper types
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
                    FieldWrapperType::Vec { inner_ty, each }
                }
            }
            _ => FieldWrapperType::None,
        };

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
            match wrapper_ty {
                FieldWrapperType::Vec { each: Some(_), .. } => quote! {
                    #fi: Some(vec![])
                },
                _ => quote! {
                    #fi: None
                },
            }
        })
        .collect::<Vec<_>>();

    let field_copies = st_fields
        .iter()
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            match wrapper_ty {
                FieldWrapperType::Option(_) => {
                    quote! { #fi: self.#fi.clone() }
                }
                _ => quote! { #fi: self.#fi.clone().ok_or("No value for field")? },
            }
        })
        .collect::<Vec<_>>();

    let methods = st_fields
        .iter()
        .filter(|(f, wrapper_ty)| {
            if let Some(f_name) = &f.ident {
                match wrapper_ty {
                    FieldWrapperType::Vec {
                        each: Some(each), ..
                    } if each == &f_name.to_string() => false,
                    _ => true,
                }
            } else {
                false
            }
        })
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

    let each_methods = st_fields
        .iter()
        .filter(|(_f, wrapper_ty)| match wrapper_ty {
            FieldWrapperType::Vec { each: Some(_), .. } => true,
            _ => false,
        })
        .map(|(f, wrapper_ty)| {
            let fi = f.ident.clone().unwrap();
            match wrapper_ty {
                FieldWrapperType::Vec {
                    inner_ty,
                    each: Some(each),
                } => {
                    let each = Ident::new(each, fi.span());
                    quote! {
                        pub fn #each(&mut self, val: #inner_ty) -> &mut Self {
                            if let ::std::option::Option::Some(v) = &mut self.#fi {
                                v.push(val);
                            } else {
                                self.#fi = ::std::option::Option::Some(vec![val]);
                            }
                            self
                        }
                    }
                }
                _ => unreachable!(),
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
            #(#each_methods)*

            pub fn build(&self) -> Result<#st_ident, Box<dyn std::error::Error>> {
                Ok(#st_ident {
                    #(#field_copies,)*
                })
            }
        }
    };

    output.into()
}
