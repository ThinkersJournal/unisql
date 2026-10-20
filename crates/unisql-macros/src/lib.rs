// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sql!`: check a literal SQL statement and translate it for each named target at build time.
//!
//! ```ignore
//! const Q: unisql_embed::Query = unisql_macros::sql!(
//!     source = postgres,
//!     targets = [sqlite, mysql],
//!     allow = [text_collation],
//!     requires = ["upsert"],
//!     forbids = ["fn:md5"],
//!     "SELECT a FROM t WHERE name < $1"
//! );
//! ```
//!
//! The expansion is a `const` expression of type `unisql_embed::Query`: the translated text, parameter
//! order and bindings of every target, as `&'static` data. See `docs/MACROS.md`.
//!
//! * `source` and `targets` are required; there is no default set.
//! * `allow` takes `Allowance` names; a construct that is only conditionally portable builds only when
//!   its allowance is named here.
//! * `requires` / `forbids` assert construct ids (the ids a `Report` uses).
//! * `crate = path` names the path of `unisql_embed` in the expansion (default `::unisql_embed`), so a
//!   crate that re-exports it can point the macro at its own re-export.
//!
//! The statement is the last argument, a string literal.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, bracketed};
use unisql_macro_core::{Input, Kind, expand};

/// Check and translate a statement for the named targets. See the crate documentation.
#[proc_macro]
pub fn sql(input: TokenStream) -> TokenStream {
    let parsed = match syn::parse::<Call>(input) {
        Ok(c) => c,
        Err(e) => return e.to_compile_error().into(),
    };
    run(parsed).into()
}

struct Spanned<T> {
    value: T,
    span: Span,
}

#[derive(Default)]
struct Call {
    source: Option<Spanned<String>>,
    targets: Option<Spanned<Vec<String>>>,
    allow: Option<Spanned<Vec<String>>>,
    requires: Option<Spanned<Vec<String>>>,
    forbids: Option<Spanned<Vec<String>>>,
    embed: Option<Path>,
    sql: Option<Spanned<String>>,
}

fn idents(input: ParseStream) -> syn::Result<Spanned<Vec<String>>> {
    let content;
    let bracket = bracketed!(content in input);
    let list = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?;
    Ok(Spanned {
        value: list.iter().map(|i| i.to_string()).collect(),
        span: bracket.span.join(),
    })
}

fn strings(input: ParseStream) -> syn::Result<Spanned<Vec<String>>> {
    let content;
    let bracket = bracketed!(content in input);
    let list = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
    Ok(Spanned {
        value: list.iter().map(|s| s.value()).collect(),
        span: bracket.span.join(),
    })
}

