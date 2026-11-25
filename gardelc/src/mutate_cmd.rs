//! `gardelc mutate` (Task H.10): changes a package's code in small ways, one change at a time,
//! and reports what refuses each changed package, the verifier or a test. A change that both
//! accept is a survivor: nothing that is proven or tested tells it from the original.
//!
//! The changes are made on the source text, at the places a recording parser gives, in a
//! private copy of the package. The package itself is only read.
//!
//! Only code is changed: the bodies of routines, and the `require` and `invariant` clauses, each
//! of which can be dropped. A `specify` routine, a law's proof, a `check` and a routine that
//! states a fact (a `BOOL` with a clause that ensures `Result`) are proofs, which a change can
//! only make fail or leave as good.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{Parser, Site, SiteKind, Token, TokenRange};

/// The file beside a package's manifest that names the mutants no proof or test can refuse.
const EQUIVALENT_FILE: &str = "Gardel.mutants";

/// How long the verifier may take over one mutant, and its tests as long again.
const DEFAULT_TIME_LIMIT_SECS: u64 = 1800;

const BUILD_VALUE_FLAGS: &[&str] = &["--fuel", "--z3-seed", "--z3-rlimit"];

struct Options {
    package: PathBuf,
    only: Vec<String>,
    limit: Option<usize>,
    seed: u64,
    jobs: usize,
    time_limit: Duration,
    list: bool,
    build_flags: Vec<String>,
}

fn usage(msg: &str) -> i32 {
    eprintln!("[ERROR] gardelc mutate: {msg}");
    eprintln!("Usage: gardelc mutate [DIR] [--only <MODULE>]... [--limit <N>] [--seed <N>] [--jobs <N>] [--time-limit <SECS>] [--list] [--fuel <N>] [--z3-seed <N>] [--z3-rlimit <N>]");
    eprintln!("  DIR is the package (default `.`). Each mutant is verified and, if DIR/tests is a package, tested, in a copy.");
    eprintln!("  --only <MODULE>  change only this module; may be repeated");
    eprintln!("  --limit <N>      try N of the mutants, chosen by --seed (default 0)");
    eprintln!("  --jobs <N>       try N mutants at a time (default 1)");
    eprintln!("  --time-limit <SECS>  stop the verifier on one mutant after SECS seconds (default {DEFAULT_TIME_LIMIT_SECS})");
    eprintln!("  --list           list the mutants and try none");
    eprintln!("  DIR/{EQUIVALENT_FILE} names the mutants that are equivalent to the original, one a line; a line that starts with `--` is a comment.");
    2
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        package: PathBuf::from("."),
        only: Vec::new(),
        limit: None,
        seed: 0,
        jobs: 1,
        time_limit: Duration::from_secs(DEFAULT_TIME_LIMIT_SECS),
        list: false,
        build_flags: Vec::new(),
    };
    let mut package = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let value = || args.get(i + 1).filter(|v| !v.starts_with("--")).cloned().ok_or(format!("{a}: expected a value"));
        let number = || value()?.parse::<u64>().map_err(|_| format!("{a}: expected a number"));
        let mut words = 2;
        match a {
            "--only" => opts.only.push(value()?),
            "--limit" => opts.limit = Some(usize::try_from(number()?).map_err(|_| format!("{a}: too large"))?),
            "--seed" => opts.seed = number()?,
            "--jobs" => {
                opts.jobs = usize::try_from(number()?).ok().filter(|n| *n >= 1).ok_or(format!("{a}: expected at least 1"))?;
            }
            "--time-limit" => opts.time_limit = Duration::from_secs(number()?.max(1)),
            "--list" => {
                opts.list = true;
                words = 1;
            }
            _ if BUILD_VALUE_FLAGS.contains(&a) => opts.build_flags.extend([a.to_string(), value()?]),
            _ if a.starts_with("--") => return Err(format!("unknown flag `{a}`")),
            _ if package.is_some() => return Err(format!("one package at a time: `{a}` is a second")),
            _ => {
                package = Some(PathBuf::from(a));
                words = 1;
            }
        }
        i += words;
    }
    opts.package = package.unwrap_or(opts.package);
    Ok(opts)
}

/// One change to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mutant {
    /// The file, relative to the package.
    file: PathBuf,
    module: String,
    /// The routine or law changed, or `invariant`.
    place: String,
    line: usize,
    /// The `char`s replaced, and by what.
    range: (usize, usize),
    replacement: String,
    /// What the change is, in a few words.
    what: String,
    /// The lines changed, before and after, on one line each.
    old: String,
    new: String,
    /// What `Gardel.mutants` calls it.
    key: String,
}

/// `text` on one line, with one space for each run of white space.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` collapsed and cut to a length that fits a report.
fn brief(text: &str) -> String {
    let t = collapse(text);
    if t.chars().count() <= 60 {
        t
    } else {
        format!("{}...", t.chars().take(57).collect::<String>())
    }
}

