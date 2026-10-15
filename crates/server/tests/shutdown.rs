//! The real binary must quit when asked to, also while a browser keeps the event stream open.
//! (It used to hang: a graceful shutdown waits for open connections, and an event stream never
//! ends by itself. SIGTERM was not handled at all.)
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    port: u16,
    _dir: tempfile::TempDir,
}

impl Server {
    fn start() -> Self {
        // A free port: bind to 0, read it, let go.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let dir = tempfile::tempdir().expect("tempdir");
        let child = Command::new(env!("CARGO_BIN_EXE_fetchbox"))
            .arg("--data-dir")
            .arg(dir.path().join("data"))
            .arg("--download-dir")
            .arg(dir.path().join("downloads"))
            .args(["--port", &port.to_string(), "--no-auth"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fetchbox");
        let server = Self {
            child,
            port,
            _dir: dir,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        server
    }

    /// An open event stream, as a browser tab holds one. Returns once the first bytes came.
    fn open_event_stream(&self) -> TcpStream {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        s.write_all(
            b"GET /api/events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
        )
        .expect("request");
        s.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).expect("the stream answers at once");
        assert!(String::from_utf8_lossy(&buf[..n]).contains("200"));
        s
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .arg(format!("-{name}"))
            .arg(self.child.id().to_string())
            .status()
            .expect("kill");
        assert!(status.success());
    }

    /// Wait for the process to end; `None` when it is still there after `within`.
    fn wait_exit(&mut self, within: Duration) -> Option<Duration> {
        let start = Instant::now();
        while start.elapsed() < within {
            if self.child.try_wait().expect("wait").is_some() {
                return Some(start.elapsed());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn quits_on(signal: &str, with_open_stream: bool) {
    let mut server = Server::start();
    let _stream = with_open_stream.then(|| server.open_event_stream());
    server.signal(signal);
    // Well inside the 5 s grace period: the stream is ended, not waited out.
    let took = server.wait_exit(Duration::from_secs(4)).unwrap_or_else(|| {
        panic!("still running after SIG{signal} (open stream: {with_open_stream})")
    });
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[test]
fn ctrl_c_quits_an_idle_server() {
    quits_on("INT", false);
}

#[test]
fn ctrl_c_quits_while_a_browser_holds_the_event_stream() {
    quits_on("INT", true);
}

#[test]
fn sigterm_quits_too_also_with_an_open_stream() {
    quits_on("TERM", false);
    quits_on("TERM", true);
}

#[test]
fn a_client_that_never_finishes_its_request_cannot_hold_the_server_for_ever() {
    let mut server = Server::start();
    // Headers that never end: not an event stream, so nothing ends this connection but the deadline.
    let mut stuck = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stuck
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n")
        .expect("partial request");
    std::thread::sleep(Duration::from_millis(200));
    server.signal("INT");
    let took = server
        .wait_exit(Duration::from_secs(9))
        .expect("the grace period ends and the server quits anyway");
    assert!(
        took >= Duration::from_secs(4),
        "it waited for the grace period: {took:?}"
    );
}

#[test]
fn a_second_signal_quits_at_once() {
    let mut server = Server::start();
    let mut stuck = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stuck
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n")
        .expect("partial request");
    std::thread::sleep(Duration::from_millis(200));
    server.signal("INT");
    std::thread::sleep(Duration::from_millis(300));
    server.signal("INT");
    let took = server
        .wait_exit(Duration::from_secs(3))
        .expect("the second signal does not wait for the grace period");
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[test]
fn the_event_stream_is_ended_by_the_server_when_it_shuts_down() {
    let mut server = Server::start();
    let mut stream = server.open_event_stream();
    server.signal("TERM");
    // The client sees the end of the stream (EOF), not a timeout.
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .expect("timeout");
    let mut sink = [0u8; 4096];
    loop {
        match stream.read(&mut sink) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(e) => panic!("expected the stream to end, got {e}"),
        }
    }
    assert!(server.wait_exit(Duration::from_secs(4)).is_some());
}
