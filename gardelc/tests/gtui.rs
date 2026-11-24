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
//!
//! `gtui` 0.3.0 (Task F.16b): wide characters take two cells and marks none, through a
//! terminal that draws them as terminals do; an editor on `TEXT_AREA` types, highlights a
//! string over lines, cuts, pastes, undoes, finds and scrolls both ways, and a key on 5000
//! lines fits in a frame; and a client proves the editing facts through the contracts.

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
            .env("GARDEL_CACHE_DIR", cache_dir())
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

/// A cell as a terminal shows it: its character (`'\0'` for the right half of a wide one), its pen, and the
/// zero-width characters drawn on it.
type Cell = (char, Vec<u32>, String);

/// The columns a terminal gives `c`, including Unicode 16.0 wide characters/emoji and
/// zero-width combining marks, variation selectors, and joiners.
fn columns(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036F | 0x0897 | 0x200B..=0x200D | 0xFE00..=0xFE0F | 0xE0020..=0xE007F => 0,
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x2614..=0x2615
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFF60
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F200..=0x1F265
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F7E0..=0x1F7F0
        | 0x1F90C..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

fn is_zwj(c: char) -> bool {
    c == '\u{200D}'
}

fn is_vs16(c: char) -> bool {
    c == '\u{FE0F}'
}

fn is_skin_tone(c: char) -> bool {
    matches!(c as u32, 0x1F3FB..=0x1F3FF)
}

fn is_regional_indicator(c: char) -> bool {
    matches!(c as u32, 0x1F1E6..=0x1F1FF)
}

/// A VT terminal, enough of one to read a frame: cursor moves, SGR, clearing the screen, and
/// the private modes, which it ignores. Every character must land on the screen of the size
/// the script last gave, and no other control byte may appear.
#[derive(Default)]
struct SgrPen {
    flags: [bool; 6],
    fg: Vec<u32>,
    bg: Vec<u32>,
}

fn parse_ext_sgr_color(params: &[u32], start_idx: usize, prefix: u32) -> (Vec<u32>, usize) {
    match params.get(start_idx + 1) {
        Some(&5) => {
            let palette_index = *params.get(start_idx + 2).expect("extended indexed SGR");
            (vec![prefix, 5, palette_index], 3)
        }
        Some(&2) => {
            let red = *params.get(start_idx + 2).expect("extended rgb r");
            let green = *params.get(start_idx + 3).expect("extended rgb g");
            let blue = *params.get(start_idx + 4).expect("extended rgb b");
            (vec![prefix, 2, red, green, blue], 5)
        }
        other => panic!("unexpected extended SGR {prefix} parameter {other:?} in {params:?}"),
    }
}

impl SgrPen {
    fn apply(&mut self, params: &[u32]) {
        if params.is_empty() {
            *self = Self::default();
            return;
        }
        let mut idx = 0;
        while idx < params.len() {
            match params[idx] {
                0 => {
                    *self = Self::default();
                    idx += 1;
                }
                1 => { self.flags[0] = true; idx += 1; }
                2 => { self.flags[1] = true; idx += 1; }
                3 => { self.flags[2] = true; idx += 1; }
                4 => { self.flags[3] = true; idx += 1; }
                7 => { self.flags[4] = true; idx += 1; }
                9 => { self.flags[5] = true; idx += 1; }
                22 => { self.flags[0] = false; self.flags[1] = false; idx += 1; }
                23 => { self.flags[2] = false; idx += 1; }
                24 => { self.flags[3] = false; idx += 1; }
                27 => { self.flags[4] = false; idx += 1; }
                29 => { self.flags[5] = false; idx += 1; }
                code @ (30..=37 | 90..=97) => {
                    self.fg = vec![code];
                    idx += 1;
                }
                39 => {
                    self.fg.clear();
                    idx += 1;
                }
                38 => {
                    let (color, used) = parse_ext_sgr_color(params, idx, 38);
                    self.fg = color;
                    idx += used;
                }
                code @ (40..=47 | 100..=107) => {
                    self.bg = vec![code];
                    idx += 1;
                }
                49 => {
                    self.bg.clear();
                    idx += 1;
                }
                48 => {
                    let (color, used) = parse_ext_sgr_color(params, idx, 48);
                    self.bg = color;
                    idx += used;
                }
                other => panic!("unexpected SGR parameter {other} in {params:?}"),
            }
        }
    }

    fn canonical(&self) -> Vec<u32> {
        const FLAG_CODES: [u32; 6] = [1, 2, 3, 4, 7, 9];
        let mut out = Vec::new();
        for (active, flag_code) in self.flags.iter().zip(FLAG_CODES) {
            if *active {
                out.push(flag_code);
            }
        }
        out.extend_from_slice(&self.fg);
        out.extend_from_slice(&self.bg);
        out
    }

    fn full_reset_len(&self) -> usize {
        let codes = self.canonical();
        let param_bytes: usize = codes
            .iter()
            .map(|code| 1 + code.to_string().len())
            .sum();
        // `\x1b[0` + `;...` + `m`
        4 + param_bytes
    }
}

struct Terminal {
    sizes: Vec<(usize, usize)>,
    clears: usize,
    size: Option<(usize, usize)>,
    grid: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    pen: Vec<u32>,
    pen_state: SgrPen,
    /// The cells just before each clear of the screen.
    before_clear: Vec<Vec<Vec<Cell>>>,
    printed: usize,
    /// Whether the cursor is shown, and where it was when it was last shown.
    cursor: Option<(usize, usize)>,
    /// What each OSC 52 sequence put on the clipboard, base64 as sent.
    copies: Vec<String>,
    sync_depth: usize,
    sync_frames: usize,
    mouse_1000: bool,
    mouse_1002: bool,
    mouse_1006: bool,
    mouse_enters: usize,
    mouse_leaves: usize,
    after_zwj: bool,
    ri_open: bool,
}

impl Terminal {
    fn new(sizes: Vec<(usize, usize)>) -> Self {
        Self {
            sizes,
            clears: 0,
            size: None,
            grid: Vec::new(),
            row: 0,
            col: 0,
            pen: Vec::new(),
            pen_state: SgrPen::default(),
            before_clear: Vec::new(),
            printed: 0,
            cursor: None,
            copies: Vec::new(),
            sync_depth: 0,
            sync_frames: 0,
            mouse_1000: false,
            mouse_1002: false,
            mouse_1006: false,
            mouse_enters: 0,
            mouse_leaves: 0,
            after_zwj: false,
            ri_open: false,
        }
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
                    (true, 'h') if params == vec![1000] => self.mouse_1000 = true,
                    (true, 'h') if params == vec![1002] => {
                        assert!(self.mouse_1000, "?1002h must follow ?1000h");
                        self.mouse_1002 = true;
                    }
                    (true, 'h') if params == vec![1006] => {
                        assert!(self.mouse_1000 && self.mouse_1002, "?1006h must follow ?1000h and ?1002h");
                        self.mouse_1006 = true;
                        self.mouse_enters += 1;
                    }
                    (true, 'l') if params == vec![1006] => {
                        assert!(self.mouse_1006, "?1006l without ?1006h");
                        self.mouse_1006 = false;
                    }
                    (true, 'l') if params == vec![1002] => {
                        assert!(!self.mouse_1006 && self.mouse_1002, "?1002l must follow ?1006l");
                        self.mouse_1002 = false;
                    }
                    (true, 'l') if params == vec![1000] => {
                        assert!(!self.mouse_1006 && !self.mouse_1002 && self.mouse_1000, "?1000l must follow ?1006l and ?1002l");
                        self.mouse_1000 = false;
                        self.mouse_leaves += 1;
                    }
                    (true, 'l') if params == vec![1049] => {
                        assert!(
                            !self.mouse_1000
                                && !self.mouse_1002
                                && !self.mouse_1006
                                && self.mouse_enters == self.mouse_leaves
                                && self.mouse_leaves > 0,
                            "mouse modes must be cleanly disabled before ?1049l"
                        );
                    }
                    (true, 'h') if params == vec![2026] => self.sync_depth += 1,
                    (true, 'l') if params == vec![2026] => {
                        assert!(self.sync_depth > 0, "unmatched ?2026l");
                        self.sync_depth -= 1;
                        self.sync_frames += 1;
                    }
                    (true, 'h') | (true, 'l') => {}
                    (false, 'H') => {
                        let (w, h) = self.size.expect("a cursor move after a size is known");
                        assert_eq!(params.len(), 2, "a move names a row and a column");
                        assert!(params[0] >= 1 && params[0] as usize <= h, "row {} outside {h} rows", params[0]);
                        assert!(params[1] >= 1 && params[1] as usize <= w, "column {} outside {w} columns", params[1]);
                        self.row = params[0] as usize - 1;
                        self.col = params[1] as usize - 1;
                        self.after_zwj = false;
                        self.ri_open = false;
                    }
                    (false, 'm') => {
                        self.pen_state.apply(&params);
                        self.pen = self.pen_state.canonical();
                    }
                    (false, 'J') => {
                        assert_eq!(params, vec![2], "only a whole clear");
                        self.before_clear.push(self.grid.clone());
                        self.size = if self.clears == 0 { None } else { Some(self.sizes[self.clears - 1]) };
                        self.clears += 1;
                        if let Some((w, h)) = self.size {
                            self.grid = vec![vec![(' ', Vec::new(), String::new()); w]; h];
                        }
                        self.after_zwj = false;
                        self.ri_open = false;
                    }
                    other => panic!("unexpected sequence {other:?}"),
                }
                i = j + 1;
            } else {
                assert!(!c.is_control(), "a control character {:?} was written", c);
                let (w, h) = self.size.expect("text after a size is known");
                let n = columns(c);
                let continues_cluster = self.col > 0
                    && (self.after_zwj
                        || is_zwj(c)
                        || is_vs16(c)
                        || is_skin_tone(c)
                        || n == 0
                        || (is_regional_indicator(c) && self.ri_open));
                if continues_cluster {
                    let mut x = self.col - 1;
                    let was_wide = self.grid[self.row][x].0 == '\0';
                    if was_wide {
                        x -= 1;
                    }
                    self.grid[self.row][x].2.push(c);
                    let promote = !was_wide
                        && (is_vs16(c)
                            || is_skin_tone(c)
                            || (is_regional_indicator(c) && self.ri_open)
                            || (self.after_zwj && n == 2));
                    if promote {
                        assert!(self.col < w, "promoted wide cluster at column {} outside {w}", self.col);
                        let pen = self.grid[self.row][x].1.clone();
                        if self.col + 1 < w && self.grid[self.row][self.col + 1].0 == '\0' {
                            let right_pen = self.grid[self.row][self.col + 1].1.clone();
                            self.grid[self.row][self.col + 1] = (' ', right_pen, String::new());
                        }
                        self.grid[self.row][self.col] = ('\0', pen, String::new());
                        self.col += 1;
                    }
                    self.after_zwj = is_zwj(c);
                    self.ri_open = false;
                    i += 1;
                    continue;
                }
                assert!(self.row < h && self.col + n <= w, "{c:?} written at row {}, column {}, outside {w} by {h}", self.row, self.col);
                let row = &mut self.grid[self.row];
                // Writing over half a wide character blanks its other half, as terminals do.
                if row[self.col].0 == '\0' {
                    row[self.col - 1] = (' ', row[self.col - 1].1.clone(), String::new());
                }
                if self.col + n < w && row[self.col + n].0 == '\0' {
                    row[self.col + n] = (' ', row[self.col + n].1.clone(), String::new());
                }
                row[self.col] = (c, self.pen.clone(), String::new());
                if n == 2 {
                    row[self.col + 1] = ('\0', self.pen.clone(), String::new());
                }
                self.col += n;
                self.printed += 1;
                self.after_zwj = false;
                self.ri_open = is_regional_indicator(c);
                i += 1;
            }
        }
    }

    fn text(&self) -> String {
        let shown = |c: &Cell| if c.0 == '\0' { c.2.clone() } else { format!("{}{}", c.0, c.2) };
        self.grid.iter().map(|r| r.iter().map(shown).collect::<String>()).collect::<Vec<_>>().join("\n")
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
    assert!(ran.stdout.ends_with(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l"), "the screen is put back");
    assert!(
        term.mouse_enters == 1 && term.mouse_leaves == 1 && !term.mouse_1000 && !term.mouse_1002 && !term.mouse_1006,
        "mouse tracking entered once and left cleanly"
    );
}

#[test]
fn a_shutdown_puts_the_terminal_back_and_a_replay_repeats_the_run() {
    let dir = Dir::new("replay");
    assert_ok(&dir.check(&root().join("examples/todo_tui")));
    let log = dir.0.join("run.log");
    let script = format!("Resized:50:6 Key:97 {}Key:Enter Key:32", keys("milk"));
    let first = run(&dir.bin("todo_tui"), &script, &[("GARDEL_RECORD", &log)]);
    assert_eq!(first.status.code(), Some(1), "the end of a script is a shutdown: {}", all(&first));
    assert!(
        first.stdout.starts_with(b"\x1b[?1049h\x1b[0m\x1b[2J\x1b[?25l\x1b[6 q\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h"),
        "the alternate screen is taken, with pastes bracketed and SGR mouse enabled"
    );
    assert!(
        first.stdout.ends_with(b"\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l"),
        "and given back"
    );
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
    let leave = "\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[0m\x1b[0 q\x1b[?25h\x1b[?1049l";
    let enter = "\x1b[?1049h";
    let gone = text.find(leave).unwrap_or_else(|| panic!("the screen is put back: {text:?}"));
    let rest = gone + leave.len();
    let back = rest + text[rest..].find(enter).unwrap_or_else(|| panic!("and taken again: {text:?}"));
    // Only what the child wrote lies between: no frame was drawn while it ran.
    assert_eq!(text[rest..back].trim_end(), "attached child", "{text:?}");
    // A terminal reads the rest: a pipe's size is 80 by 24, before and after.
    let mut before = Terminal::new(vec![(80, 24); 4]);
    before.feed(text[..gone].as_bytes());
    assert!(
        before.mouse_1000 && before.mouse_1002 && before.mouse_1006 && before.mouse_enters == 1 && before.mouse_leaves == 0,
        "mouse modes must be active before leave"
    );
    before.feed(text[gone..rest].as_bytes());
    assert!(
        !before.mouse_1000 && !before.mouse_1002 && !before.mouse_1006 && before.mouse_leaves == 1,
        "mouse modes must be disabled cleanly before attached child runs"
    );
    let mut after = Terminal::new(vec![(80, 24); 4]);
    after.feed(&text.as_bytes()[back..]);
    assert!(
        after.mouse_enters == 1 && after.mouse_leaves == 1 && !after.mouse_1000 && !after.mouse_1002 && !after.mouse_1006,
        "mouse modes must re-enter after AttachedEnded and leave cleanly at exit"
    );
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

/// A client that draws by display width (F.16b): wide characters, a combining accent, wide
/// characters cut by the clip on the right and on the left, one whose half is written over,
/// and an input line too long for its bar. Each key changes the middle of the first row
/// between `本` and `xy` and back, so the diffs write half a wide character's columns.
const WIDE: &str = "reactive program WIDE

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(if n < 100 then n + 1 else n end) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let p = STYLE.plain
      let a = blank.text(blank.area, 0, 0, \"日本語abc\", p)
      let a2 = if n % 2 = 1 then a.text(blank.area, 2, 0, \"xy\", p) else a end
      let b = a2.text(blank.area, 0, 1, \"e\\u{301}x\", p)
      let c = b.text(RECT.make(0, 2, 5, 1), 0, 2, \"ab日本\", p)
      let d0 = c.text(RECT.make(1, 3, 9, 1), 0, 3, \"日本\", p)
      -- A right half left alone where the clip cut its left half sends a space too, so the
      -- screen itself is looked at: `!` at the end of the row if its second cell is one.
      let row = d0.row(3)
      let orphan = row.length > 1 and LIST_OPS.at(row, 1).is_right_half
      let d = if orphan then d0.text(blank.area, 9, 3, \"!\", p) else d0 end
      let e = d.text(blank.area, 0, 4, \"日本語\", p).text(blank.area, 1, 4, \"x\", p)
      INPUT_LINE.of_text(\"日本語テキスト\").draw(e, RECT.make(0, 5, 10, 1), \"> \", p, p.reversed)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";

#[test]
fn wide_characters_take_two_cells_marks_none_and_a_cut_one_leaves_a_space() {
    let p = Project::new("gtui_wide", WIDE);
    let src = p.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("wide.gdl")).expect("named after its module");
    assert_ok(&p.check());
    let script = "Resized:10:6 Key:120 Resized:10:6 Key:120 Resized:10:6 Key:113";
    let ran = run(&p.0.bin("wide"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    let rest = "\ne\u{301}x        \nab日      \n  本      \n x本語    \n> キスト  ";
    let even = format!("日本語abc {rest}");
    let odd = format!("日xy語abc {rest}");
    // `before_clear[k + 1]` is the screen just before resize `k`: there the diffs drew it.
    assert_eq!(screen_text(&term.before_clear[2]), odd, "the diff from 本 to xy");
    assert_eq!(screen_text(&term.before_clear[3]), even, "the diff from xy back to 本");
    assert_eq!(term.text(), even, "drawn whole");
    let out = String::from_utf8_lossy(&ran.stdout);
    assert!(out.contains("\x1b[1;3Hxy"), "the diff writes the two columns 本 took:\n{out:?}");
    assert!(out.contains("\x1b[1;3H本\x1b"), "and 本 alone, never its right half:\n{out:?}");
    assert!(out.contains("e\u{301}x"), "the accent follows its letter:\n{out:?}");
    // The cursor's cell is reversed, after the end of the text.
    assert_eq!(term.grid[5][8].1, vec![7], "the cursor at column 8");
}

/// An editor (F.16b): a `TEXT_AREA` 30 by 5 with a status row below it, highlighting strings
/// in green, which may run over lines. F2 finds `a`, a paste is pasted, a cut or copy goes to
/// the terminal's clipboard, and Escape quits.
const EDITOR: &str = "reactive program EDITOR

feature {ANY}

  update(a: TEXT_AREA, e: EVENT): TUI_NEXT[TEXT_AREA]
    do
      match e
        case Pasted(t) then TUI_NEXT.next(a.paste(t))
        case Key(k) then
          match k
            case Escape then TUI_NEXT.quit(0)
            case Function(n) then
              if n = 2 then TUI_NEXT.next(a.find(\"a\"))
              else if n = 3 then TUI_NEXT.next(a.paste_chars(LIST.empty.prepend('2').prepend(CHAR.from_scalar(10)).prepend(CHAR.from_scalar(13)).prepend('1')))
              else TUI_NEXT.next(a)
              end end
            case _ then
              let b = a.on_key(k, RECT.make(0, 0, 30, 5))
              if b.just_copied then Next(b, LIST.empty.prepend(TUI_COMMANDS.copy(b.clipboard))) else TUI_NEXT.next(b) end
          end
        case _ then TUI_NEXT.next(a)
      end
    end

  view(a: TEXT_AREA, blank: SCREEN): SCREEN
    do
      let drawn = a.draw_code(blank, RECT.make(0, 0, 30, 5), STYLE.plain, STYLE.plain.fg(COLOR.gray), STYLE.plain.reversed,
        fn(start: NAT, line: LIST[CHAR]): LIST[SPAN] do spans(start, line, 0, 0, LIST.empty) end)
      let place = GLYPHS.join(GLYPHS.join(\"L\", NUMBER_TEXT.nat_to_text(a.line_number + 1)),
        GLYPHS.join(\" C\", NUMBER_TEXT.nat_to_text(a.column + 1)))
      drawn.text(blank.area, 0, 5, place, STYLE.plain)
    end

  -- The state after `cs` from `s`: 1 inside a string, 0 outside.
  ends(s: NAT, cs: LIST[CHAR]): NAT
    variant
      cs.length
    do
      if cs.length = 0 then s
      else ends(if cs.head = '\"' then (if s = 0 then 1 else 0 end) else s end, cs.tail)
      end
    end

  -- `acc` with the strings of `cs` as green spans, from state `s`, `cs` starting at character
  -- `i`, the open string at `from`.
  spans(s: NAT, cs: LIST[CHAR], i: NAT, from: NAT, acc: LIST[SPAN]): LIST[SPAN]
    require
      started: from <= i
    variant
      cs.length
    do
      if cs.length = 0 or i = 18446744073709551615 then
        if s != 0 and i > from then acc.prepend(SPAN.make(from, i - from, STYLE.plain.fg(COLOR.green))) else acc end
      else if cs.head = '\"' then
        if s != 0 then spans(0, cs.tail, i + 1, i + 1, acc.prepend(SPAN.make(from, i + 1 - from, STYLE.plain.fg(COLOR.green))))
        else spans(1, cs.tail, i + 1, i, acc)
        end
      else
        spans(s, cs.tail, i + 1, from, acc)
      end end
    end

  init(args: SEQ[STRING]): STEP[TUI[TEXT_AREA]]
    do
      TUI.start(TEXT_AREA.of_text(\"\").with_lexer(fn(start: NAT, line: LIST[CHAR]): NAT do ends(start, line) end), update, view)
    end

  step(t: TUI[TEXT_AREA], e: EVENT): STEP[TUI[TEXT_AREA]]
    do
      t.step(e)
    end

end
";

/// The editor, built in a directory of its own (`name`): tests run at once.
fn editor(name: &str) -> Project {
    let p = Project::new(name, EDITOR);
    let src = p.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("editor.gdl")).expect("named after its module");
    assert_ok(&p.check());
    p
}

#[test]
fn an_editor_types_highlights_over_lines_cuts_pastes_undoes_and_finds() {
    let p = editor("gtui_editor");
    let shot = "Resized:30:6 ";
    let mut script = String::from(shot);
    script += &keys("let a = 1");
    script += "Key:Enter ";
    script += &keys("say \"hi");
    script += "Key:Enter ";
    script += &keys("there\" ok");
    // 1: three lines, the string from line 2 into line 3.
    script += shot;
    script += "Key:Shift+Home Key:Ctrl:x ";
    // 2: the third line cut, and on the clipboard.
    script += shot;
    script += "Key:Up Key:Ctrl:v ";
    // 3: pasted at the start of line 2, which now closes its own strings.
    script += shot;
    script += "Key:Ctrl:z Key:Ctrl:z ";
    // 4: both undone: the text of 1, the cursor where the cut was made.
    script += shot;
    script += "Key:F2 ";
    // 5: `a` found on line 1, past the end of the text: selected.
    script += shot;
    script += "Key:98 Key:Down Key:Ctrl+Home Key:End Key:34 Key:Left Key:Enter ";
    // 6: `b` typed over it; then, from line 2, a quote at the end of line 1, moved to a line
    // of its own, opens a string the lines below are lexed again for.
    script += shot;
    script += "Key:F3 ";
    // 7: a text with a carriage return pasted (F3): it is left out.
    script += shot;
    script += "Key:Escape";
    let ran = run(&p.0.bin("editor"), &script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(&script));
    term.feed(&ran.stdout);
    let shot = |k: usize| term.before_clear[k + 1].clone();
    let rows = |k: usize| screen_text(&shot(k)).lines().map(|l| l.trim_end().to_string()).collect::<Vec<_>>();
    let first = rows(1);
    assert_eq!(first[..3], ["1 let a = 1", "2 say \"hi", "3 there\" ok"], "{first:?}");
    assert_eq!(first[5], "L3 C10");
    // The string runs from line 2 into line 3, and stops at its quote.
    assert_eq!(pen_at(&shot(1), "\"hi"), vec![32], "the string on line 2 is green");
    assert_eq!(pen_at(&shot(1), "there\""), vec![32], "and goes on into line 3");
    assert_eq!(pen_at(&shot(1), " ok"), Vec::<u32>::new(), "until its quote");
    assert_eq!(pen_at(&shot(1), "let"), Vec::<u32>::new(), "line 1 is plain");
    assert_eq!(pen_at(&shot(1), "1 let"), vec![90], "the numbers are gray");
    let cut = rows(2);
    assert_eq!(cut[..3], ["1 let a = 1", "2 say \"hi", "3"], "{cut:?}");
    assert_eq!(term.copies, ["dGhlcmUiIG9r"], "the cut text went to the clipboard");
    let pasted = rows(3);
    assert_eq!(pasted[..3], ["1 let a = 1", "2 there\" oksay \"hi", "3"], "{pasted:?}");
    assert_eq!(pen_at(&shot(3), "\" oksay \""), vec![32], "the string now opens after `there`");
    assert_eq!(pen_at(&shot(3), "hi"), Vec::<u32>::new(), "and `hi` is outside it");
    let undone = rows(4);
    assert_eq!(undone[..3], first[..3], "two undos give back the text");
    assert_eq!(undone[5], "L3 C1", "and the cursor where the cut was made");
    let found = rows(5);
    assert_eq!(found[5], "L1 C6", "the cursor after the `a` found");
    assert_eq!(pen_at(&shot(5), "a ="), vec![7], "which is selected");
    assert_eq!(pen_at(&shot(5), " = 1"), Vec::<u32>::new(), "and nothing after it");
    let quoted = rows(6);
    assert_eq!(quoted[..4], ["1 let b = 1", "2 \"", "3 say \"hi", "4 there\" ok"], "{quoted:?}");
    assert_eq!(pen_at(&shot(6), "let"), Vec::<u32>::new(), "line 1 ends outside a string");
    assert_eq!(pen_at(&shot(6), "say \""), vec![32], "line 2 opens one that line 3 closes");
    assert_eq!(pen_at(&shot(6), "hi"), Vec::<u32>::new(), "so `hi` is outside it");
    assert_eq!(pen_at(&shot(6), "there"), Vec::<u32>::new(), "and so is `there`");
    assert_eq!(pen_at(&shot(6), "\" ok"), vec![32], "whose quote opens one");
    let crlf = rows(7);
    assert_eq!(crlf[..4], ["1 let b = 1", "2 1", "3 2\"", "4 say \"hi"], "{crlf:?}");
}

#[test]
fn an_editor_scrolls_both_ways_to_keep_the_cursor_and_a_key_on_5000_lines_is_quick() {
    let p = editor("gtui_editor_long");
    // A line too long for the area, then 5000 numbered lines, ended as on Windows: the
    // carriage returns are left out.
    let mut text = String::from("0123456789abcdefghijklmnopqrstuvwxyz\r\n");
    for i in 0..5000 {
        text += &format!("line {i}\r\n");
    }
    let pasted = format!("Pasted:{} ", text.bytes().map(|b| format!("%{b:02X}")).collect::<String>());
    let script = format!("Resized:30:6 {pasted}Key:Ctrl+Home Key:End Resized:30:6 Key:Down Key:End Resized:30:6 Key:Home Resized:30:6 Key:Escape");
    let ran = run(&p.0.bin("editor"), &script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(&script));
    term.feed(&ran.stdout);
    let rows = |k: usize| screen_text(&term.before_clear[k + 1]).lines().map(|l| l.trim_end().to_string()).collect::<Vec<_>>();
    // At the end of the long line the page is scrolled right: the cursor's cell is the last.
    let right = rows(1);
    // The numbers take five columns, so the text 25: 36 characters and the cursor's cell.
    assert_eq!(right[0], "   1 cdefghijklmnopqrstuvwxyz", "{right:?}");
    assert_eq!(right[5], "L1 C37");
    // One line down, at the end of a short line, it is scrolled back as far as the cursor.
    let back = rows(2);
    assert_eq!(back[..2], ["   1 6789abcdefghijklmnopqrstu", "   2"], "{back:?}");
    assert_eq!(back[5], "L2 C7");
    assert_eq!(term.before_clear[3][1][5].1, Vec::<u32>::new(), "the cursor's cell, past the end of `line 0`");
    // Home scrolls it to the first column.
    let home = rows(3);
    assert_eq!(home[..2], ["   1 0123456789abcdefghijklmno", "   2 line 0"], "{home:?}");

    // A key costs a line, not the text: the frames of 200 keys on a text of 5000 lines.
    let time = |n: usize| {
        (0..3)
            .map(|_| {
                let mut s = format!("Resized:30:6 {pasted}");
                for i in 0..n {
                    s += if i % 2 == 0 { "Key:Up Key:120 " } else { "Key:Backspace " };
                }
                s += "Key:Escape";
                let t = std::time::Instant::now();
                let out = run(&p.0.bin("editor"), &s, &[]);
                assert_eq!(out.status.code(), Some(0), "{}", all(&out));
                t.elapsed()
            })
            .min()
            .expect("three runs")
    };
    let base = time(0);
    let keyed = time(200);
    let per_key = keyed.saturating_sub(base) / 300;
    eprintln!("a key on 5000 lines: {per_key:?} ({keyed:?} for 300 keys, {base:?} for none)");
    assert!(per_key.as_millis() < 16, "{per_key:?} per key");
}

#[test]
fn a_client_proves_the_editing_facts_through_the_contracts() {
    let source = |claims: &str| {
        format!(
            "functions CLIENT

feature {{ANY}}

  f(a: TEXT_AREA, cs: LIST[CHAR], c: CHAR, r: RECT): BOOL
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
        "      round: LINE_SPLIT.fits(cs) implies TEXT_AREA.of_chars(cs).state.chars = cs
      lines: LINE_SPLIT.join(LINE_SPLIT.lines_of(cs)) = cs
      typed: (c.code >= 32 and c.code != 127) implies a.type_char(c).undo.state = a.state
      fed: a.newline.undo.state = a.state
      erased: a.backspace.undo.state = a.state and a.delete.undo.state = a.state
      pasted: a.paste_chars(cs).undo.state = a.state
      cursor: a.line_number < a.line_count and a.column <= a.length_of_line(a.line_number)
      page: TEXT_AREA.text_part(r, a.line_count).height > 0 implies
        a.followed(r).top <= a.followed(r).line_number and
        a.followed(r).line_number < a.followed(r).top + TEXT_AREA.text_part(r, a.line_count).height
      start: a.selection_start_line < a.line_count and a.selection_start_column <= a.length_of_line(a.selection_start_line)
      end_: a.selection_end_line < a.line_count and a.selection_end_column <= a.length_of_line(a.selection_end_line)
      moves: a.state.left.lines = a.state.lines and a.state.down(3).lines = a.state.lines",
    );
    let p = Project::new("gtui_editing", &facts);
    assert_ok(&p.check());
    // Past the contracts: a typed character changes the text, and a text that does not fit
    // is not kept.
    for (name, claim) in [
        ("unchanged", "      unchanged: a.type_char(c).state = a.state"),
        ("any_text", "      any_text: TEXT_AREA.of_chars(cs).state.chars = cs"),
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

#[test]
fn gt2_extended_colors_styles_patch_and_themes_work() {
    // 1. Contract checks for COLOR (standard, indexed, rgb, reset) and STYLE (italic, strikethrough, patch).
    let source = "functions CLIENT

feature {ANY}

  f(n: NAT, r: NAT, g: NAT, b: NAT, s1: STYLE, s2: STYLE): BOOL
    do
      True
    ensure
      std_num: n < 16 implies (COLOR.standard(n).is_standard and COLOR.standard(n).number = n)
      std_def: n >= 16 implies COLOR.standard(n).is_default
      idx_set: n <= 255 implies (COLOR.indexed(n).is_indexed and COLOR.indexed(n).index = n)
      idx_bound: COLOR.indexed(n).index <= 255
      rgb_set: (r <= 255 and g <= 255 and b <= 255) implies
        (COLOR.rgb(r, g, b).is_rgb and COLOR.rgb(r, g, b).r = r and COLOR.rgb(r, g, b).g = g and COLOR.rgb(r, g, b).b = b)
      rgb_bound: COLOR.rgb(r, g, b).r <= 255 and COLOR.rgb(r, g, b).g <= 255 and COLOR.rgb(r, g, b).b <= 255
      reset_kind: COLOR.reset.is_reset and not COLOR.reset.is_default
      flags: STYLE.plain.italic.is_italic and STYLE.plain.strikethrough.is_strikethrough
      patch_bold: s1.patch(s2).is_bold = (s1.is_bold or s2.is_bold)
      patch_italic: s1.patch(s2).is_italic = (s1.is_italic or s2.is_italic)
      patch_strike: s1.patch(s2).is_strikethrough = (s1.is_strikethrough or s2.is_strikethrough)
    end

end
";
    let p = Project::new("gtui_gt2_contracts", source);
    assert_ok(&p.check());

    // 2. Runtime rendering of indexed, rgb, italic, strikethrough, and patch(reset).
    let prog = "reactive program GT2_RENDER

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(n) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let base = STYLE.plain.fg(COLOR.rgb(0, 128, 255)).bg(COLOR.indexed(236)).bold
      let patched = base.patch(STYLE.plain.fg(COLOR.reset).italic.strikethrough)
      let a = blank.text(blank.area, 0, 0, \"rgb\", base)
      a.text(blank.area, 0, 1, \"patched\", patched)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let rp = Project::new("gtui_gt2_render", prog);
    let src = rp.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt2_render.gdl")).expect("named after its module");
    assert_ok(&rp.check());
    let script = "Resized:20:4 Key:113";
    let ran = run(&rp.0.bin("gt2_render"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    assert_eq!(pen_at(&term.grid, "rgb"), vec![1, 38, 2, 0, 128, 255, 48, 5, 236]);
    assert_eq!(pen_at(&term.grid, "patched"), vec![1, 3, 9, 48, 5, 236]);

    // 3. Switching themes in gtui_tour: /theme nord (24-bit RGB + italic comments) and /theme light (256-color).
    let dir = Dir::new("tour_themes");
    assert_ok(&dir.check(&root().join("examples/gtui_tour")));
    let tour_script = format!(
        "Resized:90:24 Key:47 {}Key:Enter Resized:90:24 Key:47 {}Key:Enter Resized:90:24 Key:113",
        keys("theme nord"),
        keys("theme light")
    );
    let ran_tour = run(&dir.bin("gtui_tour"), &tour_script, &[]);
    assert_eq!(ran_tour.status.code(), Some(0), "{}", all(&ran_tour));
    let mut tour_term = Terminal::new(sizes(&tour_script));
    tour_term.feed(&ran_tour.stdout);
    let nord_shot = &tour_term.before_clear[2];
    let light_shot = &tour_term.before_clear[3];
    assert_eq!(pen_at(nord_shot, "╭─Modules"), vec![1, 38, 2, 136, 192, 208]);
    assert_eq!(pen_at(nord_shot, "-- A row"), vec![3, 38, 2, 163, 190, 140]);
    assert_eq!(pen_at(light_shot, "╭─Modules"), vec![1, 38, 5, 27]);
    assert_eq!(pen_at(light_shot, "-- A row"), vec![3, 38, 5, 28]);
}

fn sgr_delta_stats(vt: &str) -> (usize, usize, usize) {
    let bytes = vt.as_bytes();
    let mut idx = 0;
    let mut pen = SgrPen::default();
    let mut delta_count = 0;
    let mut actual_bytes = 0;
    let mut full_reset_bytes = 0;
    while idx < bytes.len() {
        if bytes[idx] == 0x1b && bytes.get(idx + 1) == Some(&b'[') {
            let mut end = idx + 2;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';' || bytes[end] == b'?') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'm' && bytes.get(idx + 2) != Some(&b'?') {
                let raw = std::str::from_utf8(&bytes[idx + 2..end]).unwrap_or("");
                let params: Vec<u32> = raw.split(';').map(|part| part.parse().unwrap_or(0)).collect();
                pen.apply(&params);
                if params.first() != Some(&0) {
                    delta_count += 1;
                }
                actual_bytes += (end + 1) - idx;
                full_reset_bytes += pen.full_reset_len();
            }
            idx = end + 1;
        } else {
            idx += 1;
        }
    }
    (delta_count, actual_bytes, full_reset_bytes)
}

#[test]
fn gt1_synchronized_output_and_sgr_deltas_work() {
    // 1. Real session (gtui_tour): balanced ?2026h/?2026l, delta SGR savings, and VT equivalence.
    let dir = Dir::new("gt1_tour");
    assert_ok(&dir.check(&root().join("examples/gtui_tour")));
    let tour_script = tour_session();
    let ran_tour = run(&dir.bin("gtui_tour"), &tour_script, &[]);
    assert_eq!(ran_tour.status.code(), Some(0), "{}", all(&ran_tour));
    let mut tour_term = Terminal::new(sizes(&tour_script));
    tour_term.feed(&ran_tour.stdout);
    assert_eq!(tour_term.sync_depth, 0, "all ?2026h/?2026l pairs must close cleanly");
    assert!(tour_term.sync_frames > 0, "gtui_tour must emit synchronized frames");
    let tour_before = tour_term.before_clear.last().expect("final resize clear in tour_session");
    assert_eq!(tour_before, &tour_term.grid, "tour incremental diff screen must equal full redraw");
    let tour_vt = String::from_utf8(ran_tour.stdout.clone()).expect("valid utf-8 VT output");
    let (tour_deltas, tour_sgr_bytes, tour_full_bytes) = sgr_delta_stats(&tour_vt);
    assert!(tour_deltas > 0, "gtui_tour must emit delta SGR sequences without leading 0;");
    assert!(
        tour_sgr_bytes < tour_full_bytes,
        "gtui_tour SGR bytes ({tour_sgr_bytes}) must be smaller than full-reset SGR bytes ({tour_full_bytes})"
    );

    // 2. Targeted SGR delta transitions, empty/idle frame suppression, and VT canonical equivalence.
    let prog = "reactive program GT1_DELTAS

feature {ANY}

  update(step: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then
              if c = 'q' then
                TUI_NEXT.quit(0)
              else if c = 'n' then
                TUI_NEXT.next(1)
              else
                TUI_NEXT.next(step)
              end end
            case _ then TUI_NEXT.next(step)
          end
        case _ then TUI_NEXT.next(step)
      end
    end

  view(step: NAT, blank: SCREEN): SCREEN
    do
      let s0 = STYLE.plain.bold.fg(COLOR.cyan)
      let s1 = s0.dim.fg(COLOR.red)
      let s2 = s1.bg(COLOR.rgb(10, 20, 30))
      let s3 = s2.italic
      let s4 = STYLE.plain.fg(COLOR.indexed(200))
      let s5 = s4.bg(COLOR.indexed(236)).underlined.reversed.strikethrough
      let a = blank.text(blank.area, 0, 0, \"AA\", s0)
      let b = a.text(blank.area, 2, 0, \"BB\", s1)
      let c = b.text(blank.area, 4, 0, \"CC\", s2)
      let d = c.text(blank.area, 6, 0, \"DD\", s3)
      let e = d.text(blank.area, 8, 0, \"EE\", s4)
      let f = e.text(blank.area, 10, 0, \"FF\", s5)
      let g = f.text(blank.area, 12, 0, \"GG\", STYLE.plain)
      let empty_check = if VT_TEXT.frame(LIST.empty, False).byte_length = 0 then \"EMPTY_OK\" else \"EMPTY_FAIL\" end
      let h = g.text(blank.area, 0, 2, empty_check, STYLE.plain)
      if step > 0 then
        h.text(blank.area, 0, 1, \"UPDATED\", s0)
      else
        h
      end
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let rp = Project::new("gtui_gt1_deltas", prog);
    let src = rp.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt1_deltas.gdl")).expect("named after its module");
    assert_ok(&rp.check());
    let script = "Resized:40:6 Key:105 Key:110 Resized:40:6 Key:113";
    let ran = run(&rp.0.bin("gt1_deltas"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    assert_eq!(term.sync_depth, 0);
    // Initial draw + Key:110 diff + final Resized full redraw; Key:105 idle emits no frame.
    assert_eq!(term.sync_frames, 3, "idle update must not emit a frame");
    let before = term.before_clear.last().expect("final resize clear");
    assert_eq!(before, &term.grid, "incremental frame matches full redraw");

    let vt = String::from_utf8(ran.stdout.clone()).expect("valid utf-8 VT output");
    for expected in [
        "\x1b[?2026h",
        "\x1b[?2026l",
        "\x1b[1;36m",
        "\x1b[2;31m",
        "\x1b[48;2;10;20;30m",
        "\x1b[3m",
        "\x1b[0;38;5;200m",
        "\x1b[4;7;9;48;5;236m",
    ] {
        assert!(vt.contains(expected), "missing expected sequence {expected:?} in:\n{vt:?}");
    }
    assert!(term.text().contains("EMPTY_OK"), "empty frame without clear must be 0 bytes");
    assert!(term.text().contains("UPDATED"), "incremental update must draw UPDATED");
    assert_eq!(pen_at(&term.grid, "AA"), vec![1, 36]);
    assert_eq!(pen_at(&term.grid, "BB"), vec![1, 2, 31]);
    assert_eq!(pen_at(&term.grid, "CC"), vec![1, 2, 31, 48, 2, 10, 20, 30]);
    assert_eq!(pen_at(&term.grid, "DD"), vec![1, 2, 3, 31, 48, 2, 10, 20, 30]);
    assert_eq!(pen_at(&term.grid, "EE"), vec![38, 5, 200]);
    assert_eq!(pen_at(&term.grid, "FF"), vec![4, 7, 9, 38, 5, 200, 48, 5, 236]);
    assert_eq!(pen_at(&term.grid, "GG"), Vec::<u32>::new());

    let (deltas, actual_bytes, full_bytes) = sgr_delta_stats(&vt);
    assert!(deltas >= 5);
    assert!(actual_bytes < full_bytes, "expected {actual_bytes} < {full_bytes}");
}

#[test]
fn gt3_layout_constraints_gaps_and_justify_work() {
    // 1. Contract checks: SIZE constructors, JUSTIFY constructors, gap & justify LAYOUT guarantees.
    let contracts = "functions CLIENT

feature {ANY}

  check_size_justify(n: NAT, p: NAT): BOOL
    do
      True
    ensure
      min_kind: SIZE.min(n).is_min and not SIZE.min(n).is_flex and SIZE.min(n).min_cells = n
      max_kind: SIZE.max(n).is_max and not SIZE.max(n).is_flex and SIZE.max(n).max_cells = n
      pct_ok: p <= 100 implies (SIZE.percent(p).is_percent and SIZE.percent(p).percentage = p)
      pct_cap: p > 100 implies (SIZE.percent(p).is_percent and SIZE.percent(p).percentage = 100)
      j_kinds: JUSTIFY.start.is_start and JUSTIFY.center.is_center and JUSTIFY.finish.is_end and
        JUSTIFY.end_align.is_end and JUSTIFY.space_between.is_space_between
    end

  check_layout(r: RECT, sizes: LIST[SIZE], gap: NAT, justify: JUSTIFY, i: NAT, j: NAT): BOOL
    do
      let cg = LAYOUT.columns_with_gap(r, sizes, gap)
      let rg = LAYOUT.rows_with_gap(r, sizes, gap)
      let cj = LAYOUT.columns_justified(r, sizes, gap, justify)
      let rj = LAYOUT.rows_justified(r, sizes, gap, justify)
      if i < j and j < sizes.length then
        check
          c_in: LAYOUT.across_inside(cg, r.x, r, i)
          r_in: LAYOUT.down_inside(rg, r.y, r, i)
          cj_in: LAYOUT.across_inside(cj, r.x, r, i)
          rj_in: LAYOUT.down_inside(rj, r.y, r, i)
          c_ap: LAYOUT_APART.columns_apart(cg, r.x, r, i, j)
          r_ap: LAYOUT_APART.rows_apart(rg, r.y, r, i, j)
          cj_ap: LAYOUT_APART.columns_apart(cj, r.x, r, i, j)
          rj_ap: LAYOUT_APART.rows_apart(rj, r.y, r, i, j)
        end
        True
      else if i < sizes.length then
        check
          c_in: LAYOUT.across_inside(cg, r.x, r, i)
          r_in: LAYOUT.down_inside(rg, r.y, r, i)
          cj_in: LAYOUT.across_inside(cj, r.x, r, i)
          rj_in: LAYOUT.down_inside(rj, r.y, r, i)
        end
        True
      else
        True
      end end
    ensure
      same_len:
        LAYOUT.columns_with_gap(r, sizes, gap).length = sizes.length and
        LAYOUT.rows_with_gap(r, sizes, gap).length = sizes.length and
        LAYOUT.columns_justified(r, sizes, gap, justify).length = sizes.length and
        LAYOUT.rows_justified(r, sizes, gap, justify).length = sizes.length
      cg_in: i < sizes.length implies LIST_OPS.at(LAYOUT.columns_with_gap(r, sizes, gap), i).inside(r)
      rg_in: i < sizes.length implies LIST_OPS.at(LAYOUT.rows_with_gap(r, sizes, gap), i).inside(r)
      cj_in: i < sizes.length implies LIST_OPS.at(LAYOUT.columns_justified(r, sizes, gap, justify), i).inside(r)
      rj_in: i < sizes.length implies LIST_OPS.at(LAYOUT.rows_justified(r, sizes, gap, justify), i).inside(r)
      cg_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.columns_with_gap(r, sizes, gap), i).disjoint(LIST_OPS.at(LAYOUT.columns_with_gap(r, sizes, gap), j))
      rg_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.rows_with_gap(r, sizes, gap), i).disjoint(LIST_OPS.at(LAYOUT.rows_with_gap(r, sizes, gap), j))
      cj_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.columns_justified(r, sizes, gap, justify), i).disjoint(LIST_OPS.at(LAYOUT.columns_justified(r, sizes, gap, justify), j))
      rj_apart: (i < j and j < sizes.length) implies
        LIST_OPS.at(LAYOUT.rows_justified(r, sizes, gap, justify), i).disjoint(LIST_OPS.at(LAYOUT.rows_justified(r, sizes, gap, justify), j))
    end

end
";
    let contract_proj = Project::new("gtui_gt3_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime constraint, gap, overflow, and justification edge cases.
    let prog = "reactive program GT3_LAYOUT

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(n) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  fmt_spans(rs: LIST[RECT], horiz: BOOL): STRING
    variant
      rs.length
    do
      if rs.length = 0 then
        \"\"
      else
        let pos = if horiz then rs.head.x else rs.head.y end
        let len = if horiz then rs.head.width else rs.head.height end
        let item = GLYPHS.join(GLYPHS.join(NUMBER_TEXT.nat_to_text(pos), \"-\"), NUMBER_TEXT.nat_to_text(len))
        if rs.tail.length = 0 then item else GLYPHS.join(GLYPHS.join(item, \";\"), fmt_spans(rs.tail, horiz)) end
      end
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let none: LIST[SIZE] = LIST.empty
      let pct_s = none.prepend(SIZE.percent(25)).prepend(SIZE.percent(50)).prepend(SIZE.percent(25))
      let mix_s = none.prepend(SIZE.min(20)).prepend(SIZE.flex(1)).prepend(SIZE.max(15))
      let ovr_s = none.prepend(SIZE.min(30)).prepend(SIZE.min(30))
      let cgap_s = none.prepend(SIZE.flex(1)).prepend(SIZE.flex(1))
      let rgap_s = none.prepend(SIZE.flex(1)).prepend(SIZE.fixed(3))
      let two_f = none.prepend(SIZE.fixed(10)).prepend(SIZE.fixed(10))
      let three_f = two_f.prepend(SIZE.fixed(10))

      let r0 = GLYPHS.join(\"PCT:\", fmt_spans(LAYOUT.columns(RECT.sized(80, 5), pct_s), True))
      let r1 = GLYPHS.join(\"MIX:\", fmt_spans(LAYOUT.columns(RECT.sized(100, 5), mix_s), True))
      let r2 = GLYPHS.join(\"OVR:\", fmt_spans(LAYOUT.columns(RECT.sized(40, 5), ovr_s), True))
      let r3 = GLYPHS.join(\"CGAP:\", fmt_spans(LAYOUT.columns_with_gap(RECT.sized(100, 5), cgap_s, 10), True))
      let r4 = GLYPHS.join(\"RGAP:\", fmt_spans(LAYOUT.rows_with_gap(RECT.sized(20, 10), rgap_s, 2), False))
      let r5 = GLYPHS.join(\"HUGE:\", fmt_spans(LAYOUT.columns_with_gap(RECT.sized(15, 5), two_f, 20), True))
      let r6 = GLYPHS.join(\"JS:\", fmt_spans(LAYOUT.columns_justified(RECT.sized(50, 5), two_f, 0, JUSTIFY.start), True))
      let r7 = GLYPHS.join(\"JC:\", fmt_spans(LAYOUT.columns_justified(RECT.sized(50, 5), two_f, 0, JUSTIFY.center), True))
      let r8 = GLYPHS.join(\"JE:\", fmt_spans(LAYOUT.columns_justified(RECT.sized(50, 5), two_f, 0, JUSTIFY.finish), True))
      let r9 = GLYPHS.join(\"JB2:\", fmt_spans(LAYOUT.columns_justified(RECT.sized(50, 5), two_f, 0, JUSTIFY.space_between), True))
      let r10 = GLYPHS.join(\"JB3:\", fmt_spans(LAYOUT.columns_justified(RECT.sized(50, 5), three_f, 0, JUSTIFY.space_between), True))
      let r11 = GLYPHS.join(\"JRC:\", fmt_spans(LAYOUT.rows_justified(RECT.sized(10, 50), two_f, 0, JUSTIFY.center), False))

      let a = blank.text(blank.area, 0, 0, r0, STYLE.plain)
      let b = a.text(blank.area, 0, 1, r1, STYLE.plain)
      let c = b.text(blank.area, 0, 2, r2, STYLE.plain)
      let d = c.text(blank.area, 0, 3, r3, STYLE.plain)
      let e = d.text(blank.area, 0, 4, r4, STYLE.plain)
      let f = e.text(blank.area, 0, 5, r5, STYLE.plain)
      let g = f.text(blank.area, 0, 6, r6, STYLE.plain)
      let h = g.text(blank.area, 0, 7, r7, STYLE.plain)
      let i = h.text(blank.area, 0, 8, r8, STYLE.plain)
      let j = i.text(blank.area, 0, 9, r9, STYLE.plain)
      let k = j.text(blank.area, 0, 10, r10, STYLE.plain)
      k.text(blank.area, 0, 11, r11, STYLE.plain)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt3_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt3_layout.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());
    let script = "Resized:80:14 Key:113";
    let ran = run(&runtime_proj.0.bin("gt3_layout"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    let shown = term.text();
    for expected in [
        "PCT:0-20;20-40;60-20",
        "MIX:0-15;15-32;47-53",
        "OVR:0-30;30-10",
        "CGAP:0-45;55-45",
        "RGAP:0-3;5-5",
        "HUGE:0-10;15-0",
        "JS:0-10;10-10",
        "JC:15-10;25-10",
        "JE:30-10;40-10",
        "JB2:0-10;40-10",
        "JB3:0-10;20-10;40-10",
        "JRC:15-10;25-10",
    ] {
        assert!(shown.contains(expected), "missing {expected:?} in:\n{shown}");
    }
}

#[test]
fn gt4_frame_borders_padding_titles_and_junction_merge_work() {
    // 1. Formal contract checks for BORDER_KIND, PADDING, inner_padded, bottom_title_area, multi-title, and merge_borders.
    let contracts = "functions CLIENT

feature {ANY}

  check_frame_ext(s: SCREEN, r: RECT, pad: PADDING, st: STYLE, title: STRING, titles: LIST[STRING], n: NAT): BOOL
    do
      True
    ensure
      kinds: BORDER_KIND.rounded.is_rounded and BORDER_KIND.plain.is_plain and
        BORDER_KIND.double.is_double and BORDER_KIND.thick.is_thick
      pad_none: PADDING.none.top = 0 and PADDING.none.right = 0 and PADDING.none.bottom = 0 and PADDING.none.left = 0
      pad_uniform: PADDING.uniform(n).top = n and PADDING.uniform(n).left = n
      pad_sym: PADDING.symmetric(n, n).top = n and PADDING.symmetric(n, n).right = n
      pad_hv: PADDING.horizontal(n).left = n and PADDING.vertical(n).bottom = n
      pad_in: FRAME.pad_rect(r, pad).inside(r)
      inner_pad_in: FRAME.inner_padded(r, pad).inside(r)
      bot_title_in: FRAME.bottom_title_area(r).inside(r) and FRAME.bottom_title_area(r).height <= 1
      styled_dim: FRAME.draw_styled(s, r, BORDER_KIND.double, title, st, st).width = s.width and
        FRAME.draw_styled(s, r, BORDER_KIND.double, title, st, st).height = s.height
      titled_dim: FRAME.draw_titled(s, r, BORDER_KIND.thick, title, title, title, st, st).width = s.width and
        FRAME.draw_titled(s, r, BORDER_KIND.thick, title, title, title, st, st).height = s.height
      multi_dim: FRAME.draw_titles(s, r, BORDER_KIND.plain, titles, st, st).width = s.width and
        FRAME.draw_titles(s, r, BORDER_KIND.plain, titles, st, st).height = s.height
      merge_dim: FRAME.merge_borders(s, r).width = s.width and FRAME.merge_borders(s, r).height = s.height
    end

end
";
    let contract_proj = Project::new("gtui_gt4_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime border kinds, padding, multi-title, and junction merging (┬, ┴, ├, ┤, ┼, ┳, ┻).
    let prog = "reactive program GT4_FRAME

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(n) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let st = STYLE.plain
      let s0 = FRAME.draw_styled(blank, RECT.make(0, 0, 10, 4), BORDER_KIND.rounded, \"R\", st, st)
      let s1 = FRAME.draw_styled(s0, RECT.make(11, 0, 10, 4), BORDER_KIND.plain, \"P\", st, st)
      let s2 = FRAME.draw_styled(s1, RECT.make(22, 0, 10, 4), BORDER_KIND.double, \"D\", st, st)
      let s3 = FRAME.draw_styled(s2, RECT.make(33, 0, 10, 4), BORDER_KIND.thick, \"T\", st, st)

      let tr = RECT.make(0, 5, 24, 5)
      let s4 = FRAME.draw_titled(s3, tr, BORDER_KIND.plain, \"Left\", \"Right\", \"Foot\", st, st)
      let ip = FRAME.inner_padded(tr, PADDING.make(1, 2, 1, 3))
      let s5 = s4.text(ip, ip.x, ip.y, \"PAD\", st)

      let none: LIST[STRING] = LIST.empty
      let multi = none.prepend(\"C\").prepend(\"B\").prepend(\"A\")
      let s6 = FRAME.draw_titles(s5, RECT.make(25, 5, 25, 5), BORDER_KIND.double, multi, st, st)

      let g0 = FRAME.draw(s6, RECT.make(0, 11, 11, 5), \"\", st, st)
      let g1 = FRAME.draw(g0, RECT.make(10, 11, 11, 5), \"\", st, st)
      let g2 = FRAME.draw(g1, RECT.make(0, 15, 11, 5), \"\", st, st)
      let g3 = FRAME.draw(g2, RECT.make(10, 15, 11, 5), \"\", st, st)
      let g4 = FRAME.merge_borders(g3, RECT.make(0, 11, 21, 9))

      let h0 = FRAME.draw_styled(g4, RECT.make(25, 11, 11, 5), BORDER_KIND.thick, \"\", st, st)
      let h1 = FRAME.draw_styled(h0, RECT.make(35, 11, 11, 5), BORDER_KIND.thick, \"\", st, st)
      FRAME.merge_borders(h1, RECT.make(25, 11, 21, 5))
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt4_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt4_frame.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());
    let script = "Resized:50:22 Key:113";
    let ran = run(&runtime_proj.0.bin("gt4_frame"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    let shown = term.text();

    // All 4 border styles
    assert_eq!((term.grid[0][0].0, term.grid[0][9].0, term.grid[3][0].0, term.grid[3][9].0), ('╭', '╮', '╰', '╯'));
    assert_eq!((term.grid[0][11].0, term.grid[0][20].0, term.grid[3][11].0, term.grid[3][20].0), ('┌', '┐', '└', '┘'));
    assert_eq!((term.grid[0][22].0, term.grid[0][31].0, term.grid[3][22].0, term.grid[3][31].0), ('╔', '╗', '╚', '╝'));
    assert_eq!((term.grid[0][33].0, term.grid[0][42].0, term.grid[3][33].0, term.grid[3][42].0), ('┏', '┓', '┗', '┛'));

    // Titled + inner_padded + multi-title
    assert!(shown.contains("┌─ Left ─────── Right ─┐"), "top left+right title:\n{shown}");
    assert!(shown.contains("└─ Foot ─"), "bottom title:\n{shown}");
    assert_eq!((term.grid[7][4].0, term.grid[7][5].0, term.grid[7][6].0), ('P', 'A', 'D'));
    assert!(shown.contains("╔═ A ═ B ═ C "), "multi-title:\n{shown}");

    // 2x2 rounded border merge junctions and preserved outer corners
    assert_eq!(term.grid[11][10].0, '┬', "top shared junction:\n{shown}");
    assert_eq!(term.grid[19][10].0, '┴', "bottom shared junction:\n{shown}");
    assert_eq!(term.grid[15][0].0, '├', "left shared junction:\n{shown}");
    assert_eq!(term.grid[15][20].0, '┤', "right shared junction:\n{shown}");
    assert_eq!(term.grid[15][10].0, '┼', "center 4-way junction:\n{shown}");
    assert_eq!((term.grid[11][0].0, term.grid[11][20].0, term.grid[19][0].0, term.grid[19][20].0), ('╭', '╮', '╰', '╯'));

    // Thick border merge junctions
    assert_eq!(term.grid[11][35].0, '┳', "thick top junction:\n{shown}");
    assert_eq!(term.grid[15][35].0, '┻', "thick bottom junction:\n{shown}");
    assert_eq!((term.grid[11][25].0, term.grid[11][45].0, term.grid[15][25].0, term.grid[15][45].0), ('┏', '┓', '┗', '┛'));
}

#[test]
fn gt5_paragraph_wrapping_alignment_and_scroll_work() {
    // 1. Formal contract checks for ALIGN, WRAP_MODE, and PARAGRAPH.
    let contracts = "functions CLIENT

feature {ANY}

  check_paragraph(s: SCREEN, r: RECT, p: PARAGRAPH, st: STYLE, w: NAT, rows: NAT, t: NAT, text: STRING): BOOL
    do
      True
    ensure
      align_kinds: ALIGN.left.is_left and ALIGN.center.is_center and ALIGN.right.is_right
      align_bounds: ALIGN.left.offset(w, rows) <= rows and ALIGN.center.offset(w, rows) <= rows and ALIGN.right.offset(w, rows) <= rows
      wrap_kinds: WRAP_MODE.word.is_word and WRAP_MODE.wrap_word.is_word and
        WRAP_MODE.char_wrap.is_char and WRAP_MODE.wrap_char.is_char and
        WRAP_MODE.none.is_none and WRAP_MODE.no_wrap.is_none
      of_text_zero: PARAGRAPH.of_text(text).top = 0 and PARAGRAPH.of_text(text).left_col = 0
      last_bound: p.last_top(w, rows) <= p.line_count(w)
      scroll_bound: p.scrolled_to(t, w, rows).top <= p.last_top(w, rows)
      draw_dim: p.draw(s, r, st).width = s.width and p.draw(s, r, st).height = s.height
    end

end
";
    let contract_proj = Project::new("gtui_gt5_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime WrapWord, WrapChar (ASCII + wide chars), NoWrap + horizontal scroll, ALIGN, and scrolling.
    let prog = "reactive program GT5_PARAGRAPH

feature {ANY}

  update(p: PARAGRAPH, e: EVENT): TUI_NEXT[PARAGRAPH]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(p) end
            case _ then TUI_NEXT.next(p.on_key(k, RECT.make(0, 12, 12, 3)))
          end
        case _ then TUI_NEXT.next(p)
      end
    end

  view(p: PARAGRAPH, blank: SCREEN): SCREEN
    do
      let st = STYLE.plain
      let s0 = PARAGRAPH.of_text(\"alpha beta gamma delta\").draw(blank, RECT.make(0, 0, 11, 4), st)
      let s1 = PARAGRAPH.of_text(\"supercalifragilistic\").draw(s0, RECT.make(14, 0, 10, 4), st)
      let s2 = PARAGRAPH.of_text(\"abcdefghij\").wrap_char.draw(s1, RECT.make(28, 0, 4, 4), st)
      let s3 = PARAGRAPH.of_text(\"0123456789\").no_wrap.with_scroll_x(3).draw(s2, RECT.make(0, 5, 5, 1), st)
      let s4 = s3.text(blank.area, 5, 5, \"|\", st)
      let s5 = PARAGRAPH.of_text(\"hi\").align_left.draw(s4, RECT.make(0, 7, 10, 1), st)
      let s6 = PARAGRAPH.of_text(\"hi\").align_center.draw(s5, RECT.make(0, 8, 10, 1), st)
      let s7 = PARAGRAPH.of_text(\"hi\").align_right.draw(s6, RECT.make(0, 9, 10, 1), st)
      let s8 = PARAGRAPH.of_text(\"日本語a\").wrap_char.draw(s7, RECT.make(28, 5, 3, 3), st)
      let s9 = p.draw(s8, RECT.make(0, 12, 12, 3), st)
      let clamped = p.scroll_down(99, 12, 3)
      let info = GLYPHS.join(NUMBER_TEXT.nat_to_text(clamped.top), \"/\")
      let full_info = GLYPHS.join(info, NUMBER_TEXT.nat_to_text(p.last_top(12, 3)))
      s9.text(blank.area, 20, 12, full_info, st)
    end

  init(args: SEQ[STRING]): STEP[TUI[PARAGRAPH]]
    do
      let sample = PARAGRAPH.of_text(\"row0 one two\\nrow1 three four\\nrow2 five\")
      TUI.start(sample, update, view)
    end

  step(t: TUI[PARAGRAPH], e: EVENT): STEP[TUI[PARAGRAPH]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt5_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt5_paragraph.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());
    let script = "Resized:40:18 Resized:40:18 Key:Down Resized:40:18 Key:PageDown Key:End Resized:40:18 Key:113";
    let ran = run(&runtime_proj.0.bin("gt5_paragraph"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|cell| cell.0).filter(|ch| *ch != '\0').collect()
    };

    let init_shot = &term.before_clear[2];
    // WrapWord
    assert_eq!(slice(init_shot, 0, 0, 11), "alpha beta ");
    assert_eq!(slice(init_shot, 1, 0, 11), "gamma delta");
    assert_eq!(slice(init_shot, 0, 14, 24), "supercalif");
    assert_eq!(slice(init_shot, 1, 14, 24), "ragilistic");

    // WrapChar (ASCII and wide Japanese characters in width 3)
    assert_eq!(slice(init_shot, 0, 28, 32), "abcd");
    assert_eq!(slice(init_shot, 1, 28, 32), "efgh");
    assert_eq!(slice(init_shot, 2, 28, 32), "ij  ");
    assert_eq!(slice(init_shot, 5, 28, 31), "日 ");
    assert_eq!(slice(init_shot, 6, 28, 31), "本 ");
    assert_eq!(slice(init_shot, 7, 28, 31), "語a");

    // NoWrap + horizontal scroll clipped to 5 columns
    assert_eq!(slice(init_shot, 5, 0, 6), "34567|");

    // Alignment (left @ 0, center @ 4, right @ 8)
    assert_eq!(slice(init_shot, 7, 0, 10), "hi        ");
    assert_eq!(slice(init_shot, 8, 0, 10), "    hi    ");
    assert_eq!(slice(init_shot, 9, 0, 10), "        hi");

    // Initial scrolled lines (`row0 one two`, `row1 three four` wrapped to width 12 -> `row0 one two`, `row1 three`, `four`, `row2 five` => 4 lines, last_top(12, 3) = 1)
    assert_eq!(slice(init_shot, 12, 0, 12), "row0 one two");
    assert_eq!(slice(init_shot, 13, 0, 12), "row1 three  ");
    assert_eq!(slice(init_shot, 14, 0, 12), "four        ");
    assert!(slice(init_shot, 12, 20, 26).starts_with("1/1"));

    // After Key:Down -> top becomes 1
    let down_shot = &term.before_clear[3];
    assert_eq!(slice(down_shot, 12, 0, 12), "row1 three  ");
    assert_eq!(slice(down_shot, 13, 0, 12), "four        ");
    assert_eq!(slice(down_shot, 14, 0, 12), "row2 five   ");

    // After PageDown + End -> stays clamped at last_top = 1
    assert_eq!(slice(&term.grid, 12, 0, 12), "row1 three  ");
    assert_eq!(slice(&term.grid, 14, 0, 12), "row2 five   ");
}

#[test]
fn gt6_popup_clear_and_centered_rect_work() {
    // 1. Formal contract checks for RECT.centered / centered_percent, CLEAR, and POPUP.
    let contracts = "functions CLIENT

feature {ANY}

  check_popup(s: SCREEN, r: RECT, p: POPUP, w: NAT, h: NAT, pw: NAT, ph: NAT, st: STYLE): BOOL
    do
      True
    ensure
      centered_in: r.centered(w, h).inside(r)
      centered_pct_in: r.centered_percent(pw, ph).inside(r)
      clear_area_dim: CLEAR.area(s, r).width = s.width and CLEAR.area(s, r).height = s.height
      clear_styled_dim: CLEAR.styled(s, r, st).width = s.width and CLEAR.styled(s, r, st).height = s.height
      place_in: POPUP.place(r, w, h).inside(r)
      bounds_in: p.bounds(r).inside(r)
      content_in: p.content_area(r).inside(r)
      draw_dim: p.draw(s, r, st, st, st).width = s.width and p.draw(s, r, st, st, st).height = s.height
    end

end
";
    let contract_proj = Project::new("gtui_gt6_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime RECT.centered, CLEAR (including over wide characters), and POPUP overlay.
    let prog = "reactive program GT6_POPUP

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(n) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  fmt_rect(r: RECT): STRING
    do
      let a = GLYPHS.join(NUMBER_TEXT.nat_to_text(r.x), \",\")
      let b = GLYPHS.join(a, NUMBER_TEXT.nat_to_text(r.y))
      let c = GLYPHS.join(b, \",\")
      let d = GLYPHS.join(c, NUMBER_TEXT.nat_to_text(r.width))
      let e = GLYPHS.join(d, \",\")
      GLYPHS.join(e, NUMBER_TEXT.nat_to_text(r.height))
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let st = STYLE.plain
      let bg = blank.fill(blank.area, 'X', st)
      let wide_bg = bg.text(blank.area, 19, 9, \"本本\", st)
      let c1 = RECT.make(10, 4, 40, 20).centered(20, 8)
      let c2 = RECT.make(5, 2, 10, 6).centered(30, 20)
      let c3 = RECT.make(0, 0, 80, 20).centered_percent(50, 50)
      let modal = POPUP.of_text(\"Modal\", \"Hello overlay!\", 20, 6)
        .with_footer(\"OK\")
        .with_padding(PADDING.symmetric(0, 1))
      let s0 = modal.draw(wide_bg, RECT.make(10, 4, 40, 14), st, st.bold.fg(COLOR.cyan), st.bold)
      let coords = GLYPHS.join(fmt_rect(c1), GLYPHS.join(\"|\", GLYPHS.join(fmt_rect(c2), GLYPHS.join(\"|\", fmt_rect(c3)))))
      s0.text(blank.area, 0, 0, coords, st)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt6_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt6_popup.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());
    let script = "Resized:60:20 Key:113";
    let ran = run(&runtime_proj.0.bin("gt6_popup"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    let shown = term.text();

    assert!(shown.contains("20,10,20,8|5,2,10,6|20,5,40,10"), "centered coordinates:\n{shown}");
    // Popup bounds for centered(20, 6) inside (10, 4, 40, 14) -> x = 20..40, y = 8..14.
    assert!(shown.contains("╭─ Modal ─"), "modal title:\n{shown}");
    assert!(shown.contains("│ Hello overlay!   │"), "cleared & padded modal body:\n{shown}");
    assert!(shown.contains("╰─ OK ─"), "modal footer:\n{shown}");
    // Surrounding background cells remain untouched `'X'`.
    assert_eq!(term.grid[9][17].0, 'X', "outside popup on left remains 'X':\n{shown}");
    assert_eq!(term.grid[9][19].0, ' ', "cut wide char half mended to space:\n{shown}");
    assert_eq!(term.grid[9][40].0, 'X', "outside popup on right remains 'X':\n{shown}");
    assert_eq!(term.grid[9][20].0, '│', "popup left border:\n{shown}");
    assert_eq!(term.grid[9][21].0, ' ', "popup cleared interior:\n{shown}");

    // 3. Help popup overlay in gtui_tour (`?` opens, `Escape` dismisses).
    let dir = Dir::new("tour_help_popup");
    assert_ok(&dir.check(&root().join("examples/gtui_tour")));
    let tour_script = "Resized:90:24 Key:63 Resized:90:24 Key:Escape Resized:90:24 Key:113";
    let ran_tour = run(&dir.bin("gtui_tour"), tour_script, &[]);
    assert_eq!(ran_tour.status.code(), Some(0), "{}", all(&ran_tour));
    let mut tour_term = Terminal::new(sizes(tour_script));
    tour_term.feed(&ran_tour.stdout);
    let help_shot = screen_text(&tour_term.before_clear[2]);
    let restored_shot = screen_text(&tour_term.before_clear[3]);
    assert!(help_shot.contains("╭─ Help ─"), "tour help popup header:\n{help_shot}");
    assert!(help_shot.contains("Tab: move between panes"), "tour help popup body:\n{help_shot}");
    assert!(help_shot.contains("╰─ Esc/? to close ─"), "tour help popup footer:\n{help_shot}");
    assert!(!restored_shot.contains("Esc/? to close"), "Escape dismisses help overlay:\n{restored_shot}");
    assert!(restored_shot.contains("module TABS"), "underlying source view restored:\n{restored_shot}");
}

#[test]
fn gt7_table_gauge_sparkline_and_scrollbar_work() {
    // 1. Formal contract checks for TABLE, GAUGE, SPARKLINE, and SCROLLBAR.
    let contracts = "functions CLIENT

feature {ANY}

  check_gt7(s: SCREEN, r: RECT, tbl: TABLE, g: GAUGE, sp: SPARKLINE, sb: SCROLLBAR,
            i: NAT, rows: NAT, n: NAT, track_len: NAT, k: KEY, st: STYLE): BOOL
    do
      True
    ensure
      tbl_sel: tbl.selected < tbl.row_count or (tbl.row_count = 0 and tbl.selected = 0)
      tbl_top: tbl.top <= tbl.selected
      tbl_some: tbl.selected_row.is_some = (tbl.row_count > 0)
      tbl_set: tbl.row_count > 0 implies tbl.select(i, rows).selected = (if i < tbl.row_count then i else tbl.row_count - 1 end)
      tbl_key: tbl.on_key(k, rows).row_count = tbl.row_count
      tbl_draw: tbl.draw(s, r, st, st, st).width = s.width and tbl.draw(s, r, st, st, st).height = s.height
      gauge_pct: g.percentage <= 100 and GAUGE.percent(n).percentage <= 100 and GAUGE.ratio(i, n).denominator > 0
      gauge_draw: g.draw(s, r, st, st).width = s.width and g.draw(s, r, st, st).height = s.height
      spark_draw: sp.draw(s, r, st).width = s.width and sp.draw(s, r, st).height = s.height
      sb_thumb: sb.thumb_size(track_len) <= track_len and (track_len > 0 implies sb.thumb_size(track_len) >= 1)
      sb_start: sb.thumb_start(track_len) + sb.thumb_size(track_len) <= track_len
      sb_draw: sb.draw(s, r, st, st).width = s.width and sb.draw(s, r, st, st).height = s.height
    end

end
";
    let contract_proj = Project::new("gtui_gt7_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime & VT snapshot checks for TABLE, GAUGE, SPARKLINE, and SCROLLBAR.
    let prog = "reactive program GT7_WIDGETS

feature {ANY}

  update(tbl: TABLE, e: EVENT): TUI_NEXT[TABLE]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(tbl) end
            case _ then TUI_NEXT.next(tbl.on_key(k, 2))
          end
        case _ then TUI_NEXT.next(tbl)
      end
    end

  make_row(id: STRING, name: STRING, load: STRING): LIST[STRING]
    do
      let none: LIST[STRING] = LIST.empty
      none.prepend(load).prepend(name).prepend(id)
    end

  view(tbl: TABLE, blank: SCREEN): SCREEN
    do
      let base = STYLE.plain
      let hdr_st = STYLE.plain.bold.fg(COLOR.cyan)
      let sel_st = STYLE.plain.reversed
      let s0 = tbl.draw(blank, RECT.make(0, 0, 24, 3), base, hdr_st, sel_st)

      let unfilled_st = STYLE.plain.fg(COLOR.gray)
      let filled_st = STYLE.plain.fg(COLOR.green)
      let s1 = GAUGE.percent(50).draw(s0, RECT.make(0, 4, 16, 1), unfilled_st, filled_st)
      let s2 = GAUGE.ratio(5, 8).with_label(\"5/8\").draw(s1, RECT.make(0, 6, 10, 1), unfilled_st, filled_st)

      let none_n: LIST[NAT] = LIST.empty
      let samples = none_n.prepend(8).prepend(7).prepend(6).prepend(5).prepend(4).prepend(3).prepend(2).prepend(1).prepend(0)
      let s3 = SPARKLINE.make(samples).draw(s2, RECT.make(0, 8, 9, 1), STYLE.plain.fg(COLOR.yellow))

      let s4 = SCROLLBAR.vertical(10, 5, 0).draw(s3, RECT.make(20, 4, 1, 6), base, base)
      let s5 = SCROLLBAR.vertical(10, 5, 5).draw(s4, RECT.make(22, 4, 1, 6), base, base)
      SCROLLBAR.horizontal(10, 5, 5).draw(s5, RECT.make(0, 10, 8, 1), base, base)
    end

  init(args: SEQ[STRING]): STEP[TUI[TABLE]]
    do
      let none_s: LIST[SIZE] = LIST.empty
      let widths = none_s.prepend(SIZE.fixed(4)).prepend(SIZE.flex(1)).prepend(SIZE.fixed(4))
      let hdr = make_row(\"ID\", \"Name\", \"Load\")
      let none_rows: LIST[LIST[STRING]] = LIST.empty
      let rows = none_rows
        .prepend(make_row(\"r3\", \"delta\", \"40%\"))
        .prepend(make_row(\"r2\", \"gamma\", \"30%\"))
        .prepend(make_row(\"r1\", \"beta\", \"20%\"))
        .prepend(make_row(\"r0\", \"alpha\", \"10%\"))
      TUI.start(TABLE.with_headers(widths, hdr, rows), update, view)
    end

  step(t: TUI[TABLE], e: EVENT): STEP[TUI[TABLE]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt7_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt7_widgets.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());
    let script = "Resized:30:12 Resized:30:12 Key:Down Resized:30:12 Key:End Resized:30:12 Key:113";
    let ran = run(&runtime_proj.0.bin("gt7_widgets"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|cell| cell.0).filter(|ch| *ch != '\0').collect()
    };

    // Initial snapshot: header in cyan+bold, r0 selected (reversed), r1 plain.
    let shot0 = &term.before_clear[2];
    assert_eq!(slice(shot0, 0, 0, 24), "ID   Name           Load");
    assert_eq!(slice(shot0, 1, 0, 24), "r0   alpha          10% ");
    assert_eq!(slice(shot0, 2, 0, 24), "r1   beta           20% ");
    assert_eq!(pen_at(shot0, "ID"), vec![1, 36]);
    assert_eq!(pen_at(shot0, "r0   alpha"), vec![7]);
    assert_eq!(pen_at(shot0, "beta"), Vec::<u32>::new());

    // After Key:Down -> r1 selected
    let shot1 = &term.before_clear[3];
    assert_eq!(pen_at(shot1, "r0   alpha"), Vec::<u32>::new());
    assert_eq!(pen_at(shot1, "r1   beta"), vec![7]);

    // After Key:End -> scrolled to show r2, r3 with r3 selected
    assert_eq!(slice(&term.grid, 1, 0, 24), "r2   gamma          30% ");
    assert_eq!(slice(&term.grid, 2, 0, 24), "r3   delta          40% ");
    assert_eq!(pen_at(&term.grid, "r3   delta"), vec![7]);

    // GAUGE 50%: centered "50%" with inverted pen on filled half ('5','0') and unfilled pen on '%'
    assert_eq!(slice(&term.grid, 4, 0, 16), "██████50%       ");
    assert_eq!(term.grid[4][0].1, vec![32], "filled bar cell is green");
    assert_eq!(term.grid[4][6].1, vec![7, 32], "label over filled side has reversed green");
    assert_eq!(term.grid[4][8].1, vec![90], "label over unfilled side is gray");

    // GAUGE ratio(5, 8) -> 6 full cells + 2/8th block '▎' with centered "5/8"
    assert_eq!(slice(&term.grid, 6, 0, 10), "███5/8▎   ");

    // SPARKLINE [0..8] -> " ▁▂▃▄▅▆▇█"
    assert_eq!(slice(&term.grid, 8, 0, 9), " ▁▂▃▄▅▆▇█");

    // SCROLLBAR vertical (top & bottom) and horizontal
    let vert_top: String = (4..10).map(|y| term.grid[y][20].0).collect();
    let vert_bot: String = (4..10).map(|y| term.grid[y][22].0).collect();
    assert_eq!(vert_top, "███│││");
    assert_eq!(vert_bot, "│││███");
    assert_eq!(slice(&term.grid, 10, 0, 8), "────████");
}

#[test]
fn gt8_unicode16_and_grapheme_clusters_work() {
    // 1. Formal contract checks for CHAR_WIDTH, CELL.as_wide, GLYPHS.width, and SCREEN.text.
    let contracts = "functions CLIENT

feature {ANY}

  check_gt8(s: SCREEN, r: RECT, c: CHAR, cell: CELL, st: STYLE, str_val: STRING, n: NAT): BOOL
    do
      True
    ensure
      cw_bound: CHAR_WIDTH.of(c) <= 2 and CHAR_WIDTH.of_code(n) <= 2
      width_bound: GLYPHS.width(str_val) <= 65535
      wide_cell: cell.as_wide.is_right_half = cell.is_right_half
      drawn_dim: s.text(r, 0, 0, str_val, st).width = s.width and s.text(r, 0, 0, str_val, st).height = s.height
    end

end
";
    let contract_proj = Project::new("gtui_gt8_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime & VT snapshot checks for Unicode 16.0 widths and UAX #29 grapheme clusters.
    let prog = "reactive program GT8_UNICODE

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(if n < 100 then n + 1 else n end) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let p = STYLE.plain
      let zwj_dev = \"\\u{1F469}\\u{200D}\\u{1F4BB}\"
      let zwj_family = \"\\u{1F468}\\u{200D}\\u{1F469}\\u{200D}\\u{1F467}\\u{200D}\\u{1F466}\"
      let toned_sci = \"\\u{1F468}\\u{1F3FD}\\u{200D}\\u{1F52C}\"
      let thumbs_tone = \"\\u{1F44D}\\u{1F3FD}\"
      let us_flag = \"\\u{1F1FA}\\u{1F1F8}\"
      let two_flags = \"\\u{1F1FA}\\u{1F1F8}\\u{1F1EF}\\u{1F1F5}\"
      let heart_emoji = \"\\u{2764}\\u{FE0F}\"
      let heart_text = \"\\u{2764}\\u{FE0E}\"
      let rainbow_flag = \"\\u{1F3F3}\\u{FE0F}\\u{200D}\\u{1F308}\"
      let u16 = \"\\u{1FA89}\\u{1FAC6}\\u{1FAE9}\"

      let widths_ok =
        CHAR_WIDTH.of_code(129673) = 2
          and CHAR_WIDTH.of_code(129734) = 2
          and CHAR_WIDTH.of_code(129769) = 2
          and CHAR_WIDTH.of_code(2199) = 0
          and GLYPHS.width(zwj_dev) = 2
          and GLYPHS.width(zwj_family) = 2
          and GLYPHS.width(toned_sci) = 2
          and GLYPHS.width(thumbs_tone) = 2
          and GLYPHS.width(us_flag) = 2
          and GLYPHS.width(two_flags) = 4
          and GLYPHS.width(heart_emoji) = 2
          and GLYPHS.width(heart_text) = 1
          and GLYPHS.width(rainbow_flag) = 2
          and GLYPHS.width(u16) = 6
      let s0 = blank.text(blank.area, 0, 0, if widths_ok then \"OK\" else \"BAD\" end, p)

      -- Row 1: compound clusters toggled by key press for diff equivalence testing.
      let row1 = if n % 2 = 1 then \"xy\" else zwj_dev end
      let s1 = s0.text(blank.area, 0, 1, GLYPHS.join(row1, toned_sci), p)

      -- Row 2: flags + VS16/VS15 hearts + skin-tone thumb + Rainbow flag.
      let row2_prefix = GLYPHS.join(GLYPHS.join(us_flag, heart_emoji), heart_text)
      let s2 = s1.text(blank.area, 0, 2, GLYPHS.join(GLYPHS.join(row2_prefix, thumbs_tone), rainbow_flag), p)

      -- Row 3: Unicode 16.0 new emoji (Harp, Fingerprint, Bags-Under-Eyes) + family ZWJ.
      let s3 = s2.text(blank.area, 0, 3, GLYPHS.join(u16, zwj_family), p)

      -- Row 4: clip a 2-col cluster to 1 col -> becomes space; overwrite right half of 🇺🇸 -> clears whole flag.
      let s4 = s3.text(RECT.make(0, 4, 1, 1), 0, 4, heart_emoji, p)
      s4.text(blank.area, 2, 4, us_flag, p).text(blank.area, 3, 4, \"Z\", p)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
";
    let runtime_proj = Project::new("gtui_gt8_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gt8_unicode.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());

    let script = "Resized:16:6 Key:120 Resized:16:6 Key:120 Resized:16:6 Key:113";
    let ran = run(&runtime_proj.0.bin("gt8_unicode"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));

    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    // Row 0: all GLYPHS.width checks matched.
    assert_eq!(
        screen_text(&term.grid).lines().next().unwrap().trim_end(),
        "OK",
        "{}",
        screen_text(&term.grid)
    );

    // Diffed frame (n = 1 -> "xy👨🏽‍🔬") matches exact expected cells on row 1.
    let shot1_row1: String = term.before_clear[2][1]
        .iter()
        .map(|c| if c.0 == '\0' { String::new() } else { format!("{}{}", c.0, c.2) })
        .collect();
    assert_eq!(shot1_row1.trim_end(), "xy\u{1F468}\u{1F3FD}\u{200D}\u{1F52C}");

    // Diff back (n = 2 -> "👩‍💻👨🏽‍🔬") matches final full redraw.
    assert_eq!(
        screen_text(&term.before_clear[3]),
        screen_text(&term.grid),
        "diff back matches full redraw"
    );

    // Full screen text assertions across rows 1..4.
    let rows: Vec<String> = screen_text(&term.grid).lines().map(|l| l.trim_end().to_string()).collect();
    assert_eq!(rows[1], "\u{1F469}\u{200D}\u{1F4BB}\u{1F468}\u{1F3FD}\u{200D}\u{1F52C}");
    assert_eq!(
        rows[2],
        "\u{1F1FA}\u{1F1F8}\u{2764}\u{FE0F}\u{2764}\u{FE0E}\u{1F44D}\u{1F3FD}\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"
    );
    assert_eq!(
        rows[3],
        "\u{1FA89}\u{1FAC6}\u{1FAE9}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"
    );
    assert_eq!(rows[4], "   Z");

    // Verify column widths directly on the terminal grid cells:
    // Row 2: flag (cols 0..2), red heart emoji (cols 2..4), text heart (col 4), thumbs up (cols 5..7), rainbow flag (cols 7..9)
    assert_eq!(term.grid[2][0].0, '\u{1F1FA}');
    assert_eq!(term.grid[2][1].0, '\0');
    assert_eq!(term.grid[2][2].0, '\u{2764}');
    assert_eq!(term.grid[2][3].0, '\0');
    assert_eq!(term.grid[2][4].0, '\u{2764}');
    assert_eq!(term.grid[2][5].0, '\u{1F44D}');
    assert_eq!(term.grid[2][6].0, '\0');
    assert_eq!(term.grid[2][7].0, '\u{1F3F3}');
    assert_eq!(term.grid[2][8].0, '\0');
}

#[test]
fn gtui_dashboard_example_verifies_and_renders_all_widgets() {
    let dir = Dir::new("dashboard_example");
    assert_ok(&dir.check(&root().join("examples/gtui_dashboard")));
    let script = "Resized:80:24 Resized:80:24 Key:Down Key:32 Key:116 Key:98 Key:119 Key:97 Resized:80:24 Key:63 Resized:80:24 Key:Escape Resized:80:24 Key:113";
    let ran = run(&dir.bin("gtui_dashboard"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));

    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);
    assert!(term.sync_frames > 0, "dashboard emits synchronized frames");

    // 1. Initial snapshot (dark theme, rounded borders with merged junction `┬` and `┴`, row 0 "LAYOUT" selected).
    let initial = screen_text(&term.before_clear[2]);
    assert!(initial.contains("Modules"), "{initial}");
    assert!(initial.contains("Telemetry"), "{initial}");
    assert!(initial.contains("Inspector"), "{initial}");
    assert!(initial.contains("1/8 modules"), "{initial}");
    assert!(initial.contains("Theme: dark"), "{initial}");
    assert!(initial.contains("┬") && initial.contains("┴"), "shared vertical border is merged:\n{initial}");

    // 2. After Down, Space, 't' (nord), 'b' (plain), 'w' (char), 'a' (center):
    let updated = screen_text(&term.before_clear[3]);
    assert!(updated.contains("2/8 modules"), "{updated}");
    assert!(updated.contains("Theme: nord"), "{updated}");
    assert!(updated.contains("wrap:char align:center"), "{updated}");
    assert!(updated.contains("FRAME (GT.4)"), "{updated}");

    // 3. After '?' (help popup open):
    let help_shown = screen_text(&term.before_clear[4]);
    assert!(help_shown.contains("Dashboard Controls"), "{help_shown}");
    assert!(help_shown.contains("Esc/? to close"), "{help_shown}");

    // 4. After Escape (help popup dismissed, underlying dashboard restored):
    let final_screen = term.text();
    assert!(!final_screen.contains("Dashboard Controls"), "{final_screen}");
    assert!(final_screen.contains("2/8 modules"), "{final_screen}");
}

#[test]
fn mouse_hit_test_and_widgets_verify_and_handle_events() {
    // 1. Formal client contract proofs for RECT.has_inside, RECT.has_disjoint, and widget on_mouse/tab_at/scroll_for_mouse.
    let contracts = r#"functions CLIENT

feature {ANY}

  check_mouse(r: RECT, sizes: LIST[SIZE], m: MOUSE, i: NAT, j: NAT,
              tabs: TABS, lv: LIST_VIEW, tbl: TABLE, tv: TEXT_VIEW,
              sb: SCROLLBAR, pop: POPUP, ta: TEXT_AREA, total: NAT, vp: NAT): BOOL
    do
      let cols = LAYOUT.columns(r, sizes)
      let rows = LAYOUT.rows(r, sizes)
      if i < j and j < sizes.length then
        let ci = LIST_OPS.at(cols, i)
        let cj = LIST_OPS.at(cols, j)
        let ri = LIST_OPS.at(rows, i)
        let rj = LIST_OPS.at(rows, j)
        check
          ci_in: LAYOUT.across_inside(cols, r.x, r, i)
          ri_in: LAYOUT.down_inside(rows, r.y, r, i)
          c_apart: LAYOUT_APART.columns_apart(cols, r.x, r, i, j)
          r_apart: LAYOUT_APART.rows_apart(rows, r.y, r, i, j)
          c_has_in: RECT.has_inside(ci, r, m.x, m.y)
          r_has_in: RECT.has_inside(ri, r, m.x, m.y)
          c_has_disj: RECT.has_disjoint(ci, cj, m.x, m.y)
          r_has_disj: RECT.has_disjoint(ri, rj, m.x, m.y)
        end
        True
      else if i < sizes.length then
        let ci = LIST_OPS.at(cols, i)
        let ri = LIST_OPS.at(rows, i)
        check
          ci_in: LAYOUT.across_inside(cols, r.x, r, i)
          ri_in: LAYOUT.down_inside(rows, r.y, r, i)
          c_has_in: RECT.has_inside(ci, r, m.x, m.y)
          r_has_in: RECT.has_inside(ri, r, m.x, m.y)
        end
        True
      else
        True
      end end
    ensure
      col_click_in_outer: (i < sizes.length and LIST_OPS.at(LAYOUT.columns(r, sizes), i).has(m.x, m.y)) implies r.has(m.x, m.y)
      row_click_in_outer: (i < sizes.length and LIST_OPS.at(LAYOUT.rows(r, sizes), i).has(m.x, m.y)) implies r.has(m.x, m.y)
      col_click_exclusive: (i < j and j < sizes.length) implies not (LIST_OPS.at(LAYOUT.columns(r, sizes), i).has(m.x, m.y) and LIST_OPS.at(LAYOUT.columns(r, sizes), j).has(m.x, m.y))
      row_click_exclusive: (i < j and j < sizes.length) implies not (LIST_OPS.at(LAYOUT.rows(r, sizes), i).has(m.x, m.y) and LIST_OPS.at(LAYOUT.rows(r, sizes), j).has(m.x, m.y))
      tab_count: tabs.on_mouse(m, r).count = tabs.count
      tab_bound: match tabs.tab_at(r, m.x, m.y) case Some(k) then k >= 0 and k < tabs.count case None then True end
      list_count: lv.on_mouse(m, r).count = lv.count
      table_count: tbl.on_mouse(m, r).row_count = tbl.row_count
      sb_bound: match sb.scroll_for_mouse(m, r, total, vp) case Some(pos) then pos <= total case None then True end
      tv_count: tv.on_mouse(m, r).line_count = tv.line_count
      ta_lines: ta.on_mouse(m, r).state.lines = ta.state.lines
      pop_contains: pop.contains(r, m.x, m.y) = pop.bounds(r).has(m.x, m.y)
    end

end
"#;
    let contract_proj = Project::new("gtui_mouse_contracts", contracts);
    assert_ok(&contract_proj.check());

    let bad_contracts = r#"functions CLIENT

feature {ANY}

  bad(r: RECT, sizes: LIST[SIZE], m: MOUSE, i: NAT, j: NAT): BOOL
    do
      True
    ensure
      overlap: (i < j and j < sizes.length and LIST_OPS.at(LAYOUT.columns(r, sizes), i).has(m.x, m.y)) implies LIST_OPS.at(LAYOUT.columns(r, sizes), j).has(m.x, m.y)
    end

end
"#;
    let bad_proj = Project::new("gtui_mouse_bad_overlap", bad_contracts);
    let bad_out = bad_proj.check();
    assert!(!bad_out.status.success(), "overlapping click claim must be refused: {}", all(&bad_out));
    assert!(all(&bad_out).contains("`overlap`"), "{}", all(&bad_out));

    // 2. Runtime unit checks for all widgets under `on_mouse` / `tab_at` / `scroll_for_mouse`.
    let unit_prog = r#"reactive program MOUSE_WIDGETS

feature {ANY}

  update(done: BOOL, e: EVENT): TUI_NEXT[BOOL]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(done) end
            case KeysEnded then TUI_NEXT.quit(0)
            case _ then TUI_NEXT.next(done)
          end
        case _ then TUI_NEXT.next(done)
      end
    end

  status_text: STRING
    do
      let tabs = TABS.empty.with("Alpha", 'a').with("Beta", 'b').with("Gamma", ' ')
      let tab_r = RECT.make(0, 0, 30, 2)
      let tabs_ok =
        (match tabs.tab_at(tab_r, 0, 0) case Some(i) then i = 0 case None then False end)
        and (match tabs.tab_at(tab_r, 7, 0) case Some(i) then i = 0 case None then False end)
        and tabs.tab_at(tab_r, 8, 0).is_none
        and (match tabs.tab_at(tab_r, 11, 0) case Some(i) then i = 1 case None then False end)
        and (match tabs.tab_at(tab_r, 22, 0) case Some(i) then i = 2 case None then False end)
        and tabs.tab_at(tab_r, 0, 1).is_none
        and tabs.on_mouse(MOUSE.MouseDown(0, 11, 0), tab_r).selected = 1
        and tabs.on_mouse(MOUSE.MouseDown(2, 22, 0), tab_r).selected = 0

      let none_s: LIST[STRING] = LIST.empty
      let lv = LIST_VIEW.make(none_s.prepend("f").prepend("e").prepend("d").prepend("c").prepend("b").prepend("a"))
      let lv_r = RECT.make(0, 0, 10, 4)
      let list_ok =
        lv.on_mouse(MOUSE.MouseDown(0, 2, 2), lv_r).selected = 2
        and lv.on_mouse(MOUSE.WheelDown(2, 2), lv_r).selected = 3
        and lv.on_mouse(MOUSE.WheelDown(2, 2), lv_r).on_mouse(MOUSE.WheelUp(2, 2), lv_r).selected = 0
        and lv.on_mouse(MOUSE.MouseDown(0, 2, 4), lv_r).selected = 0

      let widths: LIST[SIZE] = LIST.empty.prepend(SIZE.flex(1)).prepend(SIZE.fixed(3))
      let hdr = none_s.prepend("V").prepend("K")
      let mk = fn(a: STRING, b: STRING): LIST[STRING] do none_s.prepend(b).prepend(a) end
      let t_rows: LIST[LIST[STRING]] = LIST.empty
        .prepend(mk("k4", "4")).prepend(mk("k3", "3")).prepend(mk("k2", "2")).prepend(mk("k1", "1")).prepend(mk("k0", "0"))
      let tbl = TABLE.with_headers(widths, hdr, t_rows)
      let tbl_r = RECT.make(0, 0, 10, 4)
      let table_ok =
        tbl.on_mouse(MOUSE.MouseDown(0, 1, 0), tbl_r).selected = 0
        and tbl.on_mouse(MOUSE.MouseDown(0, 1, 3), tbl_r).selected = 2
        and tbl.on_mouse(MOUSE.WheelDown(1, 2), tbl_r).selected = 3
        and tbl.on_mouse(MOUSE.WheelDown(1, 2), tbl_r).on_mouse(MOUSE.WheelUp(1, 2), tbl_r).selected = 0

      let none_tn: LIST[TREE_NODE] = LIST.empty
      let tv_nodes = none_tn
        .prepend(TREE_NODE.make("tail", 0))
        .prepend(TREE_NODE.make("b", 1))
        .prepend(TREE_NODE.make("a", 1))
        .prepend(TREE_NODE.make("dir", 0))
      let tree = TREE_VIEW.make(tv_nodes)
      let tree_r = RECT.make(0, 0, 12, 4)
      let tree_row1 = tree.on_mouse(MOUSE.MouseDown(0, 5, 1), tree_r)
      let tree_open = tree.on_mouse(MOUSE.MouseDown(0, 0, 0), tree_r)
      let tree_label_sel = tree_open.on_mouse(MOUSE.MouseDown(0, 5, 1), tree_r).on_mouse(MOUSE.MouseDown(0, 5, 0), tree_r)
      let tree_toggle_sel = tree_label_sel.on_mouse(MOUSE.MouseDown(0, 5, 0), tree_r)
      let tree_ok =
        tree_row1.selected_row = 1 and tree_row1.count = 2
        and tree_open.selected_row = 0 and tree_open.count = 4
        and tree_label_sel.selected_row = 0 and tree_label_sel.count = 4
        and tree_toggle_sel.selected_row = 0 and tree_toggle_sel.count = 2
        and tree_open.on_mouse(MOUSE.WheelDown(2, 1), tree_r).selected_row = 3

      let sbv = SCROLLBAR.vertical(21, 6, 0)
      let sbv_r = RECT.make(0, 0, 10, 6)
      let sbh = SCROLLBAR.horizontal(21, 6, 0)
      let sbh_r = RECT.make(0, 0, 6, 4)
      let sb_ok =
        (match sbv.scroll_for_mouse(MOUSE.MouseDown(0, 9, 0), sbv_r, 21, 6) case Some(p) then p = 0 case None then False end)
        and (match sbv.scroll_for_mouse(MOUSE.MouseDown(0, 9, 2), sbv_r, 21, 6) case Some(p) then p = 6 case None then False end)
        and (match sbv.scroll_for_mouse(MOUSE.MouseDrag(0, 9, 5), sbv_r, 21, 6) case Some(p) then p = 15 case None then False end)
        and sbv.scroll_for_mouse(MOUSE.MouseDown(0, 8, 2), sbv_r, 21, 6).is_none
        and (match sbh.scroll_for_mouse(MOUSE.MouseDown(0, 5, 3), sbh_r, 21, 6) case Some(p) then p = 15 case None then False end)
        and (match sbh.scroll_for_mouse(MOUSE.MouseDrag(0, 0, 3), sbh_r, 21, 6) case Some(p) then p = 0 case None then False end)

      let tv = TEXT_VIEW.make(none_s.prepend("l5").prepend("l4").prepend("l3").prepend("l2").prepend("l1").prepend("l0"))
      let vr = RECT.make(0, 0, 10, 3)
      let tv_ok =
        tv.on_mouse(MOUSE.WheelDown(1, 1), vr).top = 3
        and tv.on_mouse(MOUSE.WheelDown(1, 1), vr).on_mouse(MOUSE.WheelUp(1, 1), vr).top = 0
        and tv.on_mouse(MOUSE.WheelDown(10, 1), vr).top = 0

      let pa = PARAGRAPH.of_text("l0\nl1\nl2\nl3\nl4\nl5")
      let para_ok =
        pa.on_mouse(MOUSE.WheelDown(1, 1), vr).top = 3
        and pa.on_mouse(MOUSE.WheelDown(1, 1), vr).on_mouse(MOUSE.WheelUp(1, 1), vr).top = 0
        and pa.on_mouse(MOUSE.WheelDown(10, 1), vr).top = 0

      let modal = POPUP.of_text("T", "a\nb\nc\nd\ne\nf", 10, 5)
      let area = RECT.make(0, 0, 20, 10)
      let pop_ok =
        modal.contains(area, 5, 2)
        and not modal.contains(area, 4, 2)
        and modal.on_mouse(MOUSE.WheelDown(5, 2), area).body.top = 3
        and modal.on_mouse(MOUSE.WheelDown(4, 2), area).body.top = 0

      let ta = TEXT_AREA.of_text("ab\u{754C}cd\ne\u{0301}xy\nline2\nline3\nline4\nline5\nline6\nline7")
      let tar = RECT.make(0, 0, 20, 4)
      let ta_c = ta.on_mouse(MOUSE.MouseDown(0, 6, 0), tar)
      let ta_w = ta.on_mouse(MOUSE.MouseDown(0, 5, 0), tar)
      let ta_m = ta.on_mouse(MOUSE.MouseDown(0, 3, 1), tar)
      let ta_click_ok =
        ta_c.line_number = 0 and ta_c.column = 3
        and ta_w.line_number = 0 and ta_w.column = 2
        and ta_m.line_number = 1 and ta_m.column = 2
      let ta_drag = ta.on_mouse(MOUSE.MouseDown(0, 3, 0), tar).on_mouse(MOUSE.MouseDrag(0, 7, 0), tar)
      let ta_drag_ok = ta_drag.has_selection and ta_drag.selected_text.equals("b\u{754C}c")
      let ta_shift = ta.on_mouse(MOUSE.MouseDown(0, 2, 0), tar).on_mouse(MOUSE.MouseMod(MOUSE.MouseDown(0, 3, 1), True, False, False), tar)
      let ta_shift_ok =
        ta_shift.has_selection
        and ta_shift.selection_start_line = 0 and ta_shift.selection_start_column = 0
        and ta_shift.selection_end_line = 1 and ta_shift.selection_end_column = 2
      let ta_wd = ta.on_mouse(MOUSE.WheelDown(5, 1), tar)
      let ta_wu = ta_wd.on_mouse(MOUSE.WheelUp(5, 1), tar)
      let ta_wheel_ok =
        ta_wd.top = 3 and ta_wd.followed(tar).top = 3 and ta_wu.top = 0

      if not tabs_ok then "FAIL_TABS"
      else if not list_ok then "FAIL_LIST"
      else if not table_ok then "FAIL_TABLE"
      else if not tree_ok then "FAIL_TREE"
      else if not sb_ok then "FAIL_SB"
      else if not tv_ok then "FAIL_TV"
      else if not para_ok then "FAIL_PARA"
      else if not pop_ok then "FAIL_POP"
      else if not ta_click_ok then "FAIL_TA_CLICK"
      else if not ta_drag_ok then "FAIL_TA_DRAG"
      else if not ta_shift_ok then "FAIL_TA_SHIFT"
      else if not ta_wheel_ok then "FAIL_TA_WHEEL"
      else "ALL_OK"
      end end end end end end end end end end end end
    end

  view(done: BOOL, blank: SCREEN): SCREEN
    do
      blank.text(blank.area, 0, 0, status_text, STYLE.plain)
    end

  init(args: SEQ[STRING]): STEP[TUI[BOOL]]
    do
      TUI.start(True, update, view)
    end

  step(t: TUI[BOOL], e: EVENT): STEP[TUI[BOOL]]
    do
      t.step(e)
    end

end
"#;
    let unit_proj = Project::new("gtui_mouse_widgets", unit_prog);
    let unit_src = unit_proj.0 .0.join("p/src");
    fs::rename(unit_src.join("client.gdl"), unit_src.join("mouse_widgets.gdl")).expect("named after its module");
    assert_ok(&unit_proj.check());
    let unit_script = "Resized:40:10 Key:113";
    let unit_ran = run(&unit_proj.0.bin("mouse_widgets"), unit_script, &[]);
    assert_eq!(unit_ran.status.code(), Some(0), "{}", all(&unit_ran));
    let mut unit_term = Terminal::new(sizes(unit_script));
    unit_term.feed(&unit_ran.stdout);
    let unit_text = unit_term.text();
    assert!(unit_text.contains("ALL_OK"), "widget mouse unit checks failed:\n{unit_text}");

    // 3. Scripted VT session for `examples/gtui_dashboard` with Mouse events.
    let dash_dir = Dir::new("dashboard_mouse");
    assert_ok(&dash_dir.check(&root().join("examples/gtui_dashboard")));
    let dash_script = concat!(
        "Resized:80:16 ",
        "Mouse:Down:0:16:0 Resized:80:16 ",
        "Mouse:Down:0:5:5 Resized:80:16 ",
        "Mouse:Down:0:42:7 Resized:80:16 ",
        "Mouse:WheelDown:10:11 Resized:80:16 ",
        "Key:63 Mouse:WheelDown:30:7 Resized:80:16 ",
        "Mouse:Down:0:1:1 Resized:80:16 Key:113"
    );
    let dash_ran = run(&dash_dir.bin("gtui_dashboard"), dash_script, &[]);
    assert_eq!(dash_ran.status.code(), Some(0), "{}", all(&dash_ran));
    let mut dash_term = Terminal::new(sizes(dash_script));
    dash_term.feed(&dash_ran.stdout);
    assert_eq!(dash_term.mouse_enters, 1, "dashboard enables mouse modes once on enter");
    assert_eq!(dash_term.mouse_leaves, 1, "dashboard disables mouse modes cleanly on exit");
    assert!(!dash_term.mouse_1000 && !dash_term.mouse_1002 && !dash_term.mouse_1006);
    assert_eq!(
        dash_term.before_clear.last().expect("last clear"),
        &dash_term.grid,
        "dashboard diffs match full redraw"
    );

    let d1 = screen_text(&dash_term.before_clear[2]);
    assert_eq!(pen_at(&dash_term.before_clear[2], "Tasks"), vec![1, 4, 93], "Tasks tab highlighted:\n{d1}");
    assert!(d1.contains("╭─ Tasks ─"), "left pane shows active tab title:\n{d1}");

    let d2 = screen_text(&dash_term.before_clear[3]);
    assert!(d2.contains("3/8 modules") && d2.contains("PARAGRAPH (GT.5)"), "table click selects row 2:\n{d2}");

    let d3 = screen_text(&dash_term.before_clear[4]);
    assert!(d3.contains("8/8 modules") && d3.contains("CHAR_WIDTH (GT.8)"), "scrollbar click jumps to end:\n{d3}");

    let d4 = screen_text(&dash_term.before_clear[5]);
    assert!(d4.contains("Mouse tip: wheel here scrolls detail notes."), "wheel scrolls inspector paragraph:\n{d4}");

    let d5 = screen_text(&dash_term.before_clear[6]);
    assert!(
        d5.contains("Dashboard Controls") && d5.contains("Click outside modal to dismiss"),
        "wheel inside help popup scrolls inner body:\n{d5}"
    );

    let d6 = screen_text(&dash_term.before_clear[7]);
    assert!(!d6.contains("Dashboard Controls") && d6.contains("8/8 modules"), "click outside dismisses modal:\n{d6}");

    // 4. Scripted VT session for `examples/gtui_tour` with Mouse events.
    let tour_dir = Dir::new("tour_mouse");
    assert_ok(&tour_dir.check(&root().join("examples/gtui_tour")));
    let tour_script = concat!(
        "Resized:90:24 ",
        "Mouse:Down:0:3:2 Resized:90:24 ",
        "Mouse:Down:0:17:0 Resized:90:24 ",
        "Mouse:Down:0:45:5 Mouse:WheelDown:45:5 Resized:90:24 ",
        "Mouse:Down:0:18:16 Mouse:Down:0:10:18 Resized:90:24 ",
        "Key:63 Resized:90:24 ",
        "Mouse:Down:0:1:1 Resized:90:24 Key:113"
    );
    let tour_ran = run(&tour_dir.bin("gtui_tour"), tour_script, &[]);
    assert_eq!(tour_ran.status.code(), Some(0), "{}", all(&tour_ran));
    let mut tour_term = Terminal::new(sizes(tour_script));
    tour_term.feed(&tour_ran.stdout);
    assert_eq!(tour_term.mouse_enters, 1, "tour enables mouse modes once on enter");
    assert_eq!(tour_term.mouse_leaves, 1, "tour disables mouse modes cleanly on exit");
    assert!(!tour_term.mouse_1000 && !tour_term.mouse_1002 && !tour_term.mouse_1006);
    assert_eq!(
        tour_term.before_clear.last().expect("last clear"),
        &tour_term.grid,
        "tour diffs match full redraw"
    );

    let t1 = screen_text(&tour_term.before_clear[2]);
    assert!(t1.contains("│  ▾ screen") && t1.contains("│      CELL"), "tree disclosure toggles open on click:\n{t1}");

    let t2 = screen_text(&tour_term.before_clear[3]);
    assert!(t2.contains("│▾ src"), "Files tab click switches left tree:\n{t2}");
    assert_eq!(pen_at(&tour_term.before_clear[3], "Files"), vec![1, 4, 93]);

    let t3 = screen_text(&tour_term.before_clear[4]);
    assert_eq!(pen_at(&tour_term.before_clear[4], "╭─ screen Text"), vec![1, 36], "click focuses source pane:\n{t3}");
    assert!(!t3.contains("│ 1 -- A row") && t3.contains("│ 4 representation"), "wheel scrolls source down:\n{t3}");

    let t4 = screen_text(&tour_term.before_clear[5]);
    assert!(t4.contains("tabs.gdl:29") && t4.contains("29       else 1 + TABS.key_index"), "clicking problem row marks line 29:\n{t4}");

    let t5 = screen_text(&tour_term.before_clear[6]);
    assert!(t5.contains("╭─ Help ─"), "help popup opens:\n{t5}");

    let t6 = screen_text(&tour_term.before_clear[7]);
    assert!(!t6.contains("Esc/? to close"), "click outside dismisses tour help popup:\n{t6}");
}

#[test]
fn gw2_overlay_placement_and_divider_work() {
    // 1. Formal client contract proofs for OVERLAY_SIDE, OVERLAY_PLACEMENT, and DIVIDER.
    let contracts = r#"functions CLIENT

feature {ANY}

  check_gw2(anchor: RECT, bounds: RECT, w: NAT, h: NAT, side: OVERLAY_SIDE,
            s: SCREEN, r: RECT, div: DIVIDER, st: STYLE): BOOL
    require
      anchor_in: anchor.inside(bounds)
    do
      let placed = OVERLAY_PLACEMENT.place(anchor, w, h, side, bounds)
      let rule = div.rule_area(r)
      let drawn = div.draw(s, r, st)
      check
        in_b: placed.inside(bounds)
        apart: placed.disjoint(anchor)
        opp_inv: side.opposite.opposite.same(side)
        rule_in: rule.inside(r)
        dim_w: drawn.width = s.width
        dim_h: drawn.height = s.height
      end
      True
    ensure
      placed_inside: OVERLAY_PLACEMENT.place(anchor, w, h, side, bounds).inside(bounds)
      placed_disjoint: OVERLAY_PLACEMENT.place(anchor, w, h, side, bounds).disjoint(anchor)
      rule_inside: div.rule_area(r).inside(r)
      drawn_dim: div.draw(s, r, st).width = s.width and div.draw(s, r, st).height = s.height
    end

end
"#;
    let contract_proj = Project::new("gtui_gw2_contracts", contracts);
    assert_ok(&contract_proj.check());

    let bad_contracts = r#"functions CLIENT

feature {ANY}

  bad_overlap(anchor: RECT, bounds: RECT, w: NAT, h: NAT, side: OVERLAY_SIDE): BOOL
    require
      anchor_in: anchor.inside(bounds)
      non_empty_a: not anchor.is_empty
    do
      True
    ensure
      must_overlap: not OVERLAY_PLACEMENT.place(anchor, w, h, side, bounds).disjoint(anchor)
    end

end
"#;
    let bad_proj = Project::new("gtui_gw2_bad_overlap", bad_contracts);
    let bad_out = bad_proj.check();
    assert!(!bad_out.status.success(), "overlapping placement claim must be refused: {}", all(&bad_out));
    assert!(all(&bad_out).contains("`must_overlap`"), "{}", all(&bad_out));

    // 2. Runtime & VT snapshot checks for OVERLAY_PLACEMENT and DIVIDER.
    let prog = r#"reactive program GW2_WIDGETS

feature {ANY}

  update(n: NAT, e: EVENT): TUI_NEXT[NAT]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(n) end
            case _ then TUI_NEXT.next(n)
          end
        case _ then TUI_NEXT.next(n)
      end
    end

  rect_eq(a: RECT, x: NAT, y: NAT, w: NAT, h: NAT): BOOL
    do
      a.x = x and a.y = y and a.width = w and a.height = h
    end

  view(n: NAT, blank: SCREEN): SCREEN
    do
      let bounds = RECT.make(0, 0, 20, 10)
      -- Below fits whole
      let p1 = OVERLAY_PLACEMENT.place(RECT.make(2, 1, 4, 1), 6, 3, OVERLAY_SIDE.below, bounds)
      -- Below does not fit -> flips above
      let p2 = OVERLAY_PLACEMENT.place(RECT.make(2, 8, 4, 1), 6, 3, OVERLAY_SIDE.below, bounds)
      -- Shifts left along bottom edge
      let p3 = OVERLAY_PLACEMENT.place(RECT.make(17, 1, 3, 1), 6, 3, OVERLAY_SIDE.below, bounds)
      -- RightOf does not fit -> flips LeftOf
      let p4 = OVERLAY_PLACEMENT.place(RECT.make(16, 2, 3, 1), 5, 2, OVERLAY_SIDE.right_of, bounds)
      -- Fits neither side -> cuts to larger room (below has 5 rows vs 3 above)
      let p5 = OVERLAY_PLACEMENT.place(RECT.make(2, 3, 4, 2), 6, 8, OVERLAY_SIDE.below, bounds)

      let ok =
        rect_eq(p1, 2, 2, 6, 3)
          and rect_eq(p2, 2, 5, 6, 3)
          and rect_eq(p3, 14, 2, 6, 3)
          and rect_eq(p4, 11, 2, 5, 2)
          and rect_eq(p5, 2, 5, 6, 5)

      let base = STYLE.plain
      let lbl_st = STYLE.plain.bold
      let s0 = blank.text(blank.area, 0, 0, if ok then "OK" else "BAD" end, base)
      let s1 = DIVIDER.horizontal.with_label("Settings").with_align(ALIGN.center).draw_labeled(s0, RECT.make(0, 1, 16, 1), base, lbl_st)
      let s2 = DIVIDER.horizontal.with_border(BORDER_KIND.double).with_label("Top").draw_labeled(s1, RECT.make(0, 2, 14, 1), base, lbl_st)
      let s3 = DIVIDER.horizontal.with_label("TooLongLabel").draw(s2, RECT.make(0, 3, 6, 1), base)
      DIVIDER.vertical.with_border(BORDER_KIND.thick).draw(s3, RECT.make(18, 1, 1, 4), base)
    end

  init(args: SEQ[STRING]): STEP[TUI[NAT]]
    do
      TUI.start(0, update, view)
    end

  step(t: TUI[NAT], e: EVENT): STEP[TUI[NAT]]
    do
      t.step(e)
    end

end
"#;
    let runtime_proj = Project::new("gtui_gw2_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gw2_widgets.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());

    let script = "Resized:24:8 Key:113";
    let ran = run(&runtime_proj.0.bin("gw2_widgets"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |row: usize, x0: usize, x1: usize| -> String {
        term.grid[row][x0..x1].iter().map(|c| c.0).filter(|ch| *ch != '\0').collect()
    };
    assert_eq!(slice(0, 0, 2), "OK");
    assert_eq!(slice(1, 0, 16), "─── Settings ───");
    assert_eq!(pen_at(&term.grid, "Settings"), vec![1]);
    assert_eq!(slice(2, 0, 14), "═ Top ════════");
    assert_eq!(slice(3, 0, 6), "─ To ─");
    let vert: String = (1..5).map(|r| term.grid[r][18].0).collect();
    assert_eq!(vert, "┃┃┃┃");
}

#[test]
fn gw1_breadcrumb_collapse_keys_and_mouse_work() {
    // 1. Formal client contract proofs for BREADCRUMB.
    let contracts = r#"functions CLIENT

feature {ANY}

  check_gw1(bc: BREADCRUMB, r: RECT, k: KEY, m: MOUSE, cx: NAT, cy: NAT,
            s: SCREEN, st: STYLE): BOOL
    do
      let k_next = bc.on_key(k)
      let m_next = bc.on_mouse(m, r)
      let hit = bc.crumb_at(r, cx, cy)
      let drawn = bc.draw(s, r, st, st, st, st)
      check
        k_cnt: k_next.count = bc.count
        m_cnt: m_next.count = bc.count
        sel_ok: bc.selected < bc.count or (bc.count = 0 and bc.selected = 0)
        dim_w: drawn.width = s.width
        dim_h: drawn.height = s.height
      end
      match hit
        case Some(i) then i < bc.count
        case None then True
      end
    ensure
      valid_hit: match bc.crumb_at(r, cx, cy) case Some(i) then i < bc.count case None then True end
      drawn_dim: bc.draw(s, r, st, st, st, st).width = s.width and bc.draw(s, r, st, st, st, st).height = s.height
    end

end
"#;
    let contract_proj = Project::new("gtui_gw1_contracts", contracts);
    assert_ok(&contract_proj.check());

    // 2. Runtime & VT checks for BREADCRUMB collapse, keyboard, and mouse hit-testing.
    let prog = r#"reactive program GW1_BREADCRUMB

feature {ANY}

  update(bc: BREADCRUMB, e: EVENT): TUI_NEXT[BREADCRUMB]
    do
      match e
        case Key(k) then
          match k
            case Char(c) then if c = 'q' then TUI_NEXT.quit(0) else TUI_NEXT.next(bc) end
            case _ then TUI_NEXT.next(bc.on_key(k))
          end
        case Mouse(m) then TUI_NEXT.next(bc.on_mouse(m, RECT.make(0, 1, 26, 1)))
        case _ then TUI_NEXT.next(bc)
      end
    end

  opt_eq(o: OPTION[NAT], expected: NAT): BOOL
    do
      match o
        case Some(v) then v = expected
        case None then False
      end
    end

  opt_none(o: OPTION[NAT]): BOOL
    do
      match o
        case Some(_) then False
        case None then True
      end
    end

  view(bc: BREADCRUMB, blank: SCREEN): SCREEN
    do
      let base = STYLE.plain
      let cur_st = STYLE.plain.bold
      let sel_st = STYLE.plain.reversed
      let sep_st = STYLE.plain.fg(COLOR.gray)

      let r0 = RECT.make(0, 0, 35, 1)
      let r1 = RECT.make(0, 1, 26, 1)
      let r2 = RECT.make(0, 2, 16, 1)
      let r3 = RECT.make(0, 3, 8, 1)

      let hits_ok =
        opt_eq(bc.crumb_at(r0, 0, 0), 0)
          and opt_none(bc.crumb_at(r0, 4, 0))
          and opt_eq(bc.crumb_at(r0, 6, 0), 1)
          and opt_eq(bc.crumb_at(r1, 0, 1), 0)
          and opt_none(bc.crumb_at(r1, 6, 1))
          and opt_eq(bc.crumb_at(r1, 10, 1), 2)
          and opt_eq(bc.crumb_at(r1, 17, 1), 3)
          and opt_eq(bc.crumb_at(r2, 10, 2), 3)
          and opt_eq(bc.crumb_at(r3, 2, 3), 3)

      let s0 = bc.draw(blank, r0, base, cur_st, sel_st, sep_st)
      let s1 = bc.draw(s0, r1, base, cur_st, sel_st, sep_st)
      let s2 = bc.draw(s1, r2, base, cur_st, sel_st, sep_st)
      let s3 = bc.draw(s2, r3, base, cur_st, sel_st, sep_st)
      s3.text(blank.area, 0, 4, if hits_ok then "HITS_OK" else "HITS_BAD" end, base)
    end

  init(args: SEQ[STRING]): STEP[TUI[BREADCRUMB]]
    do
      let none: LIST[STRING] = LIST.empty
      let path = none.prepend("node.gdl").prepend("tree").prepend("widget").prepend("src")
      TUI.start(BREADCRUMB.make(path), update, view)
    end

  step(t: TUI[BREADCRUMB], e: EVENT): STEP[TUI[BREADCRUMB]]
    do
      t.step(e)
    end

end
"#;
    let runtime_proj = Project::new("gtui_gw1_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gw1_breadcrumb.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());

    // Initial -> Left (selects node.gdl) -> Left (selects tree) -> Left (selects hidden widget -> highlights '…' on row 1) -> Mouse click on 'src' -> quit.
    let script = "Resized:40:6 Resized:40:6 Key:Left Key:Left Key:Left Resized:40:6 Mouse:Down:0:1:1 Resized:40:6 Key:113";
    let ran = run(&runtime_proj.0.bin("gw1_breadcrumb"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|c| c.0).filter(|ch| *ch != '\0').collect()
    };

    let shot0 = &term.before_clear[2];
    assert_eq!(slice(shot0, 0, 0, 29), "src › widget › tree › node.gd");
    assert_eq!(slice(shot0, 1, 0, 25), "src › … › tree › node.gdl");
    assert_eq!(slice(shot0, 2, 0, 16), "src › … › node.g");
    assert_eq!(slice(shot0, 3, 0, 8), "node.gdl");
    assert_eq!(slice(shot0, 4, 0, 7), "HITS_OK");
    assert_eq!(pen_at(shot0, "node.gdl"), vec![1], "last crumb is bold by default");

    // After 3x Left: picked = 1 ("widget"), hidden on row 1 so '…' gets selected_st (reversed [7])!
    let shot1 = &term.before_clear[3];
    assert_eq!(pen_at(shot1, "widget"), vec![7], "row 0 highlights widget");
    assert_eq!(shot1[1][6].1, vec![7], "row 1 highlights ellipsis for hidden selection");

    // After Mouse:Down:0:1:1 -> selects crumb 0 ("src")!
    assert_eq!(pen_at(&term.grid, "src"), vec![7], "mouse click on crumb 0 selects src");
}

#[test]
fn gw3_menu_navigation_overlay_and_mouse_work() {
    let model = r#"
module GW3_MODEL

feature {ANY}

  check_menu: BOOL
    do
      let sep = MENU_ITEM.separator
      let it_new = MENU_ITEM.item("New", 1).with_shortcut("Ctrl+N")
      let it_redo = MENU_ITEM.item("Redo", 2).with_enabled(False)
      let it_quit = MENU_ITEM.item("Quit", 3).with_shortcut("Ctrl+Q")
      let file_items: LIST[MENU_ITEM] = LIST.empty.prepend(it_quit).prepend(sep).prepend(it_redo).prepend(it_new)
      let file_entry = MENU_ITEM.submenu("File", file_items)

      let edit_entry = MENU_ITEM.submenu("Edit", LIST.empty.prepend(MENU_ITEM.item("Copy", 4)))

      let dark = MENU_ITEM.item("Dark", 10)
      let light = MENU_ITEM.item("Light", 11)
      let nord = MENU_ITEM.item("Nord", 12)
      let theme_items: LIST[MENU_ITEM] = LIST.empty.prepend(nord).prepend(light).prepend(dark)
      let theme_sub = MENU_ITEM.submenu("Theme", theme_items)
      let view_entry = MENU_ITEM.submenu("View", LIST.empty.prepend(theme_sub))
      let help_entry = MENU_ITEM.item("Help", 20)

      let entries: LIST[MENU_ITEM] = LIST.empty.prepend(help_entry).prepend(view_entry).prepend(edit_entry).prepend(file_entry)
      let m = MENU.make(entries)

      -- MENU_ITEM selectable navigation contracts
      let f_sel = file_entry.first_selectable
      let l_sel = file_entry.last_selectable
      let n_sel = file_entry.next_selectable(0)
      let p_sel = file_entry.previous_selectable(0)
      let item_nav_ok =
        match f_sel case Some(a) then a < file_entry.count and file_entry.child(a).is_selectable case None then True end
        and match l_sel case Some(b) then b < file_entry.count and file_entry.child(b).is_selectable case None then True end
        and match n_sel case Some(c) then c < file_entry.count and file_entry.child(c).is_selectable case None then True end
        and match p_sel case Some(d) then d < file_entry.count and file_entry.child(d).is_selectable case None then True end

      -- MENU contracts: closed initially, count preserved, Escape does not deepen, overlay inside screen
      let m1 = m.on_key(Function(10))
      let m2 = m1.on_key(Down)
      let m3 = m2.on_key(Right)
      let m4 = m.on_key(Mod(Char('v'), False, True, False))
      let m5 = m4.on_key(Right)
      let m6 = m5.on_key(Enter)
      let e1 = m5.on_key(Escape)
      let c1 = m5.close
      let scr = RECT.make(0, 0, 80, 24)
      let bar_r = RECT.make(0, 0, 80, 1)
      let mc1 = m.on_mouse(MOUSE.MouseDown(0, 1, 0), bar_r, scr)
      let flip_box = m1.drop_down_area(RECT.make(0, 23, 80, 1), scr, 2)
      let row_box = m1.item_area(bar_r, scr, 2, 0)

      let ok =
        not m.is_open and m.count = 4
        and item_nav_ok
        and m1.count = 4 and m2.count = 4 and m3.count = 4 and m4.count = 4 and m5.count = 4 and m6.count = 4
        and e1.depth <= m5.depth
        and not c1.is_open
        and mc1.count = 4
        and flip_box.inside(scr)
        and row_box.inside(m1.drop_down_area(bar_r, scr, 2))
        and row_box.inside(scr)

      check ok: ok end
      ok
    ensure
      verified: Result
    end

end
"#;
    let model_proj = Project::new("gtui_gw3_model", model);
    let msrc = model_proj.0 .0.join("p/src");
    fs::rename(msrc.join("client.gdl"), msrc.join("gw3_model.gdl")).expect("named after its module");
    assert_ok(&model_proj.check());

    let prog = r#"
reactive program GW3_MENU

feature {NONE}

  build_menu: MENU
    do
      let sep = MENU_ITEM.separator
      let it_new = MENU_ITEM.item("New", 1).with_shortcut("Ctrl+N")
      let it_redo = MENU_ITEM.item("Redo", 2).with_enabled(False)
      let it_quit = MENU_ITEM.item("Quit", 3).with_shortcut("Ctrl+Q")
      let file_items: LIST[MENU_ITEM] = LIST.empty.prepend(it_quit).prepend(sep).prepend(it_redo).prepend(it_new)
      let file_entry = MENU_ITEM.submenu("File", file_items)

      let edit_entry = MENU_ITEM.submenu("Edit", LIST.empty.prepend(MENU_ITEM.item("Copy", 4)))

      let dark = MENU_ITEM.item("Dark", 10)
      let light = MENU_ITEM.item("Light", 11)
      let nord = MENU_ITEM.item("Nord", 12)
      let theme_items: LIST[MENU_ITEM] = LIST.empty.prepend(nord).prepend(light).prepend(dark)
      let theme_sub = MENU_ITEM.submenu("Theme", theme_items)
      let view_entry = MENU_ITEM.submenu("View", LIST.empty.prepend(theme_sub))
      let help_entry = MENU_ITEM.item("Help", 20)

      let entries: LIST[MENU_ITEM] = LIST.empty.prepend(help_entry).prepend(view_entry).prepend(edit_entry).prepend(file_entry)
      MENU.make(entries)
    end

  update(m: MENU, e: EVENT): TUI_NEXT[MENU]
    do
      match e
        case Key(Char('q')) then Quit(0, LIST.empty)
        case Key(k) then Next(m.on_key(k), LIST.empty)
        case Mouse(ev) then
          let scr = RECT.make(0, 0, 50, 12)
          let bar = RECT.make(0, 0, 50, 1)
          Next(m.on_mouse(ev, bar, scr), LIST.empty)
        case _ then Next(m, LIST.empty)
      end
    end

  view(m: MENU, blank: SCREEN): SCREEN
    do
      let r = RECT.make(0, 0, blank.width, 1)
      let bar_st = STYLE.plain
      let menu_st = STYLE.plain
      let hi_st = STYLE.plain.reversed
      let dim_st = STYLE.plain.fg(COLOR.gray)
      let drawn = m.draw(blank, r, bar_st, menu_st, hi_st, dim_st)
      let status =
        match m.chosen
          case Some(id) then if id = 10 then "CHOSEN:10" else "CHOSEN:OTHER" end
          case None then "CHOSEN:NONE"
        end
      drawn.text(RECT.make(0, 10, blank.width, 1), 0, 10, status, STYLE.plain)
    end

  init(args: SEQ[STRING]): STEP[TUI[MENU]]
    do
      TUI.start(build_menu, update, view)
    end

  step(t: TUI[MENU], e: EVENT): STEP[TUI[MENU]]
    do
      t.step(e)
    end

end
"#;
    let runtime_proj = Project::new("gtui_gw3_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gw3_menu.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());

    // Open File via click (x=1, y=0) -> Resized snapshot -> Click View (x=13, y=0) -> Click Theme (x=14, y=2) -> Resized snapshot -> Down + Enter to pick Dark (id 10) -> quit.
    let script = "Resized:50:12 Mouse:Down:0:1:0 Resized:50:12 Mouse:Down:0:13:0 Mouse:Down:0:14:2 Resized:50:12 Key:Down Key:Enter Key:113";
    let ran = run(&runtime_proj.0.bin("gw3_menu"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|c| c.0).filter(|ch| *ch != '\0').collect()
    };

    // Snapshot 2: File drop-down open
    let shot_file = &term.before_clear[2];
    assert_eq!(slice(shot_file, 0, 0, 24), " File  Edit  View  Help ");
    assert!(slice(shot_file, 2, 0, 20).contains("New"));
    assert!(slice(shot_file, 2, 0, 20).contains("Ctrl+N"));
    assert!(slice(shot_file, 3, 0, 20).contains("Redo"));
    assert!(slice(shot_file, 4, 0, 20).contains("─"));
    assert!(slice(shot_file, 5, 0, 20).contains("Quit"));

    // Snapshot 3: View -> Theme sub-menu open
    let shot_sub = &term.before_clear[3];
    assert!(slice(shot_sub, 2, 12, 30).contains("Theme"));
    assert!(slice(shot_sub, 2, 12, 30).contains("▸"));
    assert!(slice(shot_sub, 3, 20, 35).contains("Dark"));
    assert!(slice(shot_sub, 5, 20, 35).contains("Nord"));

    // Final grid: Dark chosen (id = 10)
    assert_eq!(slice(&term.grid, 10, 0, 9), "CHOSEN:10");
}

#[test]
fn gw4_button_and_dialog_work() {
    let model = r#"
module GW4_MODEL

feature {ANY}

  check_button_and_dialog: BOOL
    do
      let btn = BUTTON.make("OK")
      let br = RECT.make(2, 4, 12, 4)
      let brow = btn.content_row(br)
      let btn_ok =
        btn.width >= GLYPHS.width(btn.label)
        and brow.inside(br)
        and brow.height <= 1
        and btn.content_offset(12) <= 12
        and (btn.hit(br, 3, 5) = br.has(3, 5))

      let area = RECT.make(0, 0, 50, 14)
      let tiny = RECT.make(0, 0, 6, 3)

      let a0 = DIALOG.alert("Notice", "Saved")
      let c0 = DIALOG.confirm("Confirm", "Delete report.txt?").open
      let okc = DIALOG.ok_cancel("Exit", "Leave now?").open

      let c1 = c0.on_key(Tab)
      let c2 = c1.on_key(Char('y'))
      let c_outside = c0.on_mouse(MOUSE.MouseDown(0, 0, 0), area)

      let okc_esc = okc.on_key(Escape)
      let tiny_box = c0.box(tiny)
      let tiny_body = c0.body_area(tiny)
      let tiny_btn0 = c0.button_area(tiny, 0)

      let d_box = c0.box(area)
      let d_body = c0.body_area(area)
      let d_btn0 = c0.button_area(area, 0)
      let d_btn1 = c0.button_area(area, 1)

      let ok =
        btn_ok
        and a0.button_count = 1 and not a0.is_open
        and c0.button_count = 2 and c0.is_open and c0.focused < c0.button_count
        and c1.button_count = 2 and c1.focused < c1.button_count
        and c2.button_count = 2 and c2.focused < c2.button_count
        and c_outside.button_count = 2 and c_outside.focused < c_outside.button_count
        and okc_esc.button_count = 2 and okc_esc.focused < okc_esc.button_count
        and tiny_box.inside(tiny) and tiny_body.inside(tiny_box) and tiny_btn0.inside(tiny_box)
        and d_box.inside(area) and d_body.inside(d_box)
        and d_btn0.inside(d_box) and d_btn1.inside(d_box)

      check ok: ok end
      ok
    ensure
      verified: Result
    end

end
"#;
    let model_proj = Project::new("gtui_gw4_model", model);
    let msrc = model_proj.0 .0.join("p/src");
    fs::rename(msrc.join("client.gdl"), msrc.join("gw4_model.gdl")).expect("named after its module");
    assert_ok(&model_proj.check());

    let prog = r#"
reactive program GW4_DIALOG

feature {NONE}

  make_ambiguous: DIALOG
    do
      let btns: LIST[STRING] = LIST.empty.prepend("Cancel").prepend("Skip").prepend("Save")
      DIALOG.make("Choice", PARAGRAPH.of_text("Pick one").align_center, btns, 0, Some(2)).open
    end

  update(d: DIALOG, e: EVENT): TUI_NEXT[DIALOG]
    do
      let area = RECT.make(0, 0, 50, 14)
      match e
        case Key(Char('q')) then Quit(0, LIST.empty)
        case Key(Char('n')) then Next(make_ambiguous, LIST.empty)
        case Key(k) then Next(d.on_key(k), LIST.empty)
        case Mouse(m) then Next(d.on_mouse(m, area), LIST.empty)
        case _ then Next(d, LIST.empty)
      end
    end

  view(d: DIALOG, blank: SCREEN): SCREEN
    do
      let area = RECT.make(0, 0, 50, 14)
      let bg = STYLE.plain
      let edge = d.kind.default_style
      let title_st = STYLE.plain.bold
      let btn_st = STYLE.plain
      let focus_st = STYLE.plain.reversed
      let with_dlg = d.draw(blank, area, bg, edge, title_st, btn_st, focus_st)
      let ans_str =
        match d.answer
          case Some(0) then "ANS:0"
          case Some(1) then "ANS:1"
          case Some(2) then "ANS:2"
          case Some(_) then "ANS:OTHER"
          case None then "ANS:NONE"
        end
      let open_str = if d.is_open then "OPEN" else "CLOSED" end
      with_dlg.text(RECT.make(0, 12, blank.width, 1), 0, 12, ans_str.concat(" ").concat(open_str), STYLE.plain)
    end

  init(args: SEQ[STRING]): STEP[TUI[DIALOG]]
    do
      TUI.start(DIALOG.confirm("Confirm", "Delete report.txt?").open, update, view)
    end

  step(t: TUI[DIALOG], e: EVENT): STEP[TUI[DIALOG]]
    do
      t.step(e)
    end

end
"#;
    let runtime_proj = Project::new("gtui_gw4_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gw4_dialog.gdl")).expect("named after its module");
    assert_ok(&runtime_proj.check());

    // Initial confirm -> Tab (focus Yes) -> snapshot -> 'y' (answer 0, closed) -> snapshot -> 'n' (open Save/Skip/Cancel) -> 's' (ambiguous, stays open) -> Mouse outside (modal, stays open) -> Mouse on [ Skip ] (x=22, y=7, answers 1) -> quit.
    let script = "Resized:50:14 Key:Tab Resized:50:14 Key:121 Resized:50:14 Key:110 Key:115 Mouse:Down:0:1:1 Mouse:Down:0:22:7 Key:113";
    let ran = run(&runtime_proj.0.bin("gw4_dialog"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|c| c.0).filter(|ch| *ch != '\0').collect()
    };

    // Snapshot 2 (after Tab): Yes focused (reversed [7])
    let shot_tab = &term.before_clear[2];
    assert!(slice(shot_tab, 4, 14, 36).contains("Confirm"));
    assert!(slice(shot_tab, 5, 14, 36).contains("Delete report.txt?"));
    assert!(slice(shot_tab, 7, 14, 36).contains("[ Yes ]"));
    assert!(slice(shot_tab, 7, 14, 36).contains("[ No ]"));
    assert_eq!(pen_at(shot_tab, "[ Yes ]"), vec![7]);

    // Snapshot 3 (after 'y'): ANS:0 CLOSED
    let shot_y = &term.before_clear[3];
    assert_eq!(slice(shot_y, 12, 0, 12), "ANS:0 CLOSED");

    // Final grid (after 'n', 's' ambiguous, outside click swallowed, click on [ Skip ]): ANS:1 CLOSED
    assert_eq!(slice(&term.grid, 12, 0, 12), "ANS:1 CLOSED");
}

#[test]
fn gw5_checkbox_slider_and_toast_work() {
    let model = r#"
module GW5_MODEL

feature {ANY}

  check_controls: BOOL
    do
      let cb = CHECKBOX.unchecked("Wrap lines")
      let cr = RECT.make(0, 0, 25, 3)
      let ca = cb.content_area(cr)
      let cb1 = cb.on_key(Char(' '))
      let rad = CHECKBOX.radio("Option A", True)
      check inv: CHECKBOX.toggle_involution(cb) end
      let cb_ok =
        ca.inside(cr)
        and ca.height <= 1
        and ca.width <= cb.width and ca.width <= cr.width
        and not cb.is_checked
        and cb1.is_checked
        and rad.is_checked

      let sl = SLIDER.make(0, 100, 10, 20).with_label
      let sr = RECT.make(0, 4, 24, 1)
      let tr = sl.track_area(sr)
      let sl_up = sl.increase.page_up
      let sl_dn = sl.decrease.page_down
      let sl_end = sl.end_val
      let sl_home = sl.home
      let sl_drag = sl.on_mouse(MOUSE.MouseDrag(0, 99, 4), sr)
      let sl_ok =
        tr.inside(sr)
        and sl.lo <= sl.value and sl.value <= sl.hi
        and sl_up.lo = sl.lo and sl_up.hi = sl.hi and sl_up.value >= sl.value
        and sl_dn.lo = sl.lo and sl_dn.hi = sl.hi and sl_dn.value <= sl.value
        and sl_end.value = sl_end.hi
        and sl_home.value = sl_home.lo
        and sl.thumb_cell(10) < 10
        and sl_drag.lo <= sl_drag.value and sl_drag.value <= sl_drag.hi

      let q0 = TOAST.empty.with_max_visible(2)
      let q1 = q0.push_info("A", 3000).push_success("B", 3000).push_warning("C", 3000)
      let q2 = q1.tick(1000)
      let q3 = q2.tick(3999)
      let q4 = q3.tick(4000)
      let tarea = RECT.make(0, 6, 40, 10)
      let b0 = q2.box_at(tarea, 0)
      let q_click = q2.on_mouse(MOUSE.MouseDown(0, b0.x, b0.y), tarea)
      let toast_ok =
        q1.visible_count <= q1.count and q1.visible_count <= q1.max_visible
        and q2.has_time and q2.now = 1000 and q2.count <= q1.count
        and q3.has_time and q3.now = 3999 and q3.count <= q2.count
        and q4.has_time and q4.now = 4000 and q4.count <= q3.count
        and b0.inside(tarea)
        and q_click.count <= q2.count

      let ok = cb_ok and sl_ok and toast_ok
      check ok: ok end
      ok
    ensure
      verified: Result
    end

end
"#;
    let model_proj = Project::new("gtui_gw5_model", model);
    let msrc = model_proj.0 .0.join("p/src");
    fs::rename(msrc.join("client.gdl"), msrc.join("gw5_model.gdl")).expect("named after its module");
    assert_ok(&model_proj.check());

    let state_mod = r#"
module GW5_STATE

representation {NONE}
  f_cb: CHECKBOX
  f_sl: SLIDER
  f_toasts: TOAST

create {ANY}

  init: GW5_STATE
    do
      let q = TOAST.empty.with_max_visible(2).push_info("First", 3000).push_success("Second", 3000).push_warning("Third", 3000).tick(1000)
      { f_cb = CHECKBOX.unchecked("Wrap"), f_sl = SLIDER.make(0, 100, 10, 20).with_label, f_toasts = q }
    end

feature {ANY}
  cb: CHECKBOX do Current.f_cb end
  sl: SLIDER do Current.f_sl end
  toasts: TOAST do Current.f_toasts end

  with_cb(c: CHECKBOX): GW5_STATE do { Current with f_cb = c } end
  with_sl(s: SLIDER): GW5_STATE do { Current with f_sl = s } end
  with_toasts(t: TOAST): GW5_STATE do { Current with f_toasts = t } end

end
"#;

    let prog = r#"
reactive program GW5_CONTROLS

feature {NONE}

  update(st: GW5_STATE, e: EVENT): TUI_NEXT[GW5_STATE]
    do
      let cb_r = RECT.make(0, 0, 20, 1)
      let sl_r = RECT.make(0, 2, 24, 1)
      let t_r = RECT.make(0, 4, 40, 10)
      match e
        case Key(Char('q')) then Quit(0, LIST.empty)
        case Key(Char('t')) then Next(st.with_toasts(st.toasts.tick(4000)), LIST.empty)
        case Key(k) then Next(st.with_cb(st.cb.on_key(k)).with_sl(st.sl.on_key(k)), LIST.empty)
        case Mouse(m) then
          Next(st.with_cb(st.cb.on_mouse(m, cb_r)).with_sl(st.sl.on_mouse(m, sl_r)).with_toasts(st.toasts.on_mouse(m, t_r)), LIST.empty)
        case _ then Next(st, LIST.empty)
      end
    end

  view(st: GW5_STATE, blank: SCREEN): SCREEN
    do
      let plain = STYLE.plain
      let s1 = st.cb.draw(blank, RECT.make(0, 0, 20, 1), plain, False)
      let s2 = st.sl.draw(s1, RECT.make(0, 2, 24, 1), plain, plain.bold, plain.reversed)
      let s3 = st.toasts.draw(s2, RECT.make(0, 4, 40, 10), plain, plain, plain, plain, plain)
      let cnt = if st.toasts.count <= 1000 then st.toasts.count else 1000 end
      let cnt_str = NUMBER_TEXT.nat_to_text(cnt)
      s3.text(RECT.make(0, 14, 40, 1), 0, 14, "TOASTS:".concat(cnt_str), plain)
    end

  init(args: SEQ[STRING]): STEP[TUI[GW5_STATE]]
    do
      TUI.start(GW5_STATE.init, update, view)
    end

  step(t: TUI[GW5_STATE], e: EVENT): STEP[TUI[GW5_STATE]]
    do
      t.step(e)
    end

end
"#;
    let runtime_proj = Project::new("gtui_gw5_runtime", prog);
    let src = runtime_proj.0 .0.join("p/src");
    fs::rename(src.join("client.gdl"), src.join("gw5_controls.gdl")).expect("named after its module");
    fs::write(src.join("gw5_state.gdl"), state_mod).expect("write gw5_state.gdl");
    assert_ok(&runtime_proj.check());

    // Initial (3 toasts, 2 visible: First + Second) -> click checkbox (x=2, y=0) -> drag slider past right (x=30, y=2) -> 't' (tick 4000 expires First & Second, shows Third) -> snapshot -> click Third toast (x=35, y=5) -> quit.
    let script = "Resized:42:16 Mouse:Down:0:2:0 Mouse:Drag:0:30:2 Key:116 Resized:42:16 Mouse:Down:0:35:5 Key:113";
    let ran = run(&runtime_proj.0.bin("gw5_controls"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let slice = |grid: &[Vec<Cell>], row: usize, x0: usize, x1: usize| -> String {
        grid[row][x0..x1].iter().map(|c| c.0).filter(|ch| *ch != '\0').collect()
    };

    // Snapshot 2 (after tick 4000): checkbox checked, slider at 100, Third toast visible
    let shot_t = &term.before_clear[2];
    assert_eq!(slice(shot_t, 0, 0, 8), "[x] Wrap");
    assert!(slice(shot_t, 2, 0, 24).contains("100"));
    assert!(slice(shot_t, 5, 30, 40).contains("Third"));
    assert_eq!(slice(shot_t, 14, 0, 8), "TOASTS:1");

    // Final grid (after click on Third toast): TOASTS:0
    assert_eq!(slice(&term.grid, 14, 0, 8), "TOASTS:0");
}

#[test]
fn gw6_examples_menu_breadcrumb_dialog_dashboard_work() {
    // 1. Negative contract proof for MENU click routing (cannot return out-of-range entry index).
    let bad_menu = r#"functions CLIENT

feature {ANY}

  bad_entry(m: MENU, ch: CHAR): BOOL
    do
      True
    ensure
      out_of_range: match m.entry_with_initial(ch) case Some(i) then i = m.count case None then True end
    end

end
"#;
    let bad_proj = Project::new("gtui_gw6_bad_menu", bad_menu);
    let bad_out = bad_proj.check();
    assert!(!bad_out.status.success(), "out-of-range menu entry claim must be refused: {}", all(&bad_out));
    assert!(all(&bad_out).contains("`out_of_range`"), "{}", all(&bad_out));

    // 2. Scripted VT session for `examples/gtui_tour` (BREADCRUMB, MENU, DIALOG).
    let tour_dir = Dir::new("gw6_tour");
    assert_ok(&tour_dir.check(&root().join("examples/gtui_tour")));
    let tour_script = concat!(
        "Resized:90:24 ",
        // Expand `screen` and select `CELL` -> breadcrumb shows `gtui › screen › CELL`.
        "Key:Down Key:Right Key:Down Resized:90:24 ",
        // Click `screen` in breadcrumb (`x=38, y=1`) -> selects `screen` node.
        "Mouse:Down:0:38:1 Resized:90:24 ",
        // Open MENU via F10, navigate View -> Theme -> Nord, activate, then Ctrl+n (modified).
        "Key:F10 Key:Right Key:Right Key:Down Key:Down Key:Enter Key:Ctrl:n Resized:90:24 ",
        // Press 'q' while modified -> opens Unsaved Changes DIALOG.
        "Key:113 Resized:90:24 ",
        // Escape cancels dialog -> tour stays open; press 'q' again then 'y' to confirm quit.
        "Key:Escape Resized:90:24 Key:113 Key:121"
    );
    let tour_ran = run(&tour_dir.bin("gtui_tour"), tour_script, &[]);
    assert_eq!(tour_ran.status.code(), Some(0), "{}", all(&tour_ran));
    let mut tour_term = Terminal::new(sizes(tour_script));
    tour_term.feed(&tour_ran.stdout);

    let bc_shot = screen_text(&tour_term.before_clear[2]);
    assert!(bc_shot.contains("gtui › screen › CELL"), "breadcrumb shows full tree path:\n{bc_shot}");

    let clicked_crumb = screen_text(&tour_term.before_clear[3]);
    assert!(clicked_crumb.contains("╭─ screen Text"), "breadcrumb click selected ancestor `screen`:\n{clicked_crumb}");
    assert!(clicked_crumb.contains("File  View  Help"), "top menu bar rendered:\n{clicked_crumb}");

    let themed_shot = &tour_term.before_clear[4];
    let themed_text = screen_text(themed_shot);
    assert!(themed_text.contains("Theme nord"), "menu View->Theme->Nord switched theme:\n{themed_text}");
    assert!(themed_text.contains("New buffer (modified)"), "Ctrl+N marked buffer modified:\n{themed_text}");
    assert_eq!(pen_at(themed_shot, "╭─Modules"), vec![38, 2, 76, 86, 106], "Nord RGB border applied");

    let dlg_shot = screen_text(&tour_term.before_clear[5]);
    assert!(dlg_shot.contains("Unsaved Changes") && dlg_shot.contains("Discard modified buffer and quit?"), "quit dialog shown:\n{dlg_shot}");
    assert!(dlg_shot.contains("[ Yes ]") && dlg_shot.contains("[ No ]"), "dialog buttons shown:\n{dlg_shot}");

    let resumed_shot = screen_text(&tour_term.before_clear[6]);
    assert!(!resumed_shot.contains("Unsaved Changes"), "Escape cancelled quit dialog:\n{resumed_shot}");

    // 3. Scripted VT session for `examples/gtui_dashboard` (DIVIDER, CHECKBOX, SLIDER, TOAST).
    let dash_dir = Dir::new("gw6_dash");
    assert_ok(&dash_dir.check(&root().join("examples/gtui_dashboard")));
    let dash_script = concat!(
        "Resized:80:24 Resized:80:24 ",
        // Click Auto-refresh checkbox (62, 18), click slider right (73, 21), press 's' to push toast.
        "Mouse:Down:0:62:18 Mouse:Down:0:73:21 Key:115 Resized:80:24 ",
        // Click top-right toast (75, 1) to dismiss it.
        "Mouse:Down:0:75:1 Resized:80:24 Key:113"
    );
    let dash_ran = run(&dash_dir.bin("gtui_dashboard"), dash_script, &[]);
    assert_eq!(dash_ran.status.code(), Some(0), "{}", all(&dash_ran));
    let mut dash_term = Terminal::new(sizes(dash_script));
    dash_term.feed(&dash_ran.stdout);

    let d_init = screen_text(&dash_term.before_clear[2]);
    assert!(d_init.contains("[x] Auto-refresh"), "checkbox drawn:\n{d_init}");
    assert!(d_init.contains("[x] Show finished"), "second checkbox drawn:\n{d_init}");
    assert!(d_init.contains("Rate"), "labeled divider drawn:\n{d_init}");

    let d_after = screen_text(&dash_term.before_clear[3]);
    assert!(d_after.contains("[ ] Auto-refresh"), "checkbox toggled on click:\n{d_after}");
    assert!(d_after.contains("━━━━━━━━━━━━●"), "slider updated on click:\n{d_after}");
    assert!(d_after.contains("Queued task"), "toast notification visible:\n{d_after}");

    let d_dismissed = screen_text(&dash_term.before_clear[4]);
    assert!(!d_dismissed.contains("Queued task"), "toast dismissed on click:\n{d_dismissed}");
}

#[test]
fn gtui_widgets_example_verifies_and_exercises_all_06_widgets() {
    let dir = Dir::new("gtui_widgets_demo");
    assert_ok(&dir.check(&root().join("examples/gtui_widgets")));
    let script = concat!(
        "Resized:90:22 Resized:90:22 ",
        // Dismiss initial welcome toast at top-right of body (80, 3), click breadcrumb "Appearance" (11, 1),
        // toggle Wrap checkbox (4, 4), click Apply button (5, 15).
        "Mouse:Down:0:80:3 Mouse:Down:0:11:1 Mouse:Down:0:4:4 Mouse:Down:0:5:15 Resized:90:22 ",
        // Click Reset button (25, 15) -> opens modal Reset confirmation DIALOG.
        "Mouse:Down:0:25:15 Resized:90:22 ",
        // Press 'y' to confirm Reset -> restores Wrap checkbox & pushes Defaults restored toast.
        "Key:121 Resized:90:22 ",
        // Open MENU via F10 -> File drop-down visible.
        "Key:F10 Resized:90:22 ",
        // Two Escapes close drop-down and menu bar, 'q' opens Quit confirmation dialog, 'y' confirms quit.
        "Key:Escape Key:Escape Key:113 Key:121"
    );
    let ran = run(&dir.bin("gtui_widgets"), script, &[]);
    assert_eq!(ran.status.code(), Some(0), "{}", all(&ran));
    let mut term = Terminal::new(sizes(script));
    term.feed(&ran.stdout);

    let init = screen_text(&term.before_clear[2]);
    assert!(init.contains("File  Controls  Toast  Help"), "menu bar rendered:\n{init}");
    assert!(init.contains("Studio › Appearance › Preview"), "breadcrumb rendered:\n{init}");
    assert!(init.contains("Options") && init.contains("Tuning") && init.contains("Actions"), "labeled dividers rendered:\n{init}");
    assert!(init.contains("[x] Wrap preview") && init.contains("[x] Line numbers"), "checkboxes rendered:\n{init}");
    assert!(init.contains("[ Apply ]") && init.contains("[ Toast ]") && init.contains("[ Reset ]"), "buttons rendered:\n{init}");
    assert!(init.contains("Welcome to gtui 0.6.0 Studio!"), "initial toast rendered:\n{init}");

    let after_interact = screen_text(&term.before_clear[3]);
    assert!(!after_interact.contains("Welcome to gtui 0.6.0 Studio!"), "welcome toast dismissed on click:\n{after_interact}");
    assert!(after_interact.contains("Appearance Controls"), "breadcrumb click switched section:\n{after_interact}");
    assert!(after_interact.contains("[ ] Wrap preview"), "checkbox toggled on click:\n{after_interact}");
    assert!(after_interact.contains("Applied (w=60, i=2)"), "Apply button pushed success toast:\n{after_interact}");

    let reset_dlg = screen_text(&term.before_clear[4]);
    assert!(reset_dlg.contains("Reset Defaults?"), "Reset button opened modal dialog:\n{reset_dlg}");
    assert!(reset_dlg.contains("[ Yes ]") && reset_dlg.contains("[ No ]"), "dialog buttons rendered:\n{reset_dlg}");

    let after_reset = screen_text(&term.before_clear[5]);
    assert!(!after_reset.contains("Reset Defaults?"), "dialog closed after Yes:\n{after_reset}");
    assert!(after_reset.contains("[x] Wrap preview"), "Reset restored Wrap checkbox:\n{after_reset}");
    assert!(after_reset.contains("Defaults restored."), "Reset pushed info toast:\n{after_reset}");

    let menu_open = screen_text(&term.before_clear[6]);
    assert!(menu_open.contains("Apply Settings") && menu_open.contains("Export..."), "F10 opened File drop-down:\n{menu_open}");
}
