//! Cloud LLM backend split into focused modules:
//! - `config`: environment-driven provider/config loading
//! - `client`: HTTP request/response handling
//! - `translate`: model JSON -> DiPECS intent translation

mod client;
mod config;
mod translate;

pub(crate) use client::CloudLlmBackend;

use config::{cloud_llm_enabled, CloudLlmConfig};

#[derive(Debug, Clone)]
pub enum CloudBackendState {
    Disabled,
    Misconfigured(String),
    Ready(CloudLlmBackend),
}

impl CloudBackendState {
    pub fn from_env() -> Self {
        if !cloud_llm_enabled() {
            return Self::Disabled;
        }

        match CloudLlmConfig::from_env() {
            Ok(config) => match CloudLlmBackend::try_new(config) {
                Ok(backend) => Self::Ready(backend),
                Err(error) => Self::Misconfigured(error),
            },
            Err(error) => Self::Misconfigured(error),
        }
    }
}
