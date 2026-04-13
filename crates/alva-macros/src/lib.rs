// INPUT:  proc_macro, proc_macro2, quote, syn, darling
// OUTPUT: #[derive(Tool)] proc macro
// POS:    Code-generation helpers for the Alva plugin layer.

//! Proc macros for the Alva agent framework.
//!
//! Currently exposes `#[derive(Tool)]` — generates an `impl Tool` for
//! a struct by pairing it with an input-parameter struct that derives
//! `schemars::JsonSchema + serde::Deserialize`.
//!
//! # Example
//!
//! ```ignore
//! use alva_types::Tool;
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct ReadFileInput {
//!     /// Absolute path to the file.
//!     path: String,
//!     /// Max lines to read; default 2000.
//!     #[serde(default)]
//!     limit: Option<u32>,
//! }
//!
//! #[derive(Tool)]
//! #[tool(
//!     name = "read_file",
//!     description = "Read a file from disk",
//!     input = ReadFileInput,
//! )]
//! pub struct ReadFileTool;
//!
//! impl ReadFileTool {
//!     async fn execute_impl(
//!         &self,
//!         input: ReadFileInput,
//!         ctx: &dyn alva_types::tool::execution::ToolExecutionContext,
//!     ) -> Result<alva_types::tool::execution::ToolOutput, alva_types::base::error::AgentError>
//!     {
//!         // ... user logic, already has parsed input
//!     }
//! }
//! ```
//!
//! The macro generates `impl Tool for ReadFileTool` whose `execute`
//! parses JSON into `ReadFileInput` and delegates to `execute_impl`,
//! and whose `parameters_schema` pipes `schema_for!(ReadFileInput)`
//! through `normalize_llm_tool_schema` (and optionally
//! `self.apply_schema_overrides` if the user implements it on the
//! concrete type — via Rust's inherent-method-wins-over-trait-method
//! lookup rules the macro can unconditionally call it).
//!
//! # Extensibility
//!
//! New attributes or new derives should extend this crate's single
//! `lib.rs`. Non-breaking additions (new attribute keys with sensible
//! defaults) are the preferred evolution path.

use darling::FromDeriveInput;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Parsed `#[tool(...)]` attribute arguments.
///
/// Using `darling` for parsing so invalid attributes produce nice
/// compile errors pointing at the offending span.
///
/// Every override flag below is emitted as a trait-method implementation
/// (not an inherent method) because middleware dispatches through the
/// vtable — inherent methods wouldn't be visible. Tools whose metadata
/// depends on the actual input value (rather than being constant) should
/// opt out of `#[derive(Tool)]` and hand-write their `impl Tool`.
#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tool), supports(struct_any))]
struct ToolAttrs {
    ident: syn::Ident,
    generics: syn::Generics,
    /// Tool name as seen by the LLM (must match what's used in
    /// `ToolCall.name`).
    name: String,
    /// Human-readable description for the LLM.
    description: String,
    /// Type that the tool's JSON input deserializes into. Must derive
    /// `schemars::JsonSchema + serde::Deserialize`.
    input: syn::Path,
    /// Override `Tool::manages_own_timeout` to return `true` — the tool
    /// bounds its own runtime, so generic per-tool timeout middleware
    /// should skip it. Default: `false`.
    #[darling(default)]
    manages_own_timeout: bool,
    /// Override `Tool::is_read_only` to always return `true`, regardless
    /// of input. Use for tools whose effect is purely informational
    /// (reads, searches, queries). Default: `false`.
    #[darling(default)]
    read_only: bool,
    /// Override `Tool::is_destructive` to always return `true`, regardless
    /// of input. Use sparingly — if *some* invocations are destructive
    /// but others aren't, hand-write the impl. Default: `false`.
    #[darling(default)]
    destructive: bool,
    /// Override `Tool::is_concurrency_safe` to always return `true`.
    /// Use for stateless / side-effect-free tools that can run in
    /// parallel with others. Default: `false`.
    #[darling(default)]
    concurrency_safe: bool,
}

#[proc_macro_derive(Tool, attributes(tool))]
pub fn derive_tool(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);

    let attrs = match ToolAttrs::from_derive_input(&derive_input) {
        Ok(a) => a,
        Err(e) => return e.write_errors().into(),
    };

    let struct_ident = &attrs.ident;
    let (impl_generics, ty_generics, where_clause) = attrs.generics.split_for_impl();
    let name_lit = &attrs.name;
    let description_lit = &attrs.description;
    let input_ty = &attrs.input;

    // Conditionally emit the boolean metadata overrides. Omitting any
    // of these when false lets the trait's default (`false`) apply
    // naturally — no redundant reassertion.
    let manages_own_timeout_impl = if attrs.manages_own_timeout {
        quote! {
            fn manages_own_timeout(&self) -> bool { true }
        }
    } else {
        quote! {}
    };
    let read_only_impl = if attrs.read_only {
        quote! {
            fn is_read_only(&self, _input: &::serde_json::Value) -> bool { true }
        }
    } else {
        quote! {}
    };
    let destructive_impl = if attrs.destructive {
        quote! {
            fn is_destructive(&self, _input: &::serde_json::Value) -> bool { true }
        }
    } else {
        quote! {}
    };
    let concurrency_safe_impl = if attrs.concurrency_safe {
        quote! {
            fn is_concurrency_safe(&self, _input: &::serde_json::Value) -> bool { true }
        }
    } else {
        quote! {}
    };

    // All references to third-party items are fully qualified so the
    // macro works even when the consumer crate hasn't imported them
    // into the local namespace.
    let expanded = quote! {
        #[::async_trait::async_trait]
        impl #impl_generics ::alva_types::tool::Tool for #struct_ident #ty_generics #where_clause {
            fn name(&self) -> &str {
                #name_lit
            }

            fn description(&self) -> &str {
                #description_lit
            }

            #manages_own_timeout_impl
            #read_only_impl
            #destructive_impl
            #concurrency_safe_impl

            fn parameters_schema(&self) -> ::serde_json::Value {
                let mut schema = ::serde_json::to_value(
                    ::schemars::schema_for!(#input_ty)
                ).expect("schemars::schema_for always produces valid JSON");

                ::alva_types::tool::schema::normalize_llm_tool_schema(&mut schema);

                // Plain method call — Rust's method resolution prefers
                // an inherent method with the same name if the type
                // defines one, otherwise falls through to the trait's
                // default (a no-op). This lets a tool plug in runtime
                // schema mutations (e.g. injecting a dynamic enum)
                // just by writing its own inherent method with the
                // same signature.
                self.apply_schema_overrides(&mut schema);

                schema
            }

            async fn execute(
                &self,
                input: ::serde_json::Value,
                ctx: &dyn ::alva_types::tool::execution::ToolExecutionContext,
            ) -> ::std::result::Result<
                ::alva_types::tool::execution::ToolOutput,
                ::alva_types::base::error::AgentError,
            > {
                let parsed: #input_ty = ::serde_json::from_value(input).map_err(|e| {
                    ::alva_types::base::error::AgentError::ToolError {
                        tool_name: #name_lit.to_string(),
                        message: format!("invalid input: {}", e),
                    }
                })?;
                self.execute_impl(parsed, ctx).await
            }
        }
    };

    expanded.into()
}
