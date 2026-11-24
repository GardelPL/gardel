//! `SORTING.sort_array`, `lower_bound` and `binary_search` (Task H.3, after H.5): a client
//! proves facts through the contracts alone, no false fact verifies, a search needs a sorted
//! array, and in C the sort is stable, sorts a million numbers that the search then finds,
//! and frees what it allocates.

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
        let dir = std::env::temp_dir().join(format!("gardelc_sort_array_{}_{name}", std::process::id()));
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

/// `SORTING` is a prelude module, so a client sees its contracts and no body.
#[test]
fn a_client_proves_facts_through_the_contracts() {
    let facts = client(
        "  -- The sorted array is sorted, as long, and holds each value as often.
  sorted_array(a: ARRAY[STRING], s: STRING): BOOL
    do
      True
    ensure
      ordered: SORTING.sorted(SORTING.sort_array(a).to_list)
      same_length: SORTING.sort_array(a).length = a.length
      same_count: SORTING.count(SORTING.sort_array(a).to_list, s) = SORTING.count(a.to_list, s)
    end

  -- What the search finds is the key; what it misses is nowhere.
  searched(a: ARRAY[NAT], k: NAT, p: NAT): BOOL
    require
      ordered: SORTING.sorted(a.to_list)
      inside: p < a.length
    do
      check
        nat_agrees: forall x: NAT. forall y: NAT. x.equals(y) = (x = y)
      end
      True
    ensure
      found: SORTING.binary_search(a, k).is_some implies a.at(SORTING.binary_search(a, k).get) = k
      missed: SORTING.binary_search(a, k).is_none implies a.at(p) != k
    end

  -- A sorted array's own item is found.
  own(a: ARRAY[NAT], p: NAT): BOOL
    require
      ordered: SORTING.sorted(a.to_list)
      inside: p < a.length
    do
      check
        nat_agrees: forall x: NAT. forall y: NAT. x.equals(y) = (x = y)
      end
      True
    ensure
      found: SORTING.binary_search(a, a.at(p)).is_some
    end

  -- The insertion point: every item before it is below the key, none after.
  bounded(a: ARRAY[I64], k: I64, p: NAT): BOOL
    require
      ordered: SORTING.sorted(a.to_list)
      inside: p < a.length
    do
      check
        i64_agrees: forall x: I64. forall y: I64. x.equals(y) = (x = y)
      end
      True
    ensure
      before: p < SORTING.lower_bound(a, k) implies a.at(p) < k
      after: SORTING.lower_bound(a, k) <= p implies a.at(p) >= k
      inside: SORTING.lower_bound(a, k) <= a.length
    end

  -- Sorting a list as an array gives a sorted list.
  through(xs: LIST[NAT]): LIST[NAT]
    do
      SORTING.sort_array(ARRAY.from_list(xs)).to_list
    ensure
      ordered: SORTING.sorted(Result)
      same_length: Result.length = xs.length
    end
",
    );
    let p = Project::new("facts", &[("client.gdl", &facts)]);
    assert_ok(&p.check());
}

#[test]
fn a_lie_about_sorting_an_array_does_not_verify() {
    let agrees = "      check\n        nat_agrees: forall x: NAT. forall y: NAT. x.equals(y) = (x = y)\n      end\n";
    let lies = [
        ("longer", "f(a: ARRAY[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.sort_array(a).length = a.length + 1\n    end\n".to_string()),
        ("kept_order", "f(a: ARRAY[NAT]): BOOL\n    do\n      True\n    ensure\n      claim: SORTING.sort_array(a).to_list = a.to_list\n    end\n".to_string()),
        ("lost", "f(a: ARRAY[NAT]): BOOL\n    require\n      some: a.length > 0\n    do\n      True\n    ensure\n      claim: SORTING.count(SORTING.sort_array(a).to_list, a.at(0)) = 0\n    end\n".to_string()),
        ("always_found", format!("f(a: ARRAY[NAT], k: NAT): BOOL\n    require\n      ordered: SORTING.sorted(a.to_list)\n    do\n{agrees}      True\n    ensure\n      claim: SORTING.binary_search(a, k).is_some\n    end\n")),
        ("found_other", format!("f(a: ARRAY[NAT], k: NAT): BOOL\n    require\n      ordered: SORTING.sorted(a.to_list)\n    do\n{agrees}      True\n    ensure\n      claim: SORTING.binary_search(a, k).is_some implies SORTING.binary_search(a, k).get = 0\n    end\n")),
        ("bound_past", format!("f(a: ARRAY[NAT], k: NAT): BOOL\n    require\n      ordered: SORTING.sorted(a.to_list)\n    do\n{agrees}      True\n    ensure\n      claim: SORTING.lower_bound(a, k) < a.length\n    end\n")),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "claim");
    }
}

/// A search on an array not known to be sorted is refused.
#[test]
fn a_search_needs_a_sorted_array() {
    let routine = "  f(a: ARRAY[NAT], k: NAT): BOOL\n    do\n      check\n        nat_agrees: forall x: NAT. forall y: NAT. x.equals(y) = (x = y)\n      end\n      SORTING.binary_search(a, k).is_some\n    end\n";
    let p = Project::new("unsorted", &[("client.gdl", &client(routine))]);
    assert_refused(&p.check(), "ordered");
}

/// Numbers and text with repeats, the empty and one-item arrays, items the order ties, which
/// keep their order, searches and bounds, and a million numbers, each found. Then the C,
/// counting live blocks: none may outlive a call.
#[test]
fn the_array_sort_runs_in_c_and_frees_what_it_allocates() {
    let p = Project::new(
        "runtime",
        &[("main.gdl", include_str!("sort_array_program.gdl")), ("item.gdl", include_str!("sorting_item.gdl"))],
    );
    assert_ok(&p.check());
    let out = Command::new(p.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    let small = "0,1,3,3,5,7,9 | apple,apple,fig,pear |  | 42 | 0:1,1:2,1:4,1:6,2:3,2:5 | 4 - 0 - | 4 2 7 0";
    assert_eq!(text, format!("{small} | 1000000 in order 1000000 found\n"));

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
    assert_eq!(counts, format!("0 0 {}", small.len() * 51), "live blocks after one call and after 51, then the lengths' sum");
}
