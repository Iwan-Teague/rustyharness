//! Model backends. The harness drives a model; the model is a swappable part.
//!
//! SCAFFOLD (2026-09-23). First backend to build: OpenAI-compatible
//! `/v1/chat/completions` (llama.cpp, Ollama, vLLM, LM Studio), the same seam
//! rustybenchmark's `bench-model` uses. Open design questions: native tool
//! calling vs a text protocol with grammar-constrained decoding for small
//! local models; streaming; whether hosted backends are allowed at all by
//! default (privacy posture says local-first).

#![forbid(unsafe_code)]

use harness_core::Untrusted;

/// Who said a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Harness-authored instructions. Only the harness writes these.
    System,
    /// The task, as given by the person who started the run.
    User,
    /// A prior model turn.
    Assistant,
    /// A tool result. Always untrusted data.
    Tool,
}

/// One message in a conversation sent to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who said it.
    pub role: Role,
    /// What was said.
    pub content: String,
}

/// Why a model call failed.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// The backend could not be reached or returned an error.
    #[error("model backend error: {0}")]
    Backend(String),
    /// The backend answered with something that is not a usable completion.
    /// Never treated as an empty success.
    #[error("unusable completion: {0}")]
    Unusable(String),
}

/// A model backend. Completions come back [`Untrusted`]: model output is data
/// until the harness has checked it.
pub trait ModelBackend {
    /// A stable identity for the model actually serving (for journals and
    /// benchmark row keys).
    fn identity(&self) -> String;

    /// Send one conversation, receive one completion.
    fn complete(&self, messages: &[Message]) -> Result<Untrusted<String>, ModelError>;
}
