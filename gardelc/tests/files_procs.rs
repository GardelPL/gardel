//! Files and processes (Task F.11a): `ReadFile`, `ListDir`, `RunProcess` with `WriteProcess`,
//! `CloseInput` and `StopProcess`, and `RunAttached`, run for real, from a script, and from a
//! recorded log with no file read and no process run.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

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

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reactive")
}

/// `program`, verified under `--strict` and built, and the child it runs, in a directory of
/// their own.
fn build(program: &str, name: &str) -> (Dir, PathBuf, PathBuf) {
    let dir = Dir(std::env::temp_dir().join(format!("gardelc_files_{}_{name}", std::process::id())));
    drop(fs::remove_dir_all(&dir.0));
    fs::create_dir_all(&dir.0).expect("temporary directory");
    fs::copy(tests_dir().join(program), dir.0.join(program)).expect("the program");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir.0)
        .env("GARDEL_CACHE_DIR", dir.0.join("cache"))
        .args([program, "--strict"])
        .output()
        .expect("gardelc runs");
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let stem = program.trim_end_matches(".gdl");
    let bin = dir.0.join(format!("build/{stem}_bin{}", std::env::consts::EXE_SUFFIX));
    assert!(bin.exists(), "no binary at {}:\n{}", bin.display(), all(&out));
    let child = dir.0.join(format!("child{}", std::env::consts::EXE_SUFFIX));
    let cc = Command::new("gcc")
        .arg("-O1")
        .arg(tests_dir().join("files_child.c"))
        .arg("-o")
        .arg(&child)
        .output()
        .expect("gcc runs");
    assert!(cc.status.success(), "{}", all(&cc));
    (dir, bin, child)
}

fn run(bin: &Path, args: &[&str], env: &[(&str, &str)], cwd: &Path) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    finish(cmd.spawn().expect("the program runs"))
}

/// The output of `p`, which must end within a minute: a child that is never stopped, or an
/// input never closed, would otherwise hang the test.
fn finish(mut p: std::process::Child) -> Output {
    use std::io::Read;
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                drop(r.read_to_end(&mut buf));
            }
            buf
        })
    }
    let out = drain(p.stdout.take());
    let err = drain(p.stderr.take());
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(st) = p.try_wait().expect("a wait") {
            break st;
        }
        if start.elapsed() > Duration::from_secs(60) {
            drop(p.kill());
            let st = p.wait().expect("it ends");
            let got = Output { status: st, stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() };
            panic!("the program did not end within a minute:\n{}", all(&got));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Output { status, stdout: out.join().expect("its output"), stderr: err.join().expect("its errors") }
}

fn has(out: &[String], line: &str) -> bool {
    out.iter().any(|l| l == line)
}

