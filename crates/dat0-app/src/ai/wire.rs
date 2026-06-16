//! Pure request/response shaping per provider wire. `build_body` is the
//! schema-only payload surface (R17) — it embeds schema NAMES + the prompt,
//! and row data only via `req.sample_rows` (caller-gated).

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::ai::request::AiRequest;

pub trait Wire {
    fn endpoint(&self, base_url: &str) -> String;
    fn auth_headers(&self, key: &str) -> Vec<(&'static str, String)>;
    fn build_body(&self, model: &str, req: &AiRequest) -> Value;
    fn parse_response(&self, body: &Value) -> Result<String>;
}

/// Build the single user-message text: schema block + optional samples + prompt.
///
/// R17 gate: `sample_rows` is the ONLY value-bearing branch — `schema` carries
/// names+types only and `prompt` is the user's NL text. Adding any other source
/// of row/cell data here would breach the schema-only payload guarantee.
fn user_content(req: &AiRequest) -> String {
    let mut c = String::new();
    let schema = req.schema.render();
    if !schema.is_empty() {
        c.push_str("Schema:\n");
        c.push_str(&schema);
        c.push('\n');
    }
    if let Some(sr) = &req.sample_rows {
        c.push_str("Sample rows:\n");
        for row in &sr.rows {
            c.push_str(&row.join("\t"));
            c.push('\n');
        }
        c.push('\n');
    }
    c.push_str(&req.prompt);
    c
}

pub struct AnthropicWire;
pub struct OpenAiCompatWire;

impl Wire for AnthropicWire {
    fn endpoint(&self, base_url: &str) -> String {
        format!("{}/v1/messages", base_url.trim_end_matches('/'))
    }
    fn auth_headers(&self, key: &str) -> Vec<(&'static str, String)> {
        vec![
            ("x-api-key", key.to_string()),
            ("anthropic-version", "2023-06-01".to_string()),
        ]
    }
    fn build_body(&self, model: &str, req: &AiRequest) -> Value {
        let mut body = json!({
            "model": model,
            "max_tokens": req.max_tokens,
            "messages": [{ "role": "user", "content": user_content(req) }],
        });
        if let Some(sys) = &req.system {
            body["system"] = json!(sys);
        }
        body
    }
    fn parse_response(&self, body: &Value) -> Result<String> {
        body["content"]
            .as_array()
            .and_then(|a| a.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .map(|s| s.to_string())
            .context("anthropic: no text block in response")
    }
}

impl Wire for OpenAiCompatWire {
    fn endpoint(&self, base_url: &str) -> String {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    }
    fn auth_headers(&self, key: &str) -> Vec<(&'static str, String)> {
        vec![("authorization", format!("Bearer {key}"))]
    }
    fn build_body(&self, model: &str, req: &AiRequest) -> Value {
        let mut messages = Vec::new();
        if let Some(sys) = &req.system {
            messages.push(json!({ "role": "system", "content": sys }));
        }
        messages.push(json!({ "role": "user", "content": user_content(req) }));
        json!({ "model": model, "max_tokens": req.max_tokens, "messages": messages })
    }
    fn parse_response(&self, body: &Value) -> Result<String> {
        body["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .context("openai-compat: no choices[0].message.content")
    }
}

/// Dispatch a [`Wire`] for a provider.
pub fn for_kind(kind: crate::ai::provider::WireKind) -> Box<dyn Wire + Send + Sync> {
    match kind {
        crate::ai::provider::WireKind::AnthropicMessages => Box::new(AnthropicWire),
        crate::ai::provider::WireKind::OpenAiCompat => Box::new(OpenAiCompatWire),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::request::{AiRequest, ColumnSchema, SampleRows, SchemaContext, TableSchema};

    fn req_with_schema(sample: Option<SampleRows>) -> AiRequest {
        AiRequest {
            model: "m".into(),
            system: Some("emit sql".into()),
            schema: SchemaContext {
                tables: vec![TableSchema {
                    name: "users".into(),
                    columns: vec![ColumnSchema { name: "email".into(), ty: "VARCHAR".into() }],
                }],
            },
            prompt: "top users".into(),
            sample_rows: sample,
            max_tokens: 32,
        }
    }

    // R17: outbound body carries schema NAMES + the prompt, never row data,
    // unless sample_rows is explicitly populated.
    fn assert_schema_only(body: &serde_json::Value) {
        let s = body.to_string();
        assert!(s.contains("users") && s.contains("email") && s.contains("top users"));
        assert!(!s.contains("SECRET_ROW_VALUE"), "row data leaked: {s}");
    }

    #[test]
    fn openai_body_is_schema_only_without_samples() {
        let w = OpenAiCompatWire;
        assert_schema_only(&w.build_body("m", &req_with_schema(None)));
    }

    #[test]
    fn anthropic_body_is_schema_only_without_samples() {
        let w = AnthropicWire;
        assert_schema_only(&w.build_body("m", &req_with_schema(None)));
    }

    #[test]
    fn sample_rows_appear_only_when_gated() {
        let w = OpenAiCompatWire;
        let with = SampleRows { rows: vec![vec!["SECRET_ROW_VALUE".into()]] };
        let body = w.build_body("m", &req_with_schema(Some(with)));
        assert!(body.to_string().contains("SECRET_ROW_VALUE"));
        // ...and absent when None:
        let body_none = w.build_body("m", &req_with_schema(None));
        assert!(!body_none.to_string().contains("SECRET_ROW_VALUE"));
    }

    #[test]
    fn parse_extracts_text() {
        let oa: serde_json::Value = serde_json::json!({
            "choices": [{"message": {"content": "hello"}}]
        });
        assert_eq!(OpenAiCompatWire.parse_response(&oa).unwrap(), "hello");
        let an: serde_json::Value = serde_json::json!({
            "content": [{"type": "text", "text": "hi"}]
        });
        assert_eq!(AnthropicWire.parse_response(&an).unwrap(), "hi");
    }
}
