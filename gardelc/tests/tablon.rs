//! Tablon (Task F.11d), `tools/tablon`: it verifies under `--strict`; a scripted session on the
//! fixture of `tests/inspect/` lists the modules the inspection gives and the files of `src`,
//! shows each view of a module, takes Enter from a feature to its line in the source, marked,
//! and from a client or supplier to that module; keys work while the inspection runs and a read
//! no longer awaited is dropped; and a replay reproduces the screen with no process run. A check
//! fills Problems, whose Enter goes to the line; a run builds first, streams the program's output
//! and sends it the lines typed; a reactive program runs attached; an edit is read again and
//! checked; a failed build runs nothing; and `/new` writes a module's file only where there is
//! none, then opens it and inspects the project again.
//!
//! F.16c: Text edits the file shown. A save writes the text with the file's own line ends and
//! checks again; an edit moves the marked line and the problems below it; an edited file stays
//! edited while another is shown; quitting with unsaved files asks first; a copy reaches the
//! terminal's clipboard; a session that edits and saves replays to the same screen and the same
//! bytes with no file written; a key on a file of 5000 lines takes less than a 16 ms frame; and a
//! real check's failure, fixed in Tablon and saved, gives a clean check.
//!
//! The scripts stand in for `gardelc`: the inspection's output is the fixture's expected object,
//! and a file read is the fixture's file, so the tests need no compiler run but the build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// The suffix gardelc gives an executable: `.exe` on Windows, none elsewhere.
const EXE: &str = std::env::consts::EXE_SUFFIX;

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
        dir.join(format!("build/tablon_bin{EXE}"))
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
    start_with(&fs::read(fixture().join("expected.json")).expect("the expected object"))
}

