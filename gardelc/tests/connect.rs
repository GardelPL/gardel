//! `Connect`, `Connected`, `ConnectFailed` and `CancelTimer` (Task F.7), over real TCP: a
//! connect that succeeds, one refused, one that times out and a host that is not an address
//! each give the right event, a cancelled timer never ticks, and a recorded run replays with
//! the same commands.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

/// `net_client.gdl`, verified under `--strict` and built, in a directory of its own.
fn client(name: &str) -> (Dir, PathBuf) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_connect_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reactive/net_client.gdl"),
        dir.0.join("net_client.gdl"),
    )
    .expect("the program");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args(["net_client.gdl", "--strict"])
        .output()
        .expect("gardelc runs");
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let bin = dir.0.join(format!("build/net_client_bin{}", std::env::consts::EXE_SUFFIX));
    assert!(bin.exists(), "no binary at {}:\n{}", bin.display(), all(&out));
    (dir, bin)
}

fn run(bin: &PathBuf, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("the client runs")
}

/// A server for one client: it reads a line, answers `pong` and closes.
fn pong_server() -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let port = listener.local_addr().expect("its address").port();
    let served = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("the client connects");
        s.set_read_timeout(Some(Duration::from_secs(10))).expect("a timeout");
        let mut got = Vec::new();
        let mut b = [0u8; 1];
        while !got.ends_with(b"\n") && s.read(&mut b).map(|n| n == 1).unwrap_or(false) {
            got.push(b[0]);
        }
        s.write_all(b"pong").expect("the answer");
        String::from_utf8_lossy(&got).into_owned()
    });
    (port, served)
}

/// A port nothing listens on: one the system gave, let go of.
fn closed_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).expect("a port").local_addr().expect("its address").port()
}

#[test]
fn a_connect_reaches_a_server_and_replays() {
    let (dir, bin) = client("ok");
    for host in ["127.0.0.1", "localhost"] {
        let (port, served) = pong_server();
        let log = dir.0.join(format!("{host}.log"));
        let out = run(&bin, &[host, &port.to_string()], &[("GARDEL_RECORD", &log.to_string_lossy())]);
        assert_eq!(served.join().expect("the server"), "ping\n", "{}", all(&out));
        assert!(out.status.success(), "{}", all(&out));
        assert_eq!(lines(&out), ["connected 5", "received pong", "closed", "done"], "{}", all(&out));

        let text = fs::read_to_string(&log).expect("the log");
        let host_text: String = host.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_string() } else { format!("%{:02X}", c as u32) }).collect();
        for line in [
            "command SetTimer:7:30".to_string(),
            "command CancelTimer:7".to_string(),
            format!("command Connect:5:{host_text}:{port}"),
            "event Connected:5:1".to_string(),
            "command Send:1:ping%0A".to_string(),
            "event Received:1:pong".to_string(),
            "command Close:1".to_string(),
        ] {
            assert!(text.lines().any(|l| l == line), "the log lacks `{line}`:\n{text}");
        }
        assert!(!text.contains("Tick:7"), "a cancelled timer ticked:\n{text}");

        // The replay runs no socket and no timer, and gives the same commands and output.
        let again = run(&bin, &[], &[("GARDEL_REPLAY", &log.to_string_lossy())]);
        assert!(again.status.success(), "{}", all(&again));
        assert_eq!(lines(&again), lines(&out), "{}", all(&again));
    }
}

#[test]
fn a_refused_connect_fails() {
    let (_dir, bin) = client("refused");
    let out = run(&bin, &["127.0.0.1", &closed_port().to_string()], &[]);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out), ["connect failed 5", "done"], "{}", all(&out));
}

/// An address that answers nothing: the connect fails when its time is up, or at once where
/// the network says it cannot be reached. Either way it does not hang.
#[test]
fn a_connect_with_no_answer_times_out() {
    let (dir, bin) = client("timeout");
    let log = dir.0.join("timeout.log");
    let start = Instant::now();
    let out = run(
        &bin,
        &["10.255.255.1", "9"],
        &[("GARDEL_CONNECT_TIMEOUT_MS", "300"), ("GARDEL_RECORD", &log.to_string_lossy())],
    );
    let took = start.elapsed();
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out), ["connect failed 5", "done"], "{}", all(&out));
    assert!(took < Duration::from_secs(8), "the connect took {took:?}");
    let text = fs::read_to_string(&log).expect("the log");
    let timed_out = if cfg!(windows) { "event ConnectFailed:5:10060" } else { "event ConnectFailed:5:110" };
    if text.lines().any(|l| l == timed_out) {
        assert!(took >= Duration::from_millis(250), "timed out before its time: {took:?}");
    }
}

/// A host that is not an IPv4 address nor `localhost` is not looked up, and a port out of
/// range is refused, both before any socket.
#[test]
fn a_connect_to_what_is_not_an_address_fails() {
    let (_dir, bin) = client("invalid");
    for (host, port) in [("example.com", "80"), ("127.0.0", "80"), ("256.1.1.1", "80"), ("127.0.0.1", "0"), ("127.0.0.1", "70000")] {
        let out = run(&bin, &[host, port], &[]);
        assert!(out.status.success(), "{host}:{port}\n{}", all(&out));
        assert_eq!(lines(&out), ["connect failed 5", "done"], "{host}:{port}\n{}", all(&out));
    }
}

/// A script stands for the network: the commands are written out and the events read in.
#[test]
fn a_script_gives_connect_events() {
    let (_dir, bin) = client("script");
    let out = run(&bin, &["127.0.0.1", "4000"], &[("GARDEL_EVENTS", "Connected:5:3 Received:3:pong Closed:3 Tick:8")]);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out), ["connected 5", "received pong", "closed", "done"], "{}", all(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("net: Connect 5 127%2E0%2E0%2E1 4000"), "{err}");
    assert!(err.contains("net: Send 3 ping%0A"), "{err}");

    let failed = run(&bin, &["127.0.0.1", "4000"], &[("GARDEL_EVENTS", "ConnectFailed:5:111 Tick:8")]);
    assert_eq!(lines(&failed), ["connect failed 5", "done"], "{}", all(&failed));
    for bad in ["Connected:5", "Connected:5:", "ConnectFailed:x:1", "Connected:5:1:2"] {
        let out = run(&bin, &["127.0.0.1", "4000"], &[("GARDEL_EVENTS", bad)]);
        assert_eq!(out.status.code(), Some(2), "`{bad}` must be refused:\n{}", all(&out));
    }
}
