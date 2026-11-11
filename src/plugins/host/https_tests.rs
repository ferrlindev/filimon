// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/https_tests.rs - Proof that the fetch path validates certificates
// (FETCH-03, cfg(test) only)
//
// A local TLS server (`openssl s_server`) with a throwaway self-signed
// certificate shows four things about the production client: the certificate
// is rejected, it is accepted only when explicitly trusted, a certificate for
// another name is rejected, and the real default fetcher component rejects it
// too. A source guard keeps the invalid-certificate switch out of `fetch.rs`
// on every normal run, and a separate ignored group (`net_`) checks real
// public hosts.
//
// The module is not called `tls_tests` on purpose: libtest group filters match
// the whole test path, so a module name containing `tls_` or `net_` would pull
// the other group's tests into each filter. Each filter here selects by test
// function name only.
//
// The key and certificate are generated per test in a temporary directory and
// are never committed; the server binds 127.0.0.1 only and is killed when its
// harness is dropped.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::engine::HostEngine;
use super::engine::source_world::filimon::plugin::types;
use super::fetch::{Allowlist, Egress, FetchPolicy, FetchRoute};
use super::loader::load_fetcher;
use super::testing::fixture_wasm;

// ── Local TLS Server ──────────────────────────────────────────────────────────

/// `openssl s_server` answering HTTPS on a free `127.0.0.1` port with a fresh
/// self-signed certificate that covers only the IP address `127.0.0.1`.
struct OpensslServer {
    port: u16,
    dir: TempDir,
    child: Child,
}

impl OpensslServer {
    /// Generates the key and certificate, starts the server and waits until it
    /// accepts connections.
    ///
    /// # Panics
    /// When `openssl` cannot be run, the certificate cannot be made, or the
    /// server does not come up.
    fn start() -> Self {
        let dir = TempDir::new().expect("tempdir for the test certificate");
        std::fs::write(
            dir.path().join("openssl.cnf"),
            "[req]\n\
             distinguished_name = dn\n\
             x509_extensions = v3\n\
             prompt = no\n\n\
             [dn]\n\
             CN = localhost\n\n\
             [v3]\n\
             subjectAltName = IP:127.0.0.1\n",
        )
        .expect("write openssl.cnf");

        let made = Command::new("openssl")
            .current_dir(dir.path())
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                "key.pem",
                "-out",
                "cert.pem",
                "-days",
                "2",
                "-config",
                "openssl.cnf",
            ])
            .stdin(Stdio::null())
            .output()
            .expect("needs the openssl binary: see plugins/README.md");
        assert!(
            made.status.success(),
            "openssl req failed: {}",
            String::from_utf8_lossy(&made.stderr)
        );

        // A free port: bind, read it, release it. A race with another process
        // is possible in theory and shows up as a failed start below.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a free port");
            listener.local_addr().expect("local address").port()
        };
        let mut child = Command::new("openssl")
            .current_dir(dir.path())
            .args([
                "s_server",
                "-accept",
                &format!("127.0.0.1:{port}"),
                "-cert",
                "cert.pem",
                "-key",
                "key.pem",
                "-www",
                "-quiet",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("needs the openssl binary: see plugins/README.md");

        let mut up = false;
        for _ in 0..50 {
            if let Some(status) = child.try_wait().expect("poll the openssl server") {
                panic!("openssl s_server exited before it accepted connections: {status}");
            }
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !up {
            let _ = child.kill();
            let _ = child.wait();
            panic!("openssl s_server did not accept connections on port {port} in time");
        }

        Self { port, dir, child }
    }

    fn port(&self) -> u16 {
        self.port
    }

    /// The server certificate as PEM bytes.
    fn cert_pem(&self) -> Vec<u8> {
        let path: PathBuf = self.dir.path().join("cert.pem");
        std::fs::read(path).expect("read the test certificate")
    }

    /// `https://<host>:<port>/`.
    fn url(&self, host: &str) -> String {
        format!("https://{host}:{}/", self.port())
    }
}

impl Drop for OpensslServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn policy_for(hosts: &[&str]) -> FetchPolicy {
    FetchPolicy::new("https-test", Allowlist::new(hosts), Duration::from_secs(20))
}

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(20)
}

fn get(url: &str) -> types::Request {
    types::Request {
        method: types::Method::Get,
        url: url.to_string(),
        headers: vec![],
        body: None,
    }
}

/// The text of a `transport` error that names a certificate problem; panics
/// on any other outcome.
fn certificate_error(result: Result<types::Response, types::FetchError>) -> String {
    match result {
        Err(types::FetchError::Transport(text)) => {
            assert!(
                text.to_lowercase().contains("certificate"),
                "expected a certificate problem, got: {text}"
            );
            text
        }
        other => panic!("expected a transport error naming a certificate, got {other:?}"),
    }
}

