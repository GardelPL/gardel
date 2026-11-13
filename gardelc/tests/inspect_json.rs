//! What the compiler gives a tool (Task F.11b): the short forms as JSON, the short forms of
//! a project whose proofs fail, and a build's executable or its C compiler's failure, all
//! in the forms of spec §12. Tablon reads these, so this file pins them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// False at one input, which the verifier finds.
const FALSE_AT_ONE_POINT: &str = "functions HIDE

feature {ANY}

  f(x: U64): U64
    do
      if x = 123456789 then 0 else 1 end
    ensure
      lie: Result = 1
    end

end
";

const TRUE: &str = "functions FINE

feature {ANY}

  f(x: U64): U64
    do
      1
    ensure
      one: Result = 1
    end

end
";

const UNKNOWN_NAME: &str = "functions NAMES

feature {ANY}

  f(x: NAT): NAT
    do
      g(x)
    end

end
";

const PROGRAM: &str = "program SAYS_HI

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(\"hi\")
        IO.exit(0)
      end
    end

end
";

/// Stands in for `gcc`: it names line 3 of the C file as an error and fails.
const FAKE_GCC: &str = r#"#include <stdio.h>
int main(int argc, char** argv) {
    const char* c = argc > 3 ? argv[3] : "?";
    fprintf(stderr, "%s: In function 'main':\n", c);
    fprintf(stderr, "%s:3:5: error: fake failure\n", c);
    return 1;
}
"#;

/// A directory of its own for `name`, holding `files`.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gardelc_inspect_{}_{name}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("temporary directory");
    for (file, src) in files {
        fs::write(dir.join(file), src).expect("source file");
    }
    dir
}

fn gardelc(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gardelc"));
    cmd.current_dir(dir).env("GARDEL_CACHE_DIR", dir.join("cache"));
    cmd
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// A JSON value, as far as these tests read one.
#[derive(Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(fields) => {
                fields.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("no `{key}` in {self:?}"))
            }
            _ => panic!("`{key}` asked of {self:?}"),
        }
    }

    fn list(&self) -> &[Json] {
        match self {
            Json::List(items) => items,
            _ => panic!("not a list: {self:?}"),
        }
    }

    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
}

/// Reads a whole JSON document, refusing anything after it.
fn parse(text: &str) -> Json {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let value = parse_value(&chars, &mut at);
    skip_space(&chars, &mut at);
    assert_eq!(at, chars.len(), "text after the JSON document: {text}");
    value
}

fn skip_space(c: &[char], at: &mut usize) {
    while *at < c.len() && c[*at].is_whitespace() {
        *at += 1;
    }
}

fn expect(c: &[char], at: &mut usize, want: char) {
    skip_space(c, at);
    assert_eq!(c.get(*at), Some(&want), "expected `{want}` at {at}");
    *at += 1;
}

