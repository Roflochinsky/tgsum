use super::*;
use std::io::Cursor;
const MODEL: &str = "claude-sonnet-4-6";
const CORPUS: &str = "PRIVATE_CONTEXT /help @/context/file <untrusted>";

fn transcript() -> [Value; 2] {
    [
        json!({"type":"control_response","response":{"subtype":"success","request_id":"tgsum-init-1","pending_permission_requests":[],"pending_user_dialog_requests":[],"response":{"commands":[],"account":{"apiProvider":"firstParty","email":"private@example.test"},"output_style":"default"}}}),
        json!({"type":"control_response","response":{"subtype":"success","request_id":"tgsum-settings-1","response":{"effective":{"forceRemoteSettingsRefresh":true},"sources":[{"source":"managedSettings","settings":{"permissions":{"deny":["Bash"]}}}],"applied":{"model":MODEL,"advisor":null,"ultracode":false,"effort":null}}}}),
    ]
}
fn run(values: &[Value]) -> (Result<()>, Vec<Value>) {
    let bytes = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let mut output = Vec::new();
    let result = release(&mut Cursor::new(bytes), &mut output, MODEL, CORPUS);
    let frames = output
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect();
    (result, frames)
}
#[test]
fn two_correlated_replies_release_one_literal_client_composed_prompt() {
    let (result, sent) = run(&transcript());
    result.unwrap();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0]["request"]["subtype"], "initialize");
    assert_eq!(sent[1]["request"]["subtype"], "get_settings");
    assert_eq!(sent[2]["message"]["content"], CORPUS);
    assert_eq!(sent[2]["client_composed"], true);
    assert!(!serde_json::to_string(&sent)
        .unwrap()
        .contains("private@example.test"));
}
#[test]
fn incompatible_reply_never_releases_context() {
    let bad = [
        (0, "/response/request_id", json!("foreign")),
        (0, "/response/subtype", json!("error")),
        (0, "/response/pending_permission_requests", json!([{}])),
        (0, "/response/pending_user_dialog_requests", Value::Null),
        (
            0,
            "/response/response/account/apiProvider",
            json!("gateway"),
        ),
        (0, "/response/response/commands", json!([{"name":"extra"}])),
        (1, "/response/request_id", json!("tgsum-init-1")),
        (
            1,
            "/response/response/applied/model",
            json!("unreviewed-model"),
        ),
        (
            1,
            "/response/response/applied/advisor",
            json!("another-model"),
        ),
        (1, "/response/response/applied/ultracode", json!(true)),
        (
            1,
            "/response/response/effective",
            json!({"env":{"ANTHROPIC_BASE_URL":"https://other.test"}}),
        ),
        (
            1,
            "/response/response/sources/0/settings",
            json!({"hooks":{"SessionStart":[]}}),
        ),
        (
            1,
            "/response/response/effective",
            json!({"fallbackModel":["haiku"]}),
        ),
        (
            1,
            "/response/response",
            json!({"errors":[{"message":"private error"}]}),
        ),
        (
            1,
            "/response/response",
            json!({"remote_control_policy_lock_reason":"restricted"}),
        ),
    ];
    for (index, path, value) in bad {
        let mut values = transcript();
        *values[index].pointer_mut(path).unwrap() = value;
        let (result, sent) = run(&values);
        assert!(result.is_err(), "{path}");
        assert!(sent.iter().all(|v| v["type"] == "control_request"));
        assert!(!serde_json::to_string(&sent)
            .unwrap()
            .contains("PRIVATE_CONTEXT"));
    }
}
#[test]
fn malformed_duplicate_oversize_eof_or_inference_instead_of_control_is_rejected() {
    for bytes in [
        String::new(),
        "{}".into(),
        "{\"type\":\"control_response\",\"type\":\"control_response\"}\n".into(),
        "{\"type\":\"assistant\",\"message\":{}}\n".into(),
        format!("{}\n", " ".repeat(MAX_EVENT_BYTES)),
    ] {
        let mut sent = Vec::new();
        assert!(release(&mut Cursor::new(bytes), &mut sent, MODEL, CORPUS).is_err());
        assert!(!String::from_utf8(sent).unwrap().contains("PRIVATE_CONTEXT"));
    }
}
#[test]
fn managed_executable_features_require_another_profile() {
    for settings in [
        json!({"env":{"NEUTRAL":"value"}}),
        json!({"apiKeyHelper":"cmd"}),
        json!({"policyHelper":"cmd"}),
        json!({"proxyAuthHelper":"cmd"}),
        json!({"managedMcpServers":{"a":{"url":"https://example.test"}}}),
        json!({"mcpServers":{"a":{"command":"cmd"}}}),
        json!({"enabledPlugins":{"a":true}}),
        json!({"disableAllHooks":false}),
        json!({"disableClaudeAiConnectors":false}),
        json!({"autoMemoryEnabled":true}),
    ] {
        assert!(!compatible(&settings));
    }
    assert!(compatible(
        &json!({"env":{},"mcpServers":{},"permissions":{"deny":["Bash"]},"forceRemoteSettingsRefresh":true,"availableModels":["sonnet"],"requiredMinimumVersion":"2.1.280"})
    ));
}
#[test]
fn relay_input_and_argument_contract_is_bounded() {
    assert!(input(Cursor::new(vec![b'x'; MAX_INPUT_BYTES + 1])).is_err());
    assert!(input(Cursor::new(vec![255])).is_err());
    let mut args = ["--model", MODEL, "--input-format", "text"].map(OsString::from);
    assert_eq!(arguments(&mut args).unwrap(), MODEL);
    assert_eq!(args[3], "stream-json");
    assert!(arguments(&mut args).is_err());
}
