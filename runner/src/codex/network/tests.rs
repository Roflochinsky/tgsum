use super::*;
use crate::egress::Failure;
use crate::hostile_files;
use crate::{PreparedContext, Termination};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
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
use tgsum_core::analysis::{AnalysisSpec, Completion};
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

fn review_spec(model: &str) -> AnalysisSpec {
    AnalysisSpec {
        agent: "codex".into(),
        agent_version: "0.155.1".into(),
        isolation_profile: LINUX_EGRESS_PROFILE.into(),
        destination: "api.openai.com".into(),
        model: model.into(),
        recipe: "synthetic-summary".into(),
        recipe_version: 1,
    }
}

#[test]
fn analysis_job_binds_context_model_receiver_version_and_profile_before_launch() {
    use super::super::AnalysisJob;
    let fixture = context();
    let request = CodexRequest::prepare(
        &fixture.context,
        "fixture-model",
        "Summary",
        &schema(),
        &Cancellation::default(),
    )
    .unwrap();
    let store = ProjectStore::new(fixture.root.path());
    let project = store.open(&fixture.project).unwrap();
    for field in [
        "model",
        "destination",
        "agent_version",
        "isolation_profile",
        "agent",
    ] {
        let mut spec = review_spec(request.model());
        match field {
            "model" => spec.model = "other-model".into(),
            "destination" => spec.destination = "other.example".into(),
            "agent_version" => spec.agent_version = "0.0.0".into(),
            "isolation_profile" => spec.isolation_profile = "unqualified".into(),
            _ => spec.agent = "other".into(),
        }
        let ticket = store
            .begin_analysis(
                &fixture.project,
                &fixture.bundle,
                project.revision,
                spec,
                || false,
            )
            .unwrap();
        assert!(AnalysisJob::new(&request, &ticket).is_err());
    }
    let second = store
        .prepare_bundle(
            &fixture.project,
            project.revision,
            BundleOptions {
                redact_candidates: true,
            },
            || false,
        )
        .unwrap();
    let ticket = store
        .begin_analysis(
            &fixture.project,
            &second.bundle_id,
            project.revision,
            review_spec(request.model()),
            || false,
        )
        .unwrap();
    assert!(AnalysisJob::new(&request, &ticket).is_err());
    let ticket = store
        .begin_analysis(
            &fixture.project,
            &fixture.bundle,
            project.revision,
            review_spec(request.model()),
            || false,
        )
        .unwrap();
    let job = AnalysisJob::new(&request, &ticket).unwrap();
    let info = QualifiedAdapter {
        id: "codex".into(),
        version_output: String::from_utf8(VERSION_STDOUT.into()).unwrap(),
        isolation_profile: LINUX_EGRESS_PROFILE,
        authentication: AuthAvailability::Unknown,
    };
    job.check(&info, &Cancellation::default()).unwrap();
    store.cancel_analysis(&ticket).unwrap();
    assert!(job.check(&info, &Cancellation::default()).is_err());
    assert_eq!(store.open(&fixture.project).unwrap(), project);
}

#[test]
fn gateway_failures_cannot_be_hidden_by_valid_json_and_exit_zero() {
    use super::super::DecodeError;
    let mut output = NetworkOutput {
        process: RunOutput {
            termination: Termination::Exited,
            exit_code: Some(0),
            stdout: include_bytes!("../../../tests/fixtures/codex-success.jsonl").to_vec(),
            stderr: vec![],
        },
        gateway: Report::default(),
        destination: Destination::OpenAiApi,
    };
    assert_eq!(
        output
            .decode::<Answer>(|_| panic!("no transport"))
            .unwrap_err(),
        NetworkDecodeError::MissingTransport
    );
    output.gateway.completed = 1;
    output.gateway.admitted_bytes = 1;
    for failure in [
        Failure::ConnectRequest,
        Failure::ServerName,
        Failure::EncryptedHello,
        Failure::PrivateAddress,
        Failure::Budget,
        Failure::Timeout,
        Failure::Cancelled,
        Failure::Io,
        Failure::Worker,
    ] {
        output.gateway.failures = vec![failure];
        assert_eq!(
            output
                .decode::<Answer>(|_| panic!("rejected gateway"))
                .unwrap_err(),
            NetworkDecodeError::Gateway(failure)
        );
    }
    output.process.termination = Termination::Cancelled;
    assert!(matches!(
        output
            .decode::<Answer>(|_| panic!("cancelled"))
            .unwrap_err(),
        NetworkDecodeError::Response(DecodeError::Process {
            termination: Termination::Cancelled,
            ..
        })
    ));
}

