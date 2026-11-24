//! Integration and negative-verification tests for `examples/tablero`:
//! 1. `tablero_verifies_and_runs_scripted_session`: verifies the full project under `--strict`,
//!    builds `tablero_bin`, and drives it through verifying the initial 8x8 parallel solve
//!    (92 solutions), switching to the Solution and Contracts tabs, loading the Pinned Corner and
//!    Diagonal Conflict (UNSAT) presets, opening Help, and clean quit.
//! 2. `tablero_sabotage_demo_is_rejected_by_strict`: removes the `safe_against` attack check in
//!    `QUEENS_SOLVER.solve_range` in a temporary project copy and asserts that `gardelc --strict`
//!    refuses to compile with a Boogie verification failure on `sound`.

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
            std::env::temp_dir().join(format!("gardelc_tablero_{}_{name}", std::process::id()));
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
fn tablero_verifies_and_runs_scripted_session() {
    let work = TempDir::new("session");
    let project = root().join("examples/tablero");
    let out = work.check(&project);
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));

    let bin = work.bin("tablero");
    assert!(bin.exists(), "expected binary at {}", bin.display());

    // Drive: resize 104x28 -> Solution tab ('s') -> Contracts tab ('v') -> Proofs tab ('p') ->
    // Preset 2 Pinned Corner ('2') -> Preset 4 Diagonal Conflict UNSAT ('4') ->
    // Help ('?') -> Escape -> Quit ('q').
    let script = [
        "Resized:104:28",
        "Key:32",  // Space -> Run parallel OpenMP solver on Classic 8x8 (92 solutions)
        "Key:115", // 's' -> Solution coordinates tab
        "Key:118", // 'v' -> Contracts tab
        "Key:112", // 'p' -> Proofs tab
        "Key:50",  // '2' -> Pinned Corner 8x8 (4 solutions)
        "Key:52",  // '4' -> Diagonal Conflict 6x6 (UNSAT)
        "Key:63",  // '?' -> Help popup
        "Key:Escape",
        "Key:113", // 'q' -> Quit
    ]
    .join(" ");

    let run_out = Command::new(&bin)
        .env("GARDEL_EVENTS", &script)
        .output()
        .expect("tablero runs");
    assert!(run_out.status.success(), "{}", all(&run_out));

    let plain = strip_ansi(&String::from_utf8_lossy(&run_out.stdout));
    assert!(
        plain.contains("92 solutions") && plain.contains("Soundness & Termination [PROVEN]"),
        "expected classic 8x8 92 solutions and proven badge in output:\n{plain}"
    );
    assert!(
        plain.contains("4 solutions"),
        "expected Pinned Corner 8x8 (4 solutions) in output:\n{plain}"
    );
    assert!(
        plain.contains("UNSAT"),
        "expected UNSAT result for diagonal conflict preset in output:\n{plain}"
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
fn tablero_sabotage_demo_is_rejected_by_strict() {
    let work = TempDir::new("sabotage");
    let copy = work.0.join("tablero_sabotaged");
    copy_dir(&root().join("examples/tablero"), &copy);

    let gtui_path = root().join("gtui").to_string_lossy().replace('\\', "/");
    fs::write(
        copy.join("Gardel.toml"),
        format!(
            "[project]\nname = \"tablero\"\nversion = \"0.1.0\"\n\n[dependencies]\ngtui = {{ path = \"{gtui_path}\" }}\n"
        ),
    )
    .expect("rewrite Gardel.toml");

    // Remove the pairwise attack check in `QUEENS_SOLVER.solve_range`.
    let solver_path = copy.join("src/domain/queens_solver.gdl");
    let src = fs::read_to_string(&solver_path).expect("read queens_solver.gdl");
    let needle = "if QUEENS_FACTS.cell_allowed(board, r, c) and QUEENS_FACTS.safe_against(r, c, placed) then";
    let broken = "if QUEENS_FACTS.cell_allowed(board, r, c) then";
    assert!(src.contains(needle), "expected target line in queens_solver.gdl");
    fs::write(&solver_path, src.replacen(needle, broken, 1)).expect("write sabotaged solver");

    let out = work.check(&copy);
    assert!(
        !out.status.success(),
        "expected --strict to reject missing attack check, but it succeeded:\n{}",
        all(&out)
    );
    let log = all(&out);
    assert!(
        log.contains("QUEENS_SOLVER") || log.contains("sound") || log.contains("Boogie"),
        "expected Boogie verification failure for QUEENS_SOLVER, got:\n{log}"
    );
}