#[test]
fn files_and_processes_give_their_events_and_a_replay_runs_none() {
    let (dir, bin, child) = build("files_procs.gdl", "real");
    let data = dir.0.join("data");
    fs::create_dir_all(data.join("sub")).expect("a directory");
    fs::write(data.join("note.txt"), "file body\n").expect("a file");
    fs::write(data.join("b.txt"), "x").expect("a file");
    // Byte order puts `Upper.txt` first; NTFS, which ignores case, would put it last.
    fs::write(data.join("Upper.txt"), "x").expect("a file");
    let log = dir.0.join("run.log");
    let child_arg = child.to_string_lossy().into_owned();
    let out = run(&bin, &["data", "data/note.txt", &child_arg], &[("GARDEL_RECORD", "run.log")], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    let got = lines(&out);

    // The listing, in byte order of the names, with each entry's kind.
    let at = got.iter().position(|l| l == "file Upper.txt").unwrap_or_else(|| panic!("{}", all(&out)));
    assert_eq!(got[at..at + 4], ["file Upper.txt", "file b.txt", "file note.txt", "dir sub"], "{}", all(&out));
    // A file read whole, and one missing.
    assert!(has(&got, "read file body"), "{}", all(&out));
    assert!(has(&got, "read failed 6"), "{}", all(&out));
    // Arguments as given, a space, a quote, a backslash and an empty one among them; the
    // directory; a line written to the child and then the end of its input; standard error;
    // and the exit code.
    for line in ["started 3", "out arg [a b]", "arg [c\"d]", "arg [e\\]", "arg []", "in [hello child]", "out input ended", "err to stderr", "exited 7"] {
        assert!(has(&got, line), "no `{line}`:\n{}", all(&out));
    }
    let cwd = got.iter().find(|l| l.starts_with("cwd [")).unwrap_or_else(|| panic!("{}", all(&out)));
    assert!(cwd.trim_end_matches(']').replace('\\', "/").ends_with("/data"), "{cwd}");
    // A child stopped: TerminateJobObject's code on Windows, SIGKILL elsewhere.
    assert!(has(&got, "started 4"), "{}", all(&out));
    assert!(has(&got, if cfg!(windows) { "exited 1" } else { "exited 137" }), "{}", all(&out));
    // A program that does not exist.
    assert!(has(&got, "failed 5"), "{}", all(&out));
    // An attached child writes to the terminal itself.
    assert!(has(&got, "attached child") && has(&got, "attached ended 3"), "{}", all(&out));
    assert!(has(&got, "done"), "{}", all(&out));

    let text = fs::read_to_string(&log).expect("the log");
    for line in [
        "command ListDir:1:data",
        "command ReadFile:2:data%2Fnote%2Etxt",
        "event DirListed:1:fUpper%2Etxt%2Ffb%2Etxt%2Ffnote%2Etxt%2Fdsub%2F",
        "event FileRead:2:file%20body%0A",
        "command WriteProcess:1:hello%20child%0A",
        "command CloseInput:1",
        "event ProcessStarted:3:1",
        "event ProcessExited:1:7",
        "event ProcessFailed:5:2",
        "event AttachedEnded:7:3",
    ] {
        assert!(text.lines().any(|l| l == line), "the log lacks `{line}`:\n{text}");
    }
    let quoted: String = child_arg.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_string() } else { format!("%{:02X}", c as u32) }).collect();
    let run3 = format!("command RunProcess:3:{quoted}:data:a%20b:c%22d:e%5C:");
    assert!(text.lines().any(|l| l == run3), "the log lacks `{run3}`:\n{text}");

    // The replay reads no file and runs no process: with the files gone, it gives the same
    // output, apart from what the attached child wrote itself.
    fs::remove_dir_all(&data).expect("the files go");
    let again = run(&bin, &[], &[("GARDEL_REPLAY", "run.log")], &dir.0);
    assert!(again.status.success(), "{}", all(&again));
    let expected: Vec<String> = got.iter().filter(|l| *l != "attached child").cloned().collect();
    assert_eq!(lines(&again), expected, "{}", all(&again));
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(err.contains("proc: RunProcess 3 ") && err.contains("file: ReadFile 2 data%2Fnote%2Etxt"), "{err}");
}

