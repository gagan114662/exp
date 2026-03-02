//! Claude Code CLI subprocess driver.
//!
//! Spawns `claude -p` as a subprocess to leverage Claude Code's built-in OAuth
//! authentication. This avoids the need for a direct Anthropic API key — the CLI
//! handles auth internally.

use crate::llm_driver::{CompletionRequest, CompletionResponse, LlmDriver, LlmError};
use async_trait::async_trait;
use openfang_types::message::{ContentBlock, MessageContent, Role, StopReason, TokenUsage};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

/// Driver that delegates to the `claude` CLI binary (Claude Code).
///
/// Auth is handled by the CLI itself (OAuth session). No API key needed.
pub struct ClaudeCodeDriver;

#[async_trait]
impl LlmDriver for ClaudeCodeDriver {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let prompt = serialize_messages(&request);

        let mut args = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];

        if !request.model.is_empty() {
            args.push("--model".to_string());
            args.push(request.model.clone());
        }

        if let Some(ref system) = request.system {
            args.push("--system-prompt".to_string());
            args.push(system.clone());
        }

        let requested_model = if request.model.trim().is_empty() {
            None
        } else {
            Some(request.model.trim().to_string())
        };

        debug!(
            args = ?args,
            prompt_len = prompt.len(),
            "Spawning claude CLI subprocess"
        );

        let mut child = tokio::process::Command::new("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| LlmError::Http(format!("Failed to spawn claude CLI: {e}")))?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| LlmError::Http(format!("Failed to write to claude stdin: {e}")))?;
            // Drop stdin to signal EOF
        }

        // Wait with timeout — kill child on timeout to avoid orphaned processes
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            child.wait_with_output(),
        )
        .await
        {
            Ok(result) => {
                result.map_err(|e| LlmError::Http(format!("claude CLI process error: {e}")))?
            }
            Err(_) => {
                warn!("claude CLI subprocess timed out after 120s, killing child");
                return Err(LlmError::Http(
                    "claude CLI subprocess timed out after 120s".to_string(),
                ));
            }
        };

        let mut failure_message = extract_claude_failure_message(
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        );

        // Guardrail: if claude rejects request format and we passed a model,
        // retry once without --model because aliases/versions may drift.
        if !output.status.success()
            && requested_model.is_some()
            && is_request_format_error(&failure_message)
        {
            warn!(
                model = requested_model.as_deref().unwrap_or_default(),
                error = %failure_message,
                "claude request format error with explicit model; retrying once without --model"
            );

            let mut retry_args = vec![
                "-p".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
            ];
            if let Some(ref system) = request.system {
                retry_args.push("--system-prompt".to_string());
                retry_args.push(system.clone());
            }

            let mut retry_child = tokio::process::Command::new("claude")
                .args(&retry_args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| LlmError::Http(format!("Failed to spawn claude CLI: {e}")))?;

            if let Some(mut stdin) = retry_child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .await
                    .map_err(|e| LlmError::Http(format!("Failed to write to claude stdin: {e}")))?;
            }

            let retry_output = match tokio::time::timeout(
                std::time::Duration::from_secs(120),
                retry_child.wait_with_output(),
            )
            .await
            {
                Ok(result) => {
                    result.map_err(|e| LlmError::Http(format!("claude CLI process error: {e}")))?
                }
                Err(_) => {
                    return Err(LlmError::Http(
                        "claude CLI subprocess timed out after 120s".to_string(),
                    ));
                }
            };

            failure_message = extract_claude_failure_message(
                &String::from_utf8_lossy(&retry_output.stdout),
                &String::from_utf8_lossy(&retry_output.stderr),
            );

            if !retry_output.status.success() {
                warn!(
                    exit_code = ?retry_output.status.code(),
                    error = %failure_message,
                    "claude CLI failed"
                );
                return Err(LlmError::Api {
                    status: retry_output.status.code().unwrap_or(1) as u16,
                    message: format!("claude CLI exited with error: {failure_message}"),
                });
            }

            let stdout = String::from_utf8_lossy(&retry_output.stdout);
            debug!(stdout_len = stdout.len(), "claude CLI returned");
            return parse_claude_json(&stdout);
        }

        if !output.status.success() {
            warn!(
                exit_code = ?output.status.code(),
                error = %failure_message,
                "claude CLI failed"
            );
            return Err(LlmError::Api {
                status: output.status.code().unwrap_or(1) as u16,
                message: format!("claude CLI exited with error: {failure_message}"),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!(stdout_len = stdout.len(), "claude CLI returned");

        parse_claude_json(&stdout)
    }
}

/// Parse the JSON output from `claude -p --output-format json`.
///
/// Expected shape:
/// ```json
/// {
///   "result": "the response text",
///   "usage": { "input_tokens": N, "output_tokens": N },
///   "total_cost_usd": 0.00X
/// }
/// ```
fn parse_claude_json(raw: &str) -> Result<CompletionResponse, LlmError> {
    let json: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| LlmError::Parse(format!("Invalid JSON from claude CLI: {e}")))?;

    // Check for error response
    if json
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let msg = json
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown claude CLI error");
        return Err(LlmError::Api {
            status: 0,
            message: format!("claude CLI error: {msg}"),
        });
    }

    let result_text = json
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let input_tokens = json
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = json
        .pointer("/usage/output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

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

fn is_request_format_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("invalid request format")
        || lower.contains("invalid request")
        || lower.contains("malformed")
        || lower.contains("missing field")
        || lower.contains("validation error")
        || lower.contains("schema")
}

