//! Codex CLI subprocess driver.
//!
//! Spawns `codex exec` as a subprocess to leverage Codex CLI's built-in ChatGPT
//! OAuth authentication. This avoids the need for a direct OpenAI API key — the
//! CLI handles auth internally.

use crate::llm_driver::{CompletionRequest, CompletionResponse, LlmDriver, LlmError};
use async_trait::async_trait;
use openfang_types::message::{ContentBlock, MessageContent, Role, StopReason, TokenUsage};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

/// Driver that delegates to the `codex` CLI binary (Codex CLI).
///
/// Auth is handled by the CLI itself (ChatGPT OAuth). No API key needed.
pub struct CodexCliDriver;

#[async_trait]
impl LlmDriver for CodexCliDriver {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let prompt = serialize_prompt(&request);
        let requested_model = selected_model_arg(&request.model);
        let mut output = run_codex_exec(&prompt, requested_model.as_deref()).await?;
        let mut failure_message = extract_codex_failure_message(&output.stdout, &output.stderr);

        // Guardrail: if codex rejects the request format and we passed a model,
        // retry once without --model because OAuth accounts often only support
        // the CLI default model.
        if !output.status.success()
            && requested_model.is_some()
            && is_request_format_error(&failure_message)
        {
            warn!(
                model = requested_model.as_deref().unwrap_or_default(),
                error = %failure_message,
                "codex request format error with explicit model; retrying once without --model"
            );
            output = run_codex_exec(&prompt, None).await?;
            failure_message = extract_codex_failure_message(&output.stdout, &output.stderr);
        }

        if !output.status.success() {
            warn!(
                exit_code = ?output.status.code(),
                error = %failure_message,
                "codex CLI failed"
            );
            return Err(LlmError::Api {
                status: output.status.code().unwrap_or(1) as u16,
                message: format!("codex CLI exited with error: {failure_message}"),
            });
        }

        debug!(stdout_len = output.stdout.len(), "codex CLI returned");
        parse_codex_jsonl(&output.stdout)
    }
}

struct CodexExecOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

