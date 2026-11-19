//! The clock and the system's generator (Task H.1): `ReadClock` and `Random` in a reactive
//! program, run for real, recorded, replayed and scripted, and `IO.now` and `IO.random` in a
//! `program`, whose results the verifier knows no more of than the runtime promises.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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
    String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim_end_matches('\r').to_string()).collect()
}

/// `source`, saved as `file` in a directory of its own and checked under `--strict`.
fn check(file: &str, source: &str, name: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_clock_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::write(dir.0.join(file), source).expect("the program");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args([file, "--strict"])
        .output()
        .expect("gardelc runs");
    (dir, out)
}

/// `file` from `tests/`, verified and built: its directory and its binary.
fn build(file: &str, name: &str) -> (Dir, PathBuf) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join(file);
    let source = fs::read_to_string(&path).expect("the program's source");
    let base = Path::new(file).file_name().and_then(|f| f.to_str()).expect("a file name");
    let (dir, out) = check(base, &source, name);
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let stem = base.trim_end_matches(".gdl");
    let bin = dir.0.join(format!("build/{stem}_bin.exe"));
    assert!(bin.exists(), "no binary at {}:\n{}", bin.display(), all(&out));
    (dir, bin)
}

fn run(bin: &Path, env: &[(&str, &str)], cwd: &Path) -> Output {
    run_with(bin, &[], env, cwd)
}

fn run_args(bin: &Path, args: &[&str], cwd: &Path) -> Output {
    run_with(bin, args, &[], cwd)
}

/// The output of `bin`, which must end within a minute: an answer never queued would
/// otherwise hang the test.
fn run_with(bin: &Path, args: &[&str], env: &[(&str, &str)], cwd: &Path) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    cmd.current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().expect("the program runs");
    let id = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || drop(tx.send(child.wait_with_output())));
    match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(out) => out.expect("the program's output"),
        Err(_) => {
            drop(Command::new(if cfg!(windows) { "taskkill" } else { "kill" })
                .args(if cfg!(windows) { vec!["/F".to_string(), "/PID".to_string(), id.to_string()] } else { vec!["-9".to_string(), id.to_string()] })
                .output());
            panic!("{} did not end within a minute", bin.display());
        }
    }
}

fn now_ms() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).expect("after 1970").as_millis()).expect("fits")
}

/// The eight numbers of a `random 8 ...` line, each a byte.
fn octets(line: &str) -> Vec<u64> {
    let rest = line.strip_prefix("random 8 ").unwrap_or_else(|| panic!("not eight random bytes: {line}"));
    let bs: Vec<u64> = rest.split(' ').map(|x| x.parse().expect("a number")).collect();
    assert!(bs.len() == 8 && bs.iter().all(|b| *b < 256), "{line}");
    bs
}

/// The time is now's, eight bytes come and a count above 1024 is refused; the recording has
/// each answer, and its replay gives the same output with no clock read; a second run draws
/// other bytes.
#[test]
fn clock_and_random_run_record_and_replay() {
    let (dir, bin) = build("reactive/clock_random.gdl", "reactive");
    let before = now_ms();
    let out = run(&bin, &[("GARDEL_RECORD", "run.log")], &dir.0);
    let after = now_ms();
    assert!(out.status.success(), "{}", all(&out));
    let got = lines(&out);
    assert_eq!(got.len(), 4, "{got:?}");
    let clock: Vec<&str> = got[0].strip_prefix("clock ").expect("the clock first").split(' ').collect();
    let wall: i64 = clock[0].parse().expect("the wall clock");
    assert!(before - 1000 <= wall && wall <= after + 1000, "{wall} is not between {before} and {after}");
    let _monotonic: u64 = clock[1].parse().expect("the monotonic clock");
    let first = octets(&got[1]);
    assert_eq!(got[2..], ["refused 3", "done"]);

    let log = fs::read_to_string(dir.0.join("run.log")).expect("a log");
    for line in ["command ReadClock:1\n", "command Random:2:8\n", "command Random:3:1025\n", &format!("event Clock:1:{wall}:"), "event RandomBytes:2:", "event RandomFailed:3:"] {
        assert!(log.contains(line), "{line} in\n{log}");
    }

    let again = run(&bin, &[("GARDEL_REPLAY", "run.log")], &dir.0);
    assert!(again.status.success(), "{}", all(&again));
    assert_eq!(lines(&again), got, "{}", all(&again));
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(err.contains("host: ReadClock 1") && err.contains("host: Random 2 8"), "{err}");

    // The clock alone: the loop reads an answer it queued itself, with no helper thread started.
    let alone = run_args(&bin, &["clock"], &dir.0);
    assert!(alone.status.success(), "{}", all(&alone));
    assert_eq!(lines(&alone).len(), 2, "{}", all(&alone));
    assert!(lines(&alone)[0].starts_with("clock ") && lines(&alone)[1] == "done", "{}", all(&alone));

    // Two draws of eight bytes are the same once in 2^64 runs.
    let other = run(&bin, &[], &dir.0);
    assert!(other.status.success(), "{}", all(&other));
    assert_ne!(octets(&lines(&other)[1]), first, "two runs drew the same bytes");
}

