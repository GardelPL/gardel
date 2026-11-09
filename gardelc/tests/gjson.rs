//! `gjson` (Task F.4): the package verifies, every case of `gjson/tests/cases.txt` holds in C,
//! and a client proves the round trip of scalars through the law alone. Also the compiler
//! changes F.4 needed: a field of a machine number type is in range.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Project {
    dir: PathBuf,
}

impl Project {
    /// A project in a directory of its own. With `gjson`, it depends on the package.
    fn new(name: &str, sources: &[(&str, &str)], gjson: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_gjson_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let mut manifest = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-"));
        if gjson {
            let path = root().join("gjson").canonicalize().expect("gjson exists");
            let path = path.to_string_lossy().replace('\\', "/").replace("//?/", "");
            manifest.push_str(&format!("\n[dependencies]\ngjson = {{ path = \"{path}\" }}\n"));
        }
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

fn assert_ok(out: &Output) {
    assert!(out.status.success(), "{}", all(out));
    assert!(all(out).contains("0 errors, 0 warnings"), "{}", all(out));
}

fn assert_refused(out: &Output, text: &str) {
    assert!(!out.status.success(), "must fail with {text}:\n{}", all(out));
    assert!(all(out).contains(text), "must say {text}:\n{}", all(out));
}

/// The package and its test package verify, and the test program accepts every case. The
/// cases include 64 levels of nesting, 200,000 items, 300,000 bytes of text and 3,000 fields in
/// descending order, so they also show that long arrays and strings take no stack.
#[test]
fn gjson_verifies_and_holds_every_case() {
    let cache = std::env::temp_dir().join(format!("gardelc_gjson_{}_cache", std::process::id()));
    drop(fs::remove_dir_all(&cache));
    assert_ok(&check_at(&root().join("gjson"), &cache));
    let tests = root().join("gjson/tests");
    assert_ok(&check_at(&tests, &cache));
    let cases = fs::read(tests.join("cases.txt")).expect("cases.txt");
    let mut run = Command::new(tests.join("build/gjson_test_bin.exe"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the suite starts");
    run.stdin.take().expect("stdin").write_all(&cases).expect("cases are sent");
    let done = run.wait_with_output().expect("the suite ends");
    assert!(done.status.success(), "exit {:?}, failing case: {}", done.status.code(), String::from_utf8_lossy(&done.stdout));
    drop(fs::remove_dir_all(&cache));
}

fn client(routines: &str) -> String {
    format!("module CLIENT\n\nrepresentation {{ANY}}\n  n: NAT\n\ncreate {{ANY}}\n\n{routines}\nend\n")
}

/// `JSON` is opaque to a client, which sees no reader or writer. The law `scalar_round_trip`
/// and the builders' contracts are enough to prove the round trip of every scalar.
#[test]
fn a_client_proves_the_scalar_round_trip_through_the_law() {
    let facts = client(
        "  ints(i: I64): BOOL
    do
      True
    ensure
      back: JSON.parse(JSON.int(i).to_bytes) = Ok(JSON.int(i))
    end

  others(b: BOOL): BOOL
    do
      True
    ensure
      flag: JSON.parse(JSON.bool(b).to_bytes) = Ok(JSON.bool(b))
      none: JSON.parse(JSON.null.to_bytes) = Ok(JSON.null)
    end

  texts(s: STRING): BOOL
    require
      short: s.byte_length <= 100000
    do
      True
    ensure
      back: JSON.parse(JSON.string(s).to_bytes) = Ok(JSON.string(s))
    end
",
    );
    assert_ok(&Project::new("facts", &[("client.gdl", &facts)], true).check());
}

#[test]
fn a_lie_about_the_round_trip_does_not_verify() {
    let lies = [
        ("other_value", "f(i: I64): BOOL\n    do\n      True\n    ensure\n      claim: JSON.parse(JSON.int(i).to_bytes) = Ok(JSON.null)\n    end\n"),
        ("refused", "f(b: BOOL): BOOL\n    do\n      True\n    ensure\n      claim: JSON.parse(JSON.bool(b).to_bytes).is_error\n    end\n"),
        // The law needs the text to fit: a string of any length is not covered.
        ("any_length", "f(s: STRING): BOOL\n    do\n      True\n    ensure\n      claim: JSON.parse(JSON.string(s).to_bytes) = Ok(JSON.string(s))\n    end\n"),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))], true);
        assert_refused(&p.check(), "claim");
    }
}

/// A field of a machine number type is in the range of its type, as every value stored there
/// was. Without that, `same(n)`'s contract, which needs `n` in range, says nothing here.
#[test]
fn a_field_of_a_machine_type_is_in_range() {
    let src = "module BOX

representation {ANY}
  | Box(n: NAT, i: I64)
  | Empty

create {ANY}

  same(n: NAT): NAT
    do
      n
    ensure
      kept: Result = n
    end

  signed(i: I64): I64
    do
      i
    ensure
      kept: Result = i
    end

  both(b: BOX): BOOL
    do
      True
    ensure
      fields: match b
        case Box(n, i) then BOX.same(n) = n and BOX.signed(i) = i
        case Empty then True
      end
    end

end
";
    assert_ok(&Project::new("range", &[("box.gdl", src)], false).check());
}
