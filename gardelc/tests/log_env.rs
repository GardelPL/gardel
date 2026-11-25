//! Logging and the environment (Task H.2): `Log` and `ReadEnv` in a reactive program, run for
//! real, recorded, replayed and scripted, and `IO.log` and `IO.env` in a `program`, whose
//! values the verifier knows no more of than the runtime promises.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The suffix gardelc gives an executable: `.exe` on Windows, none elsewhere.
const EXE: &str = std::env::consts::EXE_SUFFIX;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes).lines().map(|l| l.trim_end_matches('\r').to_string()).collect()
}

/// The lines of standard error that are log lines, without the host's own.
fn logged(out: &Output) -> Vec<String> {
    lines(&out.stderr).into_iter().filter(|l| !l.starts_with("host: ")).collect()
}

/// `source`, saved as `file` in a directory of its own and checked under `--strict`.
fn check(file: &str, source: &str, name: &str) -> (Dir, Output) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_log_env_{}_{name}", std::process::id())));
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
    assert!(!all(&out).contains("warning:"), "the C has warnings:\n{}", all(&out));
    let stem = base.trim_end_matches(".gdl");
    let bin = dir.0.join(format!("build/{stem}_bin{EXE}"));
    assert!(bin.exists(), "no binary at {}:\n{}", bin.display(), all(&out));
    (dir, bin)
}

/// The output of `bin`, run with `env` and none of the variables the tests read unless
/// given, which must end within a minute: an answer never queued would otherwise hang the test.
fn run(bin: &Path, env: &[(&str, &str)], cwd: &Path) -> Output {
    let mut cmd = Command::new(bin);
    cmd.current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for k in ["GARDEL_TEST_VALUE", "GARDEL_TEST_UNSET", "GARDEL_LOG_LEVEL"] {
        cmd.env_remove(k);
    }
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

const INFO_UP: [&str; 4] = ["info: info line", "warn: two\\x0alines \\\\ one", "severe: severe line", "info: done"];

/// A set variable is read as text and an unset one is `None`; each level at or above `Info` is
/// written, escaped to one line; the recording has each command and answer, and its replay
/// writes the same lines with no variable read. `GARDEL_LOG_LEVEL` moves the threshold, in any
/// case, and a level it does not know stops the program before it starts.
#[test]
fn log_and_env_run_record_and_replay() {
    let (dir, bin) = build("reactive/log_env.gdl", "reactive");
    let out = run(&bin, &[("GARDEL_TEST_VALUE", "h\u{e9}llo w\u{f6}rld"), ("GARDEL_RECORD", "run.log")], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out.stdout), ["env 1 set h\u{e9}llo w\u{f6}rld", "env 2 unset"], "{}", all(&out));
    assert_eq!(logged(&out), INFO_UP, "{}", all(&out));

    let log = fs::read_to_string(dir.0.join("run.log")).expect("a log");
    for line in [
        "command Log:debug:debug%20line\n",
        "command Log:warn:two%0Alines%20%5C%20one\n",
        "command ReadEnv:1:GARDEL%5FTEST%5FVALUE\n",
        "event EnvRead:1:h%C3%A9llo%20w%C3%B6rld\n",
        "event EnvMissing:2\n",
        "command Log:info:done\n",
    ] {
        assert!(log.contains(line), "{line} in\n{log}");
    }

    // The replay reads no variable: the value comes from the log, though it is now unset.
    let again = run(&bin, &[("GARDEL_REPLAY", "run.log")], &dir.0);
    assert!(again.status.success(), "{}", all(&again));
    assert_eq!(lines(&again.stdout), lines(&out.stdout), "{}", all(&again));
    assert_eq!(logged(&again), INFO_UP, "{}", all(&again));
    assert!(String::from_utf8_lossy(&again.stderr).contains("host: ReadEnv 1 GARDEL_TEST_VALUE"), "{}", all(&again));

    let debug = run(&bin, &[("GARDEL_LOG_LEVEL", "DeBuG")], &dir.0);
    assert_eq!(logged(&debug)[0], "debug: debug line", "{}", all(&debug));
    assert_eq!(logged(&debug).len(), 5, "{}", all(&debug));
    let severe = run(&bin, &[("GARDEL_LOG_LEVEL", "severe"), ("GARDEL_TEST_VALUE", "")], &dir.0);
    assert_eq!(logged(&severe), ["severe: severe line"], "{}", all(&severe));
    // A variable set to nothing is an empty text, not `None`.
    assert_eq!(lines(&severe.stdout), ["env 1 set ", "env 2 unset"], "{}", all(&severe));
    let bad = run(&bin, &[("GARDEL_LOG_LEVEL", "loud")], &dir.0);
    assert_eq!(bad.status.code(), Some(2), "{}", all(&bad));
    assert!(bad.stdout.is_empty() && all(&bad).contains("GARDEL_LOG_LEVEL is `loud`"), "{}", all(&bad));
}

