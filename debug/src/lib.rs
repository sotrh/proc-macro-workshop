use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, spanned::Spanned, DeriveInput, Field};

#[proc_macro_derive(CustomDebug, attributes(debug))]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let output = compose_debug(input)
        .map_err(|e| e.to_compile_error())
        .unwrap();
    output.into()
}

fn compose_debug(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let struct_ident = &input.ident;
    let struct_name = format!("{}", struct_ident);
    let fields = match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(syn::FieldsNamed { named: fields, .. }),
            ..
        }) => fields,
        _ => {
            return Err(syn::Error::new(
                input.span(),
                "CustomDebug is only implemented for structs",
            ))
        }
    };

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

                eprintln!("{:?}", meta);

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
            quote!{ &format_args!(#debug, &self.#ident) }
        } else {
            quote!{ &self.#ident }
        };

        field_formatters.push(formatter);
        field_names.push(name);
    });

    let errors = field_errors
        .into_iter()
        .map(|e| e.to_compile_error())
        .collect::<Vec<_>>();

    Ok(quote! {
        impl std::fmt::Debug for #struct_ident {
            fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                fmt.debug_struct(#struct_name)
                    #(.field(#field_names, #field_formatters))*
                    .finish()
            }
        }

        #(#errors)*
    })
}

fn path_to_string(p: &syn::Path) -> String {
    p.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}
