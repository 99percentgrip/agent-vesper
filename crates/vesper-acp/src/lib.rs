#![forbid(unsafe_code)]
//! ACP protocol-v1 adapter for the minimal Agent Vesper runtime.

mod adapter;
mod compat;
mod mapping;

pub use adapter::{AcpAdapter, AcpAdapterConfig};
pub use compat::{ACP_SDK_VERSION, ACP_WIRE_PROTOCOL, prompt_response_value};
pub use mapping::truthful_initialize_response;
