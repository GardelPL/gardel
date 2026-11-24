//! Integration and negative-verification tests for `examples/posta`:
//! 1. `posta_verifies_and_runs_scripted_session`: verifies the full project (`gtui` + `ghttp` +
//!    `gjson` + `posta`) under `--strict`, builds `posta_bin`, and drives it through navigating
//!    the Collections `TREE_VIEW`, cycling HTTP methods, viewing Request Headers and Contracts
//!    tabs, verifying live `gjson` validation, opening Help, and clean quit.
//! 2. `posta_sabotage_demo_is_rejected_by_strict`: introduces a resource-leak bug into
//!    `IN_FLIGHT.cancel` in a temporary project copy and asserts that `gardelc --strict` refuses
//!    to compile with a Boogie verification failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn all(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn cache_dir() -> &'static Path {
    static CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let dir = std::env::temp_dir().join("gardelc_gtui_shared_cache");
        let _ = fs::create_dir_all(&dir);
        let secret = dir.join("machine.secret");
        if !secret.exists() {
            let _ = fs::write(&secret, [0x42_u8; 32]);
        }
        dir
    })
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_posta_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        Self(dir)
    }

    fn check(&self, project: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.0)
            .env("GARDEL_CACHE_DIR", cache_dir())
            .args(["--project", &project.to_string_lossy(), "--strict"])
            .output()
            .expect("gardelc runs")
    }

    fn bin(&self, program: &str) -> PathBuf {
        let ext = if cfg!(windows) { ".exe" } else { "" };
        self.0.join(format!("build/{program}_bin{ext}"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(&'[') = chars.peek() {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn posta_verifies_and_runs_scripted_session() {
    let work = TempDir::new("session");
    let project = root().join("examples/posta");
    let out = work.check(&project);
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));

    let bin = work.bin("posta");
    assert!(bin.exists(), "expected binary at {}", bin.display());

    // Drive: resize 108x30 -> Headers tab ('h') -> Contracts tab ('c') -> Body tab ('b') ->
    // Cycle method ('m') -> Navigate tree down (Down) -> Response headers tab ('2') ->
    // Response body tab ('1') -> Help ('?') -> Escape -> Quit ('q').
    let script = [
        "Resized:108:30",
        "Key:104", // 'h' -> Request Headers tab
        "Key:99",  // 'c' -> Contracts tab
        "Key:98",  // 'b' -> Body tab
        "Key:109", // 'm' -> Cycle HTTP method
        "Key:Down",
        "Key:50",  // '2' -> Response Headers tab
        "Key:49",  // '1' -> Response Body tab
        "Key:63",  // '?' -> Help popup
        "Key:Escape",
        "Key:113", // 'q' -> Quit
    ]
    .join(" ");

    let run_out = Command::new(&bin)
        .env("GARDEL_EVENTS", &script)
        .output()
        .expect("posta runs");
    assert!(run_out.status.success(), "{}", all(&run_out));

    let plain = strip_ansi(&String::from_utf8_lossy(&run_out.stdout));
    assert!(
        plain.contains("Collections") && plain.contains("In-Flight: 0/1 [PROVEN]"),
        "expected Collections and proven In-Flight badge in output:\n{plain}"
    );
    assert!(
        plain.contains("Valid JSON (gjson)"),
        "expected live gjson validation badge in output:\n{plain}"
    );
    assert!(
        plain.contains("bounded_in_flight"),
        "expected Contracts tab content in output:\n{plain}"
    );
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir");
    for entry in fs::read_dir(src).expect("readdir") {
        let entry = entry.expect("dir entry");
        let ty = entry.file_type().expect("file type");
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), to).expect("copy file");
        }
    }
}

#[test]
fn posta_sabotage_demo_is_rejected_by_strict() {
    let work = TempDir::new("sabotage");
    let copy = work.0.join("posta_sabotaged");
    copy_dir(&root().join("examples/posta"), &copy);

    let gtui_path = root().join("gtui").to_string_lossy().replace('\\', "/");
    let ghttp_path = root().join("ghttp").to_string_lossy().replace('\\', "/");
    let gjson_path = root().join("gjson").to_string_lossy().replace('\\', "/");
    fs::write(
        copy.join("Gardel.toml"),
        format!(
            "[project]\nname = \"posta\"\nversion = \"0.1.0\"\n\n[dependencies]\ngtui = {{ path = \"{gtui_path}\" }}\nghttp = {{ path = \"{ghttp_path}\" }}\ngjson = {{ path = \"{gjson_path}\" }}\n"
        ),
    )
    .expect("rewrite Gardel.toml");

    // Introduce a resource-leak bug in `IN_FLIGHT.cancel`: fail to transition back to Idle.
    let flight_path = copy.join("src/domain/in_flight.gdl");
    let src = fs::read_to_string(&flight_path).expect("read in_flight.gdl");
    let needle = "        let resp = RESPONSE_RECORD.from_error(\"Request cancelled by user\", Current.elapsed_ms)\n        FLIGHT_STEP.make(IN_FLIGHT.idle(Current.req_tag), cmds, Some(resp))";
    let broken = "        let resp = RESPONSE_RECORD.from_error(\"Request cancelled by user\", Current.elapsed_ms)\n        FLIGHT_STEP.make(Current, cmds, Some(resp))";
    assert!(src.contains(needle), "expected target line in in_flight.gdl");
    fs::write(&flight_path, src.replacen(needle, broken, 1)).expect("write sabotaged in_flight");

    let out = work.check(&copy);
    assert!(
        !out.status.success(),
        "expected --strict to reject IN_FLIGHT.cancel leak bug, but it succeeded:\n{}",
        all(&out)
    );
    let log = all(&out);
    assert!(
        log.contains("IN_FLIGHT") || log.contains("now_idle") || log.contains("Boogie"),
        "expected Boogie verification failure for IN_FLIGHT, got:\n{log}"
    );
}
