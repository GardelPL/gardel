//! `gtui` (Task F.10): the package, its bench and `examples/todo_tui` verify under `--strict`;
//! a scripted session of the example, read by a small VT terminal, writes nothing outside the
//! screen and leaves the same cells as a full redraw; a shutdown puts the terminal back; a
//! replay repeats the run; a full redraw of 200 by 60 fits in a 16 ms frame; and a client
//! proves the layout facts through the contracts alone, while a claim past them is refused.
//!
//! `gtui` 0.2.0 (Task F.11c): a scripted session of `examples/gtui_tour` draws every new
//! widget, in colour, inside the screen; a child run attached gets the terminal, with nothing
//! drawn while it runs, and the tour takes the screen back after it; and a client proves the
//! widgets' facts through their contracts.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

fn assert_ok(out: &Output) {
    assert!(out.status.success(), "{}", all(out));
    assert!(all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

/// A directory of its own, where `gardelc` writes its build and cache.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_gtui_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("temporary directory");
        Self(dir)
    }

    fn check(&self, project: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.0)
            .env("GARDEL_CACHE_DIR", self.0.join("cache"))
            .args(["--project", &project.to_string_lossy(), "--strict"])
            .output()
            .expect("gardelc runs")
    }

    fn bin(&self, program: &str) -> PathBuf {
        self.0.join(format!("build/{program}_bin.exe"))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

/// `program` run from `script`, with any other variables in `env`.
fn run(bin: &Path, script: &str, env: &[(&str, &Path)]) -> Output {
    let mut cmd = Command::new(bin);
    cmd.env("GARDEL_EVENTS", script);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("the program runs")
}

/// A cell as a terminal shows it: a character and the SGR parameters in force.
type Cell = (char, Vec<u32>);

/// A VT terminal, enough of one to read a frame: cursor moves, SGR, clearing the screen, and
/// the private modes, which it ignores. Every character must land on the screen of the size
/// the script last gave, and no other control byte may appear.
struct Terminal {
    sizes: Vec<(usize, usize)>,
    clears: usize,
    size: Option<(usize, usize)>,
    grid: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    pen: Vec<u32>,
    /// The cells just before each clear of the screen.
    before_clear: Vec<Vec<Vec<Cell>>>,
    printed: usize,
    /// Whether the cursor is shown, and where it was when it was last shown.
    cursor: Option<(usize, usize)>,
    /// What each OSC 52 sequence put on the clipboard, base64 as sent.
    copies: Vec<String>,
}

impl Terminal {
    fn new(sizes: Vec<(usize, usize)>) -> Self {
        Self { sizes, clears: 0, size: None, grid: Vec::new(), row: 0, col: 0, pen: Vec::new(), before_clear: Vec::new(), printed: 0, cursor: None, copies: Vec::new() }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let text = std::str::from_utf8(bytes).expect("the output is UTF-8");
        let cs: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c == '\u{1b}' && cs.get(i + 1) == Some(&']') {
                let end = cs[i..].iter().position(|c| *c == '\u{7}').expect("an OSC ends with BEL") + i;
                let body: String = cs[i + 2..end].iter().collect();
                let data = body.strip_prefix("52;c;").expect("only OSC 52");
                assert!(data.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)), "base64: {data}");
                self.copies.push(data.to_string());
                i = end + 1;
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
                    assert!(params == vec![6] || params == vec![0], "a bar, or the terminal's own: {params:?}");
                    i = j + 2;
                    continue;
                }
                let fin = cs[j];
                match (private, fin) {
                    (true, 'h') if params == vec![25] => self.cursor = Some((self.row, self.col)),
                    (true, 'l') if params == vec![25] => self.cursor = None,
                    (true, 'h') | (true, 'l') => {}
                    (false, 'H') => {
                        let (w, h) = self.size.expect("a cursor move after a size is known");
                        assert_eq!(params.len(), 2, "a move names a row and a column");
                        assert!(params[0] >= 1 && params[0] as usize <= h, "row {} outside {h} rows", params[0]);
                        assert!(params[1] >= 1 && params[1] as usize <= w, "column {} outside {w} columns", params[1]);
                        self.row = params[0] as usize - 1;
                        self.col = params[1] as usize - 1;
                    }
                    (false, 'm') => self.pen = params.into_iter().filter(|p| *p != 0).collect(),
                    (false, 'J') => {
                        assert_eq!(params, vec![2], "only a whole clear");
                        self.before_clear.push(self.grid.clone());
                        self.size = if self.clears == 0 { None } else { Some(self.sizes[self.clears - 1]) };
                        self.clears += 1;
                        if let Some((w, h)) = self.size {
                            self.grid = vec![vec![(' ', Vec::new()); w]; h];
                        }
                    }
                    other => panic!("unexpected sequence {other:?}"),
                }
                i = j + 1;
            } else {
                assert!(!c.is_control(), "a control character {:?} was written", c);
                let (w, h) = self.size.expect("text after a size is known");
                assert!(self.row < h && self.col < w, "{c:?} written at row {}, column {}, outside {w} by {h}", self.row, self.col);
                self.grid[self.row][self.col] = (c, self.pen.clone());
                self.col += 1;
                self.printed += 1;
                i += 1;
            }
        }
    }

    fn text(&self) -> String {
        self.grid.iter().map(|r| r.iter().map(|c| c.0).collect::<String>()).collect::<Vec<_>>().join("\n")
    }
}

