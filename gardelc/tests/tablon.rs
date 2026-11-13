//! Tablon (Task F.11d), `tools/tablon`: it verifies under `--strict`; a scripted session on the
//! fixture of `tests/inspect/` lists the modules the inspection gives and the files of `src`,
//! shows each view of a module, takes Enter from a feature to its line in the source, marked,
//! and from a client or supplier to that module; keys work while the inspection runs and a read
//! no longer awaited is dropped; and a replay reproduces the screen with no process run.
//!
//! The scripts stand in for `gardelc`: the inspection's output is the fixture's expected object,
//! and a file read is the fixture's file, so the tests need no compiler run but the build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/inspect")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// Tablon, verified and built once for every test, in a directory of its own.
fn tablon() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("gardelc_tablon_{}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&dir)
            .env("GARDEL_CACHE_DIR", dir.join("cache"))
            .args(["--project", &root().join("tools/tablon").to_string_lossy(), "--strict"])
            .output()
            .expect("gardelc runs");
        assert!(out.status.success(), "{}", all(&out));
        assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
        dir.join("build/tablon_bin.exe")
    })
}

/// `bytes` as a script writes them: `%XX` for anything but a letter, a digit, `-`, `_` or `.`.
fn enc(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn keys(text: &str) -> String {
    text.chars().map(|c| format!("Key:{} ", c as u32)).collect()
}

fn file(name: &str) -> String {
    enc(&fs::read(fixture().join(name)).expect("a fixture file"))
}

const W: usize = 110;
const H: usize = 30;

/// The start of every session: the size, the inspection's output and its end, and the listings
/// of `src` and of `src/sub`.
fn start() -> String {
    let json = fs::read(fixture().join("expected.json")).expect("the expected object");
    format!(
        "Resized:{W}:{H} ProcessStarted:1:7 ProcessErrorOutput:7:{} ProcessOutput:7:{} ProcessExited:7:0 \
         DirListed:100:fuse_cell.gdl/dsub/fcell.gdl/ DirListed:101:fnote.txt/ ",
        enc(b"[1/5] Linked\n"),
        enc(&json)
    )
}

/// Keys that choose the module on row `row` of the Modules tree, after the start, and the read
/// of its file, tagged `tag`.
fn choose(rows_down: usize, tag: usize, name: &str) -> String {
    format!("{}Key:Enter FileRead:{tag}:{} ", "Key:Down ".repeat(rows_down), file(name))
}

/// Whether the log of a scripted run has a command line that starts with `start` and ends
/// with `end`. The runtime writes a line end of the platform's own.
fn sent(log: &str, start: &str, end: &str) -> bool {
    log.lines().any(|l| l.starts_with(start) && l.ends_with(end))
}

fn run(script: &str, env: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new(tablon());
    cmd.env("GARDEL_EVENTS", script).args([&fixture().to_string_lossy().to_string(), "gardelc"]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("Tablon runs")
}

/// A cell as a terminal shows it: a character and the SGR parameters in force.
type Cell = (char, Vec<u32>);

/// Enough of a VT terminal to read a frame, as in `tests/gtui.rs`: every character must land
/// inside the screen of the size the script gave.
struct Terminal {
    grid: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    pen: Vec<u32>,
    before_clear: Vec<Vec<Vec<Cell>>>,
}

impl Terminal {
    fn read(bytes: &[u8]) -> Self {
        let mut t = Terminal { grid: Vec::new(), row: 0, col: 0, pen: Vec::new(), before_clear: Vec::new() };
        let text = std::str::from_utf8(bytes).expect("the output is UTF-8");
        let cs: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '\u{1b}' {
                assert_eq!(cs.get(i + 1), Some(&'['), "only CSI sequences, at char {i}");
                let mut j = i + 2;
                let private = cs.get(j) == Some(&'?');
                if private {
                    j += 1;
                }
                let start = j;
                while j < cs.len() && (cs[j].is_ascii_digit() || cs[j] == ';') {
                    j += 1;
                }
                let params: Vec<u32> = cs[start..j].iter().collect::<String>().split(';').map(|p| p.parse().unwrap_or(0)).collect();
                match (private, cs[j]) {
                    (true, 'h') | (true, 'l') => {}
                    (false, 'H') => {
                        assert!(params[0] as usize >= 1 && params[0] as usize <= H && params[1] as usize >= 1 && params[1] as usize <= W);
                        t.row = params[0] as usize - 1;
                        t.col = params[1] as usize - 1;
                    }
                    (false, 'm') => t.pen = params.into_iter().filter(|p| *p != 0).collect(),
                    (false, 'J') => {
                        t.before_clear.push(t.grid.clone());
                        t.grid = vec![vec![(' ', Vec::new()); W]; H];
                    }
                    other => panic!("unexpected sequence {other:?}"),
                }
                i = j + 1;
            } else {
                assert!(!c.is_control(), "a control character {c:?} was written");
                assert!(t.row < H && t.col < W, "{c:?} written outside the screen");
                t.grid[t.row][t.col] = (c, t.pen.clone());
                t.col += 1;
                i += 1;
            }
        }
        t
    }

    fn text(&self) -> String {
        self.grid.iter().map(|r| r.iter().map(|c| c.0).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// The row that holds `needle`.
    fn row_of(&self, needle: &str) -> &Vec<Cell> {
        self.grid
            .iter()
            .find(|r| r.iter().map(|c| c.0).collect::<String>().contains(needle))
            .unwrap_or_else(|| panic!("no row holds {needle:?}:\n{}", self.text()))
    }
}

/// The screen a session leaves, after a resize to the same size, which draws it whole: the
/// cells the diffs left before it must be the same.
fn screen(steps: &str) -> (Terminal, String) {
    let script = format!("{}{steps}Resized:{W}:{H}", start());
    let ran = run(&script, &[]);
    assert_eq!(ran.status.code(), Some(1), "the end of a script is a shutdown: {}", all(&ran));
    let term = Terminal::read(&ran.stdout);
    let before = term.before_clear.last().expect("a last clear");
    assert_eq!(before, &term.grid, "the diffs left another screen than a full redraw:\n{}", term.text());
    (term, String::from_utf8_lossy(&ran.stderr).to_string())
}

#[test]
fn the_inspection_fills_the_trees_and_enter_goes_from_a_feature_to_its_line() {
    let (term, log) = screen(&format!("{}Key:Tab {}Key:Down Key:Enter ", choose(2, 1001, "cell.gdl"), keys("f")));
    let shown = term.text();
    // The inspection runs with the project as its directory, and `src` and `src/sub` are listed.
    assert!(log.contains("proc: RunProcess 1 gardelc %2D%2Dproject %2E %2D%2Dinspect %2D%2Djson%2Ddiagnostics %2D%2Dno%2Dverify in "), "{log}");
    assert!(sent(&log, "file: ListDir 100 ", "%2F%2E%2Fsrc"), "{log}");
    assert!(sent(&log, "file: ListDir 101 ", "%2F%2E%2Fsrc%2Fsub"), "{log}");
    assert!(sent(&log, "file: ReadFile 1001 ", "%2F%2E%2Fcell%2Egdl"), "{log}");
    // The project's package, its kinds and its modules.
    for row in ["▾ project", "▾ modules", "CELL ", "▾ functions", "USE_CELL"] {
        assert!(shown.contains(row), "{row:?} is not shown:\n{shown}");
    }
    assert!(shown.contains("CELL - Text"), "{shown}");
    assert!(shown.contains("Inspected: 2 modules.") && shown.contains("[1/5] Linked") && shown.contains("2 modules "), "{shown}");
    // `measure` is the second feature, declared on line 31: it is marked on blue, its keywords too.
    let marked = term.row_of("31   measure: NAT");
    let start = marked.iter().position(|c| c.0 == '3').expect("the line's number");
    assert!(marked[start..].iter().take(40).all(|c| c.1.contains(&44)), "the line is not marked across: {marked:?}");
    assert!(shown.contains("28       measure: Result <= 1000"), "a few lines before it are shown:\n{shown}");
}

#[test]
fn every_view_of_a_module_shows_its_parts() {
    let cell = choose(2, 1001, "cell.gdl");
    let cases: [(&str, &[&str]); 9] = [
        ("s", &["CELL - Short", "module CELL", "require small: (v <= 1000)", "ensure  same: (Result = Current.size)"]),
        ("f", &["CELL - Features", "size: NAT", "measure: NAT"]),
        ("n", &["CELL - Creators", "make(v: NAT): CELL"]),
        ("l", &["CELL - Laws", "size_bounded(c: CELL)"]),
        ("i", &["CELL - Invariants", "bounded: (Current.value <= 1000)"]),
        ("a", &["CELL - Concepts", "│MEASURED", "│SIZED"]),
        ("d", &["CELL - Instances", "│MEASURED", "│  none other"]),
        ("c", &["CELL - Clients", "│USE_CELL"]),
        ("u", &["CELL - Suppliers", "Uses no other module."]),
    ];
    for (key, rows) in cases {
        let (term, _) = screen(&format!("{cell}Key:Tab {}", keys(key)));
        let shown = term.text();
        for row in rows {
            assert!(shown.contains(row), "view {key}: {row:?} is not shown:\n{shown}");
        }
    }
    // The chosen tab is shown even when the row is too narrow for all ten.
    let (term, _) = screen(&format!("{cell}Key:Tab {}", keys("u")));
    assert!(term.text().contains("Suppliers(u)"), "{}", term.text());
}

#[test]
fn enter_on_a_supplier_chooses_it_and_reads_its_file() {
    let (term, log) = screen(&format!("{}Key:Tab {}Key:Enter FileRead:1002:{} ", choose(4, 1001, "use_cell.gdl"), keys("u"), file("cell.gdl")));
    let shown = term.text();
    assert!(shown.contains("CELL - Suppliers") && shown.contains("Uses no other module."), "{shown}");
    assert!(sent(&log, "file: ReadFile 1001 ", "%2F%2E%2Fuse%5Fcell%2Egdl"), "{log}");
    assert!(sent(&log, "file: ReadFile 1002 ", "%2F%2E%2Fcell%2Egdl"), "{log}");
    let (term, _) = screen(&format!("{}Key:Tab {}Key:Enter FileRead:1002:{} {}", choose(4, 1001, "use_cell.gdl"), keys("u"), file("cell.gdl"), keys("t")));
    assert!(term.text().contains(" 2 module CELL satisfies MEASURED"), "{}", term.text());
}

#[test]
fn the_files_tree_lists_directories_first_and_opens_a_file_as_text() {
    let (term, log) = screen(&format!("{}Key:Down Key:Right Key:Down Key:Down Key:Enter FileRead:1001:{} ", keys("f"), enc(b"some notes\r\nline two\n")));
    let shown = term.text();
    let rows: Vec<&str> = shown.lines().map(|l| l.trim_end()).collect();
    let at = |s: &str| rows.iter().position(|r| r.contains(s)).unwrap_or_else(|| panic!("no {s:?}:\n{shown}"));
    assert!(at("▾ src") < at("▾ sub") && at("▾ sub") < at("note.txt") && at("note.txt") < at("│    cell.gdl") && at("│    cell.gdl") < at("use_cell.gdl"), "{shown}");
    // The fixture has no `src/cell.gdl` module: the file is shown as text, a CR dropped.
    assert!(sent(&log, "file: ReadFile 1001 ", "%2F%2E%2Fsrc%2Fcell%2Egdl"), "{log}");
    assert!(shown.contains("─ src/cell.gdl ─") && shown.contains("1 some notes ") && shown.contains("2 line two"), "{shown}");
}

#[test]
fn keys_work_while_the_inspection_runs_and_a_read_no_longer_awaited_is_dropped() {
    // Before the inspection ends: the status says so, and the views and the palette answer.
    let script = format!("Resized:{W}:{H} ProcessStarted:1:7 Key:Tab {}{}Resized:{W}:{H}", keys("s"), keys("/"));
    let ran = run(&script, &[]);
    let shown = Terminal::read(&ran.stdout).text();
    assert!(shown.contains("inspecting..."), "{shown}");
    assert!(shown.contains("Choose a module on the left.") && shown.contains("/inspect") && shown.contains("/open"), "{shown}");
    // Two modules chosen in a row: the answer to the first read comes last, after the second's.
    let (term, _) = screen(&format!(
        "Key:Down Key:Down Key:Enter Key:Down Key:Down Key:Enter FileRead:1002:{} FileRead:1001:{} ",
        file("use_cell.gdl"),
        file("cell.gdl")
    ));
    let shown = term.text();
    assert!(shown.contains("USE_CELL - Text") && shown.contains("functions USE_CELL"), "{shown}");
    assert!(!shown.contains("module CELL satisfies"), "the first read was shown:\n{shown}");
}

#[test]
fn a_replay_reproduces_the_screen_with_no_process_run() {
    let dir = std::env::temp_dir().join(format!("gardelc_tablon_replay_{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("temporary directory");
    let log = dir.join("run.log");
    let script = format!("{}{}Key:Tab {}Key:Down Key:Enter", start(), choose(2, 1001, "cell.gdl"), keys("f"));
    let first = run(&script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(first.status.code(), Some(1), "{}", all(&first));
    let again = Command::new(tablon()).env("GARDEL_REPLAY", &log).output().expect("the replay runs");
    assert_eq!(again.status.code(), Some(1), "{}", all(&again));
    assert_eq!(again.stdout, first.stdout, "the replay writes the same bytes");
    assert!(Terminal::read(&again.stdout).text().contains("31   measure: NAT"));
    drop(fs::remove_dir_all(&dir));
}
