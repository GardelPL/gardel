//! `gardelc mutate` (Task H.10): a package's code is changed in small ways, one change at a time
//! in a private copy, and each change is reported by what refused it, the verifier or a test,
//! or as a survivor. Also `--fail-fast`, which it verifies with.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn slashed(p: &Path) -> String {
    let p = p.canonicalize().expect("path exists");
    p.to_string_lossy().replace('\\', "/").replace("//?/", "")
}

/// A cache for the tests of this file, so `gtest` is proven once.
fn cache() -> PathBuf {
    std::env::temp_dir().join(format!("gardelc_mutate_{}_cache", std::process::id()))
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

/// A directory of packages, each with a manifest and its files.
struct Dir {
    dir: PathBuf,
}

impl Dir {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_mutate_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        for (file, text) in files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().expect("a parent")).expect("temporary directory");
            fs::write(path, text).expect("source");
        }
        Self { dir }
    }

    fn mutate(&self, package: &str, args: &[&str]) -> Output {
        let mut full = vec!["mutate", package];
        full.extend_from_slice(args);
        gardelc(&self.dir, &full)
    }

    /// Every file under the directory and its bytes.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in fs::read_dir(dir).expect("a directory") {
                let path = e.expect("an entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.insert(path.clone(), fs::read(&path).expect("a file"));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.dir, &mut out);
        out
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn manifest(name: &str, dependencies: &[(&str, &str)]) -> String {
    let mut text = format!("[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n");
    if !dependencies.is_empty() {
        text.push_str("\n[dependencies]\n");
        for (dep, path) in dependencies {
            text.push_str(&format!("{dep} = {{ path = \"{path}\" }}\n"));
        }
    }
    text
}

/// `doubled` is proven, `fee` is only tested, and `label` is neither.
const PRICE: &str = "functions PRICE

feature {ANY}

  doubled(n: NAT): NAT
    require
      small: n <= LIMITS.most
    do
      n + n
    ensure
      twice: Result = 2 * n
    end

  fee(n: NAT): NAT
    do
      if n > 50 then 0 else n end
    end

  label(n: NAT): NAT
    do
      5
    end

end
";

const LIMITS: &str = "functions LIMITS

feature {ANY}

  most: NAT
    do
      1000
    ensure
      def: Result = 1000
    end

end
";

const PRICE_TEST: &str = "program PRICE_TEST

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let rs = LIST.empty.prepend(CHECK.equal_nat(\"free\", PRICE.fee(51), 0)).prepend(CHECK.equal_nat(\"paid\", PRICE.fee(50), 50))
      with IO
        IO.print_line(TESTS.report(rs))
        IO.exit(TESTS.exit_code(rs))
      end
    end

end
";

/// The line of the report for the mutant described by `what`.
fn line_of<'a>(text: &'a str, what: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with("[mutate] ") && l.contains(what))
        .unwrap_or_else(|| panic!("no mutant `{what}`:\n{text}"))
}

/// Each mutant is reported by what refused it. The package depends on one beside it and its
/// tests on `gtest`, so the copy keeps dependencies that are outside the package. The package
/// is left as it was, with nothing built in it.
#[test]
fn a_mutant_is_killed_by_a_proof_or_by_a_test_or_survives() {
    let gtest = slashed(&root().join("gtest"));
    let d = Dir::new(
        "kinds",
        &[
            ("limits/Gardel.toml", &manifest("limits", &[])),
            ("limits/src/limits.gdl", LIMITS),
            ("price/Gardel.toml", &manifest("price", &[("limits", "../limits")])),
            ("price/src/price.gdl", PRICE),
            ("price/tests/Gardel.toml", &manifest("price-tests", &[("price", ".."), ("gtest", &gtest)])),
            ("price/tests/src/price_test.gdl", PRICE_TEST),
        ],
    );
    let before = d.files();
    let out = d.mutate("price", &["--jobs", "2"]);
    let text = all(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("the package as it is: verified and tested"), "{text}");

    let dropped = line_of(&text, "PRICE.doubled: `require` `small` dropped");
    assert!(dropped.contains("killed by the verifier: `overflow_add` of `doubled`"), "{dropped}");
    let tested = line_of(&text, "PRICE.fee: `>` -> `>=`");
    assert!(tested.contains("killed by a test: PRICE_TEST: FAIL paid: expected 50, got 0"), "{tested}");
    // `0` is not found by a proof: the test that expects it is the one that fails.
    let zero = line_of(&text, "PRICE.fee: `0` -> `1`");
    assert!(zero.contains("killed by a test: PRICE_TEST: FAIL free: expected 0, got 1"), "{zero}");
    for (what, new) in [("`5` -> `6`", "6"), ("`5` -> `4`", "4")] {
        let survivor = line_of(&text, &format!("PRICE.label: {what}"));
        assert!(survivor.contains("SURVIVED"), "{survivor}");
        assert!(text.contains(&format!("    - 5\n    + {new}\n")), "{text}");
        assert!(text.contains(&format!("    PRICE.label: 5 => {new}\n")), "{text}");
    }
    assert!(text.contains(" 2 survived, "), "{text}");
    assert!(d.files() == before, "the package changed");
    let left = fs::read_dir(std::env::temp_dir())
        .expect("the temporary directory")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("gardel_mutate_"))
        .map(|e| e.path().join("src/price.gdl"))
        .find(|p| p.is_file());
    assert_eq!(left, None, "a copy was left behind");
}

