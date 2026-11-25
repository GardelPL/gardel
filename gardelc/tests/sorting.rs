//! `SORTING[K: ORD]` (Task H.3): a client proves facts through the contracts alone, among
//! them that the first item of a sorted list is at most every other, no false fact about a
//! sort verifies, a key must be ordered, and the sort runs in C, stable, on a million
//! numbers, and frees what it allocates.

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
        let dir = std::env::temp_dir().join(format!("gardelc_sorting_{}_{name}", std::process::id()));
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

/// Refused for `text`, by a proof that fails, not one the solver gave up on.
fn assert_refused(out: &Output, text: &str) {
    assert!(!out.status.success(), "must fail with {text}:\n{}", all(out));
    assert!(all(out).contains(text), "must say {text}:\n{}", all(out));
    assert!(!all(out).contains("Timeout") && !all(out).contains("Resource Limit"), "a refusal by the solver's limits proves nothing:\n{}", all(out));
}

fn client(routines: &str) -> String {
    format!("module CLIENT\n\nrepresentation {{ANY}}\n  n: NAT\n\ncreate {{ANY}}\n\n{routines}\nend\n")
}

/// `SORTING` is a prelude module, so a client sees its contracts and laws and no body.
#[test]
fn a_client_proves_facts_through_the_contracts() {
    let facts = client(
        "  -- The acceptance of Task H.3: the first item of a sorted list is at most every other.
  smallest(xs: LIST[NAT], j: NAT): NAT
    require
      inside: j < xs.length
    do
      check
        nat_agrees: forall a: NAT. forall b: NAT. a.equals(b) = (a = b)
        least: SORTING.first_least(SORTING.sort(xs), j)
      end
      SORTING.sort(xs).head
    ensure
      at_most: Result <= LIST_OPS.at(SORTING.sort(xs), j)
    end

  kept(xs: LIST[NAT], x: NAT): BOOL
    do
      True
    ensure
      ordered: SORTING.sorted(SORTING.sort(xs))
      same_length: SORTING.sort(xs).length = xs.length
      same_count: SORTING.count(SORTING.sort(xs), x) = SORTING.count(xs, x)
    end

  -- A merge of two sorted lists is sorted, and has the items of both.
  merged(xs: LIST[STRING], ys: LIST[STRING], s: STRING): BOOL
    require
      left: SORTING.sorted(xs)
      right: SORTING.sorted(ys)
    do
      True
    ensure
      ordered: SORTING.sorted(SORTING.merge(xs, ys))
      both: SORTING.count(SORTING.merge(xs, ys), s) = SORTING.count(xs, s) + SORTING.count(ys, s)
    end

  -- The item at the head of a list is in its sorted list.
  head_kept(xs: LIST[NAT]): BOOL
    require
      some: xs.length > 0
    do
      True
    ensure
      found: SORTING.count(SORTING.sort(xs), xs.head) > 0
    end
",
    );
    let p = Project::new("facts", &[("client.gdl", &facts)]);
    assert_ok(&p.check());
}

#[test]
fn a_lie_about_sorting_does_not_verify() {
    let lies = [
        ("longer", "f(xs: LIST[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.sort(xs).length = xs.length + 1\n    end\n"),
        ("lost", "f(xs: LIST[NAT]): BOOL\n    require\n      some: xs.length > 0\n    do\n      True\n    ensure\n      claim: SORTING.count(SORTING.sort(xs), xs.head) = 0\n    end\n"),
        ("kept_order", "f(xs: LIST[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.sort(xs) = xs\n    end\n"),
        ("descending", "f(xs: LIST[NAT]): BOOL\n    require\n      two: xs.length >= 2\n      apart: xs.head != xs.tail.head\n    do\n      True\n    ensure\n      claim: not SORTING.sorted(SORTING.sort(xs))\n    end\n"),
        ("unsorted_merge", "f(xs: LIST[NAT], ys: LIST[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.sorted(SORTING.merge(xs, ys))\n    end\n"),
        ("merge_drops", "f(xs: LIST[NAT], ys: LIST[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.merge(xs, ys).length = xs.length\n    end\n"),
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
        let routine = format!("  f(xs: LIST[{key}]): NAT\n    do\n      SORTING.sort(xs).length\n    end\n");
        let p = Project::new(&format!("key_{}", key.to_lowercase()), &[("client.gdl", &client(&routine))]);
        let out = p.check();
        assert!(!out.status.success(), "{}", all(&out));
        assert!(all(&out).contains("E0317: unsatisfied_bound"), "{}", all(&out));
    }
}

/// Numbers and text with repeats, the empty and one-item lists, a merge, items the order
/// ties, which keep their order, and a million numbers. Then the C, counting live blocks:
/// none may outlive a call.
#[test]
fn the_sort_runs_in_c_and_frees_what_it_allocates() {
    let p = Project::new(
        "runtime",
        &[("main.gdl", include_str!("sorting_program.gdl")), ("item.gdl", include_str!("sorting_item.gdl"))],
    );
    assert_ok(&p.check());
    let out = Command::new(p.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    assert_eq!(
        text,
        "0,1,3,3,5,7,9 | apple,apple,fig,pear |  | 42 | 1,2,4,4,8,9 | 0:1,1:2,1:4,1:6,2:3,2:5 | 1000000 in order\n"
    );

    let c = fs::read_to_string(p.dir.join("build").join("main.c")).expect("the C file");
    let c = c.replace("int main(int argc, char** argv) {", "int gardel_program_main(int argc, char** argv) {");
    let call = "MAIN__small_length()";
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
    let small = "0,1,3,3,5,7,9 | apple,apple,fig,pear |  | 42 | 1,2,4,4,8,9 | 0:1,1:2,1:4,1:6,2:3,2:5".len();
    assert_eq!(counts, format!("0 0 {}", small * 51), "live blocks after one call and after 51, then the lengths' sum");
}