fn parse_value(c: &[char], at: &mut usize) -> Json {
    skip_space(c, at);
    match c.get(*at) {
        Some('{') => {
            *at += 1;
            let mut fields = Vec::new();
            skip_space(c, at);
            if c.get(*at) == Some(&'}') {
                *at += 1;
                return Json::Object(fields);
            }
            loop {
                skip_space(c, at);
                let Json::Str(key) = parse_value(c, at) else { panic!("a key that is not a string at {at}") };
                expect(c, at, ':');
                fields.push((key, parse_value(c, at)));
                skip_space(c, at);
                match c.get(*at) {
                    Some(',') => *at += 1,
                    Some('}') => {
                        *at += 1;
                        return Json::Object(fields);
                    }
                    other => panic!("expected `,` or `}}` at {at}, found {other:?}"),
                }
            }
        }
        Some('[') => {
            *at += 1;
            let mut items = Vec::new();
            skip_space(c, at);
            if c.get(*at) == Some(&']') {
                *at += 1;
                return Json::List(items);
            }
            loop {
                items.push(parse_value(c, at));
                skip_space(c, at);
                match c.get(*at) {
                    Some(',') => *at += 1,
                    Some(']') => {
                        *at += 1;
                        return Json::List(items);
                    }
                    other => panic!("expected `,` or `]` at {at}, found {other:?}"),
                }
            }
        }
        Some('"') => {
            *at += 1;
            let mut s = String::new();
            loop {
                let ch = *c.get(*at).expect("an unterminated string");
                *at += 1;
                match ch {
                    '"' => return Json::Str(s),
                    '\\' => {
                        let e = c[*at];
                        *at += 1;
                        match e {
                            'n' => s.push('\n'),
                            'r' => s.push('\r'),
                            't' => s.push('\t'),
                            'b' => s.push('\u{8}'),
                            'f' => s.push('\u{c}'),
                            'u' => {
                                let hex: String = c[*at..*at + 4].iter().collect();
                                *at += 4;
                                s.push(char::from_u32(u32::from_str_radix(&hex, 16).expect("hex")).expect("a char"));
                            }
                            '"' | '\\' | '/' => s.push(e),
                            other => panic!("escape `\\{other}`"),
                        }
                    }
                    ch if (ch as u32) < 0x20 => panic!("a control character in a string"),
                    ch => s.push(ch),
                }
            }
        }
        Some(ch) if *ch == '-' || ch.is_ascii_digit() => {
            let start = *at;
            while *at < c.len() && (c[*at] == '-' || c[*at].is_ascii_digit()) {
                *at += 1;
            }
            Json::Number(c[start..*at].iter().collect())
        }
        _ => {
            for (word, value) in [("true", Json::Bool(true)), ("false", Json::Bool(false)), ("null", Json::Null)] {
                let end = *at + word.len();
                if end <= c.len() && c[*at..end].iter().collect::<String>() == word {
                    *at = end;
                    return value;
                }
            }
            panic!("no JSON value at {at}")
        }
    }
}

