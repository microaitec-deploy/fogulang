//! LLM clients, MCP and plots (feature `ai`).
//! Arrays, data frames and ML live in the `datascience` lib, face detection in the
//! `face` lib (libs/).

#[cfg(not(target_family = "wasm"))]
pub mod ai;
#[cfg(not(target_family = "wasm"))]
pub mod ai_mcp;
#[cfg(not(target_family = "wasm"))]
pub mod mcp_client;
pub mod plot;
