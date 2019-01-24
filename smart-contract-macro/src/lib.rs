#![recursion_limit="128"]

extern crate proc_macro;

use quote::quote;
use syn::File;
use syn::visit::Visit;

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::ItemImpl;

#[derive(Debug)]
struct ContractVisitor {
    struct_ident: Option<syn::Ident>,
    method_idents: Vec<syn::Ident>,
}

impl ContractVisitor {
    fn new() -> Self {
        Self { struct_ident: None, method_idents: vec![] }
    }
}

fn ensure_input_params(func_name: &str, inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>) {
    match func_name {
        "init" => {
            let init_input_error = "The `init` fn must only have 1 parameter: &mut smart_contract::payload::Parameters.";

            if inputs.len() != 1 {
                panic!(init_input_error);
            }

            match &inputs[0] {
                syn::FnArg::Captured(capture) => {
                    match &capture.ty {
                        syn::Type::Reference(tref) => {
                            let elem = &tref.elem;

                            if tref.mutability.is_none() || (quote!(#elem).to_string() != "Parameters" && quote!(#elem).to_string() != "smart_contract :: payload :: Parameters") {
                                panic!(init_input_error);
                            }
                        }
                        _ => panic!(init_input_error)
                    }
                }
                _ => panic!(init_input_error)
            }
        }
        _ => {
            if inputs.len() != 2 {
                panic!("All smart contract functions need to have a single parameter of type &mut smart_contract::payload::Parameters.");
            }

            let first_param_error = "The first parameter of a smart contract function must be &mut self.";

            match &inputs[0] {
                syn::FnArg::SelfRef(self_ref) => {
                    if self_ref.mutability.is_none() {
                        panic!(first_param_error);
                    }
                },
                _ => panic!(first_param_error)
            }

            let second_param_error = "The second parameter of a smart contract function must be &mut smart_contract::payload::Parameters.";

            match &inputs[1] {
                syn::FnArg::Captured(capture) => {
                    match &capture.ty {
                        syn::Type::Reference(tref) => {
                            let elem = &tref.elem;

                            if tref.mutability.is_none() || (quote!(#elem).to_string() != "Parameters" && quote!(#elem).to_string() != "smart_contract :: payload :: Parameters") {
                                panic!(second_param_error);
                            }
                        }
                        _ => panic!(second_param_error)
                    }
                }
                _ => panic!(second_param_error)
            }
        }
    }
}

impl<'ast> Visit<'ast> for ContractVisitor {
    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let struct_ident = &i.self_ty;
        self.struct_ident = Some(syn::Ident::new(&quote!(#struct_ident).to_string(), Span::call_site()));

        for item in &i.items {
            match item {
                syn::ImplItem::Method(method) => {
                    let func = &method.sig;

                    let name = &func.ident;
                    let inputs = &func.decl.inputs;

                    // Check that the first input parameter is &mut self, and the second is &mut smart_contract::payload::Params
                    ensure_input_params(name.to_string().as_str(), &inputs);

                    match name.to_string().as_str() {
                        "init" => {
                            match &func.decl.output {
                                syn::ReturnType::Type(_, typ) => {
                                    if quote!(#typ).to_string() != "( Self , Option < Payload > )" && quote!(#typ).to_string() != "( Self , Option < smart_contract :: payload :: Payload > )" {
                                        panic!("The `init` fn need to return (Self, Option<smart_contract::payload::Payload>).")
                                    }
                                }
                                _ => panic!("The `init` fn need to return (Self, Option<smart_contract::payload::Payload>).")
                            }

                            println!("Registered smart contract init function.");
                        },
                        _ => {
                            match &func.decl.output {
                                syn::ReturnType::Type(_, typ) => {
                                    if quote!(#typ).to_string() != "Option < Payload >" && quote!(#typ).to_string() != "Option < smart_contract :: payload :: Payload >" {
                                        panic!("Smart contract functions need to return Option<smart_contract::payload::Payload>.")
                                    }
                                }
                                _ => panic!("Smart contract functions need to return Option<smart_contract::payload::Payload>.")
                            }

                            println!("Registered smart contract function: {}", name);
                        }
                    }

                    self.method_idents.push(name.clone());
                }
                _ => continue
            }
        }
    }
}

#[proc_macro_attribute]
pub fn smart_contract(_args: TokenStream, input: TokenStream) -> TokenStream {
    let syntax: File = syn::parse2(input.into()).unwrap();

    let mut visitor = ContractVisitor::new();
    visitor.visit_file(&syntax);

    let struct_ident = visitor.struct_ident.unwrap_or_else(|| panic!("You should only tag #[smart_contract] to impl blocks!"));

    let mut tokens: TokenStream = quote! {
        #syntax

        thread_local! {
            static SMART_CONTRACT_INSTANCE: ::std::cell::RefCell<#struct_ident> = {
                let (contract, payload) = #struct_ident::init(&mut Parameters::load());

                if let Some(result) = payload {
                    let bytes = result.serialize();
                    unsafe { ::smart_contract::sys::_provide_result(bytes.as_ptr(), bytes.len()); }
                }

                ::std::cell::RefCell::new(contract)
            }
        }
    }.into();

    for name in visitor.method_idents {
        let raw_name = syn::Ident::new(&format!("_contract_{}", name.to_string()), name.span());

        let raw_func =
            match name.to_string().as_str() {
                "init" => { quote!() },
                _ => {
                    quote! {
                        #[no_mangle]
                        pub extern "C" fn #raw_name() {
                            SMART_CONTRACT_INSTANCE.with(|smart_contract| {
                                if let Some(result) = smart_contract.borrow_mut().#name(&mut Parameters::load()) {
                                    let bytes = result.serialize();
                                    unsafe { ::smart_contract::sys::_provide_result(bytes.as_ptr(), bytes.len()); }
                                }
                            });
                        }
                    }
                }
            }.into();

        tokens.extend::<TokenStream>(raw_func);
    }

    tokens
}