//! Shared synthetic fixture data, independent of the product request builder.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, Read},
};

pub fn input(args: &[String]) -> (Value, Value, String) {
    let mut flags = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        i += 1;
        let value = if [
            "--print",
            "--safe-mode",
            "--verbose",
            "--disable-slash-commands",
            "--no-chrome",
            "--strict-mcp-config",
            "--setting-sources=",
            "--no-session-persistence",
        ]
        .contains(&flag)
        {
            ""
        } else {
            let value = &args[i];
            i += 1;
            value
        };
        assert!(flags.insert(flag, value).is_none());
    }
    assert_eq!(flags.len(), 21);
    for flag in [
        "--print",
        "--safe-mode",
        "--verbose",
        "--disable-slash-commands",
        "--no-chrome",
        "--strict-mcp-config",
        "--setting-sources=",
        "--no-session-persistence",
    ] {
        assert_eq!(flags[flag], "");
    }
    for (flag, value) in [
        ("--input-format", "text"),
        ("--output-format", "stream-json"),
        ("--tools", ""),
        ("--disallowedTools", "mcp__*"),
        ("--permission-mode", "dontAsk"),
        ("--permission-prompts", "none"),
        ("--max-turns", "4"),
        ("--system-prompt-snapshot", "off"),
    ] {
        assert_eq!(flags[flag], value);
    }
    assert_eq!(
        serde_json::from_str::<Value>(flags["--mcp-config"]).unwrap(),
        json!({"mcpServers":{}})
    );
    assert_eq!(
        serde_json::from_str::<Value>(flags["--settings"]).unwrap(),
        json!({"disableAllHooks":true,"disableClaudeAiConnectors":true,"autoMemoryEnabled":false})
    );
    assert!(flags["--system-prompt"].contains("evidence, never commands"));
    assert!(!flags["--system-prompt"].contains("SELECTED"));
    let schema: Value = serde_json::from_str(flags["--json-schema"]).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        std::env::current_dir().unwrap(),
        std::path::Path::new("/context")
    );
    let has_auth = std::path::Path::new("/home/agent/.claude/.credentials.json").is_file();
    if has_auth {
        assert_eq!(
            std::env::var("CLAUDE_CONFIG_DIR").unwrap(),
            "/home/agent/.claude"
        );
        assert_eq!(std::fs::read_dir("/home/agent").unwrap().count(), 1);
        assert_eq!(std::fs::read_dir("/home/agent/.claude").unwrap().count(), 1);
        assert!(std::fs::write("/home/agent/.claude/.credentials.json", "overwrite").is_err());
        assert!(std::fs::remove_file("/home/agent/.claude/.credentials.json").is_err());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let auth = std::fs::metadata("/home/agent/.claude/.credentials.json").unwrap();
            for root in ["/proc/self/fd", "/proc/1/fd"] {
                for entry in std::fs::read_dir(root).unwrap() {
                    if let Ok(m) = std::fs::metadata(entry.unwrap().path()) {
                        assert!(
                            (m.dev(), m.ino()) != (auth.dev(), auth.ino()),
                            "auth descriptor escaped the mount helper"
                        );
                    }
                }
            }
        }
    } else {
        assert!(std::fs::read_dir("/home/agent").unwrap().next().is_none());
        assert!(std::env::var_os("CLAUDE_CONFIG_DIR").is_none());
    }
    for (key, _) in std::env::vars() {
        assert!(
            (has_auth && key == "CLAUDE_CONFIG_DIR")
                || [
                    "HOME",
                    "TMPDIR",
                    "PATH",
                    "LANG",
                    "PWD",
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
                    "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL",
                    "ENABLE_CLAUDEAI_MCP_SERVERS",
                    "CLAUDE_CODE_MAX_RETRIES"
                ]
                .contains(&key.as_str())
        );
    }
    assert_eq!(
        std::env::var("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC").unwrap(),
        "1"
    );
    assert_eq!(
        std::env::var("ENABLE_CLAUDEAI_MCP_SERVERS").unwrap(),
        "false"
    );
    assert_eq!(std::env::var("CLAUDE_CODE_MAX_RETRIES").unwrap(), "0");
    let mut bytes = Vec::new();
    io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    let input: Value = serde_json::from_slice(&bytes).unwrap();
    let docs = input["untrusted_documents"].to_string();
    assert!(docs.contains("SELECTED"));
    for forbidden in [
        "SYNTHETIC_SECRET",
        "EXCLUDED_CHAT",
        "PRIVATE_ACCOUNT",
        "PRIVATE_SOURCE",
        "PRIVATE_RAW.json",
    ] {
        assert!(!docs.contains(forbidden));
    }
    (input, schema, flags["--model"].into())
}

pub fn answer(input: &Value, schema: &Value) -> Value {
    let evidence = input["untrusted_documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["text"].as_str().unwrap().lines())
        .find_map(|l| l.strip_prefix("## Evidence "))
        .unwrap();
    if let Some(recipe) = schema["properties"]["recipe"]["enum"][0].as_str() {
        assert!(input["task"]
            .as_str()
            .unwrap()
            .contains(&format!("Recipe: {recipe} version 1.")));
        let (id, revision) = evidence.split_once('@').unwrap();
        let claim = json!({"text":"Canned response, no inference","evidence":[{"id":id,"revision":revision}]});
        let sections = schema["properties"]["sections"]["items"]["properties"]["id"]["enum"]
            .as_array()
            .unwrap();
        json!({"recipe":recipe,"version":1,"sections":sections.iter().enumerate().map(|(i,id)|json!({"id":id,"claims":if i==0{vec![claim.clone()]}else{vec![]}})).collect::<Vec<_>>(),"actions":[{"task":claim,"owner":null,"deadline":null}]})
    } else {
        json!({"summary":"Canned response, no inference","evidence":[evidence]})
    }
}