/// The sizes a script gives, in order.
fn sizes(script: &str) -> Vec<(usize, usize)> {
    script
        .split_whitespace()
        .filter_map(|t| t.strip_prefix("Resized:"))
        .map(|t| {
            let (w, h) = t.split_once(':').expect("w:h");
            (w.parse().expect("width"), h.parse().expect("height"))
        })
        .collect()
}

fn keys(text: &str) -> String {
    text.chars().map(|c| format!("Key:{} ", c as u32)).collect()
}

/// A session of `todo_tui`: twelve items in a list five rows high, one too long for the
/// screen, one holding a tab, an escape and a bell, a line typed past the width, paging,
/// marking, removing, two resizes, and at the end a resize to the same size, which draws
/// the screen whole.
fn session() -> String {
    let mut s = String::from("Resized:40:7 ");
    for i in 0..10 {
        s += &format!("Key:97 {}Key:Enter ", keys(&format!("item {i}")));
    }
    s += &format!("Key:97 {}Key:Enter ", keys("a very long item that does not fit é in forty columns"));
    s += "Key:97 Key:116 Key:9 Key:27 Key:7 Key:Enter ";
    s += "Key:Home Key:PageDown Key:PageDown Key:Up Key:32 Key:Enter Key:PageUp Key:Down Key:32 ";
    s += "Key:Home Key:Delete Key:100 Key:Home Key:Down Key:Down Key:100 ";
    s += &format!("Key:97 {}Key:Left Key:Left Key:Backspace Key:Home Key:Delete Key:End Key:Escape ", keys("typed past the right edge of a narrow screen"));
    s += "Resized:25:5 Key:End Key:Up Key:32 Resized:80:12 Key:Home Key:32 Resized:80:12 Key:113";
    s
}

#[test]
fn a_session_draws_only_inside_the_screen() {
    let dir = Dir::new("session");
    let out = dir.check(&root().join("examples/todo_tui"));
    assert_ok(&out);
    let script = session();
    let ran = run(&dir.bin("todo_tui"), &script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(&script));
    term.feed(&ran.stdout);
    assert_eq!(term.clears, 1 + sizes(&script).len(), "the screen is cleared at the start and on each resize");
    assert!(term.printed > 2000, "the session drew {} characters", term.printed);
    // The last resize kept the size, so it drew the screen whole: the cells the diffs left
    // before it must be the same, styles included.
    let before = term.before_clear.last().expect("a last clear");
    assert_eq!(before, &term.grid, "the diffs left:\n{}\n\na full redraw gives:\n{}", Terminal { grid: before.clone(), ..Terminal::new(Vec::new()) }.text(), term.text());
    let shown = term.text();
    assert!(shown.contains(" of 9 done"), "{shown}");
    assert!(shown.contains("[x] item"), "{shown}");
    assert!(shown.contains("] a very long item that does not fit é in forty column "), "{shown}");
    assert!(shown.contains("[ ] t ??"), "the tab, escape and bell are shown as a space and `?`:\n{shown}");
    assert!(ran.stdout.ends_with(b"\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l"), "the screen is put back");
}

