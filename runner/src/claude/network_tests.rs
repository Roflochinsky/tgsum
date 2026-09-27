//! Native synthetic qualification of network runner and durable lifecycle.
use super::*;
use crate::egress::{Destination, InferenceGateway, Limits};
use crate::hostile_files;
use crate::{Cancellation, PreparedContext, RuntimeFile};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tgsum_core::{
    bundle::BundleOptions,
    project::{ProjectChange, ProjectSource, ProjectStore},
    snapshot::SourceScope,
};

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
        Cursor::new(r#"{"id":77,"name":"Selected","messages":[{"id":1,"date":"2026-06-18T10:00:00","text":"Synthetic selected message /help @/runtime/peer-address"}]}"#)).unwrap();
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
fn review_binds_agent_version_model_receiver_profile_bundle_and_recipe() {
    let fixture = context();
    let cancel = Cancellation::default();
    let request = RecipeRequest::prepare(
        &fixture.context,
        "claude-sonnet-4-6",
        tgsum_core::recipe::Recipe::Summary,
        &cancel,
    )
    .unwrap();
    let store = ProjectStore::new(fixture.root.path());
    let project = store.open(&fixture.project).unwrap();
    for field in [
        "agent",
        "version",
        "model",
        "receiver",
        "profile",
        "bundle",
        "recipe",
        "recipe-version",
        "valid",
    ] {
        let mut spec = review_spec(request.request().model());
        spec.recipe = request.recipe().id().into();
        spec.recipe_version = tgsum_core::recipe::RECIPE_VERSION;
        match field {
            "agent" => spec.agent = "codex".into(),
            "version" => spec.agent_version = "0.0.0".into(),
            "model" => spec.model = "unreviewed".into(),
            "receiver" => spec.destination = "other.test".into(),
            "profile" => spec.isolation_profile = LINUX_OFFLINE_PROFILE.into(),
            "recipe" => spec.recipe = "other".into(),
            "recipe-version" => spec.recipe_version += 1,
            _ => {}
        }
        let bundle = if field == "bundle" {
            store
                .prepare_bundle(
                    &fixture.project,
                    project.revision,
                    BundleOptions {
                        redact_candidates: true,
                    },
                    || false,
                )
                .unwrap()
                .bundle_id
        } else {
            fixture.bundle.clone()
        };
        let ticket = store
            .begin_analysis(&fixture.project, &bundle, project.revision, spec, || false)
            .unwrap();
        if field == "valid" {
            let job = request.job(&ticket).unwrap();
            let info = crate::QualifiedAdapter {
                id: "claude".into(),
                version_output: String::from_utf8(VERSION_STDOUT.to_vec()).unwrap(),
                isolation_profile: LINUX_EGRESS_PROFILE,
                authentication: crate::AuthAvailability::Unknown,
            };
            job.check(&info, &cancel).unwrap();
            store.cancel_analysis(&ticket).unwrap();
            assert!(job.check(&info, &cancel).is_err());
        } else {
            assert!(request.job(&ticket).is_err(), "{field}");
        }
    }
    assert_eq!(store.open(&fixture.project).unwrap(), project);
}

#[test]
fn result_rejection_and_stale_project_never_advance_the_baseline() {
    use tgsum_core::analysis::Completion;
    for mode in [
        "destination",
        "evidence",
        "dirty-transport",
        "cancel",
        "stale",
    ] {
        let fixture = context();
        let cancel = Cancellation::default();
        let request = ClaudeRequest::prepare(
            &fixture.context,
            "claude-sonnet-4-6",
            "Synthetic",
            &schema(),
            &cancel,
        )
        .unwrap();
        let store = ProjectStore::new(fixture.root.path());
        let project = store.open(&fixture.project).unwrap();
        let ticket = store
            .begin_analysis(
                &fixture.project,
                &fixture.bundle,
                project.revision,
                review_spec(request.model()),
                || false,
            )
            .unwrap();
        let input: Value = serde_json::from_slice(&request.invocation().unwrap().stdin).unwrap();
        let reference = input["untrusted_documents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|d| d["text"].as_str().unwrap().lines())
            .find_map(|line| line.strip_prefix("## Evidence "))
            .unwrap();
        let output =
            include_str!("../../tests/fixtures/claude-success.jsonl").replace("m1@r1", reference);
        let mut output = NetworkOutput {
            process: crate::RunOutput {
                termination: crate::Termination::Exited,
                exit_code: Some(0),
                stdout: output.into_bytes(),
                stderr: vec![],
            },
            gateway: crate::egress::Report {
                completed: 1,
                admitted_bytes: 1,
                ..Default::default()
            },
            destination: Destination::Anthropic,
        };
        let expected = match mode {
            "destination" => {
                output.destination = Destination::OpenAiApi;
                project
            }
            "dirty-transport" => {
                output
                    .gateway
                    .failures
                    .push(crate::egress::Failure::ConnectRequest);
                project
            }
            "cancel" => {
                cancel.cancel();
                project
            }
            "stale" => store
                .update(
                    &fixture.project,
                    project.revision,
                    ProjectChange::Rename("Changed during inference".into()),
                )
                .unwrap(),
            _ => project,
        };
        let result = AnalysisJob::new(&request, &ticket).unwrap().finish(
            output,
            &cancel,
            |answer: &Value| {
                if mode == "evidence" {
                    return Err(std::io::Error::other("not in selected scope"));
                }
                assert_eq!(answer["evidence"], json!([reference]));
                let (id, revision) = reference.split_once('@').unwrap();
                Ok(vec![tgsum_core::bundle::EvidenceRef {
                    id: id.into(),
                    revision: revision.into(),
                }])
            },
        );
        assert!(result.is_err(), "{mode}");
        assert_eq!(store.open(&fixture.project).unwrap(), expected);
        let saved = store
            .read_analysis(&fixture.project, &ticket.request().run_id)
            .unwrap();
        assert!(saved.committed_revision.is_none());
        if mode == "stale" {
            assert!(
                matches!(saved.completion, Some(Completion::Validated { .. })),
                "validated artifact retained for explicit recovery"
            );
        } else {
            assert!(matches!(
                saved.completion,
                Some(Completion::Failed { .. } | Completion::Cancelled)
            ));
        }
    }
}

fn runtime(directory: &Path, cancel: &Cancellation) -> ClaudeNetworkRunner {
    let mut files: Vec<_> = [
        "librt.so.1",
        "libc.so.6",
        "libpthread.so.0",
        "libdl.so.2",
        "libm.so.6",
        "libgcc_s.so.1",
        "ld-linux-x86-64.so.2",
    ]
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
    for (source, guest) in [
        (
            PathBuf::from(
                std::env::var_os("TGSUM_CLAUDE_TEST_BINARY")
                    .expect("explicit native Claude2.1.280"),
            ),
            "/runtime/installed-claude",
        ),
        (directory.join("ca.pem"), "/runtime/provider-ca.pem"),
        (directory.join("peer-address"), "/runtime/peer-address"),
        (
            directory.join("hostile-host.json"),
            "/runtime/hostile-host.json",
        ),
    ] {
        files.push(RuntimeFile {
            source,
            guest: guest.into(),
        });
    }
    ClaudeNetworkRunner::qualify(
        std::env::var_os("TGSUM_CLAUDE_RELAY_TEST_BINARY")
            .expect("explicit Claude relay")
            .into(),
        std::env::var_os("TGSUM_CLAUDE_HTTPS_FIXTURE_BINARY")
            .expect("explicit HTTPS fixture")
            .into(),
        files,
        EndpointPolicy::fixture(&directory.join("endpoint"), cancel).unwrap(),
        cancel,
    )
    .unwrap()
}

fn request(stream: &mut impl Read) -> (String, Value) {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut one = [0];
        stream.read_exact(&mut one).unwrap();
        bytes.push(one[0]);
        assert!(bytes.len() < 32768);
    }
    let header = String::from_utf8(bytes).unwrap().to_lowercase();
    let path = header.lines().next().unwrap().to_owned();
    assert!(!header.contains("x-api-key:"));
    if !path.starts_with("head ") {
        assert!(
            header
                .lines()
                .any(|line| line == "authorization: bearer tgsum-synthetic-access-token"),
            "missing synthetic bearer"
        );
    }
    assert!(!header.contains("content-encoding:"));
    let length: usize = header
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map(|n| n.trim().parse().unwrap())
        .unwrap_or(0);
    assert!(length <= 2 * 1024 * 1024);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    (
        path,
        if length == 0 {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

fn events(answer: &Value) -> String {
    let records = [
        json!({"type":"message_start","message":{"id":"msg_fixture","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-6","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_fixture","name":"StructuredOutput","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":answer.to_string()}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    records
        .iter()
        .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
        .collect()
}

#[test]
#[ignore = "requires namespace backend and explicit Claude/relay/HTTPS fixture binaries; synthetic OAuth only"]
fn installed_claude_https_default_host_with_selected_synthetic_oauth() {
    for mode in ["success", "policy-200", "policy-204", "policy-404"] {
        qualify(mode);
    }
}

#[test]
#[ignore = "requires namespace backend and explicit Claude/relay/HTTPS fixture binaries; synthetic OAuth only"]
fn installed_claude_https_rejects_ca_refresh_receiver_and_reaps_cancel_timeout() {
    for mode in [
        "wrong-ca",
        "expired",
        "unauthorized",
        "receiver-mismatch",
        "cancel",
        "timeout",
    ] {
        qualify(mode);
    }
}

#[test]
#[ignore = "requires namespace backend and explicit Claude/relay/HTTPS fixture binaries; synthetic OAuth only"]
fn installed_claude_https_managed_fetch_failures_prevent_inference() {
    for mode in [
        "policy-403",
        "policy-304",
        "policy-version",
        "policy-env",
        "policy-fallback",
    ] {
        qualify(mode);
    }
}

#[test]
#[ignore = "requires namespace backend and explicit Claude/relay/HTTPS fixture binaries; synthetic endpoint policy only"]
fn installed_claude_https_preserves_endpoint_policy_and_rechecks_before_launch() {
    for mode in [
        "endpoint-version",
        "endpoint-fragment",
        "policy-force-fragment",
        "endpoint-changed",
    ] {
        qualify(mode);
    }
}

fn qualify(mode: &str) {
    use rustls::{pki_types::PrivatePkcs8KeyDer, ServerConfig, ServerConnection, StreamOwned};
    let context = context();
    let cancel = Cancellation::default();
    let request_data = ClaudeRequest::prepare(
        &context.context,
        "claude-sonnet-4-6",
        "Synthetic HTTPS summary with evidence",
        &schema(),
        &cancel,
    )
    .unwrap();
    let input: Value = serde_json::from_slice(&request_data.invocation().unwrap().stdin).unwrap();
    let reference = input["untrusted_documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["text"].as_str().unwrap().lines())
        .find_map(|l| l.strip_prefix("## Evidence "))
        .unwrap()
        .to_owned();
    let certificate =
        rcgen::generate_simple_self_signed(vec![Destination::Anthropic.host().into()]).unwrap();
    let config = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.cert.der().clone()],
                PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
            )
            .unwrap(),
    );
    let temp = tempfile::tempdir().unwrap();
    let _host_socket = hostile_files::stage(temp.path(), context.root.path());
    let trust = if mode == "wrong-ca" {
        rcgen::generate_simple_self_signed(vec!["unrelated.test".into()])
            .unwrap()
            .cert
            .pem()
    } else {
        certificate.cert.pem()
    };
    fs::write(temp.path().join("ca.pem"), trust).unwrap();
    let listener = TcpListener::bind("127.0.0.2:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    fs::write(temp.path().join("peer-address"), address.to_string()).unwrap();
    if mode.starts_with("endpoint-") || mode == "policy-force-fragment" {
        let endpoint = temp.path().join("endpoint");
        fs::create_dir(&endpoint).unwrap();
        let settings = if mode == "endpoint-version" {
            json!({"requiredMinimumVersion":"99.0.0"})
        } else {
            json!({"forceRemoteSettingsRefresh":false})
        };
        fs::write(endpoint.join("managed-settings.json"), settings.to_string()).unwrap();
        if matches!(mode, "endpoint-fragment" | "policy-force-fragment") {
            fs::create_dir(endpoint.join("managed-settings.d")).unwrap();
            let settings = if mode == "endpoint-fragment" {
                json!({"requiredMinimumVersion":"99.0.0","forceRemoteSettingsRefresh":false})
            } else {
                json!({"forceRemoteSettingsRefresh":false})
            };
            fs::write(
                endpoint.join("managed-settings.d/zzzz-last.json"),
                settings.to_string(),
            )
            .unwrap();
        }
    }
    let runner = runtime(temp.path(), &cancel);
    if mode == "endpoint-changed" {
        fs::write(temp.path().join("endpoint/managed-settings.json"), "{}").unwrap();
        assert!(matches!(
            runner
                .backend
                .run(temp.path(), &version_probe(), run_limits(), &cancel),
            Err(crate::RunnerError::ExportOnly(
                "Claude managed settings changed; prepare and review again"
            ))
        ));
        return;
    }
    let store = ProjectStore::new(context.root.path());
    let project = store.open(&context.project).unwrap();
    let ticket = store
        .begin_analysis(
            &context.project,
            &context.bundle,
            project.revision,
            review_spec(request_data.model()),
            || false,
        )
        .unwrap();
    let job = AnalysisJob::new(&request_data, &ticket).unwrap();
    job.check(runner.info(), &cancel).unwrap();
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 86_400_000;
    let expiry = if mode == "expired" {
        expiry - 86_400_001
    } else {
        expiry
    };
    let subscription = if mode.starts_with("policy-") {
        "team"
    } else {
        "pro"
    };
    let auth_bytes=json!({"claudeAiOauth":{"accessToken":"tgsum-synthetic-access-token","refreshToken":"tgsum-synthetic-refresh-token","expiresAt":expiry,"scopes":["user:inference"],"subscriptionType":subscription,"rateLimitTier":null}}).to_string();
    let auth_path = temp.path().join(".credentials.json");
    fs::write(&auth_path, &auth_bytes).unwrap();
    fs::set_permissions(&auth_path, fs::Permissions::from_mode(0o600)).unwrap();
    let auth = SelectedAuthFile::select(&auth_path).unwrap();
    let gateway = InferenceGateway::fixture(
        if mode == "receiver-mismatch" {
            Destination::OpenAiApi
        } else {
            Destination::Anthropic
        },
        Limits {
            duration: Duration::from_secs(20),
            ..Limits::default()
        },
        &cancel,
        address,
    );
    let gateway_path = gateway.socket_path().to_owned();
    let stopped = Arc::new(AtomicBool::new(false));
    let (output, paths) = std::thread::scope(|scope| {
        let stop = stopped.clone();
        let reference = reference.clone();
        let signal = cancel.clone();
        let server=scope.spawn(move||{
            let mut paths=Vec::new();
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((socket,_))=>{
                        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap(); socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                        let mut tls=StreamOwned::new(ServerConnection::new(config.clone()).unwrap(),socket);
                        if mode=="wrong-ca" { assert!(tls.conn.complete_io(&mut tls.sock).is_err()); continue; }
                        let (path,body)=request(&mut tls);
                        eprintln!("Claude TLS fixture: {path}");
                        paths.push(path.clone()); assert!(paths.len()<=8);
                        let (status,kind,response)=if path=="post /v1/messages?beta=true http/1.1" {
                            assert_eq!(body["model"],"claude-sonnet-4-6"); assert_eq!(body["stream"],true);
                            assert_eq!(body["tools"].as_array().unwrap().len(),1);
                            assert_eq!(body["tools"][0]["name"],"StructuredOutput"); assert_eq!(body["tools"][0]["input_schema"],schema());
                            assert!(body.to_string().contains(&reference));
                            assert!(body.to_string().contains("/help @/runtime/peer-address"));
                            assert!(!body.to_string().contains(&address.to_string()));
                            if mode=="cancel" { signal.cancel(); continue; }
                            if mode=="timeout" { while !stop.load(Ordering::Relaxed) {std::thread::sleep(Duration::from_millis(2));} continue; }
                            if mode=="unauthorized" {
                                ("401 Unauthorized","application/json",json!({"type":"error","error":{"type":"authentication_error","message":"Synthetic expired access"}}).to_string())
                            } else { ("200 OK","text/event-stream",events(&json!({"summary":"Canned HTTPS result","evidence":[reference]}))) }
                        } else if path=="head /api/hello http/1.1" {
                            ("200 OK","application/json",String::new())
                        } else if path=="get /api/claude_code/settings http/1.1" {
                            assert!(mode.starts_with("policy-"));
                            let status=match mode { "policy-200"|"policy-version"|"policy-env"|"policy-fallback"=>"200 OK", "policy-204"=>"204 No Content", "policy-404"=>"404 Not Found", "policy-403"|"policy-force-fragment"=>"403 Forbidden", "policy-304"=>"304 Not Modified", _=>panic!("unexpected policy fixture") };
                            let response=if matches!(mode,"policy-200"|"policy-version"|"policy-env"|"policy-fallback") {
                                let settings=match mode {
                                    "policy-version"=>json!({"requiredMinimumVersion":"99.0.0"}),
                                    "policy-env"=>json!({"env":{"TGSUM_SYNTHETIC_UNREVIEWED":"value"}}),
                                    "policy-fallback"=>json!({"fallbackModel":["haiku"]}),
                                    _=>json!({"availableModels":["sonnet"],"forceRemoteSettingsRefresh":false})
                                };
                                json!({"uuid":"937e345b-6d45-45ef-a4f1-6266ce843b60","checksum":"fixture-checksum","settings":settings}).to_string()
                            } else {String::new()};
                            (status,"application/json",response)
                        } else if path=="get /api/claude_code/policy_limits http/1.1" {
                            assert!(mode.starts_with("policy-"));
                            ("200 OK","application/json",json!({"restrictions":{},"compliance_taints":[],"monitoring_notice":null,"defaults":{}}).to_string())
                        } else { panic!("unexpected synthetic peer path: {path}"); };
                        write!(tls,"HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
                        tls.conn.send_close_notify(); tls.flush().unwrap();
                    },
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),
                    Err(e)=>panic!("fixture accept: {e}"),
                }
            }
            paths
        });
        let output = runner.run_with_gateway(
            &request_data,
            &auth,
            RunLimits {
                timeout: Duration::from_secs(if mode == "timeout" { 3 } else { 15 }),
                ..run_limits()
            },
            &cancel,
            gateway,
        );
        stopped.store(true, Ordering::Relaxed);
        (output.unwrap(), server.join().unwrap())
    });
    let NetworkOutput {
        process: output,
        gateway: report,
        destination,
    } = output;
    assert!(!gateway_path.exists());
    eprintln!(
        "Claude {mode}: exit={:?}, termination={:?}, gateway={report:?}",
        output.exit_code, output.termination
    );
    assert_eq!(fs::read_to_string(auth_path).unwrap(), auth_bytes);
    for bytes in [&output.stdout, &output.stderr] {
        for secret in [
            "tgsum-synthetic-access-token",
            "tgsum-synthetic-refresh-token",
        ] {
            assert!(!String::from_utf8_lossy(bytes).contains(secret));
        }
    }
    let count = paths
        .iter()
        .filter(|p| p.starts_with("post /v1/messages"))
        .count();
    let good = matches!(mode, "success" | "policy-200" | "policy-204" | "policy-404");
    if good {
        assert!(
            output.process_succeeded(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(count, 1);
        assert!(report.failures.is_empty());
        assert!(report.completed > 0 && report.admitted_bytes > 0);
    } else {
        if mode != "expired" {
            assert!(!output.process_succeeded());
        }
        if matches!(mode, "expired" | "receiver-mismatch") {
            assert!(report
                .failures
                .contains(&crate::egress::Failure::ConnectRequest));
        }
        if mode == "cancel" {
            assert_eq!(output.termination, crate::Termination::Cancelled);
            assert_eq!(count, 1);
        } else if mode == "timeout" {
            assert_eq!(output.termination, crate::Termination::TimedOut);
            assert_eq!(count, 1);
        } else if mode == "unauthorized" {
            assert_eq!(count, 1);
            assert_eq!(output.exit_code, Some(1));
        } else if mode != "expired" {
            assert_eq!(count, 0);
        }
    }
    let combined = NetworkOutput {
        process: output,
        gateway: report,
        destination,
    };
    let decoded = combined.decode::<Value>("claude-sonnet-4-6", |answer| {
        answer["evidence"] == json!([reference])
    });
    if good {
        assert_eq!(decoded.unwrap().value["summary"], "Canned HTTPS result");
    } else if mode == "expired" {
        assert!(
            combined.process.process_succeeded(),
            "canned fresh response after denied refresh"
        );
        assert!(matches!(
            decoded.unwrap_err(),
            NetworkDecodeError::Gateway(crate::egress::Failure::ConnectRequest)
        ));
    } else {
        assert!(decoded.is_err());
    }
    if mode.starts_with("endpoint-") {
        assert_eq!(combined.process.termination, crate::Termination::Exited);
        assert_eq!(combined.process.exit_code, Some(1));
    }
    if mode.starts_with("policy-") {
        assert_eq!(
            paths.first().map(String::as_str),
            Some("get /api/claude_code/settings http/1.1")
        );
        // Failure is a normal CLI exit, not a timeout disguised as rejection.
        if !good {
            assert_eq!(combined.process.termination, crate::Termination::Exited);
            assert_eq!(combined.process.exit_code, Some(1));
        }
    }
    let completed = job.finish(combined, &cancel, |answer: &Value| {
        let refs = answer["evidence"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("evidence missing"))?;
        refs.iter()
            .map(|reference| {
                let (id, revision) = reference
                    .as_str()
                    .and_then(|r| r.split_once('@'))
                    .ok_or_else(|| std::io::Error::other("invalid evidence"))?;
                let reference = tgsum_core::bundle::EvidenceRef {
                    id: id.into(),
                    revision: revision.into(),
                };
                store.resolve_evidence(&context.project, &context.bundle, &reference)?;
                Ok(reference)
            })
            .collect()
    });
    let saved = store
        .read_analysis(&context.project, &ticket.request().run_id)
        .unwrap();
    if good {
        let completed = completed.unwrap();
        assert_eq!(saved.committed_revision, Some(completed.committed_revision));
        assert_eq!(
            store.open(&context.project).unwrap().baselines[0].analysis_id,
            completed.run_id
        );
    } else {
        assert!(completed.is_err());
        assert!(matches!(
            saved.completion,
            Some(
                tgsum_core::analysis::Completion::Failed { .. }
                    | tgsum_core::analysis::Completion::Cancelled
            )
        ));
        assert_eq!(store.open(&context.project).unwrap(), project);
    }
}

fn review_spec(model: &str) -> tgsum_core::analysis::AnalysisSpec {
    tgsum_core::analysis::AnalysisSpec {
        agent: "claude".into(),
        agent_version: VERSION.into(),
        isolation_profile: LINUX_EGRESS_PROFILE.into(),
        destination: "api.anthropic.com".into(),
        model: model.into(),
        recipe: "synthetic-summary".into(),
        recipe_version: 1,
    }
}
