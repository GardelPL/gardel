//! `ARRAY[A]` (Task H.5): a client proves facts through the contracts alone, among them that
//! `a.set(i, x).at(i) = x`, no false fact about an array verifies, an index past the end is
//! refused, and in C `set` and `swap` change an array in place when nothing else holds it,
//! copy it once when something does, and free what they allocate.

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
        let dir = std::env::temp_dir().join(format!("gardelc_array_{}_{name}", std::process::id()));
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

/// `ARRAY` is a prelude module, so a client sees its contracts and laws and no body.
#[test]
fn a_client_proves_facts_through_the_contracts() {
    let facts = client(
        "  -- The acceptance of Task H.5.
  put_read(a: ARRAY[NAT], i: NAT, x: NAT): NAT
    require
      inside: i < a.length
    do
      a.set(i, x).at(i)
    ensure
      same: Result = x
    end

  other_read(a: ARRAY[STRING], i: NAT, j: NAT, x: STRING): STRING
    require
      inside: i < a.length
      j_inside: j < a.length
      apart: i != j
    do
      a.set(i, x).at(j)
    ensure
      kept: Result.byte_model = a.at(j).byte_model
    end

  swapped(a: ARRAY[NAT]): ARRAY[NAT]
    require
      two: a.length >= 2
    do
      a.swap(0, 1)
    ensure
      first: Result.at(0) = a.at(1)
      second: Result.at(1) = a.at(0)
      len: Result.length = a.length
    end

  made(n: NAT): NAT
    require
      some: n > 3
    do
      ARRAY.make(n, 0).set(2, 7).at(3)
    ensure
      zero: Result = 0
    end

  listed(xs: LIST[NAT]): LIST[NAT]
    do
      ARRAY.from_list(xs).to_list
    ensure
      same: Result = xs
    end

  sliced(a: ARRAY[NAT]): NAT
    require
      four: a.length >= 4
    do
      a.slice(1, 3).length
    ensure
      two: Result = 2
    end

  total(a: ARRAY[NAT]): NAT
    do
      a.fold(fn(acc: NAT, x: NAT): NAT do acc +% x end, 0)
    ensure
      def: Result = LIST_OPS.fold_left(a.to_list, fn(acc: NAT, x: NAT): NAT do acc +% x end, 0)
    end
",
    );
    let p = Project::new("facts", &[("client.gdl", &facts)]);
    assert_ok(&p.check());
}

#[test]
fn a_lie_about_an_array_does_not_verify() {
    let lies = [
        (
            "kept_old",
            "  kept_old(a: ARRAY[NAT], i: NAT, x: NAT): NAT
    require
      inside: i < a.length
    do
      a.set(i, x).at(i)
    ensure
      old: Result = a.at(i)
    end
",
        ),
        (
            "spread",
            "  spread(a: ARRAY[NAT], i: NAT, j: NAT, x: NAT): NAT
    require
      inside: i < a.length
      j_inside: j < a.length
      apart: i != j
    do
      a.set(i, x).at(j)
    ensure
      spread: Result = x
    end
",
        ),
        (
            "longer",
            "  longer(a: ARRAY[NAT], i: NAT, x: NAT): NAT
    require
      inside: i < a.length
    do
      a.set(i, x).length
    ensure
      grew: Result = a.length + 1
    end
",
        ),
        (
            "swap_nothing",
            "  swap_nothing(a: ARRAY[NAT]): NAT
    require
      two: a.length >= 2
    do
      a.swap(0, 1).at(0)
    ensure
      same: Result = a.at(0)
    end
",
        ),
        (
            "wide_slice",
            "  wide_slice(a: ARRAY[NAT]): NAT
    require
      four: a.length >= 4
    do
      a.slice(1, 3).length
    ensure
      three: Result = 3
    end
",
        ),
        (
            "made_one",
            "  made_one(n: NAT): NAT
    require
      some: n > 0
    do
      ARRAY.make(n, 0).at(0)
    ensure
      one: Result = 1
    end
",
        ),
    ];
    for (name, lie) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(lie))]);
        assert_refused(&p.check(), name);
    }
}

#[test]
fn an_index_past_the_end_is_refused() {
    for (name, body) in [("read_past", "a.at(a.length)"), ("set_past", "a.set(a.length, 0).length")] {
        let routine = format!("  {name}(a: ARRAY[NAT]): NAT\n    do\n      {body}\n    end\n");
        let p = Project::new(name, &[("client.gdl", &client(&routine))]);
        assert_refused(&p.check(), "in_bounds");
    }
}

/// The program's results, then a harness built with `-DGARDEL_COUNT_ALLOCS`: live blocks
/// after one call and after 51, and how many copies `set` and `swap` made (Task H.5).
#[test]
fn the_array_runs_in_c_in_place_and_frees_what_it_allocates() {
    let p = Project::new("runtime", &[("main.gdl", include_str!("array_program.gdl"))]);
    assert_ok(&p.check());
    let out = Command::new(p.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    let small = "9,9,9,9,9 0,1,2,3,4 | 5,0,0,6 | /c/a/a /b/c /c/a/a | 5,4,3,2,1 | /pp/qq";
    assert_eq!(text, format!("{small}\n499999500000\n500000500000\n2\n"));

    let c = fs::read_to_string(p.dir.join("build").join("main.c")).expect("the C file");
    let c = c.replace("int main(int argc, char** argv) {", "int gardel_program_main(int argc, char** argv) {");
    let harness = format!(
        "{c}\nint main(void) {{
    uint64_t r = MAIN__small_length();
    uint64_t first = gardel_live_allocations(), c0 = gardel_array_copies();
    for (int i = 0; i < 50; i++) r += MAIN__small_length();
    uint64_t c1 = gardel_array_copies();
    uint64_t once = MAIN__big();
    uint64_t c2 = gardel_array_copies();
    uint64_t twice = MAIN__twice();
    uint64_t c3 = gardel_array_copies();
    uint64_t through_let = MAIN__big_let();
    uint64_t c4 = gardel_array_copies();
    printf(\"%llu %llu %llu | %llu %llu %llu %llu | %llu %llu %llu\\n\",
        (unsigned long long)first, (unsigned long long)gardel_live_allocations(), (unsigned long long)r,
        (unsigned long long)((c1 - c0) / 50), (unsigned long long)(c2 - c1), (unsigned long long)(c3 - c2),
        (unsigned long long)(c4 - c3), (unsigned long long)once, (unsigned long long)twice, (unsigned long long)through_let);
    return 0;
}}\n"
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
    assert!(counted.status.success(), "{counted:?}");
    let counts = String::from_utf8_lossy(&counted.stdout).trim().to_string();
    // Each small run copies twice: `shared` reads its array after the loop, and `words` reads
    // `w` after its first `set`. A million `set`s on an array held once copy nothing, held
    // twice copy once, and through a `let`, then a million swaps, copy nothing.
    assert_eq!(
        counts,
        format!("0 0 {} | 2 0 1 0 | 499999500000 500000500000 2", small.len() * 51),
        "live blocks after one call and after 51, the lengths' sum; copies per small run, for the three million-item loops; their results"
    );
}
