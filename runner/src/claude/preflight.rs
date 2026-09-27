//! Same-process control handshake. No corpus reaches Claude before settings pass.
use super::{wire::UniqueValue, MAX_EVENT_BYTES, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES};
use crate::Cancellation;
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    io::{self, BufRead, BufReader, Read, Write},
    os::fd::AsFd,
    process::{ChildStdin, ChildStdout},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
const INVALID: &str = "Claude preflight rejected incompatible settings or control protocol";

pub(crate) fn arguments(args: &mut [OsString]) -> Result<String> {
    let models: Vec<_> = args.windows(2).filter(|a| a[0] == "--model").collect();
    let model = models
        .first()
        .and_then(|a| a[1].to_str())
        .ok_or(INVALID)?
        .to_owned();
    if models.len() != 1 || model.is_empty() || model.len() > 128 {
        return Err(INVALID);
    }
    let formats: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--input-format")
        .map(|(i, _)| i)
        .collect();
    if formats.len() != 1 || args.get(formats[0] + 1).is_none_or(|a| a != "text") {
        return Err(INVALID);
    }
    args[formats[0] + 1] = "stream-json".into();
    Ok(model)
}

pub(crate) fn input(mut source: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    source
        .by_ref()
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| INVALID)?;
    if bytes.len() > MAX_INPUT_BYTES || bytes.is_empty() {
        return Err(INVALID);
    }
    String::from_utf8(bytes).map_err(|_| INVALID)
}

/// Reject executable/receiver-changing configuration, without rewriting admin
/// values or implementing the native settings precedence. Restriction-only
/// settings remain the official client's responsibility.
pub(super) fn compatible(settings: &Value) -> bool {
    let Some(object) = settings.as_object() else {
        return false;
    };
    const DYNAMIC: &[&str] = &[
        "env",
        "apiKeyHelper",
        "awsAuthRefresh",
        "awsCredentialExport",
        "gcpAuthRefresh",
        "otelHeadersHelper",
        "proxyAuthHelper",
        "policyHelper",
        "policyHelpers",
        "hooks",
        "managedMcpServers",
        "mcpServers",
        "enabledPlugins",
        "extraKnownMarketplaces",
        "commands",
        "agents",
        "agent",
        "fallbackModel",
        "modelOverrides",
        "forceLoginGatewayUrl",
        "gatewayInternalNetworks",
        "statusLine",
        "fileSuggestion",
    ];
    for key in DYNAMIC {
        if object.get(*key).is_some_and(active) {
            return false;
        }
    }
    for (key, required) in [
        ("disableAllHooks", true),
        ("disableClaudeAiConnectors", true),
        ("autoMemoryEnabled", false),
    ] {
        if object.get(key).is_some_and(|v| v != required) {
            return false;
        }
    }
    true
}

fn active(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(_) => true,
    }
}

fn response(bytes: &[u8], id: &str, initializing: bool) -> Result<Value> {
    let UniqueValue(v) = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    let r = &v["response"];
    if v["type"] != "control_response" || r["subtype"] != "success" || r["request_id"] != id {
        return Err(INVALID);
    }
    for key in [
        "pending_permission_requests",
        "pending_user_dialog_requests",
    ] {
        match r.get(key) {
            Some(Value::Array(a)) if a.is_empty() => {}
            None if !initializing => {}
            _ => return Err(INVALID),
        }
    }
    if !r["response"].is_object() {
        return Err(INVALID);
    }
    Ok(r["response"].clone())
}

fn initialized(v: &Value) -> bool {
    v["account"]["apiProvider"] == "firstParty"
        && v["commands"].as_array().is_some_and(Vec::is_empty)
        && v["output_style"] == "default"
        && v.get("session_state").is_none_or(|s| s == "idle")
}