/// A script gives a value with a space, an unset variable, bytes that are not UTF-8 (`None`)
/// and an empty value, and its recording replays the same; a malformed token stops the
/// program with status 2.
#[test]
fn env_from_a_script() {
    let (dir, bin) = build("reactive/log_env.gdl", "script");
    // A script reads no variable: the one set here is not seen.
    let out = run(&bin, &[("GARDEL_EVENTS", "EnvRead:1:a%20b EnvMissing:2"), ("GARDEL_RECORD", "script.log"), ("GARDEL_TEST_UNSET", "real")], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out.stdout), ["env 1 set a b", "env 2 unset"]);
    let again = run(&bin, &[("GARDEL_REPLAY", "script.log")], &dir.0);
    assert_eq!(lines(&again.stdout), lines(&out.stdout), "{}", all(&again));
    let odd = run(&bin, &[("GARDEL_EVENTS", "EnvRead:1:%FF EnvRead:2:")], &dir.0);
    assert!(odd.status.success(), "{}", all(&odd));
    assert_eq!(lines(&odd.stdout), ["env 1 unset", "env 2 set "], "{}", all(&odd));
    for bad in ["EnvMissing:x", "EnvMissing:1:2", "EnvMissing:", "EnvRead:1", "EnvRead:x:a"] {
        let out = run(&bin, &[("GARDEL_EVENTS", bad)], &dir.0);
        assert_eq!(out.status.code(), Some(2), "{bad}: {}", all(&out));
    }
    // A script that ends before both answers come is a Shutdown, which stops with 4.
    let short = run(&bin, &[("GARDEL_EVENTS", "EnvMissing:1")], &dir.0);
    assert_eq!(short.status.code(), Some(4), "{}", all(&short));
}

/// `IO.env` gives the value as a `STRING`, which is printed, and `None` for an unset variable;
/// `IO.log` writes at and above the threshold, in order with what is printed.
#[test]
fn log_and_env_in_a_program() {
    let (dir, bin) = build("log_env_program.gdl", "program");
    let out = run(&bin, &[("GARDEL_TEST_VALUE", "v=1")], &dir.0);
    assert_eq!(out.status.code(), Some(0), "{}", all(&out));
    assert_eq!(lines(&out.stdout), ["v=1"]);
    assert_eq!(logged(&out), ["info: program info", "warn: after the value"]);
    let set = run(&bin, &[("GARDEL_TEST_VALUE", "x"), ("GARDEL_TEST_UNSET", "y"), ("GARDEL_LOG_LEVEL", "debug")], &dir.0);
    assert_eq!(set.status.code(), Some(1), "{}", all(&set));
    assert_eq!(logged(&set)[0], "debug: program debug");
    let unset = run(&bin, &[], &dir.0);
    assert_eq!(unset.status.code(), Some(2), "{}", all(&unset));
    let bad = run(&bin, &[("GARDEL_LOG_LEVEL", "x"), ("GARDEL_TEST_VALUE", "v")], &dir.0);
    assert_eq!(bad.status.code(), Some(2), "{}", all(&bad));
    assert!(all(&bad).contains("GARDEL_LOG_LEVEL is `x`"), "{}", all(&bad));

    // A value that is not text is `None`: bytes that are not UTF-8, or UTF-16 with a lone
    // surrogate on Windows.
    #[cfg(unix)]
    let odd = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![b'a', 0xff])
    };
    #[cfg(windows)]
    let odd = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[u16::from(b'a'), 0xD800])
    };
    let out = Command::new(&bin)
        .current_dir(&dir.0)
        .env("GARDEL_TEST_VALUE", "v")
        .env("GARDEL_TEST_ODD", &odd)
        .env_remove("GARDEL_TEST_UNSET")
        .output()
        .expect("the program runs");
    assert_eq!(out.status.code(), Some(0), "a value that is not text is not `None`: {}", all(&out));

    // `cmd` keeps each drive's directory in a variable named `=C:`; a name with `=` gives none.
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/c", &bin.to_string_lossy()])
            .current_dir(&dir.0)
            .env("GARDEL_TEST_VALUE", "v")
            .env_remove("GARDEL_TEST_UNSET")
            .env_remove("GARDEL_TEST_ODD")
            .output()
            .expect("the program runs");
        assert_eq!(out.status.code(), Some(0), "`=C:` was read: {}", all(&out));
    }

    // Printed text and log lines, to one file, come in the order the program gave them.
    let both = dir.0.join("both.txt");
    let file = fs::File::create(&both).expect("an output file");
    let status = Command::new(&bin)
        .current_dir(&dir.0)
        .env("GARDEL_TEST_VALUE", "v=1")
        .env_remove("GARDEL_TEST_UNSET")
        .env_remove("GARDEL_LOG_LEVEL")
        .stdout(file.try_clone().expect("the file again"))
        .stderr(file)
        .status()
        .expect("the program runs");
    assert_eq!(status.code(), Some(0));
    assert_eq!(lines(&fs::read(&both).expect("the output")), ["info: program info", "v=1", "warn: after the value"]);
}

const LIES: &str = "program ENV_LIES

feature {ANY}

  present(o: OPTION[STRING]): NAT
    require
      is_set: o.is_some
    do
      0
    end

  same(a: STRING, b: STRING): NAT
    require
      equal_reads: a.equals(b)
    do
      0
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        v <- IO.env(\"A\")
        w <- IO.env(\"A\")
        match v
          case Some(a) then
            match w
              case Some(b) then IO.exit(same(a, b))
              case None then IO.exit(1)
            end
          case None then IO.exit(present(v))
        end
      end
    end

end
";

/// A variable may be unset, and two reads of it may differ, so neither is assumed; a level of
/// another type is refused.
#[test]
fn what_the_environment_does_not_promise() {
    let (_dir, out) = check("lies.gdl", LIES, "lies");
    let text = all(&out);
    assert!(!out.status.success(), "{text}");
    for clause in ["is_set", "equal_reads"] {
        assert!(text.contains(&format!("Precondition Violation) in `main` ({clause})")), "{clause} refused in\n{text}");
    }
    let level = LIES.replace("v <- IO.env(\"A\")", "IO.log(3, \"x\")\n        v <- IO.env(\"A\")");
    let (_dir, out) = check("level.gdl", &level, "level");
    assert!(!out.status.success(), "a NAT taken as a level:\n{}", all(&out));
}
