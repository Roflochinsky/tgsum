use super::*;
use crate::egress::Failure;
use crate::{PreparedContext, Termination};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tgsum_core::bundle::{BundleOptions, EvidenceRef};
use tgsum_core::project::{ProjectChange, ProjectSource, ProjectStore};
use tgsum_core::snapshot::SourceScope;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    summary: String,
    evidence: Vec<String>,
}

struct Context {
    root: tempfile::TempDir,
    project: String,
    bundle: String,
    context: PreparedContext,
}
fn context() -> Context {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::new(root.path());
    let project = store.create("HTTPS synthetic Project").unwrap();
    let scope = SourceScope::telegram("PRIVATE_ACCOUNT", "77");
    store.snapshots(&project.project_id).unwrap().import_telegram("snapshot", &scope,
        Cursor::new(r#"{"id":77,"name":"Selected","messages":[{"id":1,"date":"2026-06-18T10:00:00","text":"Synthetic selected message"}]}"#)).unwrap();
    let project = store
        .update(
            &project.project_id,
            project.revision,
            ProjectChange::Source(ProjectSource {
                source_id: "PRIVATE_SOURCE".into(),
                connector_id: "telegram_json".into(),
                scope,
                archive_path: None,
                latest_snapshot_id: Some("snapshot".into()),
                selection: Default::default(),
            }),
        )
        .unwrap();
    let bundle = store
        .prepare_bundle(
            &project.project_id,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    let context = PreparedContext::from_bundle(
        root.path().into(),
        &project.project_id,
        &bundle.bundle_id,
        project.revision,
        &Cancellation::default(),
    )
    .unwrap();
    Context {
        root,
        project: project.project_id,
        bundle: bundle.bundle_id,
        context,
    }
}

fn schema() -> Value {
    json!({"type":"object","properties":{"summary":{"type":"string"},"evidence":{"type":"array","items":{"type":"string"}}},"required":["summary","evidence"],"additionalProperties":false})
}

#[test]
fn offline_runner_rejects_network_profile_before_opening_runtime() {
    let result = crate::OfflineRunner::qualify(
        AdapterContract {
            id: "codex".into(),
            isolation_profile: LINUX_EGRESS_PROFILE.into(),
            version_probe: super::super::version_probe(),
            expected_version_output: VERSION_STDOUT.into(),
        },
        RuntimeSpec {
            executable: "/nonexistent-runtime".into(),
            files: vec![],
        },
        &Cancellation::default(),
    );
    assert!(matches!(
        result,
        Err(RunnerError::ExportOnly("unknown isolation profile"))
    ));
}

fn sse(answer: &str) -> String {
    let item = json!({"id":"msg_https","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":answer,"annotations":[]}]});
    [json!({"type":"response.created","response":{"id":"resp_https","status":"in_progress","output":[]}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_https","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_https","output_index":0,"content_index":0,"delta":answer}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"resp_https","status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}})]
        .iter().map(|v| format!("event: {}\ndata: {v}\n\n",v["type"].as_str().unwrap())).collect()
}

