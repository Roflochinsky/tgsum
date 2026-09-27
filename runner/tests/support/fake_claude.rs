//! No installed CLI, accounts or provider; output follows the recorded native
//! transcript. Errors intentionally contain a synthetic diagnostic sentinel.
mod claude_protocol;
use serde_json::{json, Value};
use std::io::{self, Write};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("2.1.280 (Claude Code)");
        return;
    }
    let (input, schema, model) = claude_protocol::input(&args);
    let mode = input["task"].as_str().unwrap();
    let answer = claude_protocol::answer(&input, &schema);
    let mut events: Vec<Value> = include_str!("../fixtures/claude-success.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    events[0]["model"] = json!(model);
    events[1]["message"]["model"] = json!(model);
    events[1]["message"]["content"][0]["input"] = answer.clone();
    let old_model = events[3]["modelUsage"]
        .as_object_mut()
        .unwrap()
        .remove("claude-sonnet-4-6")
        .unwrap();
    events[3]["modelUsage"] = json!({model.clone():old_model});
    events[3]["modelUsage"][&model]["canonicalModel"] = json!(model);
    events[3]["result"] = json!(answer.to_string());
    events[3]["structured_output"] = answer;
    match mode {
        "malformed" => {
            println!("PRIVATE_DIAGNOSTIC");
            return;
        }
        "truncated" => {
            events.pop();
        }
        "tool" => events[1]["message"]["content"][0]["name"] = json!("Bash"),
        "error" => events[3]["is_error"] = json!(true),
        "invalid-result" => events[3]["structured_output"] = json!({"unexpected":true}),
        _ => {}
    }
    for event in events {
        println!("{event}");
    }
    io::stdout().flush().unwrap();
    eprintln!("PRIVATE_DIAGNOSTIC");
    if mode == "nonzero" {
        std::process::exit(17);
    }
    if mode == "sleep" {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
