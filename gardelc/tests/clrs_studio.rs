//! Integration and negative-verification tests for `examples/clrs_studio`:
//! 1. `clrs_studio_verifies_and_runs_scripted_session`: verifies the full project under
//!    `--strict`, builds `clrs_studio_bin`, and drives it through stepping, back, jump-to-end
//!    (verifying the `is_sorted` postcondition banner/toast), tab switching (`Code`, `Short Form`,
//!    `Invariants`), preset switching, theme cycling, help popup, and clean quit.
//! 2. `clrs_studio_sabotage_demo_is_rejected_by_strict`: introduces a classic sorting bug into
//!    `INSERTION_SORT.step_compare_shift` (flipping `>` to `<`) in a temporary project copy and
//!    asserts that `gardelc --strict` refuses to compile with a Boogie verification failure.

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
        let dir = std::env::temp_dir().join(format!("gardelc_clrs_{}_{name}", std::process::id()));
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

/// Strip ANSI CSI sequences from terminal output so text assertions are straightforward.
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
fn clrs_studio_verifies_and_runs_scripted_session() {
    let work = TempDir::new("session");
    let project = root().join("examples/clrs_studio");
    let out = work.check(&project);
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));

    let bin = work.bin("clrs_studio");
    assert!(bin.exists(), "expected binary at {}", bin.display());

    // Drive: resize 100x28 -> step forward -> step back -> jump to End (Done) ->
    // Code tab ('c') -> Short Form tab ('s') -> Invariants tab ('i') ->
    // Preset 2 ('2') -> Help ('?') -> Escape -> Quit ('q').
    let script = [
        "Resized:100:28",
        "Key:Right",
        "Key:Right",
        "Key:Left",
        "Key:End",
        "Key:99",      // 'c' -> Code tab
        "Key:115",     // 's' -> Short Form tab
        "Key:105",     // 'i' -> Invariants tab
        "Key:50",      // '2' -> Reversed (Worst) preset
        "Key:97",      // 'a' -> Switch to Dijkstra Shortest Path (CLRS Ch. 24.3)
        "Key:Right",
        "Key:End",
        "Key:63",      // '?' -> Help popup
        "Key:Escape",
        "Key:113",     // 'q' -> Quit
    ]
    .join(" ");

    let run_out = Command::new(&bin)
        .env("GARDEL_EVENTS", &script)
        .output()
        .expect("clrs_studio runs");
    assert!(run_out.status.success(), "{}", all(&run_out));

    let plain = strip_ansi(&String::from_utf8_lossy(&run_out.stdout));
    assert!(
        plain.contains("Insertion Sort") && plain.contains("CLRS Ch. 2.1"),
        "expected title in output:\n{plain}"
    );
    assert!(
        plain.contains("[PROVEN]"),
        "expected proven invariant badges in output:\n{plain}"
    );
    assert!(
        plain.contains("ensure: is_sorted(A)"),
        "expected Done postcondition badge after End key:\n{plain}"
    );
    assert!(
        plain.contains("step: INSERTION_SORT"),
        "expected Code tab content after 'c' key:\n{plain}"
    );
    assert!(
        plain.contains("module INSERTION_SORT"),
        "expected Short Form tab content after 's' key:\n{plain}"
    );
    assert!(
        plain.contains("Reversed (Worst)"),
        "expected preset switch after '2' key:\n{plain}"
    );
    assert!(
        plain.contains("Dijkstra Shortest Path") && plain.contains("CLRS Fig. 22.6"),
        "expected Dijkstra view after 'a' key:\n{plain}"
    );
    assert!(
        plain.contains("[y:5]")
            && plain.contains("[z:7]")
            && plain.contains("[t:8]")
            && plain.contains("[x:9]")
            && plain.contains("30 / 45")
            && plain.contains("Settled"),
        "expected exact CLRS Fig. 24.6 shortest paths and AutoCLRS tick bound (30 / 45):\n{plain}"
    );
    assert!(
        plain.contains("CLRS Studio Controls"),
        "expected Help popup after '?' key:\n{plain}"
    );
}

#[test]
fn clrs_studio_sabotage_demo_is_rejected_by_strict() {
    let work = TempDir::new("sabotage");
    let src_dir = work.0.join("proj/src/algo");
    fs::create_dir_all(&src_dir).expect("create sabotage src dir");

    let gtui_path = root().join("gtui").to_string_lossy().replace('\\', "/");
    let toml = format!(
        "[project]\nname = \"clrs-sabotage\"\nversion = \"0.1.0\"\n\n[dependencies]\ngtui = {{ path = \"{gtui_path}\" }}\n"
    );
    fs::write(work.0.join("proj/Gardel.toml"), toml).expect("write Gardel.toml");

    // Copy SORT_FACTS verbatim, and copy INSERTION_SORT with a classic 1-character bug:
    // flip `>` to `<` in `step_compare_shift`.
    let algo_root = root().join("examples/clrs_studio/src/algo");
    fs::copy(algo_root.join("sort_facts.gdl"), src_dir.join("sort_facts.gdl"))
        .expect("copy sort_facts.gdl");

    let good_sort =
        fs::read_to_string(algo_root.join("insertion_sort.gdl")).expect("read insertion_sort.gdl");
    let needle = "if Current.hole > 0 and LIST_OPS.at(Current.items, Current.hole - 1) > Current.key then";
    let sabotaged = good_sort.replace(
        needle,
        "if Current.hole > 0 and LIST_OPS.at(Current.items, Current.hole - 1) < Current.key then",
    );
    assert_ne!(
        good_sort, sabotaged,
        "sabotage replacement must modify insertion_sort.gdl"
    );
    fs::write(src_dir.join("insertion_sort.gdl"), sabotaged).expect("write sabotaged file");

    let out = work.check(&work.0.join("proj"));
    let text = all(&out);
    assert!(
        !out.status.success(),
        "expected gardelc --strict to reject sabotaged Insertion Sort, but it succeeded:\n{text}"
    );
    assert!(
        text.contains("Formal verification condition")
            && (text.contains("greater_inner_right")
                || text.contains("insert_ready")
                || text.contains("step_compare_shift")),
        "expected Boogie contract violation in step_compare_shift, got:\n{text}"
    );
}
