use super::{
    wire::{bounded_json, UniqueValue},
    DecodeError, Decoded, Usage, MAX_EVENTS, MAX_EVENT_BYTES, MAX_OUTPUT_BYTES, MAX_RESULT_BYTES,
    VERSION,
};
use crate::RunOutput;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashSet;

/// Accept only the observed single-prompt StructuredOutput protocol. Unknown
/// work, tools, CLI versions, model substitutions and errors fail closed. A
/// known idle/rate notice may follow the result; EOF + exit 0 are still needed.
/// `validate` must enforce the selected recipe and evidence membership.
/// Use a strict result type (`deny_unknown_fields`), as the compiled recipes do.
pub fn decode<T: DeserializeOwned>(
    output: &RunOutput,
    model: &str,
    validate: impl FnOnce(&T) -> bool,
) -> Result<Decoded<T>, DecodeError> {
    if !output.process_succeeded() {
        return Err(DecodeError::Process {
            termination: output.termination,
            exit_code: output.exit_code,
        });
    }
    if output.stdout.len() > MAX_OUTPUT_BYTES {
        return Err(error(0, "output limit exceeded"));
    }
    let mut session = None;
    let mut uuids = HashSet::new();
    let mut tool: Option<(String, Value)> = None;
    let mut tool_done = false;
    let mut result = None;
    let bytes = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    for (index, bytes) in bytes.split(|b| *b == b'\n').enumerate() {
        let line = index + 1;
        if line > MAX_EVENTS || bytes.len() > MAX_EVENT_BYTES {
            return Err(error(line, "event limit exceeded"));
        }
        let UniqueValue(v) = serde_json::from_slice(bytes)
            .map_err(|_| error(line, "malformed or duplicate JSON"))?;
        let sid = id(&v, "session_id").ok_or_else(|| error(line, "invalid session ID"))?;
        let uuid = id(&v, "uuid").ok_or_else(|| error(line, "invalid event ID"))?;
        if !uuids.insert(uuid.to_owned()) || session.as_deref().is_some_and(|s| s != sid) {
            return Err(error(line, "reused event or mixed session"));
        }
        match (v["type"].as_str(), v["subtype"].as_str()) {
            (Some("system"), Some("init")) => {
                if session.is_some() || line != 1 || !init(&v, model) {
                    return Err(error(line, "unqualified initialization"));
                }
                session = Some(sid.to_owned());
            }
            _ if session.is_none() => return Err(error(line, "event before initialization")),
            (Some("assistant"), None) if result.is_none() && !tool_done => {
                if !keys(
                    &v,
                    &[
                        "type",
                        "message",
                        "parent_tool_use_id",
                        "session_id",
                        "uuid",
                        "timestamp",
                        "request_id",
                    ],
                ) || !required_null(&v, "parent_tool_use_id")
                    || !assistant(&v["message"], model, &mut tool)
                {
                    return Err(error(line, "unsupported assistant work"));
                }
            }
            (Some("user"), None) if result.is_none() && !tool_done => {
                let Some((tool_id, _)) = &tool else {
                    return Err(error(line, "tool result without request"));
                };
                if !tool_result(&v, tool_id) {
                    return Err(error(line, "invalid or failed tool result"));
                }
                tool_done = true;
            }
            (Some("result"), Some("success")) if result.is_none() && tool_done => {
                let usage =
                    finish(&v, model).ok_or_else(|| error(line, "failed or incomplete result"))?;
                let value = v
                    .get("structured_output")
                    .ok_or(DecodeError::InvalidResult)?;
                if tool.as_ref().map(|(_, input)| input) != Some(value)
                    || bounded_json(value, MAX_RESULT_BYTES).is_err()
                {
                    return Err(DecodeError::InvalidResult);
                }
                let value = serde_json::from_value(value.clone())
                    .map_err(|_| DecodeError::InvalidResult)?;
                result = Some(Decoded { value, usage });
            }
            (Some("system"), Some("status")) if idle(&v) => {}
            (Some("rate_limit_event"), None) if rate_notice(&v) => {}
            _ => return Err(error(line, "failure, tool or unsupported event ordering")),
        }
    }
    let result = result.ok_or_else(|| error(0, "missing complete structured result"))?;
    if !validate(&result.value) {
        return Err(DecodeError::RejectedResult);
    }
    Ok(result)
}