fn request(stream: &mut impl Read, auth: bool) -> Value {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 32768);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
    }
    let header = String::from_utf8(header).unwrap().to_lowercase();
    assert!(header.starts_with("post /v1/responses http/1.1\r\n"));
    let supplied = header.lines().find(|l| l.starts_with("authorization:"));
    if auth {
        assert!(supplied == Some("authorization: bearer tgsum-synthetic-not-a-real-key"));
    } else {
        assert!(supplied.is_none());
    }
    let n: usize = header
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(n <= 4 * 1024 * 1024);
    let mut bytes = vec![0; n];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn runtime(request: &CodexRequest<'_>, directory: &std::path::Path) -> CodexNetworkRunner {
    let mut files: Vec<_> = ["libc.so.6", "libgcc_s.so.1", "ld-linux-x86-64.so.2"]
        .into_iter()
        .map(|name| RuntimeFile {
            source: ["/usr/lib", "/lib/x86_64-linux-gnu", "/usr/lib64"]
                .into_iter()
                .map(|root| PathBuf::from(root).join(name))
                .find(|p| p.is_file())
                .unwrap(),
            guest: if name.starts_with("ld-") {
                "/lib64/ld-linux-x86-64.so.2".into()
            } else {
                PathBuf::from("/usr/lib").join(name)
            },
        })
        .collect();
    files.extend([
        request.schema_runtime_file(),
        RuntimeFile {
            source: std::env::var_os("TGSUM_CODEX_TEST_BINARY")
                .expect("explicit installed static Codex path")
                .into(),
            guest: "/runtime/installed-codex".into(),
        },
        RuntimeFile {
            source: directory.join("ca.pem"),
            guest: "/runtime/provider-ca.pem".into(),
        },
        RuntimeFile {
            source: directory.join("setup.json"),
            guest: "/runtime/https-test.json".into(),
        },
    ]);
    CodexNetworkRunner::qualify(
        std::env::var_os("TGSUM_RELAY_TEST_BINARY")
            .expect("build relay and set explicit path")
            .into(),
        std::env::var_os("TGSUM_HTTPS_FIXTURE_BINARY")
            .expect("build HTTPS fixture and set explicit path")
            .into(),
        files,
        &Cancellation::default(),
    )
    .unwrap()
}

#[test]
#[ignore = "requires Linux namespace backend and explicit built relay/HTTPS fixture/installed Codex paths"]
fn installed_codex_https_through_namespace_relay_with_and_without_synthetic_auth() {
    for auth in [false, true] {
        qualify("success", auth);
    }
}

#[test]
#[ignore = "requires Linux namespace backend and explicit built relay/HTTPS fixture/installed Codex paths"]
fn installed_codex_https_rejects_unknown_ca_and_refresh_and_cleans_up_cancel_timeout() {
    for mode in ["wrong-ca", "refresh-probe", "cancel", "timeout"] {
        qualify(mode, false);
    }
}

