//! `gtui` (Task F.10): the package, its bench and `examples/todo_tui` verify under `--strict`;
//! a scripted session of the example, read by a small VT terminal, writes nothing outside the
//! screen and leaves the same cells as a full redraw; a shutdown puts the terminal back; a
//! replay repeats the run; a full redraw of 200 by 60 fits in a 16 ms frame; and a client
//! proves the layout facts through the contracts alone, while a claim past them is refused.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

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
}

impl Terminal {
    fn new(sizes: Vec<(usize, usize)>) -> Self {
        Self { sizes, clears: 0, size: None, grid: Vec::new(), row: 0, col: 0, pen: Vec::new(), before_clear: Vec::new(), printed: 0 }
    }

    fn feed(&mut self, bytes: &[u8]) {
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
                let fin = cs[j];
                match (private, fin) {
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
    assert!(ran.stdout.ends_with(b"\x1b[0m\x1b[?25h\x1b[?1049l"), "the screen is put back");
}

#[test]
fn a_shutdown_puts_the_terminal_back_and_a_replay_repeats_the_run() {
    let dir = Dir::new("replay");
    assert_ok(&dir.check(&root().join("examples/todo_tui")));
    let log = dir.0.join("run.log");
    let script = format!("Resized:50:6 Key:97 {}Key:Enter Key:32", keys("milk"));
    let first = run(&dir.bin("todo_tui"), &script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(first.status.code(), Some(1), "the end of a script is a shutdown: {}", all(&first));
    assert!(first.stdout.starts_with(b"\x1b[?1049h"), "the alternate screen is taken");
    assert!(first.stdout.ends_with(b"\x1b[0m\x1b[?25h\x1b[?1049l"), "and given back");
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