#[test]
fn a_shutdown_puts_the_terminal_back_and_a_replay_repeats_the_run() {
    let dir = Dir::new("replay");
    assert_ok(&dir.check(&root().join("examples/todo_tui")));
    let log = dir.0.join("run.log");
    let script = format!("Resized:50:6 Key:97 {}Key:Enter Key:32", keys("milk"));
    let first = run(&dir.bin("todo_tui"), &script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(first.status.code(), Some(1), "the end of a script is a shutdown: {}", all(&first));
    assert!(first.stdout.starts_with(b"\x1b[?1049h\x1b[0m\x1b[2J\x1b[?25l\x1b[6 q\x1b[?2004h"), "the alternate screen is taken, with pastes bracketed");
    assert!(first.stdout.ends_with(b"\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l"), "and given back");
    let mut term = Terminal::new(sizes(&script));
    term.feed(&first.stdout);
    assert!(term.text().contains("[x] milk"), "{}", term.text());
    let again = Command::new(dir.bin("todo_tui")).env("GARDEL_REPLAY", &log).output().expect("the replay runs");
    assert_eq!(again.status.code(), Some(1), "{}", all(&again));
    assert_eq!(again.stdout, first.stdout, "the replay writes the same bytes");
}

/// The bench redraws every cell on every key. A frame's cost is the time of a run with keys
/// less that of one with none, over the keys; the best of three runs is taken.
#[test]
fn a_full_redraw_of_200_by_60_fits_a_frame() {
    let dir = Dir::new("bench");
    assert_ok(&dir.check(&root().join("gtui/tests")));
    let bin = dir.bin("gtui_bench");
    let frames = 200;
    let time = |n: usize| {
        let script = format!("Resized:200:60 {}", "Key:Down ".repeat(n));
        (0..3)
            .map(|_| {
                let t = Instant::now();
                let out = run(&bin, &script, &[]);
                let took = t.elapsed();
                assert_eq!(out.status.code(), Some(1), "{}", all(&out));
                if n > 0 {
                    // Every frame wrote every cell.
                    assert!(out.stdout.len() > n * 12000, "{} bytes", out.stdout.len());
                }
                took
            })
            .min()
            .expect("three runs")
    };
    let base = time(0);
    let full = time(frames);
    let per_frame = full.saturating_sub(base) / frames as u32;
    eprintln!("a full redraw of 200 by 60: {per_frame:?} ({full:?} for {frames}, {base:?} for none)");
    assert!(per_frame.as_millis() < 16, "{per_frame:?} per frame");
}

fn client(claims: &str) -> String {
    format!(
        "functions CLIENT

feature {{ANY}}

  f(r: RECT, sizes: LIST[SIZE], n: NAT, i: NAT, j: NAT): BOOL
    do
      True
    ensure
{claims}
    end

end
"
    )
}

struct Project(Dir);

impl Project {
    fn new(name: &str, source: &str) -> Self {
        let dir = Dir::new(name);
        let path = root().join("gtui").canonicalize().expect("gtui exists");
        let path = path.to_string_lossy().replace('\\', "/").replace("//?/", "");
        let project = dir.0.join("p");
        fs::create_dir_all(project.join("src")).expect("project directory");
        fs::write(project.join("Gardel.toml"), format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[dependencies]\ngtui = {{ path = \"{path}\" }}\n", name.replace('_', "-"))).expect("manifest");
        fs::write(project.join("src/client.gdl"), source).expect("source");
        Self(dir)
    }

    fn check(&self) -> Output {
        self.0.check(&self.0 .0.join("p"))
    }
}

/// What a client proves from the contracts: a cut's two pieces lie in the rectangle and do not
/// overlap; every child of a row of columns, or of a column of rows, is inside its parent;
/// and no two of them overlap.
#[test]
fn a_client_proves_the_layout_through_the_contracts() {
    let facts = client(
        "      top_in: r.take_top(n).inside(r) and r.drop_top(n).inside(r)
      top_apart: r.take_top(n).disjoint(r.drop_top(n))
      one_each: LAYOUT.columns(r, sizes).length = sizes.length
      col_in: i < sizes.length implies LIST_OPS.at(LAYOUT.columns(r, sizes), i).inside(r)
      row_in: i < sizes.length implies LIST_OPS.at(LAYOUT.rows(r, sizes), i).inside(r)
      cols_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.columns(r, sizes), i).disjoint(LIST_OPS.at(LAYOUT.columns(r, sizes), j))
      rows_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.rows(r, sizes), i).disjoint(LIST_OPS.at(LAYOUT.rows(r, sizes), j))",
    );
    let body = facts.replace(
        "    do\n      True\n",
        "    do
      if i < j and j < sizes.length then
        check
          ci: LAYOUT.across_inside(LAYOUT.columns(r, sizes), r.x, r, i)
          ri: LAYOUT.down_inside(LAYOUT.rows(r, sizes), r.y, r, i)
          ca: LAYOUT_APART.columns_apart(LAYOUT.columns(r, sizes), r.x, r, i, j)
          ra: LAYOUT_APART.rows_apart(LAYOUT.rows(r, sizes), r.y, r, i, j)
        end
        True
      else if i < sizes.length then
        check
          ci: LAYOUT.across_inside(LAYOUT.columns(r, sizes), r.x, r, i)
          ri: LAYOUT.down_inside(LAYOUT.rows(r, sizes), r.y, r, i)
        end
        True
      else
        True
      end end
",
    );
    let p = Project::new("gtui_facts", &body);
    assert_ok(&p.check());
    // Past the contracts: a child need not be as wide as its share, nor two children apart
    // when they are the same one.
    for (name, claim) in [
        ("whole", "      whole: i < sizes.length implies LIST_OPS.at(LAYOUT.columns(r, sizes), i).width = r.width"),
        ("self", "      self: i < sizes.length implies LIST_OPS.at(LAYOUT.columns(r, sizes), i).disjoint(LIST_OPS.at(LAYOUT.columns(r, sizes), i))"),
    ] {
        let p = Project::new(&format!("gtui_{name}"), &client(claim));
        let out = p.check();
        assert!(!out.status.success(), "{name} must be refused: {}", all(&out));
        assert!(all(&out).contains(&format!("`{name}`")), "{name}: {}", all(&out));
    }
}