#[test]
fn short_forms_in_json_are_pinned() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("inspect");
    let files: Vec<(String, String)> = ["cell.gdl", "sized.gdl", "use_cell.gdl"]
        .iter()
        .map(|f| (f.to_string(), fs::read_to_string(fixture.join(f)).expect("fixture")))
        .collect();
    let files: Vec<(&str, &str)> = files.iter().map(|(f, s)| (f.as_str(), s.as_str())).collect();
    let dir = project("pinned", &files);
    let out = gardelc(&dir).args([".", "--inspect", "--json-diagnostics", "--strict"]).output().expect("gardelc runs");
    assert!(out.status.success(), "{}", text(&out));
    let expected = fs::read_to_string(fixture.join("expected.json")).expect("expected.json").replace("\r\n", "\n");
    assert_eq!(stdout(&out), expected, "the short forms changed; Tablon reads them");

    let doc = parse(&stdout(&out));
    assert_eq!(doc.get("verified"), &Json::Bool(true));
    let modules = doc.get("modules").list();
    let cell = &modules[0];
    assert_eq!(cell.get("concepts").list().len(), 2, "MEASURED and the SIZED it refines");
    let names: Vec<&str> = cell.get("features").list().iter().map(|f| f.get("name").str()).collect();
    assert_eq!(names, ["make", "size", "measure"], "a {{NONE}} feature is not in the short form");
    assert_eq!(modules[1].get("suppliers").list(), [Json::Str("CELL".to_string())]);
    // The text form is the same one `--inspect` prints.
    let plain = gardelc(&dir).args([".", "--inspect", "CELL"]).output().expect("gardelc runs");
    assert_eq!(stdout(&plain).trim_end(), cell.get("short").str().trim_end());
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn no_verify_gives_the_short_forms_of_a_module_whose_proof_fails() {
    let dir = project("no_verify", &[("hide.gdl", FALSE_AT_ONE_POINT)]);
    let checked = gardelc(&dir).args(["hide.gdl", "--inspect", "--json-diagnostics"]).output().expect("gardelc runs");
    assert!(!checked.status.success(), "{}", text(&checked));
    let doc = parse(&stdout(&checked));
    assert_eq!(doc.get("verified"), &Json::Bool(false));
    assert!(doc.get("modules").list().is_empty(), "a failed proof has no interface: {}", text(&checked));
    let failure = &doc.get("diagnostics").list()[0];
    assert_eq!(failure.get("rule").str(), "Phase 3 Boogie (Postcondition Violation)");
    // The values of the routine's own names, not the functions of the solver's model.
    let Json::Object(values) = failure.get("counterexample") else { panic!("no counterexample") };
    assert!(values.iter().any(|(k, _)| k == "x"), "{}", text(&checked));
    assert!(values.iter().all(|(k, _)| ["x", "Current", "Result"].contains(&k.as_str())), "{}", text(&checked));

    let types = gardelc(&dir).args(["hide.gdl", "--inspect", "--no-verify", "--json-diagnostics"]).output().expect("gardelc runs");
    assert!(types.status.success(), "{}", text(&types));
    let doc = parse(&stdout(&types));
    assert_eq!(doc.get("verified"), &Json::Bool(false), "nothing was proven");
    assert_eq!(doc.get("modules").list()[0].get("name").str(), "HIDE");
    assert!(doc.get("diagnostics").list().is_empty(), "{}", text(&types));

    let plain = gardelc(&dir).args(["hide.gdl", "--inspect", "--no-verify"]).output().expect("gardelc runs");
    assert!(plain.status.success(), "{}", text(&plain));
    assert!(stdout(&plain).starts_with("-- not verified: --no-verify checked the types only\nfunctions HIDE"), "{}", text(&plain));

    // A check that fails without a proof, an `assume` under --strict, still leaves the short
    // forms, with the failure beside them.
    fs::write(dir.join("assumed.gdl"), ASSUMED).expect("source file");
    let strict = gardelc(&dir)
        .args(["assumed.gdl", "--inspect", "--no-verify", "--strict", "--json-diagnostics"])
        .output()
        .expect("gardelc runs");
    assert!(!strict.status.success(), "{}", text(&strict));
    let doc = parse(&stdout(&strict));
    assert_eq!(doc.get("modules").list()[0].get("name").str(), "ASSUMED", "{}", text(&strict));
    assert_eq!(doc.get("diagnostics").list()[0].get("tag").str(), "taken", "{}", text(&strict));
    drop(fs::remove_dir_all(&dir));
}

const ASSUMED: &str = "functions ASSUMED

feature {ANY}

  f(x: NAT): NAT
    do
      assume taken: x > 0 end
      x
    ensure
      positive: Result > 0
    end

end
";

