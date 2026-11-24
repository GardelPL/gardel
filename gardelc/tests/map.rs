//! `MAP[K, V]` (Task F.3): a client proves facts through the laws alone, no false fact about
//! a map verifies, a key must be ordered, and the map runs in C and frees what it allocates.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// The suffix gardelc gives an executable: `.exe` on Windows, none elsewhere.
const EXE: &str = std::env::consts::EXE_SUFFIX;

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, sources: &[(&str, &str)]) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_map_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let package = name.replace('_', "-");
        let manifest = format!("[project]\nname = \"{package}\"\nversion = \"0.1.0\"\n");
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        for (file, text) in sources {
            fs::write(dir.join("src").join(file), text).expect("source file");
        }
        Self { dir }
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(&self.dir)
            .env("GARDEL_CACHE_DIR", self.dir.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
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

fn client(routines: &str) -> String {
    format!("module CLIENT\n\nrepresentation {{ANY}}\n  n: NAT\n\ncreate {{ANY}}\n\n{routines}\nend\n")
}

/// `MAP` is a prelude module, so a client sees its contracts and laws and no body: each fact
/// below rests on a law.
#[test]
fn a_client_proves_facts_through_the_laws() {
    let facts = client(
        "  same(m: MAP[NAT, NAT], x: NAT): OPTION[NAT]
    do
      m.put(1, x).get(1)
    ensure
      found: Result = Some(x)
    end

  others(m: MAP[NAT, NAT], x: NAT): BOOL
    do
      True
    ensure
      kept: m.put(1, x).get(2) = m.get(2)
      gone: m.put(1, x).remove(1).get(1) = None
      after: m.remove(2).put(1, x).get(1) = Some(x)
      through: m.put(1, x).remove(2).get(1) = Some(x)
    end

  text(m: MAP[STRING, STRING], k: STRING, s: STRING): BOOL
    do
      True
    ensure
      found: m.put(k, s).get(k).is_some
      same_text: m.put(k, s).get(k).get.equals(s)
    end

  first_two(m: MAP[NAT, NAT]): BOOL
    require
      two: m.size >= 2
    do
      True
    ensure
      below: LIST_OPS.at(m.keys, 0) < LIST_OPS.at(m.keys, 1)
    end

  nothing(m: MAP[NAT, NAT], k: NAT): BOOL
    require
      none: m.size = 0
    do
      True
    ensure
      empty: m.get(k) = None
      folded: m.fold(5, fn(acc: NAT, a: NAT, b: NAT): NAT do 7 end) = 5
    end
",
    );
    assert_ok(&Project::new("facts", &[("client.gdl", &facts)]).check());
}

#[test]
fn a_lie_about_a_map_does_not_verify() {
    let lies = [
        ("other_key", "f(m: MAP[NAT, NAT], x: NAT): BOOL\n    do\n      True\n    ensure\n      claim: m.put(1, x).get(2) = Some(x)\n    end\n"),
        ("still_there", "f(m: MAP[NAT, NAT], x: NAT): BOOL\n    do\n      True\n    ensure\n      claim: m.put(1, x).remove(1).get(1).is_some\n    end\n"),
        ("old_value", "f(m: MAP[NAT, NAT], x: NAT, y: NAT): BOOL\n    require\n      differ: x != y\n    do\n      True\n    ensure\n      claim: m.put(1, x).put(1, y).get(1) = Some(x)\n    end\n"),
        ("descending", "f(m: MAP[NAT, NAT]): BOOL\n    require\n      two: m.size >= 2\n    do\n      True\n    ensure\n      claim: LIST_OPS.at(m.keys, 1) < LIST_OPS.at(m.keys, 0)\n    end\n"),
        ("not_empty", "f(m: MAP[NAT, NAT], k: NAT): BOOL\n    do\n      True\n    ensure\n      claim: m.get(k) = None\n    end\n"),
        ("fold", "f(m: MAP[NAT, NAT]): BOOL\n    require\n      none: m.size = 0\n    do\n      True\n    ensure\n      claim: m.fold(5, fn(acc: NAT, a: NAT, b: NAT): NAT do acc end) = 6\n    end\n"),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "claim");
    }
}

/// `filter`, `map`, and what `keys` and `values` say of `get` (H.4). With `STRING` keys, for
/// which `agrees` does not prove, the laws that do not need it still hold.
#[test]
fn a_client_proves_facts_about_filter_and_map() {
    let facts = client(
        "  big(m: MAP[NAT, NAT]): MAP[NAT, NAT]
    do
      m.filter(fn(k: NAT, v: NAT): BOOL do v > 10 end)
    ensure
      not_more: Result.size <= m.size
    end

  kept(m: MAP[NAT, NAT], k: NAT): BOOL
    require
      present: m.has(k)
      large: m.get(k).get > 10
    do
      True
    ensure
      same: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k) = m.get(k)
    end

  small(m: MAP[NAT, NAT], k: NAT): BOOL
    require
      present: m.has(k)
      small: m.get(k).get <= 10
    do
      check
        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
      end
      True
    ensure
      gone: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k) = None
    end

  by_key(m: MAP[NAT, NAT], k: NAT): BOOL
    require
      low: k < 5
      present: m.has(k)
    do
      check
        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
      end
      True
    ensure
      gone: m.filter(fn(i: NAT, v: NAT): BOOL do i >= 5 end).get(k) = None
    end

  texts(m: MAP[STRING, NAT], k: STRING): BOOL
    require
      present: m.filter(fn(i: STRING, v: NAT): BOOL do v > 10 end).has(k)
    do
      True
    ensure
      large: m.filter(fn(i: STRING, v: NAT): BOOL do v > 10 end).get(k).get > 10
    end

  halves(m: MAP[NAT, NAT]): BOOL
    do
      True
    ensure
      all: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).size + m.filter(fn(i: NAT, v: NAT): BOOL do not (v > 10) end).size = m.size
    end

  ages(m: MAP[NAT, NAT], k: NAT): MAP[NAT, BOOL]
    do
      m.map(fn(i: NAT, v: NAT): BOOL do v > 10 end)
    ensure
      same: Result.size = m.size
      at: m.get(k) = Some(12) implies Result.get(k) = Some(True)
      none: m.get(k) = None implies Result.get(k) = None
    end

  keys_there(m: MAP[STRING, NAT]): BOOL
    require
      some: m.size > 0
    do
      True
    ensure
      present: m.has(LIST_OPS.at(m.keys, 0))
    end

  values_there(m: MAP[NAT, NAT], i: NAT): BOOL
    require
      inside: i < m.size
    do
      check
        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
      end
      True
    ensure
      found: m.get(LIST_OPS.at(m.keys, i)) = Some(LIST_OPS.at(m.values, i))
    end