/// A source file, with its tokens and what the parser read where.
struct Source {
    chars: Vec<char>,
    tokens: Vec<Token>,
    lines: Vec<usize>,
    spans: Vec<(usize, usize)>,
    sites: Vec<Site>,
}

impl Source {
    fn read(text: &str) -> Result<Self, String> {
        let spanned = crate::tokenize_with_spans(text).map_err(|e| e.to_string())?;
        let sites = Parser::parse_sites(spanned.tokens.clone()).map_err(|e| e.to_string())?;
        Ok(Self {
            chars: text.chars().collect(),
            tokens: spanned.tokens,
            lines: spanned.lines,
            spans: spanned.spans,
            sites,
        })
    }

    /// The `char`s of a range of tokens. An empty range has none.
    fn chars_of(&self, range: &TokenRange) -> (usize, usize) {
        if range.is_empty() {
            let at = self.spans[range.start].0;
            (at, at)
        } else {
            (self.spans[range.start].0, self.spans[range.end - 1].1)
        }
    }

    fn slice(&self, (from, to): (usize, usize)) -> String {
        self.chars[from..to].iter().collect()
    }

    fn text(&self, range: &TokenRange) -> String {
        self.slice(self.chars_of(range))
    }

    /// The innermost site of a kind `pick` takes that holds `range`.
    fn around<T>(&self, range: &TokenRange, pick: impl Fn(&SiteKind) -> Option<T>) -> Option<T> {
        self.sites
            .iter()
            .filter(|s| s.range.start <= range.start && range.end <= s.range.end)
            .filter_map(|s| pick(&s.kind).map(|t| (s.range.len(), t)))
            .min_by_key(|(len, _)| *len)
            .map(|(_, t)| t)
    }

    /// Whether a proof alone reads what is at `site`. A clause is ghost itself, and dropping it
    /// is a change to the code, unless it is in a ghost routine.
    fn is_ghost(&self, site: &Site) -> bool {
        let clause = matches!(site.kind, SiteKind::Require | SiteKind::Invariant);
        self.sites.iter().any(|s| {
            s.kind == SiteKind::Ghost
                && s.range.start <= site.range.start
                && site.range.end <= s.range.end
                && !(clause && s.range == site.range)
        })
    }