#[test]
fn offline_runner_rejects_network_profile_before_opening_runtime() {
    for profile in [LINUX_EGRESS_PROFILE, crate::claude::LINUX_EGRESS_PROFILE] {
        let result = crate::OfflineRunner::qualify(
            AdapterContract {
                id: "codex".into(),
                isolation_profile: profile.into(),
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

#[derive(Clone, Copy, Debug)]
enum FixtureAuth {
    None,
    ApiKey,
    ChatGpt,
    ExpiredChatGpt,
    StaleChatGpt,
}

impl FixtureAuth {
    fn destination(self) -> Destination {
        match self {
            Self::None | Self::ApiKey => Destination::OpenAiApi,
            _ => Destination::ChatGpt,
        }
    }

    fn data(self) -> Option<Value> {
        match self {
            Self::None => None,
            Self::ApiKey => Some(
                json!({"auth_mode":"apikey", "OPENAI_API_KEY":"tgsum-synthetic-not-a-real-key"}),
            ),
            _ => {
                // Unsigned synthetic tokens, like the tagged upstream auth suite.
                // These bytes are only sent to a loopback canned TLS server.
                let jwt = |claims: Value| {
                    format!(
                        "e30.{}.c3ludGhldGlj",
                        URL_SAFE_NO_PAD.encode(claims.to_string())
                    )
                };
                let expiration = match self {
                    Self::ExpiredChatGpt => 1,
                    _ => {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                            + 86400
                    }
                };
                let access = if matches!(self, Self::StaleChatGpt) {
                    "tgsum-synthetic-opaque-chatgpt-token".into()
                } else {
                    jwt(json!({"sub":"synthetic-user", "exp":expiration}))
                };
                Some(json!({"auth_mode":"chatgpt", "tokens":{
                    "id_token":jwt(json!({"sub":"synthetic-user"})),
                    "access_token":access,
                    "refresh_token":"tgsum-synthetic-never-refresh",
                    "account_id":"synthetic-account"
                },
                // Fresh JWT expiry takes precedence over the old cache date;
                // opaque access tokens exercise the last_refresh fallback.
                "last_refresh":"2020-01-01T00:00:00Z"}))
            }
        }
    }
}

enum FixtureRequest {
    Metadata(&'static str),
    Inference(Value),
}

fn request(stream: &mut impl Read, auth: FixtureAuth, auth_data: Option<&Value>) -> FixtureRequest {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 32768);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
    }
    let header = String::from_utf8(header).unwrap();
    let path = if auth.destination() == Destination::ChatGpt {
        "/backend-api/codex/responses"
    } else {
        "/v1/responses"
    };
    let models = auth.destination() == Destination::ChatGpt
        && header.starts_with("GET /backend-api/codex/models?client_version=0.155.1 HTTP/1.1\r\n");
    let settings = auth.destination() == Destination::ChatGpt
        && header.starts_with("GET /backend-api/wham/settings/user HTTP/1.1\r\n");
    assert!(
        models || settings || header.starts_with(&format!("POST {path} HTTP/1.1\r\n")),
        "unexpected request line: {:?}",
        header.lines().next()
    );
    let field = |name: &str| {
        header
            .lines()
            .filter_map(|l| l.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    };
    assert_eq!(field("host"), Some(auth.destination().host()));
    let supplied = field("authorization");
    let expected = auth_data.map(|data| {
        format!(
            "Bearer {}",
            match auth {
                FixtureAuth::ApiKey => data["OPENAI_API_KEY"].as_str().unwrap(),
                _ => data["tokens"]["access_token"].as_str().unwrap(),
            }
        )
    });
    // Don't print token values even when a fixture assertion fails.
    assert!(supplied == expected.as_deref());
    if auth.destination() == Destination::ChatGpt {
        assert_eq!(field("chatgpt-account-id"), Some("synthetic-account"));
    }
    if models {
        return FixtureRequest::Metadata(r#"{"models":[]}"#);
    }
    if settings {
        return FixtureRequest::Metadata(r#"{"commit_attribution_enabled":false}"#);
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
    FixtureRequest::Inference(serde_json::from_slice(&bytes).unwrap())
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
        RuntimeFile {
            source: directory.join("hostile-host.json"),
            guest: "/runtime/hostile-host.json".into(),
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
    for auth in [FixtureAuth::None, FixtureAuth::ApiKey] {
        qualify("success", auth);
    }
}

#[test]
#[ignore = "requires Linux namespace backend and explicit built relay/HTTPS fixture/installed Codex paths"]
fn installed_codex_https_rejects_unknown_ca_and_refresh_and_cleans_up_cancel_timeout() {
    for (mode, auth) in [
        ("wrong-ca", FixtureAuth::None),
        ("refresh-probe", FixtureAuth::None),
        ("cancel", FixtureAuth::None),
        ("timeout", FixtureAuth::None),
        ("unauthorized", FixtureAuth::ApiKey),
        ("receiver-mismatch", FixtureAuth::ApiKey),
    ] {
        qualify(mode, auth);
    }
}

#[test]
#[ignore = "requires Linux namespace backend and explicit built relay/HTTPS fixture/installed Codex paths"]
fn installed_codex_https_chatgpt_success_expiry_stale_cache_and_revocation() {
    for (mode, auth) in [
        ("success", FixtureAuth::ChatGpt),
        ("expired", FixtureAuth::ExpiredChatGpt),
        ("stale", FixtureAuth::StaleChatGpt),
        ("unauthorized", FixtureAuth::ChatGpt),
        ("receiver-mismatch", FixtureAuth::ChatGpt),
    ] {
        qualify(mode, auth);
    }
}

fn qualify(mode: &str, auth: FixtureAuth) {
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
    let certificate =
        rcgen::generate_simple_self_signed(vec![auth.destination().host().into()]).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
        )
        .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let _host_socket = hostile_files::stage(temporary.path(), fixture.root.path());
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
    let destination = if mode == "receiver-mismatch" {
        if auth.destination() == Destination::ChatGpt {
            Destination::OpenAiApi
        } else {
            Destination::ChatGpt
        }
    } else {
        auth.destination()
    };
    let store = ProjectStore::new(fixture.root.path());
    let project = store.open(&fixture.project).unwrap();
    let ticket = store
        .begin_analysis(
            &fixture.project,
            &fixture.bundle,
            project.revision,
            AnalysisSpec {
                agent: "codex".into(),
                agent_version: "0.155.1".into(),
                isolation_profile: runner.info().isolation_profile.into(),
                destination: destination.host().into(),
                model: request_data.model().into(),
                recipe: "synthetic-summary".into(),
                recipe_version: 1,
            },
            || false,
        )
        .unwrap();
    let job = super::super::AnalysisJob::new(&request_data, &ticket).unwrap();
    job.check(runner.info(), &cancel).unwrap();
    let auth_path = temporary.path().join("auth.json");
    let auth_data = auth.data();
    let secrets: Vec<String> = auth_data
        .as_ref()
        .into_iter()
        .flat_map(|data| {
            [
                "/OPENAI_API_KEY",
                "/tokens/id_token",
                "/tokens/access_token",
                "/tokens/refresh_token",
            ]
            .into_iter()
            .filter_map(|path| {
                data.pointer(path)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
        })
        .collect();
    let auth_bytes = auth_data.as_ref().map(|v| serde_json::to_vec(v).unwrap());
    let auth_file = if let Some(bytes) = &auth_bytes {
        fs::write(&auth_path, bytes).unwrap();
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
        destination,
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
                        let data = match request(&mut tls, auth, auth_data.as_ref()) {
                            FixtureRequest::Metadata(body) => {
                                write!(tls,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                                tls.conn.send_close_notify(); tls.flush().unwrap(); continue;
                            },
                            FixtureRequest::Inference(data) => data,
                        };
                        count += 1;
                        seen.store(true, Ordering::Relaxed);
                        if mode == "unauthorized" {
                            let body = r#"{"error":{"message":"PRIVATE_SYNTHETIC_AUTH_FAILURE","type":"invalid_request_error","code":"invalid_api_key"}}"#;
                            write!(tls,"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                            tls.conn.send_close_notify(); tls.flush().unwrap(); continue;
                        }
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
            destination,
            limits,
            &cancel,
            gateway,
        );
        stopped.store(true, Ordering::Relaxed);
        (output.unwrap(), server.join().unwrap())
    });
    assert!(!socket_path.exists());
    eprintln!("fixture {mode}/{auth:?}: inference={count}, connections={connections}, exit={:?}, gateway={:?}",output.process.exit_code,output.gateway.failures);
    if let Some(bytes) = auth_bytes {
        assert!(
            fs::read(&auth_path).unwrap() == bytes,
            "selected synthetic auth changed"
        );
    }
    for bytes in [&output.process.stdout, &output.process.stderr] {
        for secret in &secrets {
            assert!(
                !String::from_utf8_lossy(bytes).contains(secret),
                "synthetic credential reached diagnostics"
            );
        }
    }
    if mode == "success" {
        assert!(output.process.process_succeeded(), "{output:?}");
        assert_eq!(count, 1);
        assert!(output.gateway.accepted > 0);
        let completed = job
            .finish(output, &cancel, |answer: &Answer| {
                assert_eq!(answer.evidence.len(), 1);
                answer
                    .evidence
                    .iter()
                    .map(|r| {
                        let (id, revision) = r
                            .split_once('@')
                            .ok_or_else(|| std::io::Error::other("invalid reference"))?;
                        let reference = EvidenceRef {
                            id: id.into(),
                            revision: revision.into(),
                        };
                        store.resolve_evidence(&fixture.project, &fixture.bundle, &reference)?;
                        Ok(reference)
                    })
                    .collect()
            })
            .unwrap();
        assert_eq!(completed.result.value.summary, "Canned HTTPS result");
        let saved = store
            .read_analysis(&fixture.project, &ticket.request().run_id)
            .unwrap();
        assert_eq!(saved.committed_revision, Some(completed.committed_revision));
        assert_eq!(
            store.open(&fixture.project).unwrap().baselines[0].analysis_id,
            completed.run_id
        );
    } else {
        // The CLI can return success after denied proactive refresh. The
        // managed adapter must still reject the dirty gateway report.
        if !matches!(mode, "expired" | "stale") {
            assert!(!output.process.process_succeeded(), "{output:?}");
        }
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
        } else if matches!(mode, "refresh-probe" | "receiver-mismatch") {
            if mode == "refresh-probe" {
                assert_eq!(output.process.exit_code, Some(42));
            }
            assert!(output.gateway.failures.contains(&Failure::ConnectRequest));
            assert_eq!(count, 0);
            assert_eq!(connections, 0);
        } else if matches!(mode, "expired" | "stale") {
            assert!(
                output.gateway.failures.contains(&Failure::ConnectRequest),
                "{output:?}"
            );
        } else if mode == "unauthorized" {
            assert!(observed.load(Ordering::Relaxed));
            if auth.destination() == Destination::ChatGpt {
                assert_eq!(count, 2); // guarded auth reload, then denied refresh
                assert!(
                    output.gateway.failures.contains(&Failure::ConnectRequest),
                    "{output:?}"
                );
            } else {
                assert_eq!(count, 1);
            }
        } else {
            assert!(
                connections > 0,
                "must reach TLS rejection, not an earlier failure"
            );
            assert_eq!(count, 0);
        }
        let error = job
            .finish(output, &cancel, |_answer: &Answer| {
                panic!("validator must not see failed output")
            })
            .unwrap_err();
        assert!(!format!("{error} {error:?}").contains("PRIVATE_SYNTHETIC_AUTH_FAILURE"));
        let saved = store
            .read_analysis(&fixture.project, &ticket.request().run_id)
            .unwrap();
        assert!(matches!(
            saved.completion,
            Some(Completion::Failed { .. } | Completion::Cancelled)
        ));
        assert!(saved.committed_revision.is_none());
        assert_eq!(store.open(&fixture.project).unwrap(), project);
    }
}