",
    );
    assert_ok(&Project::new("filter_facts", &[("client.gdl", &facts)]).check());
}

#[test]
fn a_lie_about_filter_or_map_does_not_verify() {
    let lies = [
        ("kept_all", "f(m: MAP[NAT, NAT], k: NAT): BOOL\n    require\n      present: m.has(k)\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k) = m.get(k)\n    end\n"),
        ("kept_none", "f(m: MAP[NAT, NAT], k: NAT): BOOL\n    require\n      present: m.has(k)\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k) = None\n    end\n"),
        ("kept_small", "f(m: MAP[NAT, NAT], k: NAT): BOOL\n    require\n      present: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).has(k)\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(k).get > 11\n    end\n"),
        ("kept_text", "f(m: MAP[STRING, NAT], k: STRING): BOOL\n    require\n      present: m.has(k)\n      small: m.get(k).get <= 10\n    do\n      True\n    ensure\n      claim: m.filter(fn(i: STRING, v: NAT): BOOL do v > 10 end).get(k) = None\n    end\n"),
        ("parts_overlap", "f(m: MAP[NAT, NAT]): BOOL\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.filter(fn(i: NAT, v: NAT): BOOL do v > 10 end).size + m.filter(fn(i: NAT, v: NAT): BOOL do v > 5 end).size = m.size\n    end\n"),
        ("mapped_other", "f(m: MAP[NAT, NAT]): BOOL\n    require\n      there: m.get(1) = Some(12)\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.map(fn(i: NAT, v: NAT): BOOL do v > 10 end).get(1) = Some(False)\n    end\n"),
        ("mapped_more", "f(m: MAP[NAT, NAT]): BOOL\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.map(fn(i: NAT, v: NAT): BOOL do v > 10 end).size > m.size\n    end\n"),
        ("value_elsewhere", "f(m: MAP[NAT, NAT]): BOOL\n    require\n      two: m.size >= 2\n    do\n      check\n        agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)\n      end\n      True\n    ensure\n      claim: m.get(LIST_OPS.at(m.keys, 0)) = Some(LIST_OPS.at(m.values, 1))\n    end\n"),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "claim");
    }
}

/// `K` is bounded by `ORD`, at each use: `BYTES` and a module of the client's are not ordered.
#[test]
fn a_key_must_be_ordered() {
    for key in ["BYTES", "CLIENT"] {
        let routine = format!("  f(m: MAP[{key}, NAT]): NAT\n    do\n      m.size\n    end\n");
        let p = Project::new(&format!("key_{}", key.to_lowercase()), &[("client.gdl", &client(&routine))]);
        assert_refused(&p.check(), "E0317: unsatisfied_bound");
    }
}

/// Text and number keys put out of order, a key put twice, a key removed, two folds of three
/// arguments, and text values. Then the C, counting live blocks: none may outlive a call.
#[test]
fn the_map_runs_in_c_and_frees_what_it_allocates() {
    let p = Project::new("runtime", &[("main.gdl", include_str!("map_program.gdl"))]);
    assert_ok(&p.check());
    let out = Command::new(p.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    assert_eq!(
        text,
        "3 | apple,kiwi,pear | 10 | none | 4 | 17 | apple | TEN | twenty | none | TEN,twenty,thirty | kiwi,pear | 2 | TEN!,thirty! | 9,4,3\n"
    );

    let c = fs::read_to_string(p.dir.join("build").join("main.c")).expect("the C file");
    let c = c.replace("int main(int argc, char** argv) {", "int gardel_program_main(int argc, char** argv) {");
    let call = "MAIN__report_length()";
    let harness = format!(
        "{c}\nint main(void) {{\n    uint64_t r = (uint64_t)({call});\n    uint64_t first = gardel_live_allocations();\n    for (int i = 0; i < 50; i++) r += (uint64_t)({call});\n    printf(\"%llu %llu %llu\\n\", (unsigned long long)first, (unsigned long long)gardel_live_allocations(), (unsigned long long)r);\n    return 0;\n}}\n"
    );
    let c_path = p.dir.join("build").join("leak.c");
    let exe = p.dir.join("build").join("leak.exe");
    fs::write(&c_path, harness).expect("the harness");
    let mut gcc = Command::new("gcc");
    gcc.args(["-std=gnu11", "-O1", "-fopenmp", "-DGARDEL_COUNT_ALLOCS", "-o"]).arg(&exe).arg(&c_path);
    if cfg!(windows) {
        gcc.arg("-lws2_32");
    }
    let built = gcc.output().expect("gcc runs");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let counted = Command::new(&exe).output().expect("the harness runs");
    let counts = String::from_utf8_lossy(&counted.stdout).trim().to_string();
    assert_eq!(counts, "0 0 6579", "live blocks after one call and after 51, then the lengths' sum");
}
