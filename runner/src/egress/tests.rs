use super::*;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};
use std::os::unix::net::UnixStream;
use std::sync::atomic::AtomicUsize;

fn request(host: &str) -> Vec<u8> {
    format!("CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n").into_bytes()
}

fn vector16(bytes: &[u8]) -> Vec<u8> {
    let mut result = (bytes.len() as u16).to_be_bytes().to_vec();
    result.extend(bytes);
    result
}

fn hello(host: &str, extra: &[u8], fragment: usize) -> Vec<u8> {
    let mut name = vec![0];
    name.extend(vector16(host.as_bytes()));
    let mut extensions = vec![0, 0];
    extensions.extend(vector16(&vector16(&name)));
    extensions.extend(extra);
    let mut body = vec![3, 3];
    body.extend([0; 32]);
    body.extend([0, 0, 2, 0xc0, 0x2f, 1, 0]);
    body.extend(vector16(&extensions));
    let mut handshake = vec![1, 0, (body.len() >> 8) as u8, body.len() as u8];
    handshake.extend(body);
    handshake
        .chunks(fragment)
        .flat_map(|chunk| {
            let mut record = vec![22, 3, 1];
            record.extend(vector16(chunk));
            record
        })
        .collect()
}

fn start(dial: Arc<Dial>, limits: Limits, cancel: &Cancellation) -> InferenceGateway {
    InferenceGateway::start_with(Destination::OpenAiApi, limits, cancel, dial).unwrap()
}

fn client(gateway: &InferenceGateway) -> UnixStream {
    let stream = UnixStream::connect(gateway.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
}

fn begin(client: &mut UnixStream) {
    client.write_all(&request("api.openai.com")).unwrap();
    let mut response = [0; 39];
    client.read_exact(&mut response).unwrap();
    assert_eq!(&response, b"HTTP/1.1 200 Connection Established\r\n\r\n");
}

#[test]
fn authority_headers_and_plaintext_auth_are_rejected_before_dns() {
    for destination in [Destination::OpenAiApi, Destination::ChatGpt] {
        policy::connect_request(&request(destination.host()), destination).unwrap();
    }
    let valid = String::from_utf8(request("api.openai.com")).unwrap();
    for invalid in [
        request("auth.openai.com"),
        request("api.openai.com.evil.test"),
        request("127.0.0.1"),
        valid.replace(":443", ":80").into_bytes(),
        valid.replace("CONNECT", "GET").into_bytes(),
        valid.replace("HTTP/1.1", "HTTP/1.0").into_bytes(),
        valid.replace("Host:", "Wrong:").into_bytes(),
        valid
            .replace("\r\n\r\n", "\r\nHost: api.openai.com:443\r\n\r\n")
            .into_bytes(),
        valid
            .replace("\r\n\r\n", "\r\nContent-Length: 0\r\n\r\n")
            .into_bytes(),
        valid
            .replace("\r\n\r\n", "\r\nTransfer-Encoding: chunked\r\n\r\n")
            .into_bytes(),
        valid
            .replace(
                "\r\n\r\n",
                "\r\nProxy-Authorization: synthetic-secret\r\n\r\n",
            )
            .into_bytes(),
        valid.replace("\r\n", "\n").into_bytes(),
        valid.replace("Host:", " Host:").into_bytes(),
    ] {
        assert_eq!(
            policy::connect_request(&invalid, Destination::OpenAiApi),
            Err(Failure::ConnectRequest)
        );
    }
}

#[test]
fn resolver_rejects_private_mixed_mapped_special_and_unbounded_answers() {
    for ip in [
        "0.0.0.0",
        "10.0.0.1",
        "100.64.0.1",
        "127.99.1.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.0.0.9",
        "192.0.2.1",
        "192.88.99.2",
        "192.168.1.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "fc00::1",
        "fe80::1",
        "ff02::1",
        "2001::1",
        "2001:2::1",
        "2001:db8::1",
        "2002:808:808::1",
        "3fff::1",
    ] {
        assert!(!policy::public_address(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "8.8.8.8",
        "1.1.1.1",
        "104.18.1.1",
        "2001:4860:4860::8888",
        "2606:4700::1111",
    ] {
        assert!(policy::public_address(ip.parse().unwrap()), "{ip}");
    }
    assert_eq!(
        policy::resolved_addresses(b"8.8.8.8 STREAM x\n8.8.8.8 DGRAM\n")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        policy::resolved_addresses(b"8.8.8.8 STREAM\n127.0.0.1 STREAM"),
        Err(Failure::PrivateAddress)
    );
    for bad in [b"".as_slice(), b"invalid STREAM", b"8.8.8.8\n\n", b"\xff"] {
        assert_eq!(policy::resolved_addresses(bad), Err(Failure::Resolution));
    }
    let many = (1..=17)
        .map(|i| format!("8.8.8.{i} STREAM\n"))
        .collect::<String>();
    assert_eq!(
        policy::resolved_addresses(many.as_bytes()),
        Err(Failure::Resolution)
    );
}

#[test]
fn fragmented_hello_and_opaque_bidirectional_data_are_preserved_with_half_close() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let hello = hello("api.openai.com", &[], 17);
    let expected = hello.clone();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes[..expected.len()], expected);
        assert_eq!(
            &bytes[expected.len()..],
            b"\x17\x03\x03synthetic opaque upload"
        );
        socket.write_all(b"opaque download\0\xff").unwrap();
    });
    let gateway = start(
        Arc::new(move |_, _| TcpStream::connect(address).map_err(|_| Failure::Io)),
        Limits::default(),
        &Cancellation::default(),
    );
    let socket_path = gateway.socket_path().to_owned();
    let parent = socket_path.parent().unwrap().to_owned();
    assert_eq!(
        std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&socket_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let mut client = client(&gateway);
    begin(&mut client);
    client.write_all(&hello).unwrap();
    client
        .write_all(b"\x17\x03\x03synthetic opaque upload")
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    assert_eq!(response, b"opaque download\0\xff");
    server.join().unwrap();
    let report = gateway.finish();
    assert_eq!(report.completed, 1);
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(
        report.admitted_bytes as usize,
        hello.len() + b"\x17\x03\x03synthetic opaque upload".len() + response.len()
    );
    assert!(!socket_path.exists() && !parent.exists());
}

