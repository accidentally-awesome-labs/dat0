use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Anthropic,
    OpenAI,
    OpenRouter,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    AnthropicMessages,
    OpenAiCompat,
}

impl Provider {
    /// Stable id: keychain slot + `AiSettings.provider` value + i18n key tail.
    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAI => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "anthropic" => Some(Provider::Anthropic),
            "openai" => Some(Provider::OpenAI),
            "openrouter" => Some(Provider::OpenRouter),
            "custom" => Some(Provider::Custom),
            _ => None,
        }
    }

    pub fn wire_kind(self) -> WireKind {
        // Exhaustive (no wildcard): a new Provider variant must consciously pick
        // a wire kind — wrong wire = wrong auth headers + endpoint (T6 path).
        match self {
            Provider::Anthropic => WireKind::AnthropicMessages,
            Provider::OpenAI | Provider::OpenRouter | Provider::Custom => WireKind::OpenAiCompat,
        }
    }

    /// `None` for Custom (user supplies the base URL; the SSRF surface).
    pub fn fixed_base_url(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("https://api.anthropic.com"),
            Provider::OpenAI => Some("https://api.openai.com/v1"),
            Provider::OpenRouter => Some("https://openrouter.ai/api/v1"),
            Provider::Custom => None,
        }
    }

    /// Placeholder hint shown in the model field (NOT a default — D5).
    pub fn model_hint(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-opus-4-8 / claude-sonnet-4-6 / claude-haiku-4-5",
            Provider::OpenAI => "gpt-… (your OpenAI model)",
            Provider::OpenRouter => "vendor/model (e.g. a :free model)",
            Provider::Custom => "model name for your endpoint",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_consistent() {
        assert_eq!(Provider::Anthropic.id(), "anthropic");
        assert_eq!(Provider::OpenRouter.id(), "openrouter");
        assert_eq!(Provider::Anthropic.wire_kind(), WireKind::AnthropicMessages);
        assert_eq!(Provider::OpenAI.wire_kind(), WireKind::OpenAiCompat);
        assert_eq!(Provider::OpenRouter.wire_kind(), WireKind::OpenAiCompat);
        assert_eq!(Provider::Custom.wire_kind(), WireKind::OpenAiCompat);
        // Custom has no fixed URL; the other three do, all https.
        assert_eq!(Provider::Custom.fixed_base_url(), None);
        for p in [Provider::Anthropic, Provider::OpenAI, Provider::OpenRouter] {
            assert!(
                p.fixed_base_url().unwrap().starts_with("https://"),
                "{:?}",
                p
            );
        }
        // round-trip id <-> Provider
        assert_eq!(Provider::from_id("custom"), Some(Provider::Custom));
        assert_eq!(Provider::from_id("nope"), None);
    }
}
