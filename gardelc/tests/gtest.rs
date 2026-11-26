//! `gardelc test` and the `gtest` package (Task H.9). The command verifies and builds a
//! package's `tests/` package once, runs each program in it, with `tests/cases.txt` on its
//! standard input, and reports each by its exit status. `gtest`'s own tests run through it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn slashed(p: &Path) -> String {
    let p = p.canonicalize().expect("path exists");
    p.to_string_lossy().replace('\\', "/").replace("//?/", "")
}

/// A cache for the tests of this file, so `gtest` is proven once.
fn cache() -> PathBuf {
    std::env::temp_dir().join(format!("gardelc_gtest_{}_cache", std::process::id()))
}

fn gardelc(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(dir)
        .env("GARDEL_CACHE_DIR", cache())
        .args(args)
        .output()
        .expect("gardelc runs")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// A package with a `tests/` package that depends on it and on `gtest`.
struct Package {
    dir: PathBuf,
}

impl Package {
    fn new(name: &str, sources: &[(&str, &str)], tests: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_gtest_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        fs::create_dir_all(dir.join("tests/src")).expect("temporary directory");
        fs::write(dir.join("Gardel.toml"), format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n")).expect("manifest");
        let gtest = slashed(&root().join("gtest"));
        fs::write(
            dir.join("tests/Gardel.toml"),
            format!("[project]\nname = \"{name}-tests\"\nversion = \"0.1.0\"\n\n[dependencies]\n{name} = {{ path = \"..\" }}\ngtest = {{ path = \"{gtest}\" }}\n"),
        )
        .expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source");
        }
        for (file, text) in tests {
            fs::write(dir.join("tests").join(file), text).expect("test source");
        }
        Self { dir }
    }

    fn test(&self, args: &[&str]) -> Output {
        let mut full = vec!["test", "."];
        full.extend_from_slice(args);
        gardelc(&self.dir, &full)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

const DOUBLE: &str = "functions DOUBLE

feature {ANY}

  twice(n: NAT): NAT
    require
      small: n <= 1000
    do
      n + n
    ensure
      doubled: Result = 2 * n
    end

end
";

/// A test program named `name` that checks `checks`, a list of `TEST_RESULT`s.
fn program(name: &str, checks: &str) -> String {
    format!(
        "program {name}

feature {{ANY}}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let rs = {checks}
      with IO
        IO.print_line(TESTS.report(rs))
        IO.exit(TESTS.exit_code(rs))
      end
    end

end
"
    )
}

/// `gtest`'s own tests: its checks and runner, `CASES` on a `cases.txt`, SplitMix64 against
/// its reference outputs, generators and properties, and a scripted step.
#[test]
fn gtest_passes_its_own_tests() {
    let out = gardelc(&root().join("gtest"), &["test"]);
    assert!(out.status.success(), "{}", all(&out));
    for name in ["GTEST_CASES", "GTEST_MORE", "GTEST_SELF"] {
        assert!(all(&out).contains(&format!("[test] {name} ... ok")), "{}", all(&out));
    }
    assert!(all(&out).contains("3 programs: 3 passed, 0 failed"), "{}", all(&out));
}

/// Each program of `tests/` is built and run, not only the one a build would pick. A failing
/// check fails its program, which prints its name and what was expected, and the command
/// exits with 1. `--filter` runs only the programs it names.
#[test]
fn every_program_runs_and_a_failure_is_reported() {
    let good = program("GOOD_TEST", "LIST.empty.prepend(CHECK.equal_nat(\"twice\", DOUBLE.twice(21), 42))");
    let bad = program("BAD_TEST", "LIST.empty.prepend(CHECK.equal_nat(\"wrong\", DOUBLE.twice(2), 5))");
    let p = Package::new("double", &[("double.gdl", DOUBLE)], &[("src/good_test.gdl", &good), ("src/bad_test.gdl", &bad)]);

    let out = p.test(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", all(&out));
    let text = all(&out);
    assert!(text.contains("[test] GOOD_TEST ... ok"), "{text}");
    assert!(text.contains("[test] BAD_TEST ... FAILED (exit 1"), "{text}");
    assert!(text.contains("FAIL wrong: expected 5, got 4"), "{text}");
    assert!(text.contains("2 programs: 1 passed, 1 failed"), "{text}");

    let only = p.test(&["--filter", "good"]);
    assert!(only.status.success(), "{}", all(&only));
    assert!(!all(&only).contains("BAD_TEST"), "{}", all(&only));
    assert!(all(&only).contains("1 program: 1 passed, 0 failed"), "{}", all(&only));

    let none = p.test(&["--filter", "nothing"]);
    assert_eq!(none.status.code(), Some(1), "{}", all(&none));
    assert!(all(&none).contains("no program of"), "{}", all(&none));
}

/// A program gets `tests/cases.txt` on its standard input, and runs in `tests/`.
#[test]
fn a_program_reads_the_cases_file() {
    let cases = "program CASES_TEST

feature {ANY}

  finish(rs: LIST[TEST_RESULT]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(TESTS.report(rs))
        IO.exit(TESTS.exit_code(rs))
      end
    end

  run_cases(n: NAT, rs: LIST[TEST_RESULT]): IO[EXIT_CODE]
    require
      bounded: n <= 1000
    variant
      1000 - n
    do
      with IO
        line_res <- IO.read_line
        match line_res
          case Ok(line) then
            if n < 1000 then
              run_cases(n + 1, CASES.check(rs, n, line, fn(i: STRING): STRING do
                match NUMBER_TEXT.parse_nat(i)
                  case Ok(k) then if k <= 1000 then NUMBER_TEXT.nat_to_text(DOUBLE.twice(k)) else \"big\" end
                  case Error(_) then \"not a number\"
                end
              end))
            else
              finish(rs)
            end
          case Error(_) then
            finish(rs)
        end
      end
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      run_cases(1, LIST.empty)
    end

end
";
    let p = Package::new(
        "cased",
        &[("double.gdl", DOUBLE)],
        &[("src/cases_test.gdl", cases), ("cases.txt", "# doubles\n2 => 4\n\n21 => 42\nx => not a number\n")],
    );
    let out = p.test(&[]);
    assert!(out.status.success(), "{}", all(&out));

    fs::write(p.dir.join("tests/cases.txt"), "2 => 4\n3 => 7\n").expect("cases");
    let out = p.test(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", all(&out));
    assert!(all(&out).contains("FAIL line 2: 3: expected \"7\", got \"6\""), "{}", all(&out));
}

/// A reactive program runs from the script in its `.events` file, whose lines that start with
/// `#` are skipped. A line that starts with `> ` is a `Line` with the rest of it, as it is.
#[test]
fn a_reactive_program_runs_from_its_events_file() {
    let source = "reactive program CONNS_TEST

feature {ANY}

  init(args: SEQ[STRING]): STEP[LIST[CONN_ID]]
    do
      Continue(LIST.empty, LIST.empty)
    end

  step(cs: LIST[CONN_ID], e: EVENT): STEP[LIST[CONN_ID]]
    do
      match e
        case Accepted(c) then Continue(cs.append(LIST.singleton(c)), LIST.empty)
        case Line(Ok(t)) then if t.equals(\"50% of  it, café\") then Continue(cs, LIST.empty) else Stop(3, LIST.empty.prepend(Print(t))) end
        case Shutdown then
          let rs = LIST.empty.prepend(CHECK.equal_nat(\"connections\", cs.length, 3))
          Stop(TESTS.exit_code(rs), LIST.empty.prepend(Print(TESTS.report(rs))))
        case _ then Continue(cs, LIST.empty)
      end
    end

end
";
    let p = Package::new(
        "events",
        &[("double.gdl", DOUBLE)],
        &[("src/conns_test.gdl", source), ("conns_test.events", "# three\nAccepted:1 Accepted:2\n  # not this: Accepted:9\n> 50% of  it, café\nAccepted:3\nShutdown\n")],
    );
    let out = p.test(&[]);
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("[test] CONNS_TEST ... ok"), "{}", all(&out));

    fs::write(p.dir.join("tests/conns_test.events"), "Accepted:1").expect("events");
    let out = p.test(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", all(&out));
    assert!(all(&out).contains("FAIL connections: expected 3, got 1"), "{}", all(&out));
}

/// A second run with nothing changed proves nothing again and compiles nothing again: every
/// proof comes from the cache, and the executable is up to date. A change to a test is built
/// again.
#[test]
fn a_second_run_reuses_proofs_and_executables() {
    let good = program("FAST_TEST", "LIST.empty.prepend(CHECK.equal_nat(\"twice\", DOUBLE.twice(4), 8))");
    let p = Package::new("fast", &[("double.gdl", DOUBLE)], &[("src/fast_test.gdl", &good)]);
    let first = p.test(&[]);
    assert!(first.status.success(), "{}", all(&first));
    let exe = p.dir.join(format!("tests/build/fast_test_bin{}", std::env::consts::EXE_SUFFIX));
    let built = fs::metadata(&exe).and_then(|m| m.modified()).expect("executable");

    let start = Instant::now();
    let second = p.test(&[]);
    let took = start.elapsed();
    assert!(second.status.success(), "{}", all(&second));
    assert!(all(&second).contains("verified: 0 modules verified now"), "{}", all(&second));
    assert_eq!(fs::metadata(&exe).and_then(|m| m.modified()).expect("executable"), built, "not compiled again");
    println!("second run: {took:?}");

    let build = gardelc(&p.dir.join("tests"), &["--project", ".", "--strict"]);
    assert!(all(&build).contains("Native executable up to date"), "{}", all(&build));

    let changed = program("FAST_TEST", "LIST.empty.prepend(CHECK.equal_nat(\"twice\", DOUBLE.twice(5), 10))");
    fs::write(p.dir.join("tests/src/fast_test.gdl"), changed).expect("source");
    let build = gardelc(&p.dir.join("tests"), &["--project", ".", "--strict"]);
    assert!(all(&build).contains("Compiled native executable"), "{}", all(&build));
}

/// A test's checks are written `let rs = LIST.empty.prepend(..)`: the verifier types that
/// list by what is put in it, and a pair by its parts. Both were `int` before, which Boogie
/// refused (E0199).
#[test]
fn a_let_of_a_new_list_or_a_pair_is_typed() {
    let src = "functions TYPED

feature {ANY}

  pair(s: U64): TUPLE[U64, U64]
    do
      let r = s +% 1
      let out = (r, s)
      out
    ensure
      first: Result.first = s +% 1
    end

  count(n: NAT): NAT
    do
      let xs = LIST.empty.prepend(n).prepend(3)
      let ys = LIST.empty.append(xs)
      ys.length
    ensure
      two: Result = 2
    end

end
";
    let dir = std::env::temp_dir().join(format!("gardelc_gtest_{}_typed", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("temporary directory");
    fs::write(dir.join("typed.gdl"), src).expect("source");
    let out = gardelc(&dir, &["typed.gdl", "--strict"]);
    drop(fs::remove_dir_all(&dir));
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
}

#[test]
fn a_wrong_command_line_is_refused() {
    let out = gardelc(&root(), &["test", "--nope"]);
    assert_eq!(out.status.code(), Some(2), "{}", all(&out));
    assert!(all(&out).contains("unknown flag `--nope`"), "{}", all(&out));

    let out = gardelc(&root(), &["test", "spec"]);
    assert_eq!(out.status.code(), Some(2), "{}", all(&out));
    assert!(all(&out).contains("has no Gardel.toml"), "{}", all(&out));

    let p = Package::new("untested", &[("double.gdl", DOUBLE)], &[]);
    fs::remove_dir_all(p.dir.join("tests")).expect("no tests");
    let out = p.test(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", all(&out));
    assert!(all(&out).contains("has no tests package"), "{}", all(&out));
}
