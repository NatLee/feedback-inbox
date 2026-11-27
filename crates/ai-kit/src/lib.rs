//! Bounded structured-output transports. The host owns permissions, secrets at rest,
//! destination allowlists, prompts, budgets, retries and all domain side effects.
mod anthropic;
mod client;
mod ollama;
mod openai;
mod types;
pub use client::AiClient;
pub use types::*;
