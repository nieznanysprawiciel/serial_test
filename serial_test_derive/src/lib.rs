//! # serial_test_derive
//! Helper crate for [serial_test](../serial_test/index.html)

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenTree;
use quote::{quote, ToTokens};
use std::ops::Deref;
use syn;

/// Allows for the creation of serialised Rust tests
/// ````
/// #[test]
/// #[serial]
/// fn test_serial_one() {
///   // Do things
/// }
///
/// #[test]
/// #[serial]
/// fn test_serial_another() {
///   // Do things
/// }
/// ````
/// Multiple tests with the [serial](attr.serial.html) attribute are guaranteed to be executed in serial. Ordering
/// of the tests is not guaranteed however. If you want different subsets of tests to be serialised with each
/// other, but not depend on other subsets, you can add an argument to [serial](attr.serial.html), and all calls
/// with identical arguments will be called in serial. e.g.
/// ````
/// #[test]
/// #[serial(something)]
/// fn test_serial_one() {
///   // Do things
/// }
///
/// #[test]
/// #[serial(something)]
/// fn test_serial_another() {
///   // Do things
/// }
///
/// #[test]
/// #[serial(other)]
/// fn test_serial_third() {
///   // Do things
/// }
///
/// #[test]
/// #[serial(other)]
/// fn test_serial_fourth() {
///   // Do things
/// }
/// ````
/// `test_serial_one` and `test_serial_another` will be executed in serial, as will `test_serial_third` and `test_serial_fourth`
/// but neither sequence will be blocked by the other
#[proc_macro_attribute]
pub fn serial(attr: TokenStream, input: TokenStream) -> TokenStream {
    return serial_core(attr.into(), input.into()).into();
}

fn serial_core(
    attr: proc_macro2::TokenStream,
    input: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let attrs = attr.into_iter().collect::<Vec<TokenTree>>();
    let key = match attrs.len() {
        0 => "".to_string(),
        1 => {
            if let TokenTree::Ident(id) = &attrs[0] {
                id.to_string()
            } else {
                panic!("Expected a single name as argument, got {:?}", attrs);
            }
        }
        n => {
            panic!("Expected either 0 or 1 arguments, got {}: {:?}", n, attrs);
        }
    };
    let ast: syn::ItemFn = syn::parse2(input).unwrap();
    let asyncness = ast.sig.asyncness;
    let name = ast.sig.ident;
    let return_type = match ast.sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_rarrow, ref box_type) => Some(box_type.deref()),
    };
    let block = ast.block;
    let mut gen_reactor = true;
    let attrs: Vec<syn::Attribute> = ast
        .attrs
        .into_iter()
        .filter(|at| {
            if let Ok(m) = at.parse_meta() {
                let path = m.path();
                if path.segments.len() == 2 {
                    if path.segments[1].ident.to_string() == "test"
                        && (path.segments[0].ident.to_string() == "tokio"
                            || path.segments[0].ident.to_string() == "actix_rt")
                    {
                        // we will generate reactor code ourselves for async fns
                        gen_reactor = false;
                        println!(
                            "path ts: {:?} [{}]",
                            path.to_token_stream(),
                            path.segments.len()
                        );
                        return true;
                    }
                }

                if path.is_ident("ignore") || path.is_ident("should_panic") {
                    // we skip ignore/should_panic because the test framework already deals with it
                    false
                } else {
                    true
                }
            } else {
                true
            }
        })
        .collect();
    let gen = if let Some(ret) = return_type {
        match asyncness {
            Some(_) => {
                if gen_reactor {
                    quote! {
                        #(#attrs)*
                        #[test]
                        fn #name () -> #ret {
                            tokio::runtime::Runtime::new().unwrap().block_on(
                                serial_test::async_serial_core_with_return(#key, async { #block } )
                            )
                        }
                    }
                } else {
                    quote! {
                        #(#attrs)*
                        async fn #name () -> #ret {
                            serial_test::async_serial_core_with_return(#key, || async { #block }).await;
                        }
                    }
                }
            }
            None => quote! {
                #(#attrs)*
                fn #name () -> #ret {
                    serial_test::serial_core_with_return(#key, || {
                        #block
                    })
                }
            },
        }
    } else {
        match asyncness {
            Some(_) => {
                if gen_reactor {
                    quote! {
                        #(#attrs)*
                        #[test]
                        fn #name () {
                            tokio::runtime::Runtime::new().unwrap().block_on(
                                serial_test::async_serial_core(#key, async { #block } )
                            )
                        }
                    }
                } else {
                    quote! {
                        #(#attrs)*
                        async fn #name () {
                            serial_test::async_serial_core(#key, || async { #block }).await;
                        }
                    }
                }
            }
            None => quote! {
                #(#attrs)
                *
                fn #name () {
                    serial_test::serial_core(#key, || {
                        #block
                    });
                }
            },
        }
    };
    return gen.into();
}

#[test]
fn test_serial() {
    let attrs = proc_macro2::TokenStream::new();
    let input = quote! {
        #[test]
        fn foo() {}
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[test]
        fn foo () {
            serial_test::serial_core("", || {
                {}
            });
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}

#[test]
fn test_stripped_attributes() {
    let _ = env_logger::builder().is_test(true).try_init();
    let attrs = proc_macro2::TokenStream::new();
    let input = quote! {
        #[test]
        #[ignore]
        #[should_panic(expected = "Testing panic")]
        #[something_else]
        fn foo() {}
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[test]
        #[something_else]
        fn foo () {
            serial_test::serial_core("", || {
                {}
            });
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}

#[test]
fn test_serial_async() {
    let attrs = proc_macro2::TokenStream::new();
    let input = quote! {
        #[actix_rt::test]
        async fn foo() {}
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[actix_rt::test]
        async fn foo () {
            serial_test::async_serial_core("", || async {
                {}
            }).await;
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}

#[test]
fn test_serial_async_return() {
    let attrs = proc_macro2::TokenStream::new();
    let input = quote! {
        #[tokio::test]
        async fn foo() -> Result<(), ()> { Ok(()) }
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[tokio::test]
        async fn foo () -> Result<(), ()> {
            serial_test::async_serial_core_with_return("", || async {
                { Ok(()) }
            }).await;
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}

#[test]
fn test_serial_async_return_reactor() {
    use quote::TokenStreamExt;
    let mut attrs = proc_macro2::TokenStream::new();
    attrs.append(syn::parse_str::<proc_macro2::Ident>("key").unwrap());
    let input = quote! {
        async fn foo() -> Result<(), ()> { Ok(()) }
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[test]
        fn foo () -> Result<(), ()> {
            tokio::runtime::Runtime::new().unwrap().block_on (
                serial_test::async_serial_core_with_return("key", async {
                    { Ok(()) }
                })
            )
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}

#[test]
fn test_serial_async_reactor() {
    use quote::TokenStreamExt;
    let mut attrs = proc_macro2::TokenStream::new();
    attrs.append(syn::parse_str::<proc_macro2::Ident>("key").unwrap());
    let input = quote! {
        async fn foo() { () }
    };
    let stream = serial_core(attrs.into(), input);
    let compare = quote! {
        #[test]
        fn foo () {
            tokio::runtime::Runtime::new().unwrap().block_on (
                serial_test::async_serial_core("key", async {
                    { () }
                })
            )
        }
    };
    assert_eq!(format!("{}", compare), format!("{}", stream));
}
