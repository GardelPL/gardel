//! `SET`, `QUEUE`, `HEAP` and the size and fold laws of `MAP` (Task H.4): a client proves
//! facts through the contracts and laws alone, no false fact verifies, and the structures run
//! in C, where a heap changes its array in place, and free what they allocate. A record that
//! holds an array gives it up only where nothing can still see it.

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
        let dir = std::env::temp_dir().join(format!("gardelc_collections_{}_{name}", std::process::id()));
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

    /// What the built program prints.
    fn run(&self) -> String {
        let out = Command::new(self.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).expect("UTF-8 output").replace("\r\n", "\n")
    }

    /// Runs `body` as the C `main` of the built program, and gives what it prints.
    fn harness(&self, body: &str) -> String {
        let c = fs::read_to_string(self.dir.join("build").join("main.c")).expect("the C file");
        let c = c.replace("int main(int argc, char** argv) {", "int gardel_program_main(int argc, char** argv) {");
        let c_path = self.dir.join("build").join("harness.c");
        let exe = self.dir.join("build").join("harness.exe");
        fs::write(&c_path, format!("{c}\nint main(void) {{\n{body}\n    return 0;\n}}\n")).expect("the harness");
        let mut gcc = Command::new("gcc");
        gcc.args(["-std=gnu11", "-O1", "-fopenmp", "-DGARDEL_COUNT_ALLOCS", "-o"]).arg(&exe).arg(&c_path);
        if cfg!(windows) {
            gcc.arg("-lws2_32");
        }
        let built = gcc.output().expect("gcc runs");
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        let out = Command::new(&exe).output().expect("the harness runs");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Live blocks after one call and after 51, then the results' sum.
    fn leaks(&self, call: &str) -> String {
        self.harness(&format!(
            "    uint64_t r = (uint64_t)({call});\n    uint64_t first = gardel_live_allocations();\n    for (int i = 0; i < 50; i++) r += (uint64_t)({call});\n    printf(\"%llu %llu %llu\\n\", (unsigned long long)first, (unsigned long long)gardel_live_allocations(), (unsigned long long)r);"
        ))
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

/// What a client states about `K` before it calls what needs it.
fn agrees(k: &str) -> String {
    format!("check\n        agrees: forall p: {k}. forall q: {k}. p.equals(q) = (p = q)\n      end")
}

#[test]
fn a_client_proves_the_size_of_a_map_and_what_a_fold_meets_last() {
    let nat = agrees("NAT");
    let facts = client(&format!(
        "  sizes(m: MAP[STRING, NAT], k: STRING, j: STRING, x: NAT): BOOL
    require
      absent: not m.has(k)
    do
      True
    ensure
      grows: m.put(k, x).size = m.size + 1
      at_most: m.put(j, x).size <= m.size + 1
      still: m.remove(k).size = m.size
      fewer: m.remove(j).size <= m.size
    end

  present(m: MAP[NAT, NAT], x: NAT): BOOL
    require
      there: m.has(7)
    do
      {nat}
      True
    ensure
      same: m.put(7, x).size = m.size
      shrinks: m.remove(7).size + 1 = m.size
    end

  newest(m: MAP[NAT, NAT]): NAT
    do
      m.fold(0, fn(acc: NAT, k: NAT, v: NAT): NAT do v end)
    end

  last_value(m: MAP[NAT, NAT], k: NAT, v: NAT): BOOL
    require
      absent: not m.has(k)
      last: m.size = 0 or LIST_OPS.at(m.keys, m.size - 1) < k
    do
      True
    ensure
      is_v: CLIENT.newest(m.put(k, v)) = v
    end

  two_puts: NAT
    do
      CLIENT.newest(MAP.empty.put(1, 5).put(2, 6))
    ensure
      second: Result = 6
    end
"
    ));
    assert_ok(&Project::new("map_facts", &[("client.gdl", &facts)]).check());
}

#[test]
fn a_client_proves_facts_of_sets_queues_and_heaps() {
    let nat = agrees("NAT");
    let int = agrees("I64");
    let facts = client(&format!(
        "  sets(s: SET[NAT], t: SET[NAT], x: NAT): BOOL
    do
      True
    ensure
      added: s.add(1).has(1)
      other: s.add(1).has(2) = s.has(2)
      gone: not s.add(1).remove(1).has(1)
      either: s.union(t.add(x)).has(x)
      both: s.add(x).intersection(t.add(x)).has(x)
      left_out: not s.intersection(t.remove(x)).has(x)
      at_most: s.add(x).size <= s.size + 1
    end

  new_name(s: SET[STRING], k: STRING): BOOL
    require
      absent: not s.has(k)
    do
      True
    ensure
      grows: s.add(k).size = s.size + 1
      still: s.remove(k).size = s.size
    end

  ascending(s: SET[NAT]): BOOL
    require
      two: s.size >= 2
    do
      True
    ensure
      below: LIST_OPS.at(s.items, 0) < LIST_OPS.at(s.items, 1)
    end

  first_out(x: NAT, y: NAT): OPTION[NAT]
    do
      let e: QUEUE[NAT] = QUEUE.empty
      e.push(x).push(y).peek
    ensure
      first: Result = Some(x)
    end

  then_next(x: NAT, y: NAT): OPTION[NAT]
    do
      let e: QUEUE[NAT] = QUEUE.empty
      e.push(x).push(y).pop.peek
    ensure
      next: Result = Some(y)
    end

  popped(q: QUEUE[STRING]): BOOL
    require
      some: q.size > 0
    do
      True
    ensure
      rest: q.pop.items = q.items.tail
      shrinks: q.pop.size < q.size
    end

  least(h: HEAP[NAT], x: NAT): BOOL
    require
      room: h.size < 1000
    do
      {nat}
      let g = h.push(x)
      check
        counted: g.count(x) > 0
      end
      not x.less_than(g.peek.get)
    ensure
      holds: Result
    end

  pushed_popped(h: HEAP[I64], x: I64, y: I64): BOOL
    require
      room: h.size < 1000
    do
      {int}
      let g = h.push(x)
      g.pop_min.size = h.size
    ensure
      holds: Result
      counted: h.push(x).count(y) = h.count(y) + (if x = y then 1 else 0 end)
    end

  taken(h: HEAP[NAT], y: NAT): BOOL
    require
      some: h.size > 0
      other: h.peek.get != y
    do
      {nat}
      True
    ensure
      kept: h.pop_min.count(y) = h.count(y)
    end
"
    ));
    assert_ok(&Project::new("facts", &[("client.gdl", &facts)]).check());
}

#[test]
fn a_lie_about_a_collection_does_not_verify() {
    let nat = agrees("NAT");
    let lie = |params: &str, require: &str, body: &str, claim: &str| {
        let require = if require.is_empty() { String::new() } else { format!("    require\n      premise: {require}\n") };
        format!("f({params}): BOOL\n{require}    do\n      {body}True\n    ensure\n      claim: {claim}\n    end\n")
    };
    let checked = format!("{nat}\n      ");
    let lies = [
        ("map_grows", lie("m: MAP[NAT, NAT], x: NAT", "", "", "m.put(x, x).size = m.size + 1")),
        ("map_same", lie("m: MAP[NAT, NAT], x: NAT", "not m.has(x)", "", "m.put(x, x).size = m.size")),
        ("map_shrinks", lie("m: MAP[NAT, NAT], x: NAT", "m.size > 0", &checked, "m.remove(x).size + 1 = m.size")),
        // Keys of `STRING` are not known to be `=` when they are equal.
        ("map_text", lie("m: MAP[STRING, NAT], k: STRING", "m.has(k)", "", "m.put(k, 1).size = m.size")),
        (
            "fold_last",
            lie(
                "m: MAP[NAT, NAT], k: NAT",
                "not m.has(k)",
                "",
                "m.put(k, 7).fold(0, fn(acc: NAT, a: NAT, b: NAT): NAT do b end) = 7",
            ),
        ),
        ("set_lost", lie("s: SET[NAT], x: NAT", "", "", "not s.add(x).has(x)")),
        ("set_union", lie("s: SET[NAT], t: SET[NAT], x: NAT", "t.has(x)", "", "s.union(t).has(x) = s.has(x)")),
        ("set_both", lie("s: SET[NAT], t: SET[NAT], x: NAT", "s.has(x)", "", "s.intersection(t).has(x)")),
        ("queue_newest", lie("q: QUEUE[NAT]", "q.size = 1", "", "q.push(9).peek = Some(9)")),
        ("queue_stays", lie("q: QUEUE[NAT]", "q.size > 0", "", "q.pop.items = q.items")),
        ("heap_greatest", lie("h: HEAP[NAT], y: NAT", "h.count(y) > 0", &checked, "not h.peek.get.less_than(y)")),
        ("heap_lost", lie("h: HEAP[NAT], x: NAT", "h.size < 9", &checked, "h.push(x).count(x) = h.count(x)")),
        ("heap_kept", lie("h: HEAP[NAT], y: NAT", "h.size > 0", &checked, "h.pop_min.count(y) = h.count(y)")),
        ("heap_order", lie("h: HEAP[NAT], x: NAT", "h.size < 9", &checked, "h.push(x).peek = Some(x)")),
    ];
    for (name, routine) in lies {
        let p = Project::new(&format!("lie_{name}"), &[("client.gdl", &client(&format!("  {routine}")))]);
        assert_refused(&p.check(), "claim");
    }
}

/// `push` and `pop_min` need `equals` to be `=`, which does not prove for `STRING`, and `push`
/// on a queue or a heap needs room.
#[test]
fn a_heap_needs_agreeing_keys_and_a_push_needs_room() {
    let heap = client("  f(h: HEAP[STRING], s: STRING): HEAP[STRING]\n    require\n      room: h.size < 9\n    do\n      h.push(s)\n    end\n");
    assert_refused(&Project::new("heap_text", &[("client.gdl", &heap)]).check(), "agrees");
    let queue = client("  f(q: QUEUE[NAT]): QUEUE[NAT]\n    do\n      q.push(1)\n    end\n");
    assert_refused(&Project::new("queue_room", &[("client.gdl", &queue)]).check(), "room");
    for key in ["BYTES", "CLIENT"] {
        for module in ["SET", "HEAP"] {
            let routine = client(&format!("  f(s: {module}[{key}]): NAT\n    do\n      s.size\n    end\n"));
            let p = Project::new(&format!("key_{}_{}", module.to_lowercase(), key.to_lowercase()), &[("client.gdl", &routine)]);
            assert_refused(&p.check(), "E0317: unsatisfied_bound");
        }
    }
}

/// Items put out of order and repeated, a heap used twice, and 100,000 numbers through a heap
/// and a queue. Then the C: none of the small cases leaves a live block, and the big heap
/// copies its array once at most, when the loop is handed a heap it does not own.
#[test]
fn the_collections_run_in_c_and_a_heap_changes_in_place() {
    let p = Project::new("runtime", &[("main.gdl", include_str!("collections_program.gdl"))]);
    assert_ok(&p.check());
    let small = "1,3,5,9 | 1,3,4,5,9 | 3,9 | 1,5,9 | apple,pear | yes | 1,2,3 | 2,3,4 | 2 | - | two,three | 0,2,2,5,7,8 | 2,2,5 | 0 | -";
    assert_eq!(p.run(), format!("{small} | 100000 | 5000050000\n"));
    assert_eq!(p.leaks("MAIN__small_length()"), format!("0 0 {}", small.len() * 51));
    assert_eq!(p.leaks("MAIN__heap_run(1000)"), "0 0 51000");
    assert_eq!(p.leaks("MAIN__queue_run(1000)"), format!("0 0 {}", 500500 * 51));
    let copies = p.harness(
        "    uint64_t c0 = gardel_array_copies();\n    uint64_t r = MAIN__heap_run(1000);\n    printf(\"%llu %d\\n\", (unsigned long long)r, gardel_array_copies() - c0 <= 2);",
    );
    assert_eq!(copies, "1000 1");
}

/// A record that holds an array, given up by a loop, by a chain of calls, and where it must
/// not be: while another name holds it, while a `match` binder points into it, or while its
/// array is read twice. The first case is a plain array (Task H.5), whose `match` binder was
/// left pointing at an item the `set` had released.
#[test]
fn an_array_is_taken_only_where_nothing_still_sees_it() {
    let p = Project::new("bag", &[("bag.gdl", include_str!("collections_bag.gdl")), ("main.gdl", include_str!("collections_bag_program.gdl"))]);
    assert_ok(&p.check());
    let text = "item number 2 | item number 1 | s! | item number 2 | item number 1 | item number 2 | start | c | start+d | c";
    assert_eq!(p.run(), format!("{text}\n"));
    assert_eq!(p.leaks("MAIN__report_length()"), format!("0 0 {}", text.len() * 51));
}