fn screen_text(grid: &[Vec<Cell>]) -> String {
    Terminal { grid: grid.to_vec(), ..Terminal::new(Vec::new()) }.text()
}

/// The row of `grid` whose text holds `text`.
fn row_with(grid: &[Vec<Cell>], text: &str) -> usize {
    let shown = screen_text(grid);
    shown.lines().position(|l| l.contains(text)).unwrap_or_else(|| panic!("no row holds {text:?}:\n{shown}"))
}

/// The SGR parameters of the cell where `text` starts.
fn pen_at(grid: &[Vec<Cell>], text: &str) -> Vec<u32> {
    let y = row_with(grid, text);
    let line: String = grid[y].iter().map(|c| c.0).collect();
    let x = line.find(text).expect("on the row");
    grid[y][line[..x].chars().count()].1.clone()
}

/// A session of `gtui_tour`, with a resize to the same size after each step that is looked
/// at: the clear it makes keeps the cells the diffs left (`before_clear`).
fn tour_session() -> String {
    let mut s = String::from("Resized:90:24 ");
    // 1: the node `screen` opened by Right.
    s += "Key:Down Key:Right Resized:90:24 ";
    // 2: closed by Left, then the Files tab chosen by its key.
    s += "Key:Left Key:102 Resized:90:24 ";
    // 3: back to Modules; the keys to the source, a page down; the keys to the bottom, the
    // Problems tab, and its second problem marked in the source, which gets the keys.
    s += "Key:109 Key:Tab Key:PageDown Key:Tab Key:112 Key:Down Key:Enter Resized:90:24 ";
    // 4: the palette, narrowed to `shell` by `s`.
    s += "Key:47 Key:115 Resized:90:24 ";
    // 5: `c` instead: `/clear` run by Enter, then `/top`.
    s += "Key:Backspace Key:99 Key:Enter Key:47 Key:116 Key:Enter Resized:90:24 ";
    // A smaller screen, the keys around the panes, and a last resize to the same size.
    s += "Resized:50:14 Key:Tab Key:Tab Key:Down Key:Right Resized:50:14 Key:113";
    s
}