fn error(line: usize, reason: &'static str) -> DecodeError {
    DecodeError::Stream { line, reason }
}
fn keys(v: &Value, allowed: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.keys().all(|k| allowed.contains(&k.as_str())))
}
fn id<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    })
}
fn empty_array(v: &Value) -> bool {
    v.as_array().is_some_and(Vec::is_empty)
}
fn required_null(v: &Value, key: &str) -> bool {
    v.get(key) == Some(&Value::Null)
}
fn number(v: &Value) -> bool {
    v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0)
}

fn init(v: &Value, model: &str) -> bool {
    keys(
        v,
        &[
            "type",
            "subtype",
            "cwd",
            "session_id",
            "tools",
            "mcp_servers",
            "model",
            "permissionMode",
            "slash_commands",
            "apiKeySource",
            "claude_code_version",
            "output_style",
            "agents",
            "skills",
            "plugins",
            "capabilities",
            "analytics_disabled",
            "product_feedback_disabled",
            "uuid",
            "messaging_socket_path",
            "fast_mode_state",
            "fast_mode_disabled_reason",
        ],
    ) && v["cwd"] == "/context"
        && v["model"] == model
        && v["claude_code_version"] == VERSION
        && v["permissionMode"] == "dontAsk"
        && v["output_style"] == "default"
        && v["tools"] == serde_json::json!(["StructuredOutput"])
        && ["mcp_servers", "slash_commands", "skills", "plugins"]
            .iter()
            .all(|k| empty_array(&v[k]))
        && v["agents"] == serde_json::json!(["claude", "Explore", "general-purpose", "Plan"])
        && v["analytics_disabled"] == true
        // This is org policy metadata (allow_product_feedback), not a claim
        // that feedback ran. Reviewed traffic env + empty tools/slash commands
        // disable the action; an unrestricted Team policy legitimately says false.
        && v["product_feedback_disabled"].is_boolean()
        && v["fast_mode_state"] == "off"
        && matches!(
            v["apiKeySource"].as_str(),
            Some("none" | "ANTHROPIC_API_KEY")
        )
}