/// A script gives a time before 1970, bytes that are not text and a failure, and its
/// recording replays the same; a malformed token stops the program with status 2.
#[test]
fn clock_and_random_from_a_script() {
    let (dir, bin) = build("reactive/clock_random.gdl", "script");
    let script = "Clock:1:-5:7 RandomBytes:2:%00%FFab RandomFailed:3:22";
    let out = run(&bin, &[("GARDEL_EVENTS", script), ("GARDEL_RECORD", "script.log")], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out), ["clock -5 7", "random 4 0 255 97 98", "refused 3", "done"]);
    let log = fs::read_to_string(dir.0.join("script.log")).expect("a log");
    assert!(log.contains("event Clock:1:-5:7
") && log.contains("event RandomBytes:2:%00%FFab
") && log.contains("event RandomFailed:3:22
"), "{log}");
    let again = run(&bin, &[("GARDEL_REPLAY", "script.log")], &dir.0);
    assert!(again.status.success(), "{}", all(&again));
    assert_eq!(lines(&again), lines(&out), "{}", all(&again));
    let edge = run(&bin, &[("GARDEL_EVENTS", "Clock:1:-9223372036854775808:18446744073709551615")], &dir.0);
    assert_eq!(lines(&edge)[0], "clock -9223372036854775808 18446744073709551615", "{}", all(&edge));
    for bad in ["Clock:1:x:7", "Clock:1:2", "Clock:1:-9223372036854775809:1", "Clock:1:9223372036854775808:1", "RandomBytes:2", "RandomFailed:3:x"] {
        let out = run(&bin, &[("GARDEL_EVENTS", bad)], &dir.0);
        assert_eq!(out.status.code(), Some(2), "{bad}: {}", all(&out));
    }
}

/// `IO.now` is an `I64` and an `Ok` of `IO.random(8)` has eight bytes, so a routine that
/// needs eight is called; 1025 bytes is an `Error`.
#[test]
fn now_and_random_in_a_program() {
    let (dir, bin) = build("clock_random_program.gdl", "program");
    let before = now_ms();
    let out = run(&bin, &[], &dir.0);
    let after = now_ms();
    assert_eq!(out.status.code(), Some(0), "{}", all(&out));
    let got = lines(&out);
    let wall: i64 = got[0].parse().expect("the time");
    assert!(before - 1000 <= wall && wall <= after + 1000, "{wall} is not between {before} and {after}");
    assert_eq!(got[1], "8");
    assert!(got[2].parse::<u64>().expect("a byte") < 256, "{got:?}");
}

const LIES: &str = "program CLOCK_RANDOM_LIES

feature {ANY}

  same(a: BYTES, b: BYTES): NAT
    require
      equal_draws: a.equals(b)
    do
      0
    end

  nine(bs: BYTES): NAT
    require
      nine_bytes: bs.length = 9
    do
      0
    end

  after_1970(t: I64): NAT
    require
      not_before: t >= 0
    do
      0
    end

  later(a: I64, b: I64): NAT
    require
      in_order: a <= b
    do
      0
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        t <- IO.now
        t2 <- IO.now
        r <- IO.random(8)
        r2 <- IO.random(8)
        match r
          case Ok(a) then
            match r2
              case Ok(b) then
                IO.exit(USES)
              case Error(_) then
                IO.exit(2)
            end
          case Error(_) then
            IO.exit(1)
        end
      end
    end

end
";

/// Two draws are not provably equal and eight bytes are not nine; the wall clock may be
/// before 1970 and may go back between two reads, so neither is assumed.
#[test]
fn what_the_clock_and_random_do_not_promise() {
    for (name, uses, refused) in [
        ("draws", "same(a, b) + nine(a)", ["equal_draws", "nine_bytes"]),
        ("times", "after_1970(t) + later(t, t2)", ["not_before", "in_order"]),
    ] {
        let (_dir, out) = check("lies.gdl", &LIES.replace("USES", uses), name);
        let text = all(&out);
        assert!(!out.status.success(), "{text}");
        for clause in refused {
            assert!(text.contains(&format!("Precondition Violation) in `main` ({clause})")), "{clause} refused in\n{text}");
        }
    }
}