/// The start of a session whose inspection gives `json`.
fn start_with(json: &[u8]) -> String {
    format!(
        "Resized:{W}:{H} ProcessStarted:1:7 ProcessErrorOutput:7:{} ProcessOutput:7:{} ProcessExited:7:0 \
         DirListed:100:fuse_cell.gdl/dsub/fcell.gdl/ DirListed:101:fnote.txt/ ",
        enc(b"[1/5] Linked\n"),
        enc(json)
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
    /// The operating system commands written, such as a copy (`52;c;...`).
    osc: Vec<String>,
}

impl Terminal {
    fn read(bytes: &[u8]) -> Self {
        let mut t = Terminal { grid: Vec::new(), row: 0, col: 0, pen: Vec::new(), before_clear: Vec::new(), osc: Vec::new() };
        let text = std::str::from_utf8(bytes).expect("the output is UTF-8");
        let cs: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '\u{1b}' && cs.get(i + 1) == Some(&']') {
                // An OSC sequence, ended by BEL or by ESC \.
                let mut j = i + 2;
                while j < cs.len() && cs[j] != '\u{7}' && !(cs[j] == '\u{1b}' && cs.get(j + 1) == Some(&'\\')) {
                    j += 1;
                }
                t.osc.push(cs[i + 2..j.min(cs.len())].iter().collect());
                i = if cs.get(j) == Some(&'\u{7}') { j + 1 } else { j + 2 };
                continue;
            }
            if c == '\u{1b}' {
                assert_eq!(cs.get(i + 1), Some(&'['), "only CSI and OSC sequences, at char {i}");
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
                // The cursor's shape, `CSI n SP q`.
                if cs[j] == ' ' && cs.get(j + 1) == Some(&'q') {
                    i = j + 2;
                    continue;
                }
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
    screen_from(&start(), steps)
}

/// The screen a session that starts with `first` leaves, as `screen`.
fn screen_from(first: &str, steps: &str) -> (Terminal, String) {
    let script = format!("{first}{steps}Resized:{W}:{H}");
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

/// A diagnostic as `gardelc --json-diagnostics` prints it. `message` is JSON text.
fn diag(level: &str, module: &str, tag: &str, file: &str, line: usize, message: &str) -> String {
    format!(
        r#"{{"level": "{level}", "rule": "Phase 3 Boogie", "module": "{module}", "routine": "{tag}", "tag": "{tag}", "contract_tag": "{tag}", "span": {{ "file": "{file}", "line": {line}, "column": 1, "end_line": {line}, "end_column": 2 }}, "path_conditions": [], "counterexample": {{}}, "relevant_lemmas": [], "message": "{message}"}}"#
    )
}

/// The fixture's object with `diags` as its diagnostics, and `kinds` replaced in it.
fn inspection_with(diags: &[String], kinds: &[(&str, &str)]) -> Vec<u8> {
    let mut text = fs::read_to_string(fixture().join("expected.json")).expect("the expected object");
    assert!(text.contains("\"diagnostics\": []"));
    text = text.replace("\"diagnostics\": []", &format!("\"diagnostics\": [{}]", diags.join(", ")));
    for (from, to) in kinds {
        assert!(text.contains(from), "{from}");
        text = text.replace(from, to);
    }
    text.into_bytes()
}

/// The note of a build that names the executable of `module`.
fn executable_note(module: &str) -> String {
    diag("note", module, "executable", "build/use_cell_bin.exe", 1, "Compiled native executable -> build/use_cell_bin.exe")
}

/// The events of a build, process 8, that prints `steps` and `json` and ends with `code`.
fn build(steps: &[u8], json: &str, code: usize) -> String {
    format!("ProcessStarted:3:8 ProcessErrorOutput:8:{} ProcessOutput:8:{} ProcessExited:8:{code} ", enc(steps), enc(json.as_bytes()))
}

#[test]
fn a_check_lists_the_problems_and_enter_goes_to_the_line() {
    let checked = inspection_with(
        &[
            diag("error", "CELL", "same", "./cell.gdl", 31, "Formal verification condition `same` failed in `measure`"),
            diag("warning", "USE_CELL", "naming", "./use_cell.gdl", 1, "W0201: a warning\\nwith a second line"),
        ],
        &[],
    );
    let check = format!(
        "Key:Ctrl:k ProcessStarted:2:8 ProcessErrorOutput:8:{} ProcessOutput:8:{} ProcessExited:8:1 ",
        enc(b"[cache] 1 modules verified now\n"),
        enc(&checked)
    );
    // While it runs: the status says so, and the keys work.
    let (term, log) = screen(&format!("Key:Ctrl:k ProcessStarted:2:8 {}", keys("s")));
    assert!(log.contains("proc: RunProcess 2 gardelc %2D%2Dproject %2E %2D%2Dinspect %2D%2Djson%2Ddiagnostics in "), "{log}");
    assert!(term.text().contains("checking...") && term.text().contains("^X stop"), "{}", term.text());
    // Its end: Problems is chosen, with both, the warning's message to its first line.
    let (term, _) = screen(&format!("{check}Key:Tab Key:Tab Key:Down Key:Up Key:Enter FileRead:1001:{} ", file("cell.gdl")));
    let shown = term.text();
    assert!(shown.contains("Problems (2)"), "{shown}");
    assert!(shown.contains("cell.gdl:31  error  Formal verification condition `same` failed in `measure`"), "{shown}");
    assert!(shown.contains("use_cell.gdl:1  warning  W0201: a warning") && !shown.contains("second line"), "{shown}");
    assert!(shown.contains("2 modules, 1 error, 1 warning"), "{shown}");
    // Enter on the error chose its module and marked its line.
    assert!(shown.contains("CELL - Text"), "{shown}");
    let marked = term.row_of("31   measure: NAT");
    let start = marked.iter().position(|c| c.0 == '3').expect("the line's number");
    assert!(marked[start..].iter().take(20).all(|c| c.1.contains(&44)), "the line is not marked: {marked:?}");
    // Output has what the check said.
    let (term, _) = screen(&format!("{check}Key:Tab Key:Tab {}", keys("o")));
    assert!(term.text().contains("[cache] 1 modules verified now") && term.text().contains("Checked: 2 modules, 1 error, 1 warning"), "{}", term.text());
}

#[test]
fn a_run_builds_then_streams_the_output_and_sends_the_input() {
    let built = build(b"[5/5] Emitted C11 runtime target\n      Compiled", &format!("[{}]", executable_note("USE_CELL")), 0);
    let ran = format!("{}{built}ProcessStarted:4:9 ProcessOutput:9:{} ", keys("/run 3 4") + "Key:Enter ", enc(b"Name? "));
    // While it runs, the keys type to it, `q` too; Tab leaves the input, and the other panes work.
    let (term, log) = screen(&format!("{ran}{}Key:Tab {}Key:Tab Key:Tab ", keys("qu"), keys("f")));
    let shown = term.text();
    assert!(log.contains("proc: RunProcess 3 gardelc %2D%2Dproject %2E %2D%2Djson%2Ddiagnostics in "), "{log}");
    assert!(log.lines().any(|l| l.starts_with("proc: RunProcess 4 ") && l.contains("%2Fbuild%2Fuse%5Fcell%5Fbin%2Eexe 3 4 in ")), "{log}");
    assert!(shown.contains("> qu") && shown.contains("$ build/use_cell_bin.exe 3 4") && shown.contains("Name?"), "{shown}");
    assert!(shown.contains("running...") && shown.contains("Enter sends the line"), "{shown}");
    assert!(shown.contains("▾ src"), "the Files tab was chosen while it ran:\n{shown}");
    // A line sent, output in pieces that make one line, and the end.
    let (term, log) = screen(&format!(
        "{ran}{}Key:Enter ProcessOutput:9:{} ProcessErrorOutput:9:{} ProcessExited:9:0 ",
        keys("quinn"),
        enc(b"Hel"),
        enc(b"lo, quinn\n")
    ));
    let shown = term.text();
    assert!(sent(&log, "proc: WriteProcess 9 ", "quinn%0A"), "{log}");
    for row in ["Name? quinn", "Hello, quinn", "The program ended with code 0"] {
        assert!(shown.contains(row), "{row:?} is not shown:\n{shown}");
    }
    assert!(!shown.contains("> "), "the input line outlived the program:\n{shown}");
    // Build has the compiler's steps, the last one ended by the end of the build.
    let (term, _) = screen(&format!("{ran}Key:Escape {}", keys("b")));
    let shown = term.text();
    assert!(shown.contains("[5/5] Emitted C11 runtime target") && shown.contains("      Compiled") && shown.contains("Built build/use_cell_bin.exe"), "{shown}");
    // Ctrl-X stops it, and Ctrl-D ends its input.
    let (_, log) = screen(&format!("{ran}Key:Ctrl:d Key:Ctrl:x "));
    assert!(log.contains("proc: CloseInput 9") && log.contains("proc: StopProcess 9"), "{log}");
}

#[test]
fn a_reactive_program_runs_attached_and_an_edit_is_read_again_and_checked() {
    let reactive = inspection_with(&[], &[("\"name\": \"USE_CELL\", \"kind\": \"functions\"", "\"name\": \"USE_CELL\", \"kind\": \"reactive program\"")]);
    let first = start_with(&reactive);
    let built = build(b"", &format!("[{}]", executable_note("USE_CELL")), 0);
    let (term, log) = screen_from(&first, &format!("Key:Ctrl:r {built}AttachedEnded:5:0 Resized:{W}:{H} "));
    assert!(log.lines().any(|l| l.starts_with("proc: RunAttached 5 ") && l.contains("%2Fbuild%2Fuse%5Fcell%5Fbin%2Eexe in ")), "{log}");
    let shown = term.text();
    assert!(shown.contains("(attached: it has the terminal)") && shown.contains("The program ended with code 0"), "{shown}");
    assert!(shown.contains("▾ reactive programs"), "{shown}");
    // The editor on the file shown, then the file read again and a check.
    let cell = choose(4, 1001, "cell.gdl");
    let (term, log) = screen_from(&first, &format!("{cell}Key:Ctrl:e AttachedEnded:6:0 Resized:{W}:{H} FileRead:1002:{} ", file("cell.gdl")));
    assert!(sent(&log, "proc: RunAttached 6 vi %2E%2Fcell%2Egdl in ", ""), "{log}");
    assert!(sent(&log, "file: ReadFile 1002 ", "%2F%2E%2Fcell%2Egdl"), "{log}");
    assert!(log.contains("proc: RunProcess 2 gardelc "), "{log}");
    assert!(term.text().contains("checking...") && term.text().contains("CELL - Text"), "{}", term.text());
    // An editor that does not start says how to give another.
    let (term, _) = screen_from(&first, &format!("{cell}{}Key:Enter AttachFailed:6:2 Resized:{W}:{H} Key:Tab Key:Tab {}", keys("/edit nano -w"), keys("o")));
    assert!(term.text().contains("Could not run the editor nano -w"), "{}", term.text());
}

#[test]
fn a_failed_build_runs_nothing_and_its_errors_go_to_problems() {
    let error = diag("error", "CELL", "same", "./cell.gdl", 31, "Formal verification condition `same` failed in `measure`");
    let steps = format!("Key:Ctrl:r {}Key:Tab Key:Tab ", build(b"", &format!("[{error}]"), 1));
    let (term, log) = screen(&steps);
    let shown = term.text();
    assert!(!log.contains("RunProcess 4") && !log.contains("RunAttached"), "{log}");
    assert!(shown.contains("Problems (1)") && shown.contains("cell.gdl:31  error"), "{shown}");
    let (term, _) = screen(&format!("{steps}{}", keys("b")));
    assert!(term.text().contains("The build failed: 1 error; see Problems."), "{}", term.text());
    let (term, _) = screen(&format!("{steps}{}", keys("r")));
    assert!(term.text().contains("Not run: the build failed."), "{}", term.text());
}

#[test]
fn new_makes_a_module_where_there_is_no_file_then_opens_it_and_inspects_again() {
    let text = "-- CELL_TWO: what it is for.\nfunctions CELL_TWO\n\nfeature {ANY}\n\nend\n";
    let asked = format!("{}Key:Enter ", keys("/new cell_Two"));
    // No file there: it is written, read and shown, and the project inspected again.
    let steps = format!("{asked}FileReadFailed:7:2 FileWritten:8:{} FileRead:1001:{} Key:Tab Key:Tab {}", text.len(), enc(text.as_bytes()), keys("o"));
    let (term, log) = screen(&steps);
    assert!(sent(&log, "file: ReadFile 7 ", "%2Fsrc%2Fcell%5Ftwo%2Egdl"), "{log}");
    let traced: String = text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    assert!(sent(&log, "file: WriteFile 8 ", &format!("%2Fsrc%2Fcell%5Ftwo%2Egdl {traced}")), "{log}");
    assert!(sent(&log, "file: ReadFile 1001 ", "%2Fsrc%2Fcell%5Ftwo%2Egdl"), "{log}");
    assert_eq!(log.matches("proc: RunProcess 1 ").count(), 2, "inspected at the start and again: {log}");
    let shown = term.text();
    assert!(shown.contains("Made src/cell_two.gdl") && shown.contains("functions CELL_TWO"), "{shown}");
    // A file there already is kept: nothing is written.
    let (term, log) = screen(&format!("{asked}FileRead:7:{} Key:Tab Key:Tab {}", file("cell.gdl"), keys("o")));
    assert!(!log.contains("WriteFile"), "{log}");
    assert!(term.text().contains("src/cell_two.gdl is there already"), "{}", term.text());
    // A name must start with a letter.
    let (term, log) = screen(&format!("{}Key:Enter Key:Tab Key:Tab {}", keys("/new 2cells"), keys("o")));
    assert!(!log.contains("ReadFile 7"), "{log}");
    assert!(term.text().contains("/new needs a module's name"), "{}", term.text());
}

// F.16c: editing in Text.

/// The bytes a scripted run's log says were written by the command `WriteFile tag`, decoded
/// from the `%XX` the log writes for anything but a letter or a digit.
fn written(log: &str, tag: usize) -> Option<Vec<u8>> {
    let line = log.lines().find(|l| l.starts_with(&format!("file: WriteFile {tag} ")))?;
    let text = line.split(' ').nth(4).unwrap_or("");
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).expect("hex"), 16).expect("hex"));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// CELL chosen, the keys moved to Text, and Enter pressed to edit it: the cursor is at the
/// start of its header, line 2.
fn edit_cell() -> String {
    format!("{}Key:Tab Key:Enter ", choose(2, 1001, "cell.gdl"))
}

/// The fixture's `cell.gdl` with `text` put at the start of its second line, its line ends
/// kept: CR LF in a Windows checkout (git's `autocrlf`), LF in one on Linux, as git stores it.
fn cell_with(text: &str) -> Vec<u8> {
    let cell = fs::read_to_string(fixture().join("cell.gdl")).expect("cell.gdl");
    let end = if cell.contains("\r\nmodule CELL") { "\r\n" } else { "\n" };
    assert!(cell.contains(&format!("{end}module CELL")), "the fixture's second line is `module CELL`");
    cell.replacen(&format!("{end}module CELL"), &format!("{end}{text}module CELL"), 1).into_bytes()
}

#[test]
fn text_is_edited_and_saved_with_its_own_line_ends_then_checked_again() {
    // Typed: the title says unsaved; saved: the text written with its line ends kept, and a check.
    let steps = format!("{}{}Key:Ctrl:s ", edit_cell(), keys("x"));
    let (term, log) = screen(&steps);
    let shown = term.text();
    assert!(shown.contains("CELL - Text (unsaved)") && shown.contains(" 2 xmodule CELL"), "{shown}");
    assert!(shown.contains("Esc stops editing") && shown.contains("saving..."), "{shown}");
    assert!(sent(&log, "file: WriteFile 9 ", ""), "{log}");
    assert!(log.contains("%2F%2E%2Fcell%2Egdl "), "{log}");
    assert_eq!(written(&log, 9).expect("a save"), cell_with("x"));
    assert!(!log.contains("RunProcess 2"), "checked before the file was written: {log}");
    // Written: the title no longer says so, the project is checked, and Output says it.
    let (term, log) = screen(&format!("{steps}FileWritten:9:900 Key:Tab {}", keys("o")));
    let shown = term.text();
    assert!(log.contains("proc: RunProcess 2 gardelc "), "{log}");
    assert!(shown.contains("CELL - Text") && !shown.contains("(unsaved)") && shown.contains("checking..."), "{shown}");
    assert!(shown.contains("Saved cell.gdl") && shown.contains("Checking the project."), "{shown}");
    // A file whose lines end with LF alone is written so.
    let lf = format!(
        "{}Key:Down Key:Right Key:Down Key:Down Key:Enter FileRead:1001:{} Key:Tab Key:Enter {}Key:Ctrl:s ",
        keys("f"),
        enc(b"some notes\nline two\n"),
        keys("y")
    );
    let (term, log) = screen(&lf);
    assert!(term.text().contains("src/cell.gdl (unsaved)"), "{}", term.text());
    assert_eq!(written(&log, 9).expect("a save"), b"ysome notes\nline two\n");
    // Before Enter, letters choose views and move nothing; after Escape, too.
    let (term, log) = screen(&format!("{}Key:Tab {}Key:Down {}", choose(2, 1001, "cell.gdl"), keys("x"), keys("s")));
    assert!(term.text().contains("CELL - Short") && !term.text().contains("(unsaved)") && !log.contains("WriteFile"), "{}", term.text());
    let (term, _) = screen(&format!("{}{}Key:Escape {}", edit_cell(), keys("x"), keys("s")));
    assert!(term.text().contains("CELL - Short (unsaved)"), "{}", term.text());
}

#[test]
fn an_edit_moves_the_marked_line_and_the_problems_and_a_save_checks_again() {
    let error = diag("error", "CELL", "same", "./cell.gdl", 31, "Formal verification condition `same` failed in `measure`");
    let checked = inspection_with(&[error], &[]);
    let check = format!("Key:Ctrl:k ProcessStarted:2:8 ProcessOutput:8:{} ProcessExited:8:1 ", enc(&checked));
    // To the failure, then a line added above it: the failure and the mark move down a line.
    let at = format!("{check}Key:Tab Key:Tab Key:Enter FileRead:1001:{} Key:Tab Key:Tab Key:Enter ", file("cell.gdl"));
    let (term, _) = screen(&format!("{at}Key:Enter "));
    let shown = term.text();
    assert!(shown.contains("cell.gdl:32  error  Formal verification condition"), "{shown}");
    let marked = term.row_of("32   measure: NAT");
    let start = marked.iter().position(|c| c.0 == '3').expect("the line's number");
    assert!(marked[start..].iter().take(20).all(|c| c.1.contains(&44)), "the line is not marked: {marked:?}");
    // Taken out again, they move back; an edit that adds no line moves nothing.
    let (term, _) = screen(&format!("{at}Key:Enter Key:Backspace {}", keys("ab")));
    let shown = term.text();
    assert!(shown.contains("cell.gdl:31  error") && shown.contains("31 ab  measure: NAT"), "{shown}");
    // Saved: checked again, and a clean check leaves no problem.
    let clean = inspection_with(&[], &[]);
    let (term, log) = screen(&format!(
        "{at}Key:Enter Key:Ctrl:s FileWritten:9:900 ProcessStarted:2:10 ProcessOutput:10:{} ProcessExited:10:0 Key:Tab {}",
        enc(&clean),
        keys("p")
    ));
    assert_eq!(log.matches("proc: RunProcess 2 ").count(), 2, "{log}");
    assert!(term.text().contains("No problems: the project verifies."), "{}", term.text());
    // A save while a check runs: the check that follows waits for it to end.
    let (_, log) = screen(&format!("{}{}Key:Ctrl:k ProcessStarted:2:8 Key:Ctrl:s FileWritten:9:900 ", edit_cell(), keys("x")));
    assert_eq!(log.matches("proc: RunProcess 2 ").count(), 1, "{log}");
    let (_, log) = screen(&format!(
        "{}{}Key:Ctrl:k ProcessStarted:2:8 Key:Ctrl:s FileWritten:9:900 ProcessOutput:8:{} ProcessExited:8:0 ",
        edit_cell(),
        keys("x"),
        enc(&clean)
    ));
    assert_eq!(log.matches("proc: RunProcess 2 ").count(), 2, "{log}");
}

#[test]
fn an_edited_file_stays_edited_while_another_is_shown_and_quitting_asks_first() {
    // Edited, then USE_CELL shown, then CELL again: it is not read again, and is still edited.
    let away = format!(
        "{}{}Key:Escape Key:Tab Key:Tab Key:Down Key:Down Key:Enter FileRead:1002:{} Key:Up Key:Up Key:Enter ",
        edit_cell(),
        keys("x"),
        file("use_cell.gdl")
    );
    let (term, log) = screen(&away);
    assert!(!log.contains("ReadFile 1003"), "{log}");
    assert!(term.text().contains("CELL - Text (unsaved)") && term.text().contains(" 2 xmodule CELL"), "{}", term.text());
    // `q` asks; another key goes back, and `y` quits.
    let (term, _) = screen(&format!("{away}{}", keys("q")));
    assert!(term.text().contains("Unsaved: cell.gdl. Quit without saving them?"), "{}", term.text());
    let (term, _) = screen(&format!("{away}{}", keys("qn")));
    assert!(!term.text().contains("Quit without saving"), "{}", term.text());
    let quit = run(&format!("{}{away}{}", start(), keys("qy")), &[]);
    assert_eq!(quit.status.code(), Some(0), "{}", all(&quit));
    let asked = run(&format!("{}{away}{}", start(), keys("q")), &[]);
    assert_eq!(asked.status.code(), Some(1), "quit without asking: {}", all(&asked));
    // So does `/open`; and `/edit` waits for a save, as the editor reads the disk.
    let (term, log) = screen(&format!("{away}{}Key:Enter ", keys("/open elsewhere")));
    assert!(term.text().contains("Open another project without saving them?"), "{}", term.text());
    assert_eq!(log.matches("proc: RunProcess 1 ").count(), 1, "{log}");
    let (term, log) = screen(&format!("{away}Key:Ctrl:e Key:Tab Key:Tab {}", keys("o")));
    assert!(!log.contains("RunAttached 6") && term.text().contains("Save the file first (^S)"), "{log}");
    // Nothing unsaved: `q` quits at once.
    let clean = run(&format!("{}{}Key:Escape {}", start(), edit_cell(), keys("q")), &[]);
    assert_eq!(clean.status.code(), Some(0), "{}", all(&clean));
}

/// `text` in base 64, as OSC 52 carries it.
fn base64(text: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in text.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            out.push(if k <= c.len() { A[((n >> (18 - 6 * k)) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[test]
fn a_selection_is_cut_copied_and_pasted_and_a_copy_reaches_the_terminal() {
    // Shift-End selects the header, Ctrl-Insert copies it to the terminal's clipboard.
    let (term, _) = screen(&format!("{}Key:Shift+End Key:Ctrl+Insert ", edit_cell()));
    assert!(term.osc.iter().any(|o| o == &format!("52;c;{}", base64(b"module CELL satisfies MEASURED"))), "{:?}", term.osc);
    // Ctrl-X cuts it, which leaves Tablon's Ctrl-X alone, and Ctrl-V puts it back twice.
    let (term, log) = screen(&format!("{}Key:Shift+End Key:Ctrl:x Key:Ctrl:v Key:Ctrl:v ", edit_cell()));
    assert!(!log.contains("StopProcess"), "{log}");
    assert!(term.text().contains(" 2 module CELL satisfies MEASUREDmodule CELL satisfies MEASURED"), "{}", term.text());
    // A terminal's paste goes in as one edit; Ctrl-Z takes it out.
    let (term, _) = screen(&format!("{}Pasted:{} ", edit_cell(), enc(b"one\r\ntwo ")));
    assert!(term.text().contains(" 2 one") && term.text().contains(" 3 two module CELL"), "{}", term.text());
    let (term, _) = screen(&format!("{}Pasted:{} Key:Ctrl:z ", edit_cell(), enc(b"one\r\ntwo ")));
    assert!(term.text().contains(" 2 module CELL") && !term.text().contains(" 3 two"), "{}", term.text());
}

#[test]
fn a_session_that_edits_and_saves_replays_to_the_same_screen_and_bytes_with_no_file_written() {
    let dir = std::env::temp_dir().join(format!("gardelc_tablon_edit_replay_{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("temporary directory");
    let log = dir.join("run.log");
    let before = fs::read(fixture().join("cell.gdl")).expect("cell.gdl");
    // Types, selects what it typed, pastes over it, undoes the paste, types again and saves.
    let script = format!(
        "{}{}{}Key:Shift+Left Key:Shift+Left Pasted:{} Key:Ctrl:z Key:End {}Key:Ctrl:s FileWritten:9:900",
        start(),
        edit_cell(),
        keys("ab"),
        enc(b"cd"),
        keys("!")
    );
    let first = run(&script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(first.status.code(), Some(1), "{}", all(&first));
    let first_log = String::from_utf8_lossy(&first.stderr).to_string();
    let saved = written(&first_log, 9).expect("a save");
    let cell = String::from_utf8(before.clone()).expect("UTF-8");
    assert_eq!(saved, cell.replacen("module CELL satisfies MEASURED", "abmodule CELL satisfies MEASURED!", 1).into_bytes());
    let again = Command::new(tablon()).env("GARDEL_REPLAY", &log).output().expect("the replay runs");
    assert_eq!(again.status.code(), Some(1), "{}", all(&again));
    assert_eq!(again.stdout, first.stdout, "the replay writes the same bytes to the terminal");
    let recorded = fs::read_to_string(&log).expect("the recording");
    assert!(recorded.contains("command WriteFile:9:"), "the save is in the recording:\n{recorded}");
    assert_eq!(fs::read(fixture().join("cell.gdl")).expect("cell.gdl"), before, "a file was written");
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_key_on_a_file_of_5000_lines_draws_within_a_frame() {
    let mut text = String::new();
    // A script is one variable, which Linux takes up to 128 KiB: so the lines are short, and
    // only what a script cannot hold as it is is escaped.
    for i in 0..5000 {
        text += &format!("line {i}: NAT\n");
    }
    let escaped: String = text
        .bytes()
        .map(|b| if b.is_ascii_graphic() && b != b'%' { char::from(b).to_string() } else { format!("%{b:02X}") })
        .collect();
    let open = format!(
        "{}{}Key:Down Key:Right Key:Down Key:Down Key:Enter FileRead:1001:{} Key:Tab Key:Enter Key:Ctrl+End ",
        start(),
        keys("f"),
        escaped
    );
    let typed: String = (0..200).map(|i| if i % 20 == 19 { "Key:Enter ".to_string() } else { "Key:97 ".to_string() }).collect::<String>()
        + &"Key:Up ".repeat(100);
    let time = |script: &str| {
        // The best of five, as other tests may run at the same time.
        (0..5)
            .map(|_| {
                let t = std::time::Instant::now();
                let out = run(script, &[]);
                assert_eq!(out.status.code(), Some(1), "{}", all(&out));
                t.elapsed()
            })
            .min()
            .expect("five runs")
    };
    let base = time(&open);
    let keyed = time(&format!("{open}{typed}"));
    let per_key = keyed.saturating_sub(base) / 300;
    eprintln!("a key on 5000 lines, with its frame: {per_key:?}");
    assert!(per_key < std::time::Duration::from_millis(16), "{per_key:?} a key");
}

#[test]
fn a_real_failure_is_fixed_and_saved_in_tablon_and_the_check_after_it_is_clean() {
    let dir = std::env::temp_dir().join(format!("gardelc_tablon_fix_{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(dir.join("src")).expect("temporary directory");
    fs::write(dir.join("Gardel.toml"), "[project]\nname = \"broken\"\nversion = \"0.1.0\"\n").expect("the manifest");
    let broken = "-- Doubling, with a contract that is wrong.\nfunctions TWICE\n\nfeature {ANY}\n\n  twice(n: NAT): NAT\n    require\n      small: n <= 1000\n    do\n      n + n\n    ensure\n      doubled: Result = n + n + 1\n    end\n\nend\n";
    fs::write(dir.join("src/twice.gdl"), broken).expect("the module");
    // The check Tablon asks for, run for real.
    let check = || {
        let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&dir)
            .env("GARDEL_CACHE_DIR", dir.join("cache"))
            .args(["--project", ".", "--inspect", "--json-diagnostics"])
            .output()
            .expect("gardelc runs");
        (out.stdout, out.status.code().unwrap_or(-1))
    };
    let (failed, code) = check();
    assert_ne!(code, 0);
    assert!(String::from_utf8_lossy(&failed).contains("`doubled` failed"), "{}", String::from_utf8_lossy(&failed));
    // Tablon on the project: the check, Enter on its failure, the line fixed and saved.
    let session = |tail: &str| {
        let script = format!(
            "Resized:{W}:{H} ProcessStarted:1:7 ProcessOutput:7:{} ProcessExited:7:1 DirListed:100:ftwice.gdl/ \
             Key:Ctrl:k ProcessStarted:2:8 ProcessOutput:8:{} ProcessExited:8:1 \
             Key:Tab Key:Tab Key:Enter FileRead:1001:{} Key:Tab Key:Tab Key:Enter Key:End \
             Key:Backspace Key:Backspace Key:Backspace Key:Backspace Key:Ctrl:s FileWritten:9:100 {tail}Resized:{W}:{H}",
            enc(&failed),
            enc(&failed),
            enc(broken.as_bytes())
        );
        let mut cmd = Command::new(tablon());
        cmd.env("GARDEL_EVENTS", script).args([&dir.to_string_lossy().to_string(), "gardelc"]);
        cmd.output().expect("Tablon runs")
    };
    let ran = session("");
    let log = String::from_utf8_lossy(&ran.stderr).to_string();
    assert!(Terminal::read(&ran.stdout).text().contains("src/twice.gdl:12  error"), "{}", Terminal::read(&ran.stdout).text());
    let fixed = written(&log, 9).expect("a save");
    assert_eq!(String::from_utf8_lossy(&fixed), broken.replace("n + n + 1", "n + n"));
    // What Tablon wrote, checked for real, verifies; and Tablon shows it clean.
    fs::write(dir.join("src/twice.gdl"), &fixed).expect("the fixed module");
    let (clean, code) = check();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&clean));
    let ran = session(&format!("ProcessStarted:2:9 ProcessOutput:9:{} ProcessExited:9:0 Key:Tab {}", enc(&clean), keys("p")));
    let shown = Terminal::read(&ran.stdout).text();
    assert!(shown.contains("No problems: the project verifies.") && shown.contains("verified"), "{shown}");
    drop(fs::remove_dir_all(&dir));
}
