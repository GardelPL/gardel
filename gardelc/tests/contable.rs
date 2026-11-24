//! Integration and negative-verification tests for `examples/contable`:
//! 1. `contable_verifies_and_runs_scripted_session`: verifies the full project under `--strict`,
//!    builds `contable_bin`, and drives it through switching tabs (`Entry`, `Contracts`, `Budgets`),
//!    posting a new balanced entry via the modal, appending a compensating reversal entry,
//!    scrubbing history, switching preset books, opening help, and clean quit.
//! 2. `contable_sabotage_demo_is_rejected_by_strict`: introduces a 1-cent imbalance bug into
//!    `JOURNAL_ENTRY.make_split_debit2_credit1` in a temporary project copy and asserts that
//!    `gardelc --strict` refuses to compile with a Boogie verification failure.

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
        let dir =
            std::env::temp_dir().join(format!("gardelc_contable_{}_{name}", std::process::id()));
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
fn contable_verifies_and_runs_scripted_session() {
    let work = TempDir::new("session");
    let project = root().join("examples/contable");
    let out = work.check(&project);
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));

    let bin = work.bin("contable");
    assert!(bin.exists(), "expected binary at {}", bin.display());

    // Drive: resize 108x30 -> Entry tab ('e') -> Contracts tab ('c') -> Budgets tab ('b') ->
    // Open New Entry modal ('n') -> Submit default balanced transfer (Enter) ->
    // Reverse selected entry ('r') -> Confirm dialog (Enter) ->
    // Scrub back (Left) -> Jump to latest (End) -> Load Preset 2 ('2') ->
    // Help ('?') -> Escape -> Quit ('q').
    let script = [
        "Resized:108:30",
        "Key:101", // 'e' -> Entry Detail tab
        "Key:99",  // 'c' -> Contracts tab
        "Key:98",  // 'b' -> Budgets tab
        "Key:110", // 'n' -> Open Post Entry modal
        "Key:Enter",
        "Key:114", // 'r' -> Open Reverse confirmation dialog
        "Key:Enter",
        "Key:Left",
        "Key:End",
        "Key:50",  // '2' -> Personal Monthly Budget
        "Key:63",  // '?' -> Help popup
        "Key:Escape",
        "Key:113", // 'q' -> Quit
    ]
    .join(" ");

    let run_out = Command::new(&bin)
        .env("GARDEL_EVENTS", &script)
        .output()
        .expect("contable runs");
    assert!(run_out.status.success(), "{}", all(&run_out));

    let plain = strip_ansi(&String::from_utf8_lossy(&run_out.stdout));
    assert!(
        plain.contains("Freelance Studio") && plain.contains("Trial Balance = $0.00 [PROVEN]"),
        "expected title and proven trial balance badge in output:\n{plain}"
    );
    assert!(
        plain.contains("Posted #9"),
        "expected posted entry toast in output:\n{plain}"
    );
    assert!(
        plain.contains("Personal Monthly Budget"),
        "expected preset switch in output:\n{plain}"
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
fn contable_sabotage_demo_is_rejected_by_strict() {
    let work = TempDir::new("sabotage");
    let copy = work.0.join("contable_sabotaged");
    copy_dir(&root().join("examples/contable"), &copy);

    let gtui_path = root().join("gtui").to_string_lossy().replace('\\', "/");
    fs::write(
        copy.join("Gardel.toml"),
        format!(
            "[project]\nname = \"contable\"\nversion = \"0.1.0\"\n\n[dependencies]\ngtui = {{ path = \"{gtui_path}\" }}\n"
        ),
    )
    .expect("rewrite Gardel.toml");

    // Introduce a 1-cent imbalance bug in `JOURNAL_ENTRY.make_split_debit2_credit1`.
    let entry_path = copy.join("src/domain/journal_entry.gdl");
    let src = fs::read_to_string(&entry_path).expect("read journal_entry.gdl");
    let needle = "let p2 = POSTING.debit(dr2_acct, dr2_amt)";
    let broken = "let p2 = POSTING.debit(dr2_acct, if dr2_amt > 1 then dr2_amt - 1 else 1 end)";
    assert!(src.contains(needle), "expected target line in journal_entry.gdl");
    fs::write(&entry_path, src.replacen(needle, broken, 1)).expect("write sabotaged entry");

    let out = work.check(&copy);
    assert!(
        !out.status.success(),
        "expected --strict to reject 1-cent imbalance bug, but it succeeded:\n{}",
        all(&out)
    );
    let log = all(&out);
    assert!(
        log.contains("JOURNAL_ENTRY") || log.contains("balanced") || log.contains("Boogie"),
        "expected Boogie verification failure for JOURNAL_ENTRY, got:\n{log}"
    );
}