impl Parse for Call {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut call = Call::default();
        while !input.is_empty() {
            if input.peek(LitStr) {
                let lit: LitStr = input.parse()?;
                if call.sql.is_some() {
                    return Err(syn::Error::new(lit.span(), "the statement is given twice"));
                }
                call.sql = Some(Spanned {
                    value: lit.value(),
                    span: lit.span(),
                });
                if !input.is_empty() {
                    return Err(input.error("the statement must be the last argument"));
                }
                break;
            }
            let key: syn::Ident = if input.peek(Token![crate]) {
                input.parse::<Token![crate]>()?;
                Ident::new("crate", Span::call_site())
            } else {
                input.parse()?
            };
            input.parse::<Token![=]>()?;
            let dup = |name: &str| syn::Error::new(key.span(), format!("`{name}` is given twice"));
            match key.to_string().as_str() {
                "source" => {
                    let v: Ident = input.parse()?;
                    if call.source.is_some() {
                        return Err(dup("source"));
                    }
                    call.source = Some(Spanned {
                        value: v.to_string(),
                        span: v.span(),
                    });
                }
                "targets" => {
                    if call.targets.is_some() {
                        return Err(dup("targets"));
                    }
                    call.targets = Some(idents(input)?);
                }
                "allow" => {
                    if call.allow.is_some() {
                        return Err(dup("allow"));
                    }
                    call.allow = Some(idents(input)?);
                }
                "requires" => {
                    if call.requires.is_some() {
                        return Err(dup("requires"));
                    }
                    call.requires = Some(strings(input)?);
                }
                "forbids" => {
                    if call.forbids.is_some() {
                        return Err(dup("forbids"));
                    }
                    call.forbids = Some(strings(input)?);
                }
                "crate" => {
                    if call.embed.is_some() {
                        return Err(dup("crate"));
                    }
                    call.embed = Some(input.parse()?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown argument `{other}` (known: source, targets, allow, requires, forbids, crate)"
                        ),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(call)
    }
}

fn error(span: Span, message: &str) -> TokenStream2 {
    quote_spanned!(span => ::core::compile_error!(#message))
}

fn variant(dialect: &str) -> Ident {
    let name = match dialect {
        "postgres" => "Postgres",
        "sqlite" => "Sqlite",
        "mysql" => "Mysql",
        _ => "Mariadb",
    };
    Ident::new(name, Span::call_site())
}

fn binding(b: unisql_macro_core::ParamBinding) -> &'static str {
    match b {
        unisql_macro_core::ParamBinding::Scalar => "Scalar",
        unisql_macro_core::ParamBinding::NativeArray => "NativeArray",
        unisql_macro_core::ParamBinding::JsonArrayText => "JsonArrayText",
    }
}

fn run(call: Call) -> TokenStream2 {
    let here = Span::call_site();
    let Some(sql) = call.sql else {
        return error(
            here,
            "missing the statement: the last argument is a string literal",
        );
    };
    let Some(source) = call.source else {
        return error(sql.span, "missing `source = <dialect>`");
    };
    let Some(targets) = call.targets else {
        return error(
            sql.span,
            "missing `targets = [..]`: a call translates for exactly the targets it lists",
        );
    };
    let input = Input {
        source: source.value,
        targets: targets.value,
        allow: call
            .allow
            .as_ref()
            .map(|a| a.value.clone())
            .unwrap_or_default(),
        requires: call
            .requires
            .as_ref()
            .map(|a| a.value.clone())
            .unwrap_or_default(),
        forbids: call
            .forbids
            .as_ref()
            .map(|a| a.value.clone())
            .unwrap_or_default(),
        sql: sql.value,
    };
    let e = match expand(&input) {
        Ok(e) => e,
        Err(problems) => {
            return problems
                .iter()
                .map(|p| {
                    let span = match p.kind {
                        Kind::Source => source.span,
                        Kind::Targets => targets.span,
                        Kind::Allow => call.allow.as_ref().map_or(sql.span, |a| a.span),
                        Kind::Requires => call.requires.as_ref().map_or(sql.span, |a| a.span),
                        Kind::Forbids => call.forbids.as_ref().map_or(sql.span, |a| a.span),
                        Kind::Syntax | Kind::Placeholders | Kind::Refused => sql.span,
                    };
                    error(span, &p.message)
                })
                .collect();
        }
    };
    let embed = call
        .embed
        .map_or_else(|| quote!(::unisql_embed), |p| quote!(#p));
    let params = e.params;
    let constructs = &e.constructs;
    let rendered = e.rendered.iter().map(|r| {
        let target = variant(&r.target);
        let text = &r.sql;
        let order = &r.param_order;
        let bindings = r
            .param_bindings
            .iter()
            .map(|b| Ident::new(binding(*b), here));
        quote! {
            #embed::Rendered {
                target: #embed::Target::#target,
                sql: #text,
                param_order: &[#(#order),*],
                param_bindings: &[#(#embed::Binding::#bindings),*],
            }
        }
    });
    quote! {
        #embed::Query {
            params: #params,
            constructs: &[#(#constructs),*],
            rendered: &[#(#rendered),*],
        }
    }
}