#[test]
fn a_type_check_is_never_kept_as_a_proof() {
    let dir = project("no_proof_kept", &[("fine.gdl", TRUE)]);
    let types = gardelc(&dir).args(["fine.gdl", "--inspect", "--no-verify"]).output().expect("gardelc runs");
    assert!(types.status.success(), "{}", text(&types));
    let checked = gardelc(&dir).arg("fine.gdl").output().expect("gardelc runs");
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(
        text(&checked).contains("[cache] 1 modules verified now, 0 proven earlier"),
        "a --no-verify run left a proof in the cache: {}",
        text(&checked)
    );
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn an_inspection_that_stops_early_is_still_the_object() {
    let dir = project("early", &[("names.gdl", UNKNOWN_NAME), ("broken.gdl", "functions BROKEN feature")]);
    for (file, rule) in [("names.gdl", "E0108: unknown_routine"), ("broken.gdl", "Parser")] {
        for flags in [&[][..], &["--no-verify"][..]] {
            let out = gardelc(&dir).args([file, "--inspect", "--json-diagnostics"]).args(flags).output().expect("gardelc runs");
            assert!(!out.status.success(), "{}", text(&out));
            let doc = parse(&stdout(&out));
            assert_eq!(doc.get("verified"), &Json::Bool(false));
            assert!(doc.get("modules").list().is_empty());
            assert_eq!(doc.get("diagnostics").list()[0].get("rule").str(), rule, "{}", text(&out));
        }
    }
    // Without --inspect the document is the array, as before.
    let out = gardelc(&dir).args(["names.gdl", "--json-diagnostics"]).output().expect("gardelc runs");
    assert!(matches!(parse(&stdout(&out)), Json::List(_)), "{}", text(&out));
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn no_verify_goes_only_with_inspect() {
    let dir = project("refused", &[("fine.gdl", TRUE)]);
    for args in [
        &["fine.gdl", "--no-verify"][..],
        &["fine.gdl", "--inspect", "--no-verify", "--run"][..],
        &["fine.gdl", "--inspect", "--no-verify", "--bounded-check"][..],
    ] {
        let out = gardelc(&dir).args(args).output().expect("gardelc runs");
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(text(&out).contains("--no-verify"), "{}", text(&out));
        assert!(!dir.join("build").exists(), "{args:?} built something");
    }
    drop(fs::remove_dir_all(&dir));
}

#[test]
fn a_build_names_its_executable_and_a_failed_c_compiler_is_a_diagnostic() {
    let dir = project("build", &[("says_hi.gdl", PROGRAM), ("fake_gcc.c", FAKE_GCC)]);
    let out = gardelc(&dir).args(["says_hi.gdl", "--json-diagnostics"]).output().expect("gardelc runs");
    assert!(out.status.success(), "{}", text(&out));
    let doc = parse(&stdout(&out));
    let note = &doc.list()[0];
    assert_eq!(note.get("level").str(), "note");
    assert_eq!(note.get("tag").str(), "executable");
    let exe = note.get("span").get("file").str();
    assert_eq!(exe, "build/says_hi_bin.exe");
    assert!(dir.join(exe).is_file(), "{}", text(&out));
    // The steps are on standard error, so standard output stays one document.
    assert!(String::from_utf8_lossy(&out.stderr).contains("[5/5]"), "{}", text(&out));

    // `--run` runs the executable, and not the verifier a second time.
    let run = gardelc(&dir).args(["says_hi.gdl", "--run"]).output().expect("gardelc runs");
    assert!(run.status.success(), "{}", text(&run));
    assert!(stdout(&run).contains("hi\n"), "{}", text(&run));
    assert!(!text(&run).contains("Boogie program verifier"), "{}", text(&run));

    // A C compiler that fails, found first in PATH.
    let fake = dir.join("fake");
    fs::create_dir_all(&fake).expect("fake directory");
    let built = Command::new("gcc")
        .arg(dir.join("fake_gcc.c"))
        .arg("-o")
        .arg(fake.join(if cfg!(windows) { "gcc.exe" } else { "gcc" }))
        .status()
        .expect("gcc runs");
    assert!(built.success());
    let path = std::env::join_paths(
        std::iter::once(fake.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .expect("PATH");
    fs::remove_dir_all(dir.join("build")).expect("build removed");
    let failed = gardelc(&dir).args(["says_hi.gdl", "--json-diagnostics"]).env("PATH", &path).output().expect("gardelc runs");
    assert!(!failed.status.success(), "{}", text(&failed));
    let doc = parse(&stdout(&failed));
    let d = &doc.list()[0];
    assert_eq!(d.get("rule").str(), "E0401: c_compile_failed");
    assert_eq!(d.get("span").get("file").str(), "build/says_hi.c");
    assert_eq!(d.get("span").get("line"), &Json::Number("3".to_string()));
    assert!(d.get("message").str().contains("build/says_hi.c:3:5: error: fake failure"), "{}", text(&failed));
    assert!(!dir.join("build/says_hi_bin.exe").exists());

    let plain = gardelc(&dir).arg("says_hi.gdl").env("PATH", &path).output().expect("gardelc runs");
    assert!(!plain.status.success(), "{}", text(&plain));
    assert!(stdout(&plain).contains("[ERROR] E0401: c_compile_failed at build/says_hi.c:3:5"), "{}", text(&plain));
    drop(fs::remove_dir_all(&dir));
}