// ── Offline TLS Proofs (group `tls_`) ─────────────────────────────────────────

#[tokio::test]
#[ignore = "needs the openssl binary: see plugins/README.md"]
async fn tls_a_self_signed_certificate_is_rejected() {
    let server = OpensslServer::start();
    let policy = policy_for(&["127.0.0.1"]);

    let result = Egress::new()
        .execute(&policy, get(&server.url("127.0.0.1")), deadline())
        .await;

    certificate_error(result);
}

#[tokio::test]
#[ignore = "needs the openssl binary: see plugins/README.md"]
async fn tls_the_same_server_answers_a_client_that_skips_validation() {
    let server = OpensslServer::start();
    // The control: a client built with the switch the fetch path must never
    // use. If this one cannot reach the server, the rejection above would
    // prove a broken server and not certificate validation.
    let control = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("control client");

    let response = control
        .get(server.url("127.0.0.1"))
        .send()
        .await
        .expect("the server answers a client that skips validation");

    assert_eq!(response.status().as_u16(), 200);
}

#[tokio::test]
#[ignore = "needs the openssl binary: see plugins/README.md"]
async fn tls_a_certificate_trusted_as_a_root_is_accepted() {
    let server = OpensslServer::start();
    let policy = policy_for(&["127.0.0.1"]);

    let response = Egress::with_extra_root(&server.cert_pem())
        .execute(&policy, get(&server.url("127.0.0.1")), deadline())
        .await
        .expect("a certificate trusted as a root is accepted");

    assert_eq!(response.status, 200);
}

#[tokio::test]
#[ignore = "needs the openssl binary: see plugins/README.md"]
async fn tls_a_certificate_for_another_name_is_rejected() {
    let server = OpensslServer::start();
    // The issuer is trusted, so a failure here is the name check: the
    // certificate covers only the IP address, not `localhost`.
    let policy = policy_for(&["localhost"]);

    let result = Egress::with_extra_root(&server.cert_pem())
        .execute(&policy, get(&server.url("localhost")), deadline())
        .await;

    certificate_error(result);
}

#[tokio::test]
#[ignore = "needs the openssl binary and the plugin fixtures: see plugins/README.md"]
async fn tls_default_fetcher_component_rejects_a_self_signed_certificate() {
    let server = OpensslServer::start();
    let engine = HostEngine::new().expect("engine");
    let path = fixture_wasm("http-fetcher", "http_fetcher.wasm");
    let pre = load_fetcher("http", &path, &engine).expect("the default fetcher loads");
    let route = FetchRoute::new(
        "http",
        Arc::new(pre),
        engine.engine().clone(),
        Arc::new(policy_for(&["127.0.0.1"])),
        engine.egress(),
    );

    let result = route.fetch(get(&server.url("127.0.0.1"))).await;

    certificate_error(result);
    assert_eq!(route.calls(), 1, "the request went through the fetcher");
}

// ── Source Guard (always on) ──────────────────────────────────────────────────

/// The fetch path must never turn certificate or hostname validation off. This
/// reads `fetch.rs` only (never the crawler, whose own client has the switch,
/// and never this file, which names the settings on purpose).
#[test]
fn the_fetch_path_has_no_invalid_certificate_switch() {
    let source = include_str!("fetch.rs");

    // The file read must be the real client builder, not an empty or wrong file.
    assert!(
        source.contains("redirect::Policy::none()"),
        "fetch.rs no longer shows the client builder"
    );
    for forbidden in [
        "danger_accept_invalid_certs",
        "danger_accept_invalid_hostnames",
    ] {
        assert!(
            !source.contains(forbidden),
            "fetch.rs must not use `{forbidden}`: certificate validation stays on"
        );
    }
}

// ── Public Internet Proofs (group `net_`, never a gating command) ─────────────

#[tokio::test]
#[ignore = "needs internet access: see plugins/README.md"]
async fn net_a_public_https_site_succeeds_with_validation() {
    let policy = policy_for(&["example.com"]);

    let response = Egress::new()
        .execute(&policy, get("https://example.com/"), deadline())
        .await
        .expect("a real HTTPS site answers with validation on");

    assert_eq!(response.status, 200);
    let body = String::from_utf8_lossy(&response.body);
    assert!(body.contains("Example Domain"), "{body}");
}

#[tokio::test]
#[ignore = "needs internet access: see plugins/README.md"]
async fn net_badssl_invalid_certificates_are_rejected() {
    let hosts = [
        "self-signed.badssl.com",
        "expired.badssl.com",
        "wrong.host.badssl.com",
        "untrusted-root.badssl.com",
    ];
    let policy = policy_for(&hosts);
    let egress = Egress::new();

    for host in hosts {
        let result = egress
            .execute(&policy, get(&format!("https://{host}/")), deadline())
            .await;
        let text = certificate_error(result);
        eprintln!("{host}: {text}");
    }
}
