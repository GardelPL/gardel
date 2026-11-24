//! `gardelc test` (Task H.9, decision 10): a package's `tests/` directory is a client package,
//! and each `program` in it is a test. The command verifies and builds that package once, so
//! the package and its tests share one proof cache and one build, then runs each program and
//! reports it by its exit status.
//!
//! A program gets `tests/cases.txt` on its standard input when there is one, and runs in
//! `tests/`. Its output is shown only when it fails.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a test program may run before it is stopped and counted as failed.
const TIME_LIMIT: Duration = Duration::from_secs(600);

/// Flags passed on to the build, each with its value.
const BUILD_VALUE_FLAGS: &[&str] = &["--fuel", "--z3-seed", "--z3-rlimit"];
const BUILD_PLAIN_FLAGS: &[&str] = &["--verify-deps"];

struct Options {
    package: PathBuf,
    filter: Option<String>,
    build_flags: Vec<String>,
}

fn usage(msg: &str) -> i32 {
    eprintln!("[ERROR] gardelc test: {msg}");
    eprintln!("Usage: gardelc test [DIR] [--filter <NAME>] [--fuel <N>] [--z3-seed <N>] [--z3-rlimit <N>] [--verify-deps]");
    eprintln!("  DIR is the package (default `.`); its tests are the programs of the package in DIR/tests.");
    eprintln!("  --filter <NAME>  run only the programs whose name contains NAME, ignoring case");
    2
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut package = None;
    let mut filter = None;
    let mut build_flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let value = || args.get(i + 1).filter(|v| !v.starts_with("--")).cloned();
        if a == "--filter" {
            filter = Some(value().ok_or("--filter: expected a program name")?.to_lowercase());
            i += 2;
        } else if BUILD_VALUE_FLAGS.contains(&a) {
            let v = value().ok_or(format!("{a}: expected a value"))?;
            build_flags.extend([a.to_string(), v]);
            i += 2;
        } else if BUILD_PLAIN_FLAGS.contains(&a) {
            build_flags.push(a.to_string());
            i += 1;
        } else if a.starts_with("--") {
            return Err(format!("unknown flag `{a}`"));
        } else if package.is_some() {
            return Err(format!("one package at a time: `{a}` is a second"));
        } else {
            package = Some(PathBuf::from(a));
            i += 1;
        }
    }
    Ok(Options { package: package.unwrap_or_else(|| PathBuf::from(".")), filter, build_flags })
}

/// The entry points of the root package in `tests`, in the order of its files.
fn programs(tests: &Path) -> Result<Vec<String>, String> {
    let packages = crate::resolve_package_inputs(tests).map_err(|e| e.message)?;
    let (_, files) = packages.last().ok_or("the tests package has no sources")?;
    let mut names = Vec::new();
    for file in files {
        let src = fs::read_to_string(file).map_err(|e| format!("cannot read `{file}`: {e}"))?;
        let tokens = crate::tokenize(&src).map_err(|e| format!("{file}: {e}"))?;
        let ast = crate::Parser::new(tokens).parse_file().map_err(|e| format!("{file}: {e}"))?;
        names.extend(ast.modules.iter().filter(|m| m.kind.is_entry_point()).map(|m| m.name.clone()));
    }
    Ok(names)
}

fn seconds(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64())
}

/// Indents captured output under a failure, at most `limit` lines of it.
fn quote(text: &str, limit: usize) {
    let lines: Vec<&str> = text.lines().collect();
    for line in lines.iter().take(limit) {
        println!("    | {line}");
    }
    if lines.len() > limit {
        println!("    | ... ({} more lines)", lines.len() - limit);
    }
}

/// Reads a child's stream on a thread of its own, so a full pipe never blocks it.
fn drain(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(mut s) = stream {
            drop(s.read_to_end(&mut out));
        }
        out
    })
}

enum Outcome {
    Exited(Option<i32>),
    TimedOut,
}