#[test]
fn bad_tls_name_ech_duplicate_extensions_and_excess_records_never_dial() {
    let mut bad_type = hello("api.openai.com", &[], 1024);
    bad_type[0] = 23;
    for bytes in [
        hello("auth.openai.com", &[], 1024),
        hello("api.openai.com", &[0xfe, 0x0d, 0, 0], 1024),
        hello("api.openai.com", &[0, 0, 0, 0], 1024),
        hello("api.openai.com", &[], 1),
        bad_type,
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let gateway = start(
            Arc::new(move |_, _| {
                observed.fetch_add(1, Ordering::Relaxed);
                Err(Failure::Unreachable)
            }),
            Limits::default(),
            &Cancellation::default(),
        );
        let mut client = client(&gateway);
        begin(&mut client);
        let _ = client.write_all(&bytes);
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response);
        let report = gateway.finish();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(report.failures.len(), 1, "{report:?}");
        assert_eq!(report.admitted_bytes, 0);
        assert!(response.is_empty());
    }
}

#[test]
fn slow_handshake_cancellation_and_admission_are_bounded() {
    let cancel = Cancellation::default();
    let no_dial: Arc<Dial> = Arc::new(|_, _| panic!("handshake must not dial"));
    let limits = Limits {
        handshake: Duration::from_millis(200),
        concurrent: 1,
        connections: 2,
        ..Limits::default()
    };
    let gateway = start(no_dial.clone(), limits, &cancel);
    let mut first = client(&gateway);
    begin(&mut first); // Handshake response proves admission before client two.
    let mut second = client(&gateway);
    let _ = second.write_all(&request("api.openai.com"));
    let mut ignored = Vec::new();
    let _ = first.read_to_end(&mut ignored);
    let report = gateway.finish();
    assert!(report.failures.contains(&Failure::Timeout), "{report:?}");
    assert!(
        report.failures.contains(&Failure::Connections),
        "{report:?}"
    );
    let gateway = start(no_dial, Limits::default(), &cancel);
    let mut client = client(&gateway);
    begin(&mut client);
    cancel.cancel();
    let before = Instant::now();
    let report = gateway.finish();
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(report.failures.contains(&Failure::Cancelled));
}