/// A package with no tests is verified only. A mutant marked in `Gardel.mutants` is not tried,
/// and a mark that fits no mutant is reported.
#[test]
fn a_mutant_marked_equivalent_is_not_tried() {
    let src = "functions ONE\n\nfeature {ANY}\n\n  one(n: NAT): NAT\n    do\n      5\n    end\n\nend\n";
    let d = Dir::new(
        "marked",
        &[
            ("p/Gardel.toml", &manifest("p", &[])),
            ("p/src/one.gdl", src),
            ("p/Gardel.mutants", "-- Any number will do.\nONE.one: 5 => 6\n\nONE.one: 9 => 8\n"),
        ],
    );
    let out = d.mutate("p", &[]);
    let text = all(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("verified; it has no tests package"), "{text}");
    assert!(text.contains("no mutant is `ONE.one: 9 => 8`"), "{text}");
    assert!(text.contains("1 marked equivalent in Gardel.mutants and not tried"), "{text}");
    assert!(!text.contains("`5` -> `6`"), "{text}");
    assert!(text.contains(" 1 survived, "), "{text}");

    fs::write(d.dir.join("p/Gardel.mutants"), "ONE.one: 5 => 6\r\nONE.one: 5 => 4\r\n").expect("marks");
    let out = d.mutate("p", &[]);
    assert_eq!(out.status.code(), Some(0), "{}", all(&out));
    assert!(all(&out).contains("2 marked equivalent"), "{}", all(&out));
}

