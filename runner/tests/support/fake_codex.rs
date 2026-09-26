//! Test-only CLI, never delegates to Codex or a provider. Protocol checks are
//! independent of the request builder and run inside the offline sandbox.
use std::collections::HashSet;
use std::io::{self, Read, Write};

use serde_json::{json, Value};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("codex-cli 0.155.1");
        return;
    }
    assert_eq!(&args[..3], ["--ask-for-approval", "never", "exec"]);
    assert_eq!(args.last().unwrap(), "-");
    let mut flags = HashSet::new();
    let mut disabled = HashSet::new();
    let mut schema = None;
    let mut cursor = 3;
    while cursor < args.len() - 1 {
        let arg = args[cursor].as_str();
        assert!(flags.insert(arg) || arg == "--disable");
        cursor += 1;
        match arg {
            "--ignore-user-config"
            | "--ignore-rules"
            | "--ephemeral"
            | "--skip-git-repo-check"
            | "--strict-config"
            | "--json" => {}
            "--model" | "--sandbox" | "--cd" | "--color" | "--config" | "--output-schema"
            | "--disable" => {
                let value = &args[cursor];
                cursor += 1;
                match arg {
                    "--model" => assert_eq!(value, "synthetic-model"),
                    "--sandbox" => assert_eq!(value, "read-only"),
                    "--cd" => assert_eq!(value, "/context"),
                    "--color" => assert_eq!(value, "never"),
                    "--config" => assert_eq!(value, "web_search=\"disabled\""),
                    "--output-schema" => {
                        assert_eq!(value, "/runtime/tgsum-codex-result.schema.json");
                        schema = Some(
                            serde_json::from_slice::<Value>(&std::fs::read(value).unwrap())
                                .unwrap(),
                        );
                        assert!(std::fs::write(value, "corrupt").is_err());
                    }
                    "--disable" => {
                        assert!(disabled.insert(value.as_str()));
                    }
                    _ => unreachable!(),
                }
            }
            _ => panic!("unexpected argv"),
        }
    }
    for required in [
        "--ignore-user-config",
        "--ignore-rules",
        "--ephemeral",
        "--skip-git-repo-check",
        "--strict-config",
        "--json",
        "--model",
        "--sandbox",
        "--cd",
        "--color",
        "--config",
        "--output-schema",
    ] {
        assert!(flags.contains(required));
    }
    for required in [
        "shell_tool",
        "unified_exec",
        "hooks",
        "apps",
        "plugins",
        "remote_plugin",
        "multi_agent",
        "browser_use",
        "computer_use",
        "code_mode",
        "code_mode_host",
        "skill_search",
        "view_image",
        "image_generation",
    ] {
        assert!(disabled.contains(required));
    }
    assert_eq!(schema.unwrap()["additionalProperties"], false);
    assert_eq!(
        std::env::current_dir().unwrap(),
        std::path::Path::new("/context")
    );
    assert!(std::fs::read_dir("/home/agent").unwrap().next().is_none());
    let mut bytes = Vec::new();
    io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    let input: Value = serde_json::from_slice(&bytes).unwrap();
    let docs = input["untrusted_documents"].as_array().unwrap();
    let text = docs
        .iter()
        .map(|d| d["text"].as_str().unwrap())
        .collect::<String>();
    assert!(text.contains("SELECTED"));
    assert!(!text.contains("SYNTHETIC_SECRET"));
    assert!(!text.contains("EXCLUDED_CHAT"));
    assert!(!text.contains("PRIVATE_ACCOUNT"));
    let evidence = text
        .lines()
        .find_map(|s| s.strip_prefix("## Evidence "))
        .unwrap();
    let mode = input["task"].as_str().unwrap();
    emit(json!({"type":"thread.started","thread_id":"synthetic"}));
    emit(json!({"type":"turn.started"}));
    match mode {
        "malformed" => {
            println!("PRIVATE_DIAGNOSTIC");
            return;
        }
        "truncated" => return,
        "tool" => emit(
            json!({"type":"item.completed","item":{"id":"tool","type":"command_execution","text":"untrusted command"}}),
        ),
        _ => {}
    }
    let text = if mode == "invalid-result" {
        "not JSON".to_owned()
    } else {
        json!({"summary":"Synthetic result","evidence":[evidence]}).to_string()
    };
    emit(
        json!({"type":"item.completed","item":{"id":"answer","type":"agent_message","text":text}}),
    );
    emit(
        json!({"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0}}),
    );
    io::stdout().flush().unwrap();
    eprintln!("PRIVATE_DIAGNOSTIC");
    if mode == "nonzero" {
        std::process::exit(17);
    }
    if mode == "sleep" {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}

fn emit(value: Value) {
    println!("{value}");
}