#[test]
fn the_tour_draws_every_widget_in_colour_inside_the_screen() {
    let dir = Dir::new("tour");
    assert_ok(&dir.check(&root().join("examples/gtui_tour")));
    let script = tour_session();
    let ran = run(&dir.bin("gtui_tour"), &script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(&script));
    term.feed(&ran.stdout);
    assert_eq!(term.clears, 1 + sizes(&script).len());
    // `before_clear[k + 1]` is the screen just before resize `k`, counted from 0.
    let shot = |k: usize| term.before_clear[k + 1].clone();

    // 1: a tree with a node opened: `▾` and its children, indented, and `▸` on a closed one.
    let one = shot(1);
    let t1 = screen_text(&one);
    for text in ["│▾ gtui", "│  ▾ screen", "│      CELL", "│      COLOR", "│  ▸ widget", "│▸ prelude"] {
        assert!(t1.contains(text), "no {text:?}:\n{t1}");
    }
    assert!(!t1.contains("FRAME") && !t1.contains("KEY"), "a closed node's children are not shown:\n{t1}");
    assert!(pen_at(&one, "▾ screen").contains(&7), "the selected row is reversed");
    // Rounded frames; the left pane, which has the keys, in bold cyan, the others in gray; the
    // tab chosen in bold, underlined bright yellow, and its key plain.
    assert!(t1.starts_with("╭─Modules(m) | Files(f)─"), "{t1}");
    assert_eq!(pen_at(&one, "╭─Modules"), vec![1, 36]);
    assert_eq!(pen_at(&one, "Modules"), vec![1, 4, 93]);
    assert_eq!(pen_at(&one, "(m)"), Vec::<u32>::new());
    assert_eq!(pen_at(&one, "╭─ screen Text"), vec![90]);
    // The source: numbers in gray, right-aligned; keywords in bold bright blue; a comment green.
    assert!(t1.contains("│ 1 -- A row") && t1.contains("│ 2 module TABS"), "{t1}");
    assert_eq!(pen_at(&one, "1 -- A row"), vec![90]);
    assert_eq!(pen_at(&one, "-- A row"), vec![32]);
    assert_eq!(pen_at(&one, "module TABS"), vec![1, 94]);
    assert_eq!(pen_at(&one, "TABS  "), Vec::<u32>::new(), "a name is plain");

    // 2: the node closed again, and the Files tab chosen, with its own tree.
    let two = shot(2);
    let t2 = screen_text(&two);
    assert!(t2.contains("│▾ src") && t2.contains("│  ▸ screen"), "{t2}");
    assert_eq!(pen_at(&two, "Files"), vec![1, 4, 93]);

    // 3: the problem's line, 29, shown and marked across its row on blue, its keywords on the
    // same blue; the source framed in cyan, as it has the keys now.
    let three = shot(3);
    let t3 = screen_text(&three);
    let y = row_with(&three, "29       else 1 + TABS.key_index");
    let x0 = three[y].iter().position(|c| c.0 == '2').expect("the number");
    assert!(three[y][x0..x0 + 40].iter().all(|c| c.1.contains(&44)), "the marked row is blue: {:?}", three[y]);
    assert_eq!(pen_at(&three, "else 1 +"), vec![1, 94, 44]);
    assert_eq!(pen_at(&three, "╭─ screen Text"), vec![1, 36]);
    assert_eq!(pen_at(&three, " screen Text "), vec![1, 7, 36]);
    assert!(t3.contains("Problems (3)(p)") && t3.contains("tabs.gdl:29  overflow_add in key_index"), "{t3}");
    assert!(pen_at(&three, "tabs.gdl:29").contains(&7), "the problem chosen is selected");

    // 4: the palette, over the bottom pane, narrowed to the one command that starts with `s`.
    let four = shot(4);
    let t4 = screen_text(&four);
    assert!(t4.contains("╭─ Commands ─") && t4.contains("│/s ") && t4.contains("│/shell  [ARGS] run the program"), "{t4}");
    assert!(!t4.contains("/clear") && !t4.contains("/quit"), "{t4}");
    assert_eq!(pen_at(&four, "/shell"), vec![44], "the command selected is on blue");

    // 5: the palette closed, the output emptied by `/clear`, the source at its top again.
    let t5 = screen_text(&shot(5));
    assert!(!t5.contains("Commands") && !t5.contains("Tab moves between panes"), "{t5}");
    assert!(t5.contains("│ 1 -- A row"), "{t5}");

    // The diffs left the same cells as a full redraw at the end, colours included.
    let before = term.before_clear.last().expect("a last clear");
    assert_eq!(before, &term.grid, "the diffs left:\n{}\n\na full redraw gives:\n{}", screen_text(before), term.text());
}