#[test]
fn connection_budget_closes_tunnel_without_forwarding_overflow() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let hello = hello("api.openai.com", &[], 1024);
    let expected = hello.clone();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut hello = vec![0; expected.len()];
        socket.read_exact(&mut hello).unwrap();
        assert_eq!(hello, expected);
        socket.write_all(&[0; 1024]).unwrap();
    });
    let cap = hello.len() as u64 + 32;
    let limits = Limits {
        connection_bytes: cap,
        total_bytes: cap * 2,
        ..Limits::default()
    };
    let gateway = start(
        Arc::new(move |_, _| TcpStream::connect(address).map_err(|_| Failure::Io)),
        limits,
        &Cancellation::default(),
    );
    let mut client = client(&gateway);
    begin(&mut client);
    client.write_all(&hello).unwrap();
    let mut response = Vec::new();
    let _ = client.read_to_end(&mut response);
    server.join().unwrap();
    let report = gateway.finish();
    assert!(report.failures.contains(&Failure::Budget), "{report:?}");
    assert!(report.admitted_bytes <= cap);
    assert!(response.len() <= 32);
}

#[test]
fn aggregate_budget_is_shared_by_distinct_connections() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let hello = hello("api.openai.com", &[], 1024);
    let expected = hello.clone();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut input = Vec::new();
            socket.read_to_end(&mut input).unwrap();
            assert_eq!(input, expected);
            socket.write_all(&[0; 512]).unwrap();
        }
    });
    let limits = Limits {
        connection_bytes: 1000,
        total_bytes: 1000,
        ..Limits::default()
    };
    let gateway = start(
        Arc::new(move |_, _| TcpStream::connect(address).map_err(|_| Failure::Io)),
        limits,
        &Cancellation::default(),
    );
    for n in 0..2 {
        let mut client = client(&gateway);
        begin(&mut client);
        client.write_all(&hello).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        if n == 0 {
            assert_eq!(response.len(), 512);
        } else {
            assert!(response.len() < 512);
        }
    }
    server.join().unwrap();
    let report = gateway.finish();
    assert_eq!(report.completed, 1);
    assert_eq!(report.failures, [Failure::Budget]);
    assert!(report.admitted_bytes <= 1000);
}

#[test]
fn actual_tls13_authenticates_the_local_server_and_keeps_credentials_inside_tls() {
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use rustls::{
        ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    };

    let certificate = rcgen::generate_simple_self_signed(vec!["api.openai.com".into()]).unwrap();
    let der = certificate.cert.der().clone();
    let key = PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der());
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key.into())
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(der).unwrap();
    let client_config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut stream = StreamOwned::new(
            ServerConnection::new(Arc::new(server_config)).unwrap(),
            socket,
        );
        let mut bytes = [0; 29];
        stream.read_exact(&mut bytes).unwrap();
        assert!(&bytes == b"Authorization: synthetic-only");
        assert_eq!(
            stream.conn.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        stream.write_all(b"canned answer").unwrap();
        stream.conn.send_close_notify();
        stream.flush().unwrap();
        // Wait for the client's close_notify; neither side is a real service.
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).unwrap();
    });
    let gateway = start(
        Arc::new(move |_, _| TcpStream::connect(address).map_err(|_| Failure::Io)),
        Limits::default(),
        &Cancellation::default(),
    );
    let mut socket = client(&gateway);
    begin(&mut socket);
    let connection = ClientConnection::new(
        Arc::new(client_config),
        "api.openai.com".try_into().unwrap(),
    )
    .unwrap();
    let mut tls = StreamOwned::new(connection, socket);
    tls.write_all(b"Authorization: synthetic-only").unwrap();
    tls.flush().unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).unwrap();
    assert_eq!(response, b"canned answer");
    tls.conn.send_close_notify();
    tls.flush().unwrap();
    drop(tls);
    server.join().unwrap();
    let report = gateway.finish();
    assert!(!format!("{report:?}").contains("synthetic-only"));
    assert!(report.admitted_bytes > response.len() as u64);
    assert!(
        report.failures.iter().all(|f| *f == Failure::Cancelled),
        "{report:?}"
    );
}