fn filtered_stderr(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("WARNING:"))
        .collect();

    if lines.is_empty() {
        "unknown claude CLI error".to_string()
    } else {
        lines.join(" | ")
    }
}

fn extract_claude_failure_message(stdout: &str, stderr: &str) -> String {
    let stderr_msg = filtered_stderr(stderr);
    if stderr_msg != "unknown claude CLI error" {
        return stderr_msg;
    }

    // Claude --output-format json emits structured errors in stdout.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        if let Some(msg) = json
            .get("result")
            .or_else(|| json.get("message"))
            .or_else(|| json.pointer("/error/message"))
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

/// Serialize `CompletionRequest` messages into a plain-text prompt string.
fn serialize_messages(request: &CompletionRequest) -> String {
    let mut parts = Vec::new();

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

/// Extract plain text from a `MessageContent` (delegates to shared method).
fn extract_text(content: &MessageContent) -> String {
    content.text_with_tool_results()
}

/// Check if the `claude` binary is available on PATH.
pub fn is_available() -> bool {
    super::binary_on_path("claude")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_claude_json_full() {
        let json = r#"{
            "result": "Hello, world!",
            "usage": {"input_tokens": 10, "output_tokens": 5},
            "total_cost_usd": 0.001
        }"#;
        let resp = parse_claude_json(json).unwrap();
        assert_eq!(resp.text(), "Hello, world!");
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_parse_claude_json_missing_usage() {
        let json = r#"{"result": "Just text"}"#;
        let resp = parse_claude_json(json).unwrap();
        assert_eq!(resp.text(), "Just text");
        assert_eq!(resp.usage.input_tokens, 0);
        assert_eq!(resp.usage.output_tokens, 0);
    }

    #[test]
    fn test_parse_claude_json_invalid() {
        let result = parse_claude_json("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_serialize_messages() {
        use openfang_types::message::Message;

        let request = CompletionRequest {
            model: "sonnet".to_string(),
            messages: vec![
                Message {
                    role: Role::User,
                    content: MessageContent::Text("Hello".to_string()),
                },
                Message {
                    role: Role::Assistant,
                    content: MessageContent::Text("Hi there!".to_string()),
                },
                Message {
                    role: Role::User,
                    content: MessageContent::Text("How are you?".to_string()),
                },
            ],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.0,
            system: None,
            thinking: None,
        };

        let prompt = serialize_messages(&request);
        assert!(prompt.contains("User: Hello"));
        assert!(prompt.contains("Assistant: Hi there!"));
        assert!(prompt.contains("User: How are you?"));
    }

    #[test]
    fn test_serialize_messages_with_blocks() {
        use openfang_types::message::Message;

        let request = CompletionRequest {
            model: "sonnet".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![
                    ContentBlock::Text {
                        text: "First".to_string(),
                    },
                    ContentBlock::Text {
                        text: "Second".to_string(),
                    },
                ]),
            }],
            tools: vec![],
            max_tokens: 1024,
            temperature: 0.0,
            system: None,
            thinking: None,
        };

        let prompt = serialize_messages(&request);
        assert!(prompt.contains("First"));
        assert!(prompt.contains("Second"));
    }

    #[test]
    fn test_extract_text_simple() {
        let content = MessageContent::Text("hello".to_string());
        assert_eq!(extract_text(&content), "hello");
    }

    #[test]
    fn test_extract_text_blocks() {
        let content = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "a".to_string(),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: "result".to_string(),
                is_error: false,
            },
            ContentBlock::Image {
                media_type: "image/png".to_string(),
                data: "base64data".to_string(),
            },
        ]);
        let text = extract_text(&content);
        assert!(text.contains("a"));
        assert!(text.contains("result"));
        assert!(!text.contains("base64data"));
    }

    #[test]
    fn test_extract_claude_failure_message_from_stdout_json() {
        let stdout = r#"{"is_error":true,"result":"Invalid request format. This may be a bug."}"#;
        let msg = extract_claude_failure_message(stdout, "");
        assert_eq!(msg, "Invalid request format. This may be a bug.");
    }

    #[test]
    fn test_extract_claude_failure_message_prefers_stderr_when_present() {
        let stdout = r#"{"is_error":true,"result":"stdout message"}"#;
        let stderr = "validation error: malformed payload";
        let msg = extract_claude_failure_message(stdout, stderr);
        assert_eq!(msg, "validation error: malformed payload");
    }

    #[test]
    fn test_is_request_format_error_patterns() {
        assert!(is_request_format_error(
            "Invalid request format. This may be a bug."
        ));
        assert!(is_request_format_error("missing field `id_token`"));
        assert!(!is_request_format_error("rate limit exceeded"));
    }
}