    /// Whether the expression is made of parts joined by operators, so it needs parentheses
    /// where a part is expected.
    fn is_compound(&self, range: &TokenRange) -> bool {
        let mut depth = 0;
        for t in &self.tokens[range.clone()] {
            match t {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => depth -= 1,
                Token::Ident(k) if matches!(k.as_str(), "if" | "match" | "fn" | "with") => depth += 1,
                Token::Ident(k) if k == "end" => depth -= 1,
                Token::Ident(k) if depth == 0 && matches!(k.as_str(), "and" | "or" | "not" | "implies" | "forall" | "exists") => {
                    return true;
                }
                Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::PlusPercent
                | Token::MinusPercent
                | Token::StarPercent
                | Token::Amp
                | Token::Caret
                | Token::Pipe
                | Token::Shl
                | Token::Shr
                | Token::Eq
                | Token::Ne
                | Token::Lt
                | Token::Le
                | Token::Gt
                | Token::Ge
                    if depth == 0 =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// The text of a part, with parentheses if it needs them.
    fn part(&self, range: &TokenRange) -> String {
        let t = self.text(range);
        if self.is_compound(range) {
            format!("({t})")
        } else {
            t
        }
    }

    /// Whether the receiver of a call is a value, not the module of `MODULE.routine(...)`.
    fn is_value(&self, range: &TokenRange) -> bool {
        match &self.tokens[range.clone()] {
            [Token::Ident(name)] => name == "Current" || !name.starts_with(|c: char| c.is_ascii_uppercase()),
            [Token::Ident(_), Token::ColonColon, Token::Ident(_)] => false,
            _ => true,
        }
    }
}

/// The changes one site allows: the tokens replaced, by what, and in a few words.
fn changes(src: &Source, site: &Site) -> Vec<(TokenRange, String, String)> {
    let mut out = Vec::new();
    let range = &site.range;
    match &site.kind {
        SiteKind::Literal => match &src.tokens[range.start] {
            Token::Int(n) => {
                let mut to = |m: u128| out.push((range.clone(), m.to_string(), format!("`{n}` -> `{m}`")));
                if let Some(m) = n.checked_add(1) {
                    to(m);
                }
                if let Some(m) = n.checked_sub(1) {
                    to(m);
                }
            }
            Token::Ident(b) => {
                let other = if b == "True" { "False" } else { "True" };
                out.push((range.clone(), other.to_string(), format!("`{b}` -> `{other}`")));
            }
            _ => {}
        },
        SiteKind::Operator => {
            let other = match &src.tokens[range.start] {
                Token::Lt => "<=",
                Token::Le => "<",
                Token::Gt => ">=",
                Token::Ge => ">",
                Token::Eq => "!=",
                Token::Ne => "=",
                Token::Ident(k) if k == "and" => "or",
                Token::Ident(k) if k == "or" => "and",
                _ => return out,
            };
            out.push((range.clone(), other.to_string(), format!("`{}` -> `{other}`", src.text(range))));
        }
        SiteKind::Condition => {
            let c = src.text(range);
            if matches!(c.as_str(), "True" | "False") {
                return out;
            }
            out.push((range.clone(), format!("not ({c})"), format!("condition negated: `{}`", brief(&c))));
            out.push((range.clone(), "True".to_string(), format!("always `then`: `{}`", brief(&c))));
            out.push((range.clone(), "False".to_string(), format!("always `else`: `{}`", brief(&c))));
        }
        SiteKind::Arms(arms) => {
            for (i, (arm, _)) in arms.iter().enumerate() {
                let j = (i + 1) % arms.len();
                let (other, names) = &arms[j];
                // The other arm's body has no meaning here if it uses what its pattern binds.
                let binds = src.tokens[other.clone()].iter().any(|t| matches!(t, Token::Ident(x) if names.contains(x)));
                if i != j && !binds && collapse(&src.text(arm)) != collapse(&src.text(other)) {
                    out.push((arm.clone(), src.text(other), format!("arm {} of a `match` gets arm {}'s body", i + 1, j + 1)));
                }
            }
        }
        SiteKind::Require | SiteKind::Invariant => {
            let kind = if site.kind == SiteKind::Require { "require" } else { "invariant" };
            // `tag: clause`
            let tag = range.start.checked_sub(2).map_or_else(String::new, |t| src.text(&(t..t + 1)));
            if src.text(range) != "True" {
                out.push((range.clone(), "True".to_string(), format!("`{kind}` `{tag}` dropped")));
            }
        }
        SiteKind::Call { receiver, args } => {
            for (a, b) in args.iter().zip(args.iter().skip(1)) {
                let (x, y) = (src.text(a), src.text(b));
                if collapse(&x) != collapse(&y) {
                    let between = src.slice((src.chars_of(a).1, src.chars_of(b).0));
                    let what = format!("arguments swapped: `{}` and `{}`", brief(&x), brief(&y));
                    out.push((a.start..b.end, format!("{y}{between}{x}"), what));
                }
            }
            let parts = receiver.iter().filter(|r| src.is_value(r)).chain(args);
            for p in parts {
                let what = format!("`{}` -> `{}`", brief(&src.text(range)), brief(&src.text(p)));
                out.push((range.clone(), src.part(p), what));
            }
        }
        SiteKind::Module(_) | SiteKind::Routine { .. } | SiteKind::Law(_) | SiteKind::Ghost | SiteKind::Body => {}
    }
    out
}

/// The whole lines that hold the `char`s from `from` to `to`, as a range of `char`s.
fn whole_lines(chars: &[char], from: usize, to: usize) -> (usize, usize) {
    let start = chars[..from].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
    let end = chars[to..].iter().position(|c| *c == '\n').map_or(chars.len(), |i| to + i);
    (start, end)
}

/// Every mutant of a file, in order of place.
fn mutants_of(file: &Path, text: &str) -> Result<Vec<Mutant>, String> {
    let src = Source::read(text)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut sites: Vec<&Site> = src.sites.iter().collect();
    sites.sort_by_key(|s| (s.range.start, s.range.end));
    for site in sites {
        let module = src.around(&site.range, |k| match k {
            SiteKind::Module(m) => Some(m.clone()),
            _ => None,
        });
        let Some(module) = module else { continue };
        let place = src.around(&site.range, |k| match k {
            SiteKind::Routine { name, fact } => Some((name.clone(), *fact, false)),
            SiteKind::Law(name) => Some((name.clone(), false, true)),
            _ => None,
        });
        let place = match (&site.kind, place) {
            (SiteKind::Invariant, _) => "invariant".to_string(),
            (SiteKind::Require, Some((name, false, _))) => name,
            (_, Some((name, false, false))) => {
                if src.around(&site.range, |k| (*k == SiteKind::Body).then_some(())).is_none() {
                    continue;
                }
                name
            }
            _ => continue,
        };
        if src.is_ghost(site) {
            continue;
        }
        for (tokens, replacement, what) in changes(&src, site) {
            let range = src.chars_of(&tokens);
            if src.slice(range) == replacement || !seen.insert((range, replacement.clone())) {
                continue;
            }
            let (from, to) = whole_lines(&src.chars, range.0, range.1);
            let old = collapse(&src.slice((from, to)));
            let new = collapse(&format!("{}{replacement}{}", src.slice((from, range.0)), src.slice((range.1, to))));
            out.push(Mutant {
                file: file.to_path_buf(),
                key: format!("{module}.{place}: {old} => {new}"),
                module: module.clone(),
                place: place.clone(),
                line: src.lines[tokens.start],
                range,
                replacement,
                what,
                old,
                new,
            });
        }
    }
    Ok(out)
}

/// Gives each mutant a key of its own: the second with the same one gets `#2`.
fn number_keys(mutants: &mut [Mutant]) {
    let mut count: HashMap<String, usize> = HashMap::new();
    for m in mutants {
        let n = count.entry(m.key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            m.key = format!("{} #{n}", m.key);
        }
    }
}

/// The text with the mutant's change made.
fn apply(text: &str, m: &Mutant) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: String = chars[..m.range.0].iter().collect();
    out.push_str(&m.replacement);
    out.extend(&chars[m.range.1..]);
    out
}

/// `n` of the numbers below `len`, in order, chosen by `seed` (SplitMix64, Fisher-Yates).
fn choose(len: usize, n: usize, seed: u64) -> Vec<usize> {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut all: Vec<usize> = (0..len).collect();
    for i in (1..len).rev() {
        let j = usize::try_from(next() % (i as u64 + 1)).unwrap_or(0);
        all.swap(i, j);
    }
    all.truncate(n);
    all.sort_unstable();
    all
}

/// The lines of `Gardel.mutants` that are not comments.
fn equivalents(package: &Path) -> Result<Vec<String>, String> {
    let path = package.join(EQUIVALENT_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    Ok(text.lines().map(collapse).filter(|l| !l.is_empty() && !l.starts_with("--")).collect())
}

/// What happened to a mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// The verifier refused it, by this clause.
    Verifier(String),
    /// The verifier gave up on it: it is unproven, not shown false.
    GaveUp(String),
    /// A test failed.
    Test(String),
    Survived,
    /// It is not a program: the front end refused it.
    Refused(String),
}

/// What a failed run of the verifier says. `lines` are the lines it printed.
fn verdict<'a>(lines: impl Iterator<Item = &'a str>) -> Outcome {
    let errors: Vec<&str> = lines.filter_map(|l| l.trim().strip_prefix("[ERROR] ")).collect();
    let gave_up = |e: &str| {
        e.starts_with("Phase 3 Boogie (Verification Timeout)")
            || e.starts_with("Phase 3 Boogie (Verification Resource Limit)")
            || (e.starts_with("Phase 3 Boogie (Verification Incomplete)") && e.contains("the solver was stopped after"))
    };
    let refuted = |e: &str| {
        !gave_up(e)
            && (e.starts_with("Phase 4 (Reactive Violation)")
                || (e.starts_with("Phase 3 Boogie (") && !e.starts_with("Phase 3 Boogie (Verification Incomplete)")))
    };
    if let Some(e) = errors.iter().find(|e| refuted(e)) {
        Outcome::Verifier(clause_of(e))
    } else if let Some(e) = errors.iter().find(|e| gave_up(e)) {
        Outcome::GaveUp(clause_of(e))
    } else {
        Outcome::Refused(errors.first().map_or_else(|| "the build failed".to_string(), |e| brief(e)))
    }
}

/// The clause of an error line: "RULE in `routine` (tag) at file:1:1: message" is
/// "`tag` of `routine` (what went wrong)".
fn clause_of(error: &str) -> String {
    let parts = || {
        let (rule, rest) = error.split_once(" in `")?;
        let (routine, rest) = rest.split_once("` (")?;
        let (tag, _) = rest.split_once(") at ")?;
        let rule = rule.split_once('(').map_or(rule, |(_, r)| r.trim_end_matches(')'));
        Some(format!("`{tag}` of `{routine}` ({rule})"))
    };
    parts().unwrap_or_else(|| brief(error))
}

/// What a failed run of `gardelc test` says.
fn test_verdict(text: &str) -> Outcome {
    let mut lines = text.lines();
    while let Some(l) = lines.next() {
        if l.starts_with("[test] ") && l.contains("did not verify or build:") {
            return match verdict(lines.map(|l| l.trim().trim_start_matches("| "))) {
                Outcome::Verifier(c) => Outcome::Verifier(format!("{c}, in the tests package")),
                Outcome::GaveUp(c) => Outcome::GaveUp(format!("{c}, in the tests package")),
                other => other,
            };
        }
        if let Some(rest) = l.strip_prefix("[test] ") {
            if let Some((name, why)) = rest.split_once(" ... not built: ") {
                return Outcome::Refused(format!("{name} was not built: {}", brief(why)));
            }
            if let Some((name, why)) = rest.split_once(" ... FAILED") {
                let shown = lines.next().and_then(|l| l.trim().strip_prefix("| ")).map(brief);
                let why = shown.unwrap_or_else(|| why.trim().to_string());
                return Outcome::Test(format!("{name}: {why}"));
            }
        }
    }
    let error = text.lines().find_map(|l| l.trim().strip_prefix("[ERROR] "));
    Outcome::Refused(error.map_or_else(|| "the tests could not be run".to_string(), brief))
}

fn slashed(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").trim_start_matches("//?/").to_string()
}

/// Copies the package in `from` to `to`, without what a build makes. A path dependency outside
/// the package keeps its place: its path in the copy's manifest is absolute.
fn copy_package(root: &Path, from: &Path, to: &Path) -> Result<(), String> {
    let fail = |p: &Path, e: std::io::Error| format!("cannot copy `{}`: {e}", p.display());
    fs::create_dir_all(to).map_err(|e| fail(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| fail(from, e))? {
        let entry = entry.map_err(|e| fail(from, e))?;
        let (path, name) = (entry.path(), entry.file_name());
        let kind = entry.file_type().map_err(|e| fail(&path, e))?;
        if kind.is_dir() {
            if !matches!(name.to_string_lossy().as_ref(), "build" | "target" | ".git") {
                copy_package(root, &path, &to.join(&name))?;
            }
        } else if name == "Gardel.toml" {
            let text = fs::read_to_string(&path).map_err(|e| fail(&path, e))?;
            let text = absolute_dependencies(root, from, &text)?;
            fs::write(to.join(&name), text).map_err(|e| fail(&path, e))?;
        } else if kind.is_file() {
            fs::copy(&path, to.join(&name)).map_err(|e| fail(&path, e))?;
        }
    }
    Ok(())
}

/// The manifest in `dir` with an absolute path for each dependency outside `root`.
fn absolute_dependencies(root: &Path, dir: &Path, text: &str) -> Result<String, String> {
    let (manifest, _) = crate::manifest::resolve_manifest_project_dir(dir).map_err(|e| e.message)?;
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for d in &manifest.dependencies {
        let target = dir.join(&d.path);
        let target = fs::canonicalize(&target).map_err(|e| format!("`{}`: {e}", target.display()))?;
        if target.starts_with(root) {
            continue;
        }
        let line = lines.get_mut(d.line - 1).ok_or("a dependency with no line")?;
        let quoted = format!("\"{}\"", d.path);
        if !line.contains(&quoted) {
            return Err(format!("`{}` line {}: the path `{}` is not written as it is read", dir.join("Gardel.toml").display(), d.line, d.path));
        }
        *line = line.replacen(&quoted, &format!("\"{}\"", slashed(&target)), 1);
    }
    Ok(lines.join("\n") + "\n")
}

/// A private copy of the package, removed when dropped.
struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(package: &Path, number: usize) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("gardel_mutate_{}_{number}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        let copy = Self { dir };
        copy_package(package, package, &copy.dir)?;
        Ok(copy)
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

struct Run<'a> {
    opts: &'a Options,
    gardelc: PathBuf,
    has_tests: bool,
}

impl Run<'_> {
    fn gardelc(&self, dir: &Path, args: &[&str], limit: Duration) -> Result<Option<(bool, String)>, String> {
        let mut cmd = Command::new(&self.gardelc);
        cmd.current_dir(dir).args(args).args(&self.opts.build_flags).stdin(Stdio::null());
        let out = crate::output_within(&mut cmd, limit).map_err(|e| format!("gardelc could not be started: {e}"))?;
        Ok(out.map(|o| {
            let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            (o.status.success(), text)
        }))
    }

    /// Verifies the package in `dir`, `module` first, and then runs its tests.
    fn try_package(&self, dir: &Path, module: &str, limit: Duration) -> Result<(Outcome, String), String> {
        let stopped = |what: &str| format!("{what} stopped after {} s", limit.as_secs());
        match self.gardelc(dir, &["--project", ".", "--strict", "--fail-fast", module], limit)? {
            None => return Ok((Outcome::GaveUp(stopped("the verifier was")), String::new())),
            Some((false, text)) => return Ok((verdict(text.lines()), text)),
            Some((true, _)) => {}
        }
        if !self.has_tests {
            return Ok((Outcome::Survived, String::new()));
        }
        match self.gardelc(dir, &["test", "."], limit)? {
            None => Ok((Outcome::Test(stopped("the tests were")), String::new())),
            Some((false, text)) => Ok((test_verdict(&text), text)),
            Some((true, _)) => Ok((Outcome::Survived, String::new())),
        }
    }

    fn try_mutant(&self, copy: &Copy, m: &Mutant) -> Result<Outcome, String> {
        let path = copy.dir.join(&m.file);
        let original = fs::read_to_string(&path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
        let put = |text: &str| fs::write(&path, text).map_err(|e| format!("cannot write `{}`: {e}", path.display()));
        put(&apply(&original, m))?;
        let outcome = self.try_package(&copy.dir, &m.module, self.opts.time_limit);
        put(&original)?;
        outcome.map(|(o, _)| o)
    }
}

fn seconds(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

fn show(m: &Mutant) -> String {
    format!("{}:{} {}.{}: {}", slashed(&m.file), m.line, m.module, m.place, m.what)
}

/// The mutants of the package's own files, of the modules asked for.
fn find_mutants(package: &Path, only: &[String]) -> Result<Vec<Mutant>, String> {
    let packages = crate::resolve_package_inputs(package).map_err(|e| e.message)?;
    let (_, files) = packages.last().ok_or("the package has no sources")?;
    let mut out = Vec::new();
    let mut modules = HashSet::new();
    for file in files {
        let path = fs::canonicalize(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
        let relative = path.strip_prefix(package).map_err(|_| format!("`{file}` is outside the package"))?.to_path_buf();
        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read `{file}`: {e}"))?;
        let found = mutants_of(&relative, &text).map_err(|e| format!("{file}: {e}"))?;
        let tokens = crate::tokenize(&text).map_err(|e| format!("{file}: {e}"))?;
        let ast = Parser::new(tokens).parse_file().map_err(|e| format!("{file}: {e}"))?;
        modules.extend(ast.modules.iter().map(|m| m.name.clone()));
        out.extend(found);
    }
    if let Some(name) = only.iter().find(|n| !modules.contains(*n)) {
        return Err(format!("the package has no module `{name}`"));
    }
    out.retain(|m| only.is_empty() || only.contains(&m.module));
    number_keys(&mut out);
    Ok(out)
}

/// `gardelc mutate`: the exit status is 0 when no mutant survived, 1 when one did or the
/// package itself does not pass, and 2 for a wrong command line.
pub(crate) fn run(args: &[String]) -> i32 {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    if !opts.package.join("Gardel.toml").is_file() {
        return usage(&format!("`{}` has no Gardel.toml, so it is not a package", opts.package.display()));
    }
    match mutate(&opts) {
        Ok(survivors) => i32::from(survivors > 0),
        Err(e) => {
            eprintln!("[ERROR] gardelc mutate: {e}");
            1
        }
    }
}

/// Tries the mutants and returns how many survived.
fn mutate(opts: &Options) -> Result<usize, String> {
    let package = fs::canonicalize(&opts.package).map_err(|e| format!("`{}`: {e}", opts.package.display()))?;
    // With `--only`, a mark for another module is not stale.
    let every = find_mutants(&package, &[])?;
    let marked = equivalents(&package)?;
    for line in marked.iter().filter(|l| !every.iter().any(|m| m.key == **l)) {
        println!("[mutate] {EQUIVALENT_FILE}: no mutant is `{line}`");
    }
    let mut mutants = find_mutants(&package, &opts.only)?;
    let total = mutants.len();
    mutants.retain(|m| !marked.contains(&m.key));
    let equivalent = total - mutants.len();
    let all = mutants.len();
    if let Some(n) = opts.limit.filter(|n| *n < all) {
        let chosen = choose(all, n, opts.seed);
        mutants = chosen.into_iter().map(|i| mutants[i].clone()).collect();
        println!("[mutate] {n} of {all} mutants, chosen by seed {}", opts.seed);
    } else {
        println!("[mutate] {all} mutants");
    }
    if equivalent > 0 {
        println!("[mutate] {equivalent} marked equivalent in {EQUIVALENT_FILE} and not tried");
    }
    if opts.list {
        for m in &mutants {
            println!("{}\n    {}", show(m), m.key);
        }
        return Ok(0);
    }
    if mutants.is_empty() {
        return Ok(0);
    }

    let run = Run {
        opts,
        gardelc: std::env::current_exe().map_err(|e| format!("cannot find this gardelc: {e}"))?,
        has_tests: package.join("tests/Gardel.toml").is_file(),
    };
    let start = Instant::now();
    let copies: Vec<Copy> =
        (0..opts.jobs.min(mutants.len())).map(|i| Copy::new(&package, i)).collect::<Result<_, _>>()?;

    // Nothing can be said of a change to a package that does not pass as it is. Its modules may
    // have to be proven first, so it has no time limit.
    let (outcome, text) = run.try_package(&copies[0].dir, &mutants[0].module, Duration::MAX)?;
    if outcome != Outcome::Survived {
        print!("{text}");
        return Err(format!("the package itself does not pass: {outcome:?}"));
    }
    let tested = if run.has_tests { "verified and tested" } else { "verified; it has no tests package" };
    println!("[mutate] the package as it is: {tested} ({})", seconds(start.elapsed()));

    let next = AtomicUsize::new(0);
    let results: Mutex<BTreeMap<usize, Outcome>> = Mutex::new(BTreeMap::new());
    let failure: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for copy in &copies {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let stopped = failure.lock().map_or(true, |f| f.is_some());
                let Some(m) = mutants.get(i).filter(|_| !stopped) else { break };
                let began = Instant::now();
                match run.try_mutant(copy, m) {
                    Ok(outcome) => {
                        let what = match &outcome {
                            Outcome::Verifier(c) => format!("killed by the verifier: {c}"),
                            Outcome::GaveUp(c) => format!("not proven: {c}"),
                            Outcome::Test(t) => format!("killed by a test: {t}"),
                            Outcome::Survived => "SURVIVED".to_string(),
                            Outcome::Refused(e) => format!("not a program: {e}"),
                        };
                        if let Ok(mut r) = results.lock() {
                            println!("[mutate] {}/{} {} ... {what} ({})", r.len() + 1, mutants.len(), show(m), seconds(began.elapsed()));
                            if outcome == Outcome::Survived {
                                println!("    - {}\n    + {}", m.old, m.new);
                            }
                            r.insert(i, outcome);
                        }
                    }
                    Err(e) => {
                        if let Ok(mut f) = failure.lock() {
                            *f = Some(e);
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap_or_default() {
        return Err(e);
    }
    let results = results.into_inner().map_err(|_| "a worker stopped")?;

    let count = |f: fn(&Outcome) -> bool| results.values().filter(|o| f(o)).count();
    let survivors: Vec<&Mutant> =
        results.iter().filter(|(_, o)| **o == Outcome::Survived).map(|(i, _)| &mutants[*i]).collect();
    if !survivors.is_empty() {
        println!("[mutate] survivors. A test or a contract that tells one from the original kills it; one that means the same goes in {EQUIVALENT_FILE}, as its second line here:");
        for m in &survivors {
            println!("{}\n    {}", show(m), m.key);
        }
    }
    println!(
        "[mutate] {} mutants: {} killed by the verifier, {} not proven (the solver gave up), {} killed by a test, {} survived, {} not a program, {equivalent} marked equivalent ({})",
        results.len(),
        count(|o| matches!(o, Outcome::Verifier(_))),
        count(|o| matches!(o, Outcome::GaveUp(_))),
        count(|o| matches!(o, Outcome::Test(_))),
        survivors.len(),
        count(|o| matches!(o, Outcome::Refused(_))),
        seconds(start.elapsed())
    );
    Ok(survivors.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "module COUNTER

representation {NONE}
  n: NAT

invariant
  bounded: n <= 100

create {ANY}

  make(a: NAT, b: NAT): COUNTER
    require
      fits: a + b <= 100
    do
      let x: NAT = if a < b and True then add(a, b) else 7 end
      check
        here: x >= 0 or a = b
        known: COUNTER.fact(a)
      end
      { n = x }
    ensure
      set: Result.value = a + b
    end

  of(o: OPTION[NAT]): NAT
    do
      match o
        case Some(v) then v
        case None then 0
      end
    end

  fact(a: NAT): BOOL
    require
      small: a < 3
    do
      a < 4
    ensure
      holds: Result
    end

feature {ANY}

  value: NAT
    do
      Current.n
    end

specify {ANY}

  ghost(a: NAT): NAT
    do
      a + 1
    end

  law positive(c: COUNTER, a: NAT)
    require
      small: a < 3
    do
      a < 9
    ensure
      ok: c.value + a >= a
    end

end
";

    fn whats(text: &str) -> Vec<String> {
        mutants_of(Path::new("c.gdl"), text)
            .expect("parses")
            .iter()
            .map(|m| format!("{}: {}", m.place, m.what))
            .collect()
    }

    /// Each operator at each place of code, and none in a proof.
    #[test]
    fn mutants_are_made_of_code_alone() {
        let got = whats(SOURCE);
        let expected = [
            "invariant: `invariant` `bounded` dropped",
            "make: `require` `fits` dropped",
            "make: condition negated: `a < b and True`",
            "make: always `then`: `a < b and True`",
            "make: always `else`: `a < b and True`",
            "make: `<` -> `<=`",
            "make: `and` -> `or`",
            "make: `True` -> `False`",
            "make: arguments swapped: `a` and `b`",
            "make: `add(a, b)` -> `a`",
            "make: `add(a, b)` -> `b`",
            "make: `7` -> `8`",
            "make: `7` -> `6`",
            "of: arm 1 of a `match` gets arm 2's body",
            "of: `0` -> `1`",
            "value: `Current.n` -> `Current`",
            "positive: `require` `small` dropped",
        ];
        let mut sorted = got.clone();
        sorted.sort();
        let mut want: Vec<String> = expected.iter().map(|s| (*s).to_string()).collect();
        want.sort();
        assert_eq!(sorted, want);
    }

    /// Each mutant is still a file that parses, and differs from the original at its place.
    #[test]
    fn a_mutant_changes_its_place_only() {
        for m in mutants_of(Path::new("c.gdl"), SOURCE).expect("parses") {
            let text = apply(SOURCE, &m);
            assert_ne!(text, SOURCE, "{m:?}");
            let tokens = crate::tokenize(&text).expect("lexes");
            assert!(Parser::new(tokens).parse_file().is_ok(), "{m:?}\n{text}");
            assert!(m.key.starts_with(&format!("COUNTER.{}: ", m.place)), "{m:?}");
        }
        let swapped = mutants_of(Path::new("c.gdl"), SOURCE)
            .expect("parses")
            .into_iter()
            .find(|m| m.what.starts_with("arguments swapped"))
            .expect("a swap");
        assert_eq!(swapped.old, "let x: NAT = if a < b and True then add(a, b) else 7 end");
        assert_eq!(swapped.new, "let x: NAT = if a < b and True then add(b, a) else 7 end");
    }

    /// A part that replaces a call keeps its meaning, and a text with letters of more than one
    /// byte is changed in the right place.
    #[test]
    fn a_part_is_parenthesised_and_places_count_chars() {
        let src = "functions F\n\nfeature {ANY}\n\n  -- h\u{e9}llo \u{2192}\n  f(a: NAT): NAT\n    do\n      g(a + 1) * 2\n    end\n\nend\n";
        let m = mutants_of(Path::new("f.gdl"), src)
            .expect("parses")
            .into_iter()
            .find(|m| m.what == "`g(a + 1)` -> `a + 1`")
            .expect("a call replaced");
        assert!(apply(src, &m).contains("      (a + 1) * 2\n"), "{}", apply(src, &m));
    }

    #[test]
    fn keys_are_distinct() {
        let src = "functions F\n\nfeature {ANY}\n\n  f(a: NAT): NAT\n    do\n      let b = g(1)\n      let c = g(1)\n      b + c\n    end\n\nend\n";
        let mut ms = mutants_of(Path::new("f.gdl"), src).expect("parses");
        number_keys(&mut ms);
        let keys: HashSet<&String> = ms.iter().map(|m| &m.key).collect();
        assert_eq!(keys.len(), ms.len(), "{ms:?}");
    }

    /// The same seed chooses the same mutants, each once, and another seed others.
    #[test]
    fn a_choice_is_decided_by_its_seed() {
        let a = choose(100, 10, 7);
        assert_eq!(a, choose(100, 10, 7));
        assert_ne!(a, choose(100, 10, 8));
        assert_eq!(a.iter().collect::<HashSet<_>>().len(), 10);
        assert!(a.iter().all(|i| *i < 100));
        assert_eq!(choose(3, 3, 1), vec![0, 1, 2]);
    }

    #[test]
    fn a_verdict_names_the_clause() {
        let refuted = "      [ERROR] Phase 3 Boogie (Postcondition Violation) in `offered` (def) at ./src/w.gdl:134:7: failed";
        assert_eq!(verdict(refuted.lines()), Outcome::Verifier("`def` of `offered` (Postcondition Violation)".to_string()));
        let gave_up = "[ERROR] Phase 3 Boogie (Verification Resource Limit) in `f` (t) at ./a.gdl:1:1: gave up";
        assert_eq!(verdict(gave_up.lines()), Outcome::GaveUp("`t` of `f` (Verification Resource Limit)".to_string()));
        // A refutation says more than a proof not found.
        let both = format!("{gave_up}\n{refuted}");
        assert!(matches!(verdict(both.lines()), Outcome::Verifier(_)));
        let front = "[ERROR] E0108: unknown_routine at ./src/arr.gdl:44:72: no routine";
        assert!(matches!(verdict(front.lines()), Outcome::Refused(_)));
        let failed = "[test] `tests` verified: 1\n[test] A_TEST ... ok (0.1 s)\n[test] B_TEST ... FAILED (exit 1, 0.2 s)\n    | FAIL twice: expected 5, got 4\n";
        assert_eq!(test_verdict(failed), Outcome::Test("B_TEST: FAIL twice: expected 5, got 4".to_string()));
    }

    #[test]
    fn a_wrong_command_line_is_refused() {
        let words = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        for bad in ["--nope", "a b", "--limit", "--limit x", "--jobs 0", "--only"] {
            assert!(parse(&words(bad)).is_err(), "{bad}");
        }
        let o = parse(&words("pkg --only A --only B --limit 3 --seed 9 --list --fuel 2")).expect("parses");
        assert_eq!((o.only.len(), o.limit, o.seed, o.list), (2, Some(3), 9, true));
        assert_eq!(o.build_flags, vec!["--fuel", "2"]);
    }
}