fn qualify(mode: &str, auth: bool) {
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use rustls::{ServerConfig, ServerConnection, StreamOwned};
    let fixture = context();
    let cancel = Cancellation::default();
    let request_data = CodexRequest::prepare(
        &fixture.context,
        "gpt-6-astra",
        "Synthetic HTTPS summary with evidence",
        &schema(),
        &cancel,
    )
    .unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["api.openai.com".into()]).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
        )
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let trust = if mode == "wrong-ca" {
        rcgen::generate_simple_self_signed(vec!["unrelated.test".into()])
            .unwrap()
            .cert
            .pem()
    } else {
        certificate.cert.pem()
    };
    fs::write(temporary.path().join("ca.pem"), trust).unwrap();
    let listener = TcpListener::bind("127.0.0.2:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    fs::write(
        temporary.path().join("setup.json"),
        json!({"address":address.to_string(),"mode":mode}).to_string(),
    )
    .unwrap();
    let runner = runtime(&request_data, temporary.path());
    let auth_path = temporary.path().join("auth.json");
    let auth_file = if auth {
        fs::write(
            &auth_path,
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"tgsum-synthetic-not-a-real-key"}"#,
        )
        .unwrap();
        fs::set_permissions(&auth_path, fs::Permissions::from_mode(0o600)).unwrap();
        Some(SelectedAuthFile::select(&auth_path).unwrap())
    } else {
        None
    };
    let limits = RunLimits {
        timeout: Duration::from_secs(if mode == "timeout" { 4 } else { 15 }),
        ..super::super::run_limits()
    };
    let gateway = InferenceGateway::fixture(
        Destination::OpenAiApi,
        Limits {
            // Keep fixture gateway alive past the process deadline to test
            // the runner's own timeout/namespace cleanup independently.
            duration: Duration::from_secs(15),
            ..Limits::default()
        },
        &cancel,
        address,
    );
    let socket_path = gateway.socket_path().to_owned();
    let stopped = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    // Capture the reviewed reference, without moving the request into the server.
    let input: Value = serde_json::from_slice(&request_data.invocation().unwrap().stdin).unwrap();
    let reference = input["untrusted_documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["text"].as_str().unwrap().lines())
        .find_map(|l| l.strip_prefix("## Evidence "))
        .unwrap()
        .to_owned();
    let (output, (count, connections)) = std::thread::scope(|scope| {
        let stop = stopped.clone();
        let seen = observed.clone();
        let signal = cancel.clone();
        let server = scope.spawn(move || {
            let mut count = 0;
            let mut connections = 0;
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        connections += 1;
                        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                        socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                        let mut tls = StreamOwned::new(ServerConnection::new(Arc::new(config.clone())).unwrap(), socket);
                        if mode == "wrong-ca" {
                            assert!(tls.conn.complete_io(&mut tls.sock).is_err()); continue;
                        }
                        let data = request(&mut tls, auth); count += 1;
                        seen.store(true, Ordering::Relaxed);
                        assert!(data["tools"].is_null() || data["tools"].as_array().unwrap().is_empty());
                        assert_eq!(data["text"]["format"]["schema"], schema());
                        if mode == "cancel" {
                            signal.cancel(); continue;
                        }
                        if mode == "timeout" {
                            // Hold the real HTTPS request open until the runner
                            // has timed out and returned. No SSE response.
                            while !stop.load(Ordering::Relaxed) {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            continue;
                        }
                        let serialized = data["input"].to_string();
                        // Resolve the exact reviewed reference from original stdin;
                        // the canned model result never invents a message ID.
                        assert!(serialized.contains(&reference));
                        let body = sse(&json!({"summary":"Canned HTTPS result","evidence":[reference]}).to_string());
                        write!(tls,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                        tls.conn.send_close_notify(); tls.flush().unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("local fixture accept: {e}"),
                }
            }
            (count, connections)
        });
        let output = runner.run_with_gateway(
            &request_data,
            auth_file.as_ref(),
            Destination::OpenAiApi,
            limits,
            &cancel,
            gateway,
        );
        stopped.store(true, Ordering::Relaxed);
        (output.unwrap(), server.join().unwrap())
    });
    assert!(!socket_path.exists());
    let store = ProjectStore::new(fixture.root.path());
    if mode == "success" {
        assert!(
            output.process.process_succeeded(),
            "{output:?}\n{}\n{}",
            String::from_utf8_lossy(&output.process.stdout),
            String::from_utf8_lossy(&output.process.stderr)
        );
        assert_eq!(count, 1);
        let result = super::super::decode(&output.process, |answer: &Answer| {
            answer.evidence.len() == 1
                && answer.evidence.iter().all(|r| {
                    let Some((id, revision)) = r.split_once('@') else {
                        return false;
                    };
                    store
                        .resolve_evidence(
                            &fixture.project,
                            &fixture.bundle,
                            &EvidenceRef {
                                id: id.into(),
                                revision: revision.into(),
                            },
                        )
                        .is_ok()
                })
        })
        .unwrap();
        assert_eq!(result.value.summary, "Canned HTTPS result");
        assert!(output.gateway.accepted > 0);
    } else {
        assert!(!output.process.process_succeeded(), "{output:?}");
        if matches!(mode, "cancel" | "timeout") {
            assert!(observed.load(Ordering::Relaxed));
            assert_eq!(
                output.process.termination,
                if mode == "cancel" {
                    Termination::Cancelled
                } else {
                    Termination::TimedOut
                }
            );
        } else if mode == "refresh-probe" {
            assert_eq!(output.process.exit_code, Some(42));
            assert!(output.gateway.failures.contains(&Failure::ConnectRequest));
            assert_eq!(count, 0);
        } else {
            assert!(
                connections > 0,
                "must reach TLS rejection, not an earlier failure"
            );
            assert_eq!(count, 0);
        }
    }
    for bytes in [&output.process.stdout, &output.process.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains("tgsum-synthetic-not-a-real-key"));
    }
    assert!(store.open(&fixture.project).unwrap().baselines.is_empty());
}
