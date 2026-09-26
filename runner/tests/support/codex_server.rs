//! Test-only server and launcher. Both processes live in the offline network
//! namespace. Responses are static fixtures; no credentials or model backend.
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

fn main() {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    if args == ["--version"] {
        let status = Command::new("/runtime/codex").args(&args).status().unwrap();
        std::process::exit(status.code().unwrap_or(1));
    }
    assert_eq!(args.pop().unwrap(), "-");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let provider = format!(
        "model_providers.tgsum_fixture={{name=\"TGSUM synthetic provider\",base_url=\"http://{}/v1\",wire_api=\"responses\",requires_openai_auth=false,request_max_retries=0,stream_max_retries=0,stream_idle_timeout_ms=3000}}",
        listener.local_addr().unwrap()
    );
    // Test-only provider injection. Product request API accepts no overrides.
    for setting in [
        "model_provider=\"tgsum_fixture\"",
        provider.as_str(),
        "features.unbounded_connection_retries=false",
    ] {
        args.extend(["--config".into(), setting.into()]);
    }
    args.push("-".into());
    let stop = Arc::new(AtomicBool::new(false));
    let mut bytes = Vec::new();
    io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    let input: Value = serde_json::from_slice(&bytes).unwrap();
    let evidence = input["untrusted_documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["text"].as_str().unwrap().lines())
        .find_map(|line| line.strip_prefix("## Evidence "))
        .unwrap()
        .to_owned();
    let answer =
        json!({"summary":"Canned response, no inference","evidence":[evidence]}).to_string();
    let output = std::thread::scope(|scope| {
        let signal = stop.clone();
        let server = scope.spawn(move || {
            let mut reports = Vec::new();
            while !signal.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut socket, peer)) => {
                        assert!(peer.ip().is_loopback());
                        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                        socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                        let request = read_request(&mut socket);
                        let report = json!({
                            "tools": request["tools"], "model": request["model"],
                            "input_bytes": request["input"].to_string().len(),
                            "structured_output": request["text"]["format"],
                        });
                        reports.push(report);
                        assert!(reports.len() <= 4, "unexpected repeated request");
                        let body = events(&answer);
                        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
                    Err(error) => panic!("fixture listener: {error}"),
                }
            }
            reports
        });
        let mut child = Command::new("/runtime/codex")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Input is small synthetic data; the outer runner supervises this
        // whole namespace and kills it on timeout or cancellation.
        let write_result = child.stdin.take().unwrap().write_all(&bytes);
        let output = child.wait_with_output().unwrap();
        stop.store(true, Ordering::Relaxed);
        let reports = server.join().unwrap();
        eprintln!("TGSUM_CATALOG {}", json!({"requests":reports}));
        if output.status.success() {
            write_result.unwrap();
        }
        output
    });
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    std::process::exit(output.status.code().unwrap_or(1));
}

fn read_request(socket: &mut TcpStream) -> Value {
    let mut bytes = Vec::new();
    let header_end = loop {
        assert!(bytes.len() < 32 * 1024);
        let mut byte = [0u8];
        socket.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let headers = std::str::from_utf8(&bytes).unwrap().to_lowercase();
    assert!(
        headers.starts_with("post /v1/responses http/1.1\r\n"),
        "unexpected fixture request path"
    );
    assert!(
        !headers.contains("authorization:"),
        "fixture must not receive credentials"
    );
    assert!(
        !headers.contains("content-encoding:"),
        "unsupported compressed request"
    );
    let size: usize = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(size <= 4 * 1024 * 1024);
    bytes.resize(header_end + size, 0);
    socket.read_exact(&mut bytes[header_end..]).unwrap();
    serde_json::from_slice(&bytes[header_end..]).unwrap()
}

fn events(text: &str) -> String {
    let item = json!({"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
    let events = [
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_fixture","object":"response","status":"in_progress","output":[]}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_fixture","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.content_part.added","sequence_number":2,"item_id":"msg_fixture","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_fixture","output_index":0,"content_index":0,"delta":text}),
        json!({"type":"response.output_text.done","sequence_number":4,"item_id":"msg_fixture","output_index":0,"content_index":0,"text":text}),
        json!({"type":"response.output_item.done","sequence_number":5,"output_index":0,"item":item}),
        json!({"type":"response.completed","sequence_number":6,"response":{"id":"resp_fixture","object":"response","status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}}}),
    ];
    events
        .iter()
        .map(|value| {
            format!(
                "event: {}\ndata: {value}\n\n",
                value["type"].as_str().unwrap()
            )
        })
        .collect()
}