fn settings(v: &Value, model: &str) -> bool {
    let applied = &v["applied"];
    compatible(&v["effective"])
        && v["sources"]
            .as_array()
            .is_some_and(|sources| sources.iter().all(|s| compatible(&s["settings"])))
        && applied["model"] == model
        && applied.get("advisor") == Some(&Value::Null)
        && applied["ultracode"] == false
        && v.get("errors")
            .is_none_or(|e| e.as_array().is_some_and(Vec::is_empty))
        && v.get("remote_control_policy_lock_reason")
            .is_none_or(|r| r.is_null())
}

// Nonblocking child pipes make timeout/error cleanup independent of a CLI or
// descendant holding a pipe open. The host runner also enforces its own deadline.
struct Pipe<T> {
    inner: T,
    stop: Cancellation,
    deadline: Instant,
}
impl<T: AsFd> Pipe<T> {
    fn new(inner: T, stop: &Cancellation) -> Result<Self> {
        use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
        let flags = fcntl_getfl(&inner).map_err(|_| INVALID)?;
        fcntl_setfl(&inner, flags | OFlags::NONBLOCK).map_err(|_| INVALID)?;
        Ok(Self {
            inner,
            stop: stop.clone(),
            deadline: Instant::now() + Duration::from_secs(1800),
        })
    }
}
impl<T> Pipe<T> {
    fn check(&self) -> io::Result<()> {
        if self.stop.is_cancelled() || Instant::now() >= self.deadline {
            Err(io::Error::other(INVALID))
        } else {
            Ok(())
        }
    }
}
impl<T: Read> Read for Pipe<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.inner.read(bytes) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}
impl<T: Write> Write for Pipe<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.inner.write(bytes) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn line(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_EVENT_BYTES as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| INVALID)?;
    if bytes.len() > MAX_EVENT_BYTES || (!bytes.is_empty() && !bytes.ends_with(b"\n")) {
        return Err(INVALID);
    }
    Ok(bytes)
}
fn send(writer: &mut impl Write, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, &value).map_err(|_| INVALID)?;
    writer.write_all(b"\n").map_err(|_| INVALID)?;
    writer.flush().map_err(|_| INVALID)
}

pub(crate) fn run(
    stdin: ChildStdin,
    stdout: ChildStdout,
    corpus: String,
    model: String,
    stop: Cancellation,
) -> Result<()> {
    let mut writer = Pipe::new(stdin, &stop)?;
    let mut reader = BufReader::new(Pipe::new(stdout, &stop)?);
    release(&mut reader, &mut writer, &model, &corpus)?;
    drop(writer);
    let mut output = Pipe::new(io::stdout(), &stop)?;
    let mut count = 0;
    loop {
        let bytes = line(&mut reader)?;
        if bytes.is_empty() {
            return Ok(());
        }
        count += bytes.len();
        let UniqueValue(v) = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        if count > MAX_OUTPUT_BYTES
            || !matches!(
                v["type"].as_str(),
                Some("system" | "assistant" | "user" | "result" | "rate_limit_event")
            )
        {
            return Err(INVALID);
        }
        output.write_all(&bytes).map_err(|_| INVALID)?;
    }
}

fn release(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    model: &str,
    corpus: &str,
) -> Result<()> {
    send(
        writer,
        json!({"type":"control_request","request_id":"tgsum-init-1","request":{"subtype":"initialize"}}),
    )?;
    let init = response(&line(reader)?, "tgsum-init-1", true)?;
    if !initialized(&init) {
        return Err(INVALID);
    }
    send(
        writer,
        json!({"type":"control_request","request_id":"tgsum-settings-1","request":{"subtype":"get_settings"}}),
    )?;
    let config = response(&line(reader)?, "tgsum-settings-1", false)?;
    if !settings(&config, model) {
        return Err(INVALID);
    }
    send(
        writer,
        json!({"type":"user","message":{"role":"user","content":corpus},"parent_tool_use_id":null,"client_composed":true}),
    )
}

#[cfg(test)]
mod tests;