async fn run_codex_exec(prompt: &str, model: Option<&str>) -> Result<CodexExecOutput, LlmError> {
    let mut args = vec!["exec".to_string(), "--json".to_string()];
    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    // Read prompt from stdin to avoid argv length/escaping edge cases.
    args.push("-".to_string());

    debug!(
        args_count = args.len(),
        has_model = model.is_some(),
        prompt_len = prompt.len(),
        "Spawning codex CLI subprocess"
    );

    let mut child = tokio::process::Command::new("codex")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| LlmError::Http(format!("Failed to spawn codex CLI: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| LlmError::Http(format!("Failed to write prompt to codex stdin: {e}")))?;
    }

    // Wait with timeout — kill child on timeout to avoid orphaned processes
    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    {
        Ok(result) => {
            result.map_err(|e| LlmError::Http(format!("codex CLI process error: {e}")))?
        }
        Err(_) => {
            warn!("codex CLI subprocess timed out after 120s, killing child");
            return Err(LlmError::Http(
                "codex CLI subprocess timed out after 120s".to_string(),
            ));
        }
    };

    Ok(CodexExecOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

/// Parse JSONL output from `codex exec --json`.
///
/// The output is one JSON object per line. We look for:
/// - `item.completed` events where `item.type == "agent_message"` → extract text
/// - `turn.completed` event → extract `usage.input_tokens` + `usage.output_tokens`
fn parse_codex_jsonl(raw: &str) -> Result<CompletionResponse, LlmError> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let json: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // Skip non-JSON lines
        };

        let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            // Codex error events — surface these as LLM errors
            "error" | "turn.failed" => {
                let msg = json
                    .get("message")
                    .or_else(|| json.pointer("/error/message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown codex error");
                return Err(LlmError::Api {
                    status: 0,
                    message: format!("codex CLI error: {msg}"),
                });
            }
            "item.completed" => {
                let item_type = json
                    .pointer("/item/type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if item_type == "agent_message" {
                    // Codex uses flat "text" field on agent_message items
                    if let Some(text) = json.pointer("/item/text").and_then(|v| v.as_str()) {
                        text_parts.push(text.to_string());
                    }
                    // Also check content array (forward compat)
                    if let Some(content) = json.pointer("/item/content").and_then(|v| v.as_array())
                    {
                        for block in content {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                    }
                }
            }
            "turn.completed" => {
                input_tokens = json
                    .pointer("/usage/input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                output_tokens = json
                    .pointer("/usage/output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
            _ => {} // Ignore other event types
        }
    }

    // If no structured events found, treat the entire output as plain text
    if text_parts.is_empty() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            // Try to parse as a single JSON response (fallback — not expected from codex --json)
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                warn!("codex CLI returned non-JSONL response; using single-JSON fallback");
                if let Some(text) = json.get("result").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                } else if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
        }
    }

    let result_text = text_parts.join("");

    if result_text.is_empty() {
        return Err(LlmError::Parse(
            "No agent_message content found in codex CLI output".to_string(),
        ));
    }

    Ok(CompletionResponse {
        content: vec![ContentBlock::Text { text: result_text }],
        stop_reason: StopReason::EndTurn,
        tool_calls: vec![],
        usage: TokenUsage {
            input_tokens,
            output_tokens,
        },
    })
}

/// Serialize a `CompletionRequest` into a single prompt string for codex exec.
///
/// Codex CLI doesn't have a `--system-prompt` flag, so system prompts are
/// prepended to the user prompt.
fn serialize_prompt(request: &CompletionRequest) -> String {
    let mut parts = Vec::new();

    // Codex doesn't support --system-prompt, so prepend it
    if let Some(ref system) = request.system {
        parts.push(format!("[System]\n{system}"));
    }

    for msg in &request.messages {
        let role_label = match msg.role {
            Role::System => "System",
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        let text = extract_text(&msg.content);
        if !text.is_empty() {
            parts.push(format!("{role_label}: {text}"));
        }
    }

    parts.join("\n\n")
}

fn selected_model_arg(model: &str) -> Option<String> {
    let normalized = model.trim();
    if normalized.is_empty()
        || normalized == "default"
        || normalized == "gpt-4o"
        || normalized == "gpt-4o-mini"
    {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn is_request_format_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("invalid request format")
        || lower.contains("invalid request")
        || lower.contains("malformed")
        || lower.contains("missing field")
        || lower.contains("validation error")
        || lower.contains("schema")
}

fn extract_codex_failure_message(stdout: &str, stderr: &str) -> String {
    let stderr_msg = filtered_stderr(stderr);
    if stderr_msg != "unknown codex CLI error" {
        return stderr_msg;
    }

    // codex --json frequently reports failures as JSONL events on stdout.
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(event_type, "error" | "turn.failed") {
            if let Some(msg) = json
                .get("message")
                .or_else(|| json.pointer("/error/message"))
                .and_then(|v| v.as_str())
            {
                let trimmed = msg.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }

    // Single-JSON fallback shape.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if let Some(msg) = json
            .get("message")
            .or_else(|| json.pointer("/error/message"))
            .or_else(|| json.get("result"))
            .and_then(|v| v.as_str())
        {
            let trimmed = msg.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    stderr_msg
}

fn filtered_stderr(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("WARNING: proceeding"))
        .collect();

    if lines.is_empty() {
        "unknown codex CLI error".to_string()
    } else {
        lines.join(" | ")
    }
}

/// Extract plain text from a `MessageContent` (delegates to shared method).
fn extract_text(content: &MessageContent) -> String {
    content.text_with_tool_results()
}

/// Check if the `codex` binary is available on PATH.
pub fn is_available() -> bool {
    super::binary_on_path("codex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_codex_jsonl_agent_message_flat_text() {
        // Real codex output format: flat "text" field on agent_message
        let jsonl = r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"Hello from codex!"}}
{"type":"turn.completed","usage":{"input_tokens":15,"output_tokens":8}}"#;

        let resp = parse_codex_jsonl(jsonl).unwrap();
        assert_eq!(resp.text(), "Hello from codex!");
        assert_eq!(resp.usage.input_tokens, 15);
        assert_eq!(resp.usage.output_tokens, 8);
        assert!(resp.tool_calls.is_empty());
    }

    #[test]
    fn test_parse_codex_jsonl_content_array() {
        // Forward compat: content array format
        let jsonl = r#"{"type":"item.completed","item":{"type":"agent_message","content":[{"text":"Hello from codex!"}]}}
{"type":"turn.completed","usage":{"input_tokens":15,"output_tokens":8}}"#;

        let resp = parse_codex_jsonl(jsonl).unwrap();
        assert_eq!(resp.text(), "Hello from codex!");
    }

    #[test]
    fn test_parse_codex_jsonl_multiple_messages() {
        let jsonl = r#"{"type":"item.completed","item":{"type":"agent_message","text":"Part 1"}}
{"type":"item.completed","item":{"type":"agent_message","text":" Part 2"}}
{"type":"turn.completed","usage":{"input_tokens":20,"output_tokens":12}}"#;

        let resp = parse_codex_jsonl(jsonl).unwrap();
        assert_eq!(resp.text(), "Part 1 Part 2");
        assert_eq!(resp.usage.input_tokens, 20);
    }

    #[test]
    fn test_parse_codex_jsonl_no_content() {
        let jsonl = r#"{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":0}}"#;
        let result = parse_codex_jsonl(jsonl);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_codex_jsonl_error_event() {
        let jsonl = r#"{"type":"error","message":"model not supported"}
{"type":"turn.failed","error":{"message":"model not supported"}}"#;
        let result = parse_codex_jsonl(jsonl);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("model not supported"));
    }

    #[test]
    fn test_parse_codex_jsonl_ignores_non_agent_items() {
        let jsonl = r#"{"type":"item.completed","item":{"type":"reasoning","text":"thinking..."}}
{"type":"item.completed","item":{"type":"agent_message","text":"Actual response"}}
{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#;

        let resp = parse_codex_jsonl(jsonl).unwrap();
        assert_eq!(resp.text(), "Actual response");
    }

    #[test]
    fn test_parse_codex_jsonl_with_blank_lines() {
        let jsonl = r#"
{"type":"item.completed","item":{"type":"agent_message","text":"works"}}

{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}
"#;

        let resp = parse_codex_jsonl(jsonl).unwrap();
        assert_eq!(resp.text(), "works");
    }

    #[test]
    fn test_serialize_prompt_with_system() {
        use openfang_types::message::Message;

        let request = CompletionRequest {
            model: "gpt-4o".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Text("Hello".to_string()),
            }],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.0,
            system: Some("You are helpful.".to_string()),
            thinking: None,
        };

        let prompt = serialize_prompt(&request);
        assert!(prompt.contains("[System]\nYou are helpful."));
        assert!(prompt.contains("User: Hello"));
    }

    #[test]
    fn test_serialize_prompt_no_system() {
        use openfang_types::message::Message;

        let request = CompletionRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Text("Hello!".to_string()),
                },
            ],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.0,
            system: None,
            thinking: None,
        };

        let prompt = serialize_prompt(&request);
        assert!(!prompt.contains("[System]"));
        assert!(prompt.contains("User: Hi"));
        assert!(prompt.contains("Assistant: Hello!"));
    }

    #[test]
    fn test_selected_model_arg_skips_defaultish_models() {
        assert_eq!(selected_model_arg(""), None);
        assert_eq!(selected_model_arg("default"), None);
        assert_eq!(selected_model_arg("gpt-4o"), None);
        assert_eq!(selected_model_arg("gpt-4o-mini"), None);
        assert_eq!(selected_model_arg(" o3 "), Some("o3".to_string()));
    }

    #[test]
    fn test_is_request_format_error_patterns() {
        assert!(is_request_format_error(
            "Invalid request format. This may be a bug."
        ));
        assert!(is_request_format_error("missing field `id_token`"));
        assert!(is_request_format_error("Validation error: expected object"));
        assert!(!is_request_format_error("rate limit exceeded"));
    }

    #[test]
    fn test_filtered_stderr_removes_codex_warning_noise() {
        let stderr =
            "WARNING: proceeding, even though we could not update PATH\nmissing field id_token";
        assert_eq!(filtered_stderr(stderr), "missing field id_token");
    }

    #[test]
    fn test_extract_codex_failure_message_from_stdout_jsonl() {
        let stdout = r#"{"type":"error","message":"Invalid request format. This may be a bug."}"#;
        let msg = extract_codex_failure_message(stdout, "");
        assert_eq!(msg, "Invalid request format. This may be a bug.");
    }

    #[test]
    fn test_extract_codex_failure_message_prefers_stderr_when_present() {
        let stdout = r#"{"type":"error","message":"stdout error"}"#;
        let stderr = "validation error: bad schema";
        let msg = extract_codex_failure_message(stdout, stderr);
        assert_eq!(msg, "validation error: bad schema");
    }
}