/// A child run attached from the tour's palette: the tour puts the screen back before it and
/// draws nothing until it ends; then it takes the alternate screen again and draws it whole.
/// Keys come through a pipe, so that the key reader waits while the child runs.
#[test]
fn an_attached_child_gets_the_terminal_and_the_tour_comes_back() {
    let dir = Dir::new("attach");
    assert_ok(&dir.check(&root().join("examples/gtui_tour")));
    let child = dir.0.join(if cfg!(windows) { "child.exe" } else { "child" });
    let cc = Command::new("gcc")
        .arg(root().join("gardelc/tests/reactive/files_child.c"))
        .arg("-o")
        .arg(&child)
        .output()
        .expect("gcc runs");
    assert!(cc.status.success(), "{}", all(&cc));
    let mut p = Command::new(dir.bin("gtui_tour"))
        .arg(&child)
        .current_dir(&dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the tour runs");
    let mut input = p.stdin.take().expect("its input");
    for (chunk, pause) in [("/sh attached", 300), ("\r", 800), ("q", 0)] {
        input.write_all(chunk.as_bytes()).expect("a write");
        input.flush().expect("a flush");
        std::thread::sleep(Duration::from_millis(pause));
    }
    drop(input);
    let started = Instant::now();
    let out = p.wait_with_output().expect("the tour ends");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(out.status.code(), Some(0), "{}", all(&out));
    let text = String::from_utf8(out.stdout.clone()).expect("UTF-8");
    let leave = "\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l";
    let enter = "\x1b[?1049h";
    let gone = text.find(leave).unwrap_or_else(|| panic!("the screen is put back: {text:?}"));
    let rest = gone + leave.len();
    let back = rest + text[rest..].find(enter).unwrap_or_else(|| panic!("and taken again: {text:?}"));
    // Only what the child wrote lies between: no frame was drawn while it ran.
    assert_eq!(text[rest..back].trim_end(), "attached child", "{text:?}");
    // A terminal reads the rest: a pipe's size is 80 by 24, before and after.
    let mut before = Terminal::new(vec![(80, 24); 4]);
    before.feed(text[..gone].as_bytes());
    let mut after = Terminal::new(vec![(80, 24); 4]);
    after.feed(&text.as_bytes()[back..]);
    let shown = after.text();
    assert!(shown.contains("The program ended with 3"), "{shown}");
    assert!(shown.starts_with("╭─Modules(m)"), "the screen is drawn whole again:\n{shown}");
    assert!(text.ends_with(leave), "and given back at the end");
}

/// What a client proves of the widgets from their contracts alone, and claims past them.
#[test]
fn a_client_proves_the_widget_facts_through_the_contracts() {
    let source = |claims: &str| {
        format!(
            "functions CLIENT

feature {{ANY}}

  f(t: TABS, fo: FOCUS, tv: TEXT_VIEW, tr: TREE_VIEW, p: PALETTE, r: RECT, n: NAT, rows: NAT, k: KEY): BOOL
    do
      True
    ensure
{claims}
    end

end
"
        )
    };
    let facts = source(
        "      tab: t.choose(n).selected < t.count or t.count = 0
      tab_set: n < t.count implies t.choose(n).selected = n
      tab_keys: t.on_key(k).count = t.count
      focus: fo.next.current < fo.count
      focus_keys: fo.on_key(k).count = fo.count
      shown: (n < tv.line_count and rows > 0) implies (tv.showing(n, rows).top <= n and n < tv.showing(n, rows).top + rows)
      row: tr.selected_row < tr.count or tr.count = 0
      some_row: tr.selected.is_some = (tr.count > 0)
      inner: FRAME.inner(r).inside(r)
      title: FRAME.title_area(r).inside(r)
      over: p.place(r, n, rows).inside(r)
      chosen: p.chosen.is_some = (p.count > 0)",
    );
    let p = Project::new("gtui_widgets", &facts);
    assert_ok(&p.check());
    // Past the contracts: a tab past the last is not chosen, and with no rows no line is shown.
    for (name, claim) in [
        ("past", "      past: t.choose(n).selected = n"),
        ("no_rows", "      no_rows: n < tv.line_count implies n < tv.showing(n, rows).top + rows"),
    ] {
        let p = Project::new(&format!("gtui_{name}"), &source(claim));
        let out = p.check();
        assert!(!out.status.success(), "{name} must be refused: {}", all(&out));
        assert!(all(&out).contains(&format!("`{name}`")), "{name}: {}", all(&out));
    }
}

/// A client that types, takes a paste, shows the cursor after its text and copies the text:
/// a paste is one event after which keys are read again, the frame places the cursor, a copy
/// reaches the terminal as OSC 52, and any other `Write` of the client is dropped.
const TYPING: &str = "reactive program TYPING

feature {ANY}

  update(s: STRING, e: EVENT): TUI_NEXT[STRING]
    do
      match e
        case Pasted(t) then TUI_NEXT.next(GLYPHS.join(s, t))
        case Key(k) then
          match k
            case Char(c) then
              if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(GLYPHS.join(s, GLYPHS.text_of(LIST.empty.prepend(c)))) end
            case Ctrl(c) then
              if c = 'y' then Next(s, LIST.empty.prepend(Write(\"dropped\")).prepend(TUI_COMMANDS.copy(s))) else TUI_NEXT.next(s) end
            case Escape then TUI_NEXT.next(\"\")
            case _ then TUI_NEXT.next(s)
          end
        case _ then TUI_NEXT.next(s)
      end
    end

  view(s: STRING, blank: SCREEN): SCREEN
    do
      let drawn = blank.text(blank.area, 0, 0, s, STYLE.plain)
      if s.byte_length = 0 then drawn else drawn.with_cursor(GLYPHS.of_text(s).length, 0) end
    end

  init(args: SEQ[STRING]): STEP[TUI[STRING]]
    do
      TUI.start(\"\", update, view)
    end

  step(t: TUI[STRING], e: EVENT): STEP[TUI[STRING]]
    do
      t.step(e)
    end

end
";

#[test]
fn a_paste_is_one_event_the_cursor_follows_the_text_and_a_copy_reaches_the_clipboard() {
    let p = Project::new("gtui_typing", TYPING);
    let src = p.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("typing.gdl")).expect("named after its module");
    assert_ok(&p.check());
    let log = p.0 .0.join("run.log");
    let script = "Resized:20:4 Key:104 Key:105 Pasted:%20th%C3%A9re Key:Ctrl:y Key:120 Key:Ctrl:y Key:Escape Key:113";
    let ran = run(&p.0.bin("typing"), script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    // "hi thére" is eight characters: the cursor stood after them before Escape emptied it.
    // Its nine bytes need no padding, and the ten of "hi thérex" two `=`.
    assert_eq!(term.copies, ["aGkgdGjDqXJl", "aGkgdGjDqXJleA=="], "the copy is the text's UTF-8 in base64");
    let out = String::from_utf8_lossy(&ran.stdout);
    assert!(out.contains("\x1b[1;9H\x1b[?25h"), "the cursor is placed after the text and shown:\n{out:?}");
    assert!(out.contains("\x1b[0m\x1b[?25l"), "it is hidden while a frame is drawn, and when the text is gone:\n{out:?}");
    assert!(!out.contains("dropped"), "a Write that is not a copy is dropped");
    let text = fs::read_to_string(&log).expect("a log");
    let after_paste = text.split("event Pasted:%20th%C3%A9re\n").nth(1).expect("the paste is logged");
    let before_next = after_paste.split("event ").next().expect("commands");
    assert!(before_next.contains("command ReadKey\n"), "keys are read again after a paste:\n{before_next}");
}