fn assistant(v: &Value, model: &str, tool: &mut Option<(String, Value)>) -> bool {
    if !keys(
        v,
        &[
            "id",
            "type",
            "role",
            "model",
            "content",
            "stop_reason",
            "stop_sequence",
            "usage",
            "context_management",
        ],
    ) || id(v, "id").is_none()
        || v["type"] != "message"
        || v["role"] != "assistant"
        || v["model"] != model
        || !(required_null(v, "stop_reason")
            || matches!(v["stop_reason"].as_str(), Some("end_turn" | "tool_use")))
        || !required_null(v, "stop_sequence")
        || !v["context_management"].is_null()
    {
        return false;
    }
    let Some(content) = v["content"].as_array() else {
        return false;
    };
    if content.is_empty() {
        return false;
    }
    for block in content {
        let valid = match block["type"].as_str() {
            Some("text") => keys(block, &["type", "text"]) && block["text"].is_string(),
            Some("thinking") => {
                keys(block, &["type", "thinking", "signature"])
                    && block["thinking"].is_string()
                    && block["signature"].is_string()
            }
            Some("redacted_thinking") => {
                keys(block, &["type", "data"]) && block["data"].is_string()
            }
            Some("tool_use") if tool.is_none() => {
                if !keys(block, &["type", "id", "name", "input"])
                    || block["name"] != "StructuredOutput"
                    || !block["input"].is_object()
                {
                    return false;
                }
                let Some(tool_id) = id(block, "id") else {
                    return false;
                };
                *tool = Some((tool_id.to_owned(), block["input"].clone()));
                true
            }
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    true
}

fn tool_result(v: &Value, tool_id: &str) -> bool {
    const SUCCESS: &str = "Structured output provided successfully";
    if !keys(
        v,
        &[
            "type",
            "message",
            "parent_tool_use_id",
            "session_id",
            "uuid",
            "timestamp",
            "tool_use_result",
        ],
    ) || !required_null(v, "parent_tool_use_id")
        || v["tool_use_result"] != SUCCESS
        || !keys(&v["message"], &["role", "content"])
        || v["message"]["role"] != "user"
    {
        return false;
    }
    let Some(content) = v["message"]["content"].as_array().filter(|a| a.len() == 1) else {
        return false;
    };
    let b = &content[0];
    keys(b, &["tool_use_id", "type", "content", "is_error"])
        && b["type"] == "tool_result"
        && b["tool_use_id"] == tool_id
        && b["content"] == SUCCESS
        && b.get("is_error").is_none_or(|e| e == false)
}

fn finish(v: &Value, model: &str) -> Option<Usage> {
    if !keys(
        v,
        &[
            "type",
            "subtype",
            "session_id",
            "uuid",
            "duration_api_ms",
            "duration_ms",
            "stop_reason",
            "total_cost_usd",
            "usage",
            "modelUsage",
            "permission_denials",
            "terminal_reason",
            "fast_mode_state",
            "fast_mode_disabled_reason",
            "subagent_stats",
            "is_error",
            "num_turns",
            "api_error_status",
            "result",
            "structured_output",
            "ttft_ms",
            "ttft_stream_ms",
            "time_to_request_ms",
            "first_content_frame_ms",
            "queued_turn_count",
            "result_index",
        ],
    ) || v["is_error"] != false
        || !v["result"].is_string()
        || v["stop_reason"] != "tool_use"
        || v["terminal_reason"] != "completed"
        || !v
            .get("num_turns")?
            .as_u64()
            .is_some_and(|n| (1..=4).contains(&n))
        || v["queued_turn_count"] != 0
        || v["result_index"] != 0
        || !v["api_error_status"].is_null()
        || !empty_array(&v["permission_denials"])
        || !["duration_api_ms", "duration_ms", "total_cost_usd"]
            .iter()
            .all(|k| number(&v[k]))
        || v["fast_mode_state"] != "off"
        || !v["subagent_stats"].is_object()
        || !all_zero(&v["subagent_stats"])
    {
        return None;
    }
    let models = v["modelUsage"].as_object()?;
    if models.len() != 1 {
        return None;
    }
    let m = models.get(model)?;
    if !keys(
        m,
        &[
            "inputTokens",
            "outputTokens",
            "cacheReadInputTokens",
            "cacheCreationInputTokens",
            "webSearchRequests",
            "costUSD",
            "contextWindow",
            "maxOutputTokens",
            "thinkingTokens",
            "canonicalModel",
            "provider",
            "costBasis",
        ],
    ) || m["webSearchRequests"] != 0
        || m["provider"] != "firstParty"
        || m["canonicalModel"] != model
        || !number(&m["costUSD"])
    {
        return None;
    }
    let u = &v["usage"];
    if !keys(
        u,
        &[
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens_details",
            "server_tool_use",
            "service_tier",
            "cache_creation",
            "inference_geo",
            "iterations",
            "speed",
        ],
    ) || !all_zero(&u["server_tool_use"])
        || !u["server_tool_use"].is_object()
        || !empty_array(&u["iterations"])
    {
        return None;
    }
    // A single model/turn cannot hide additional sidechain usage in modelUsage.
    for (aggregate, main) in [
        ("inputTokens", "input_tokens"),
        ("outputTokens", "output_tokens"),
        ("cacheReadInputTokens", "cache_read_input_tokens"),
        ("cacheCreationInputTokens", "cache_creation_input_tokens"),
    ] {
        if m[aggregate].as_u64()? != u[main].as_u64()? {
            return None;
        }
    }
    Some(Usage {
        input_tokens: u["input_tokens"].as_u64()?,
        output_tokens: u["output_tokens"].as_u64()?,
        cache_read_input_tokens: u["cache_read_input_tokens"].as_u64()?,
        cache_creation_input_tokens: u["cache_creation_input_tokens"].as_u64()?,
    })
}
fn all_zero(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.values().all(all_zero),
        _ => v.as_u64() == Some(0),
    }
}
fn idle(v: &Value) -> bool {
    keys(
        v,
        &[
            "type",
            "subtype",
            "status",
            "permissionMode",
            "uuid",
            "session_id",
        ],
    ) && required_null(v, "status")
        && v.get("permissionMode").is_none_or(|p| p == "dontAsk")
}
fn rate_notice(v: &Value) -> bool {
    let r = &v["rate_limit_info"];
    keys(v, &["type", "rate_limit_info", "uuid", "session_id"])
        && keys(
            r,
            &[
                "status",
                "resetsAt",
                "rateLimitType",
                "utilization",
                "overageStatus",
                "overageResetsAt",
                "overageDisabledReason",
                "isUsingOverage",
            ],
        )
        && matches!(r["status"].as_str(), Some("allowed" | "allowed_warning"))
        && r.get("overageStatus")
            .is_none_or(|s| matches!(s.as_str(), Some("allowed" | "allowed_warning")))
}
