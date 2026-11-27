//! `gparse` (Task H.7): a cursor over bytes whose readers ensure progress or failure. The
//! package verifies, every case of `gparse/tests/cases.txt` holds in C, run by `gardelc test`,
//! and a client proves what it read, and that its loop ends, through the contracts alone.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Project {
    dir: PathBuf,
}

impl Project {
    /// A project in a directory of its own that depends on `gparse`.
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_gparse_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let path = root().join("gparse").canonicalize().expect("gparse exists");
        let path = path.to_string_lossy().replace('\\', "/").replace("//?/", "");
        let manifest = format!(
            "[project]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[dependencies]\ngparse = {{ path = \"{path}\" }}\n",
            name.replace('_', "-")
        );
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source file");
        }
        Self { dir }
    }

    fn check(&self) -> Output {
        check_at(&self.dir, &self.dir.join("cache"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn check_at(dir: &Path, cache: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(dir)
        .env("GARDEL_CACHE_DIR", cache)
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs")
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// The package and its test package verify, and `gardelc test` runs the test program, which
/// accepts every case: numbers at and past their limit, words, runs of bytes, and a loop.
#[test]
fn gparse_verifies_and_holds_every_case() {
    let cache = std::env::temp_dir().join(format!("gardelc_gparse_{}_cache", std::process::id()));
    drop(fs::remove_dir_all(&cache));
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .env("GARDEL_CACHE_DIR", &cache)
        .args(["test"])
        .arg(root().join("gparse"))
        .output()
        .expect("gardelc runs");
    assert!(out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("[test] GPARSE_TEST ... ok"), "{}", all(&out));
    assert!(all(&out).contains("1 program: 1 passed, 0 failed"), "{}", all(&out));
    drop(fs::remove_dir_all(&cache));
}

/// `CURSOR` is opaque to a client. Its contracts are enough to prove where a reader stopped,
/// what it read there, which error it gives and why, and that a loop of readers ends.
const CLIENT: &str = r#"functions CLIENT

feature {ANY}

  is_comma(b: U64): BOOL
    do
      b = 44
    end

  -- Items split by `,`: a loop that ends by the readers' progress alone.
  count_items(c: CURSOR, n: NAT): NAT
    require
      bounded: n <= c.offset
    variant
      c.remaining
    do
      match c.take_some(fn(b: U64): BOOL do not CLIENT.is_comma(b) end)
        case Error(_) then n
        case Ok(item) then
          match item.second.expect_byte(44)
            case Error(_) then n + 1
            case Ok(next) then count_items(next, n + 1)
          end
      end
    ensure
      at_most: Result <= c.input.length
    end

  word_read(c: CURSOR, w: BYTES): BOOL
    require
      not_empty: w.length > 0
    do
      True
    ensure
      past: c.expect(w).is_ok implies c.expect(w).get.offset = c.offset + w.length
      there: c.expect(w).is_ok implies
        LIST_OPS.at(c.input.model, c.offset) = LIST_OPS.at(w.model, 0)
      kept: c.expect(w).is_error implies c.expect(w).get_error.offset = c.offset
    end

  number_read(c: CURSOR): BOOL
    do
      True
    ensure
      small: c.read_nat(9).is_ok implies c.read_nat(9).get.first <= 9
      one_digit: c.read_nat(9).is_ok implies
        DECIMAL.is_digit(LIST_OPS.at(c.input.model, c.offset))
      moved: c.read_nat(9).is_ok implies c.read_nat(9).get.second.offset > c.offset
    end

  digits_stop(c: CURSOR): BOOL
    do
      True
    ensure
      stop: c.take_while(fn(b: U64): BOOL do DECIMAL.is_digit(b) end).second.offset < c.input.length implies
        not DECIMAL.is_digit(LIST_OPS.at(c.input.model,
          c.take_while(fn(b: U64): BOOL do DECIMAL.is_digit(b) end).second.offset))
    end

  why_not(c: CURSOR): BOOL
    do
      True
    ensure
      ended: c.at_end implies c.read_nat(9).get_error.expected = MoreBytes
      other: (not c.at_end and not DECIMAL.is_digit(LIST_OPS.at(c.input.model, c.offset))) implies
        c.read_nat(9).get_error.expected = Digit
      digit: (not c.at_end and DECIMAL.is_digit(LIST_OPS.at(c.input.model, c.offset))) implies
        (c.read_nat(9).is_ok or c.read_nat(9).get_error.expected = SmallerNumber)
    end

end
"#;

#[test]
fn a_client_proves_what_it_read_through_the_contracts() {
    let out = Project::new("facts", &[("client.gdl", CLIENT)]).check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
}

/// Each claim past the contracts is refused, and not by a timeout: a loop on `skip_while`,
/// which may read nothing, a bound too tight, an error past where it was found, progress for
/// `take_while`, and a number reader that never reports the end.
const LIES: &str = r#"functions LIES

feature {ANY}

  is_space(b: U64): BOOL
    do
      b = 32
    end

  -- `skip_while` may read nothing, so it is no progress for a loop.
  spaces(c: CURSOR, n: NAT): NAT
    variant
      c.remaining
    do
      if c.at_end or n >= 10 then n else spaces(c.skip_while(fn(b: U64): BOOL do LIES.is_space(b) end), n + 1) end
    end

  at_limit(c: CURSOR): BOOL
    do
      True
    ensure
      claim: c.read_nat(9).is_ok implies c.read_nat(9).get.first < 9
    end

  error_past(c: CURSOR, w: BYTES): BOOL
    require
      not_empty: w.length > 0
    do
      True
    ensure
      claim: c.expect(w).is_error implies c.expect(w).get_error.offset = c.offset + 1
    end

  run_moves(c: CURSOR): BOOL
    do
      True
    ensure
      claim: c.take_while(fn(b: U64): BOOL do LIES.is_space(b) end).second.offset > c.offset
    end

  never_ends(c: CURSOR): BOOL
    do
      True
    ensure
      claim: c.read_nat(9).get_error.expected != MoreBytes
    end

end
"#;

#[test]
fn a_lie_about_a_cursor_does_not_verify() {
    let out = Project::new("lies", &[("lies.gdl", LIES)]).check();
    let text = all(&out);
    assert!(!out.status.success(), "must fail:\n{text}");
    assert!(!text.contains("Timeout"), "refused by a timeout:\n{text}");
    for (routine, tag) in [
        ("spaces", "variant_decrease"),
        ("at_limit", "claim"),
        ("error_past", "claim"),
        ("run_moves", "claim"),
        ("never_ends", "claim"),
    ] {
        assert!(text.contains(&format!("in `{routine}` ({tag})")), "`{routine}` must fail `{tag}`:\n{text}");
    }
}