/// Runs a test program in `dir` with `input` on its standard input.
fn run_program(exe: &Path, dir: &Path, input: Option<&Path>) -> std::io::Result<(Outcome, String)> {
    let stdin = match input {
        Some(p) => Stdio::from(fs::File::open(p)?),
        None => Stdio::null(),
    };
    let mut child = Command::new(exe)
        .current_dir(dir)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    let outcome = loop {
        if let Some(status) = child.try_wait()? {
            break Outcome::Exited(status.code());
        }
        if start.elapsed() > TIME_LIMIT {
            drop(child.kill());
            drop(child.wait());
            break Outcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut text = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    text.push_str(&String::from_utf8_lossy(&err.join().unwrap_or_default()));
    Ok((outcome, text))
}

/// `gardelc test`: the exit status is 0 when every program passed, 1 when one failed or the
/// tests did not verify or build, and 2 for a wrong command line.
pub(crate) fn run(args: &[String]) -> i32 {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    if !opts.package.join("Gardel.toml").is_file() {
        return usage(&format!("`{}` has no Gardel.toml, so it is not a package", opts.package.display()));
    }
    let tests = if opts.package == Path::new(".") { PathBuf::from("tests") } else { opts.package.join("tests") };
    if !tests.join("Gardel.toml").is_file() {
        eprintln!(
            "[ERROR] gardelc test: `{}` has no tests package: its tests are a package in `{}`, with a Gardel.toml that depends on it",
            opts.package.display(),
            tests.display()
        );
        return 1;
    }
    let all = match programs(&tests) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[ERROR] gardelc test: {e}");
            return 1;
        }
    };
    let chosen: Vec<&String> =
        all.iter().filter(|n| opts.filter.as_ref().is_none_or(|f| n.to_lowercase().contains(f))).collect();
    if chosen.is_empty() {
        let why = match &opts.filter {
            Some(f) => format!("no program of `{}` matches `{f}`", tests.display()),
            None => format!("`{}` has no program", tests.display()),
        };
        eprintln!("[ERROR] gardelc test: {why}");
        return 1;
    }

    let start = Instant::now();
    let me = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[ERROR] gardelc test: cannot find this gardelc: {e}");
            return 1;
        }
    };
    let build = Command::new(&me)
        .current_dir(&tests)
        .args(["--project", ".", "--strict"])
        .args(&opts.build_flags)
        .stdin(Stdio::null())
        .output();
    let build = match build {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] gardelc test: the build could not start: {e}");
            return 1;
        }
    };
    let text = format!("{}{}", String::from_utf8_lossy(&build.stdout), String::from_utf8_lossy(&build.stderr));
    if !build.status.success() {
        println!("[test] `{}` did not verify or build:", tests.display());
        quote(&text, 200);
        return 1;
    }
    let cache = text.lines().find_map(|l| l.trim().strip_prefix("[cache] ")).unwrap_or("verified");
    println!("[test] `{}` verified: {cache} ({})", tests.display(), seconds(start.elapsed()));

    let input = tests.join("cases.txt");
    let input = input.is_file().then_some(input.as_path());
    let mut failed = Vec::new();
    for name in &chosen {
        let stem = name.to_lowercase();
        let c = tests.join(format!("build/{stem}.c"));
        let exe = tests.join(format!("build/{stem}_bin{}", std::env::consts::EXE_SUFFIX));
        let (c_s, exe_s) = (c.to_string_lossy().into_owned(), exe.to_string_lossy().into_owned());
        if !crate::executable_is_current(&c_s, &exe_s) {
            match crate::compile_program_c(&c_s, &exe_s) {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    println!("[test] {name} ... not built: gcc failed on `{c_s}`");
                    quote(&String::from_utf8_lossy(&out.stderr), 20);
                    failed.push(name.as_str());
                    continue;
                }
                Err(e) => {
                    println!("[test] {name} ... not built: gcc could not be started ({e})");
                    failed.push(name.as_str());
                    continue;
                }
            }
        }
        let began = Instant::now();
        match run_program(&exe, &tests, input) {
            Ok((Outcome::Exited(Some(0)), _)) => println!("[test] {name} ... ok ({})", seconds(began.elapsed())),
            Ok((outcome, output)) => {
                let why = match outcome {
                    Outcome::Exited(Some(code)) => format!("exit {code}"),
                    Outcome::Exited(None) => "stopped by a signal".to_string(),
                    Outcome::TimedOut => format!("stopped after {}", seconds(TIME_LIMIT)),
                };
                println!("[test] {name} ... FAILED ({why}, {})", seconds(began.elapsed()));
                quote(&output, 60);
                failed.push(name.as_str());
            }
            Err(e) => {
                println!("[test] {name} ... FAILED: it could not be started ({e})");
                failed.push(name.as_str());
            }
        }
    }
    let passed = chosen.len() - failed.len();
    println!(
        "[test] {} program{}: {passed} passed, {} failed ({})",
        chosen.len(),
        if chosen.len() == 1 { "" } else { "s" },
        failed.len(),
        seconds(start.elapsed())
    );
    i32::from(!failed.is_empty())
}