/// A script stands for the files and the processes: the commands are written out and the
/// events read in.
#[test]
fn a_script_gives_file_and_process_events() {
    let (dir, bin, _child) = build("files_procs.gdl", "script");
    let script = "DirListed:1:fb.txt/dsub/ FileRead:2:hi FileReadFailed:6:2 ProcessStarted:3:9 ProcessOutput:9:x \
                  ProcessErrorOutput:9:y ProcessFailed:5:2 ProcessStarted:4:10 ProcessExited:10:1 AttachFailed:7:170 \
                  ProcessExited:9:7";
    let out = run(&bin, &["d", "f", "c"], &[("GARDEL_EVENTS", script)], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(
        lines(&out),
        [
            "file b.txt", "dir sub", "read hi", "read failed 6", "started 3", "out x", "err y", "failed 5", "started 4",
            "exited 1", "attach failed", "done", "exited 7"
        ],
        "{}",
        all(&out)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    for line in ["file: ListDir 1 d", "proc: RunProcess 3 c a%20b c%22d e%5C  in d", "proc: WriteProcess 9 hello%20child%0A", "proc: StopProcess 10"] {
        assert!(err.lines().any(|l| l == line), "no `{line}`:\n{err}");
    }
    // A name that is not UTF-8 is left out of a listing, from a script as from the system.
    let out = run(&bin, &["d", "f", "c"], &[("GARDEL_EVENTS", "DirListed:1:fok/f%FF/ Shutdown")], &dir.0);
    assert_eq!(lines(&out), ["file ok"], "{}", all(&out));
    for bad in ["DirListed:1:xb/", "DirListed:1:fb", "DirListed:1:f/", "FileRead:2", "ProcessExited:9:x", "AttachedEnded:7:", "ProcessStarted:3:1:2"] {
        let out = run(&bin, &["d", "f", "c"], &[("GARDEL_EVENTS", bad)], &dir.0);
        assert_eq!(out.status.code(), Some(2), "`{bad}` must be refused:\n{}", all(&out));
    }
}

/// While a child has the terminal, the program's reader takes none of its keys, even one
/// already waiting for a key: after `r` the program asks for a key and runs a child attached,
/// which reads `hello`; the program gets `l` after it. A `ReadLine` waiting refuses a
/// RunAttached, as its read cannot be called back.
#[test]
fn an_attached_child_gets_the_keys_the_program_was_waiting_for() {
    let (dir, bin, child) = build("attach_keys.gdl", "attach");
    let mut p = Command::new(&bin)
        .arg(&child)
        .current_dir(&dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program runs");
    let mut input = p.stdin.take().expect("its input");
    for (chunk, pause) in [("r", 600), ("hello\n", 600), ("l", 400), ("typed line\n", 400), ("z", 0)] {
        input.write_all(chunk.as_bytes()).expect("a write");
        input.flush().expect("a flush");
        std::thread::sleep(Duration::from_millis(pause));
    }
    drop(input);
    let out = finish(p);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(
        lines(&out),
        ["got r", "attaching", "child read [hello]", "attached ended 2", "got l", "attach refused 3", "line typed line", "got 122"],
        "{}",
        all(&out)
    );
}

/// A child that writes far more than the host keeps queued: every byte arrives, in order.
#[test]
fn a_flood_of_output_arrives_whole_and_in_order() {
    let (dir, bin, child) = build("proc_flood.gdl", "flood");
    let out = run(&bin, &[&child.to_string_lossy(), "30000000"], &[], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    assert_eq!(lines(&out), ["30000000"], "{}", all(&out));
}

/// A child that ends at once, leaving a child of its own that writes half a second later:
/// `ProcessExited` waits for the end of the output, not of the process, so nothing comes
/// after it.
#[test]
fn a_process_ends_after_all_its_output() {
    let (dir, bin, child) = build("proc_order.gdl", "order");
    let out = run(&bin, &[&child.to_string_lossy()], &[], &dir.0);
    assert!(out.status.success(), "{}", all(&out));
    let got: Vec<String> = lines(&out).into_iter().filter(|l| !l.is_empty()).collect();
    assert_eq!(got, ["late", "exited"], "{}", all(&out));
}

/// WriteFile (Task F.16a): a new file and a replaced one are written whole, through a file beside
/// them that is renamed over them and leaves nothing behind; a missing directory and a path
/// that is a directory fail; and a replay writes nothing.
#[test]
fn write_file_replaces_a_file_whole_and_a_replay_writes_none() {
    let (dir, bin, _child) = build("write_file.gdl", "write");
    let data = dir.0.join("data");
    fs::create_dir_all(&data).expect("a directory");
    fs::write(data.join("old.txt"), "old content that is longer").expect("a file");
    let out = run(&bin, &[], &[("GARDEL_RECORD", "../run.log")], &data);
    assert!(out.status.success(), "{}", all(&out));
    let got = lines(&out);
    for line in ["written 1 17", "written 2 8", "failed 3", "failed 4", "read first line", "second", "done"] {
        assert!(has(&got, line), "{line} in {got:?}");
    }
    assert_eq!(fs::read_to_string(data.join("new.txt")).expect("written"), "first line\nsecond");
    assert_eq!(fs::read_to_string(data.join("old.txt")).expect("replaced"), "replaced");
    let mut names: Vec<String> = fs::read_dir(&data).expect("a listing").map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["new.txt", "old.txt"], "no file is left beside them");

    let log = fs::read_to_string(dir.0.join("run.log")).expect("a log");
    for line in ["command WriteFile:1:%2E%2Fnew%2Etxt:first%20line%0Asecond\n", "event FileWritten:1:17\n", "event FileWritten:2:8\n", "event WriteFailed:3:", "event WriteFailed:4:"] {
        assert!(log.contains(line), "{line} in\n{log}");
    }

    // The replay gives the same output from the log, and touches no file.
    fs::remove_file(data.join("new.txt")).expect("removed");
    fs::write(data.join("old.txt"), "changed").expect("a file");
    let again = run(&bin, &[], &[("GARDEL_REPLAY", "../run.log")], &data);
    assert!(again.status.success(), "{}", all(&again));
    assert_eq!(lines(&again), got, "{}", all(&again));
    assert!(!data.join("new.txt").exists(), "a replay wrote a file");
    assert_eq!(fs::read_to_string(data.join("old.txt")).expect("kept"), "changed");
    assert!(String::from_utf8_lossy(&again.stderr).contains("file: WriteFile 1 %2E%2Fnew%2Etxt first%20line%0Asecond"), "{}", all(&again));
}