/// `--only`, `--limit` and `--seed` choose mutants, and `--list` tries none.
#[test]
fn mutants_are_listed_and_chosen() {
    let d = Dir::new(
        "chosen",
        &[
            ("p/Gardel.toml", &manifest("p", &[("limits", "../limits")])),
            ("p/src/price.gdl", PRICE),
            ("p/src/two.gdl", "functions TWO\n\nfeature {ANY}\n\n  two: NAT\n    do\n      2\n    end\n\nend\n"),
            ("limits/Gardel.toml", &manifest("limits", &[])),
            ("limits/src/limits.gdl", LIMITS),
        ],
    );
    let list = |args: &[&str]| {
        let mut full = vec!["--list"];
        full.extend_from_slice(args);
        let out = d.mutate("p", &full);
        assert_eq!(out.status.code(), Some(0), "{}", all(&out));
        all(&out)
    };
    let every = list(&[]);
    assert!(every.contains("src/two.gdl:7 TWO.two: `2` -> `3`\n    TWO.two: 2 => 3\n"), "{every}");
    assert!(every.contains("PRICE.fee: condition negated: `n > 50`"), "{every}");
    // A dependency's code is not the package's to change.
    assert!(!every.contains("limits.gdl"), "{every}");
    let two = list(&["--only", "TWO"]);
    assert!(two.contains("[mutate] 2 mutants") && !two.contains("PRICE"), "{two}");
    let some = list(&["--limit", "3", "--seed", "5"]);
    assert!(some.contains("3 of ") && some.contains("chosen by seed 5"), "{some}");
    assert_eq!(some.lines().filter(|l| l.starts_with("src/")).count(), 3, "{some}");
    assert_eq!(some, list(&["--limit", "3", "--seed", "5"]));

    for (args, message) in [
        (vec!["--only", "NONE_SUCH"], "the package has no module `NONE_SUCH`"),
        (vec!["--nope"], "unknown flag `--nope`"),
        (vec!["--jobs", "0"], "--jobs: expected at least 1"),
    ] {
        let out = d.mutate("p", &args);
        assert!(matches!(out.status.code(), Some(1 | 2)), "{}", all(&out));
        assert!(all(&out).contains(message), "{}", all(&out));
    }
    let out = d.mutate("nowhere", &[]);
    assert_eq!(out.status.code(), Some(2), "{}", all(&out));
    assert!(all(&out).contains("has no Gardel.toml"), "{}", all(&out));
}

/// Nothing can be said of changes to a package that does not verify as it is.
#[test]
fn a_package_that_does_not_verify_is_not_mutated() {
    let src = "functions ONE\n\nfeature {ANY}\n\n  one: NAT\n    do\n      5\n    ensure\n      six: Result = 6\n    end\n\nend\n";
    let d = Dir::new("wrong", &[("p/Gardel.toml", &manifest("p", &[])), ("p/src/one.gdl", src)]);
    let out = d.mutate("p", &[]);
    assert_eq!(out.status.code(), Some(1), "{}", all(&out));
    assert!(all(&out).contains("the package itself does not pass"), "{}", all(&out));
    assert!(!all(&out).contains("SURVIVED"), "{}", all(&out));
}

/// `--fail-fast M` checks `M` first and stops there when it fails. A package with no error is
/// checked whole.
#[test]
fn fail_fast_stops_at_the_first_module_that_fails() {
    let wrong = |name: &str| {
        format!("functions {name}\n\nfeature {{ANY}}\n\n  one: NAT\n    do\n      5\n    ensure\n      six_{}: Result = 6\n    end\n\nend\n", name.to_lowercase())
    };
    let d = Dir::new(
        "fast",
        &[("p/Gardel.toml", &manifest("p", &[])), ("p/src/aa.gdl", &wrong("AA")), ("p/src/bb.gdl", &wrong("BB"))],
    );
    let p = d.dir.join("p");
    let plain = all(&gardelc(&p, &["--project", ".", "--strict"]));
    assert!(plain.contains("six_aa") && plain.contains("six_bb"), "{plain}");
    let out = gardelc(&p, &["--project", ".", "--strict", "--fail-fast", "BB"]);
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("six_bb") && !all(&out).contains("six_aa"), "{}", all(&out));

    fs::write(p.join("src/bb.gdl"), wrong("BB").replace("= 6", "= 5")).expect("source");
    let out = gardelc(&p, &["--project", ".", "--strict", "--fail-fast", "BB"]);
    assert!(!out.status.success() && all(&out).contains("six_aa"), "{}", all(&out));
    fs::write(p.join("src/aa.gdl"), wrong("AA").replace("= 6", "= 5")).expect("source");
    let out = gardelc(&p, &["--project", ".", "--strict", "--fail-fast", "BB"]);
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));

    let out = gardelc(&p, &["--project", ".", "--fail-fast"]);
    assert!(!out.status.success() && all(&out).contains("--fail-fast: expected the module"), "{}", all(&out));
}
