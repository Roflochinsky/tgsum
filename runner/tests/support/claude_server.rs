//! Installed CLI + canned Anthropic Messages server, both inside the private
//! offline namespace. Only the fixture sets a synthetic key and loopback URL.
mod claude_protocol;
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        assert!(std::fs::read_dir("/home/agent").unwrap().next().is_none());
        let status = Command::new("/runtime/claude")
            .args(&args)
            .status()
            .unwrap();
        std::process::exit(status.code().unwrap_or(125));
    }
    let (input, schema, model) = claude_protocol::input(&args);
    let answer = claude_protocol::answer(&input, &schema);
    let api_error = input["task"] == "http-401";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let server = std::thread::spawn(move || {
        let mut reports = Vec::new();
        let mut messages = 0;
        while !signal.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut socket, peer)) => {
                    assert!(peer.ip().is_loopback());
                    let (path, request) = read_request(&mut socket);
                    eprintln!("TGSUM_CLAUDE_FIXTURE_REQUEST {path}");
                    assert!(reports.len() < 4);
                    let (status, ct, body) = if path == "POST /v1/messages?beta=true HTTP/1.1" {
                        messages += 1;
                        assert_eq!(messages, 1);
                        assert_eq!(request["model"], model);
                        assert_eq!(request["stream"], true);
                        let tools = request["tools"].as_array().unwrap();
                        assert_eq!(tools.len(), 1);
                        assert_eq!(tools[0]["name"], "StructuredOutput");
                        assert_eq!(tools[0]["input_schema"], schema);
                        reports.push(json!({"kind":"messages","tools":["StructuredOutput"],"model":model,"schema":tools[0]["input_schema"]}));
                        if api_error {
                            ("401 Unauthorized","application/json",json!({"type":"error","error":{"type":"authentication_error","message":"SYNTHETIC_REJECTED_KEY"}}).to_string())
                        } else {
                            ("200 OK", "text/event-stream", events(&model, &answer))
                        }
                    } else {
                        assert_eq!(path, "HEAD /api/hello HTTP/1.1");
                        reports.push(json!({"kind":"hello"}));
                        ("200 OK", "application/json", String::new())
                    };
                    write!(socket,"HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("fixture listener failed: {e}"),
            }
        }
        assert_eq!(messages, 1);
        reports
    });
    let mut child = Command::new("/runtime/claude")
        .args(&args)
        // All inherited variables here already belong to the offline profile.
        .env("ANTHROPIC_API_KEY", "tgsum-synthetic-not-a-real-key")
        .env("ANTHROPIC_BASE_URL", format!("http://{address}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let write_result = child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes());
    let output = child.wait_with_output().unwrap();
    stop.store(true, Ordering::Relaxed);
    let reports = server.join().unwrap();
    if output.status.success() {
        write_result.unwrap();
    }
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    eprintln!("TGSUM_CLAUDE_CATALOG {}", json!(reports));
    std::process::exit(output.status.code().unwrap_or(125));
}

fn read_request(socket: &mut TcpStream) -> (String, Value) {
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut h = Vec::new();
    while !h.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        socket.read_exact(&mut b).unwrap();
        h.push(b[0]);
        assert!(h.len() < 32768);
    }
    let h = String::from_utf8(h).unwrap().to_lowercase();
    let path = h.lines().next().unwrap().to_owned();
    // Do not print headers even on mismatch. There are no actual credentials.
    let key = h.lines().find_map(|l| l.strip_prefix("x-api-key:"));
    if path.starts_with("post ") {
        assert!(key.is_some_and(|k| k.trim() == "tgsum-synthetic-not-a-real-key"));
    }
    assert!(!h.contains("authorization:") && !h.contains("content-encoding:"));
    let n: usize = h
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .map(|s| s.trim().parse().unwrap())
        .unwrap_or(0);
    assert!(n <= 2 * 1024 * 1024);
    let mut bytes = vec![0; n];
    socket.read_exact(&mut bytes).unwrap();
    let request = if n == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let path = if path == "post /v1/messages?beta=true http/1.1" {
        "POST /v1/messages?beta=true HTTP/1.1"
    } else if path == "head /api/hello http/1.1" {
        "HEAD /api/hello HTTP/1.1"
    } else {
        panic!("unexpected local request path")
    };
    (path.into(), request)
}

fn events(model: &str, answer: &Value) -> String {
    let events = [
        json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","content":[],"model":model,"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_fixture","name":"StructuredOutput","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":answer.to_string()}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    events
        .iter()
        .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
        .collect()
}
