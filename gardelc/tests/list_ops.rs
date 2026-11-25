//! `LIST_OPS`' operations and laws (Task H.21), through the command line: a client that calls
//! each law proves what it states, a false version of each is refused, and the operations run
//! in C as their contracts say.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// The suffix gardelc gives an executable: `.exe` on Windows, none elsewhere.
const EXE: &str = std::env::consts::EXE_SUFFIX;

struct Project {
    dir: PathBuf,
}

impl Project {
    fn new(name: &str, source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_list_ops_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(dir.join("src")).expect("temporary directory");
        let manifest = format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", name.replace('_', "-"));
        fs::write(dir.join("Gardel.toml"), manifest).expect("manifest");
        fs::write(dir.join("src").join("main.gdl"), source).expect("source file");
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

/// Each law, called by a client that sees `LIST_OPS` only through its contracts and laws.
const TRUTHS: &str = r#"program MAIN

feature {ANY}

  halves(xs: LIST[U64], n: NAT): LIST[U64]
    do
      check
        whole: LIST_OPS.take_drop(xs, n)
      end
      LIST_OPS.take(xs, n).append(LIST_OPS.drop(xs, n))
    ensure
      same: Result = xs
    end

  sized(xs: LIST[U64], n: NAT): BOOL
    require
      fits: n <= xs.length
    do
      True
    ensure
      taken: LIST_OPS.take(xs, n).length = n
      rest: LIST_OPS.drop(xs, n).length = xs.length - n
    end

  spans(xs: LIST[U64]): LIST[U64]
    do
      let small = fn(x: U64): BOOL do x < 5 end
      check
        whole: LIST_OPS.take_while_drop_while(xs, small)
      end
      LIST_OPS.take_while(xs, small).append(LIST_OPS.drop_while(xs, small))
    ensure
      same: Result = xs
    end

  twice(xs: LIST[U64]): LIST[U64]
    do
      check
        back: LIST_OPS.reverse_reverse(xs)
      end
      LIST_OPS.reverse(LIST_OPS.reverse(xs))
    ensure
      same: Result = xs
    end

  both(xs: LIST[U64], ys: LIST[U64]): LIST[U64]
    do
      check
        swapped: LIST_OPS.reverse_append(xs, ys)
      end
      LIST_OPS.reverse(xs.append(ys))
    ensure
      swapped: Result = LIST_OPS.reverse(ys).append(LIST_OPS.reverse(xs))
    end

  ends(xs: LIST[U64]): LIST[U64]
    require
      not_empty: xs.length > 0
    do
      check
        whole: LIST_OPS.init_last(xs)
      end
      LIST_OPS.init(xs).append(LIST.empty.prepend(LIST_OPS.last(xs)))
    ensure
      same: Result = xs
    end

  fused(xs: LIST[U64], g: U64 -> BOOL, f: BOOL -> BOOL): LIST[BOOL]
    do
      check
        one_map: LIST_OPS.map_fusion(xs, g, f)
      end
      LIST_OPS.map(LIST_OPS.map(xs, g), f)
    ensure
      one_map: Result = LIST_OPS.map(xs, FUNCTION_OPS.compose(f, g))
    end

  kept(xs: LIST[U64], f: U64 -> BOOL, p: BOOL -> BOOL): LIST[BOOL]
    do
      check
        commutes: LIST_OPS.filter_map(xs, f, p)
      end
      LIST_OPS.filter(LIST_OPS.map(xs, f), p)
    ensure
      commutes: Result = LIST_OPS.map(LIST_OPS.filter(xs, FUNCTION_OPS.compose(p, f)), f)
    end

  -- The first duality theorem, over the monoid of `or` and `False`.
  any_both_ways(xs: LIST[BOOL]): BOOL
    do
      let either = fn(p: BOOL, q: BOOL): BOOL do p or q end
      check
        agree: LIST_OPS.fold_duality_first(xs, either, False)
      end
      LIST_OPS.fold_right(xs, either, False) = LIST_OPS.fold_left(xs, either, False)
    ensure
      holds: Result
    end

  -- The second: `f` and `g` associate with each other and agree on `True`.
  second(xs: LIST[BOOL]): BOOL
    do
      let f = fn(x: BOOL, acc: BOOL): BOOL do x and acc end
      let g = fn(acc: BOOL, x: BOOL): BOOL do acc and x end
      check
        agree: LIST_OPS.fold_duality_second(xs, f, g, True)
      end
      LIST_OPS.fold_right(xs, f, True) = LIST_OPS.fold_left(xs, g, True)
    ensure
      holds: Result
    end

  -- The third: a right fold is a left fold of the reverse with its function flipped.
  third(xs: LIST[BOOL]): BOOL
    do
      let f = fn(x: BOOL, acc: BOOL): BOOL do x and not acc end
      let g = fn(acc: BOOL, x: BOOL): BOOL do x and not acc end
      check
        agree: LIST_OPS.fold_duality_third(xs, f, g, False)
      end
      LIST_OPS.fold_right(xs, f, False) = LIST_OPS.fold_left(LIST_OPS.reverse(xs), g, False)
    ensure
      holds: Result
    end

  split_fold(xs: LIST[BOOL], ys: LIST[BOOL]): BOOL
    do
      let both = fn(p: BOOL, q: BOOL): BOOL do p and q end
      check
        splits: LIST_OPS.fold_homomorphism(xs, ys, both, True)
      end
      LIST_OPS.fold_left(xs.append(ys), both, True) =
        (LIST_OPS.fold_left(xs, both, True) and LIST_OPS.fold_left(ys, both, True))
    ensure
      holds: Result
    end

  flat(xs: LIST[BOOL], f: BOOL -> LIST[BOOL]): LIST[BOOL]
    do
      check
        same: LIST_OPS.concat_map_is_concat_map(xs, f)
      end
      LIST_OPS.concat_map(xs, f)
    ensure
      same: Result = LIST_OPS.concat(LIST_OPS.map(xs, f))
    end

  units(xs: LIST[U64]): LIST[U64]
    do
      let one = fn(x: U64): LIST[U64] do LIST.empty.prepend(x) end
      check
        right_id: LIST_OPS.concat_map_right_identity(xs, one)
      end
      LIST_OPS.concat_map(xs, one)
    ensure
      same: Result = xs
    end

  snoc(x: U64, xs: LIST[U64]): LIST[U64]
    do
      check
        snoc: LIST_OPS.reverse_cons(x, xs)
      end
      LIST_OPS.reverse(xs.prepend(x))
    ensure
      last_first: Result = LIST_OPS.reverse(xs).append(LIST.empty.prepend(x))
    end

  flat_both(xs: LIST[BOOL], ys: LIST[BOOL], f: BOOL -> LIST[BOOL]): LIST[BOOL]
    do
      check
        split: LIST_OPS.concat_map_append(xs, ys, f)
      end
      LIST_OPS.concat_map(xs.append(ys), f)
    ensure
      split: Result = LIST_OPS.concat_map(xs, f).append(LIST_OPS.concat_map(ys, f))
    end

  flat_one(x: BOOL, f: BOOL -> LIST[BOOL]): LIST[BOOL]
    do
      check
        left_id: LIST_OPS.concat_map_left_unit(x, f)
      end
      LIST_OPS.concat_map(LIST.empty.prepend(x), f)
    ensure
      left_id: Result = f(x)
    end

  flat_twice(xs: LIST[BOOL], f: BOOL -> LIST[BOOL], g: BOOL -> LIST[BOOL]): BOOL
    do
      let h = fn(p: BOOL): LIST[BOOL] do LIST_OPS.concat_map(f(p), g) end
      check
        assoc: LIST_OPS.concat_map_associative(xs, f, g, h)
      end
      True
    ensure
      holds: Result
    end

  -- `SEQ.reduce` folds the sides of a `Concat` apart, and is the sequential fold.
  any_seq(s: SEQ[BOOL]): BOOL
    do
      let either = fn(p: BOOL, q: BOOL): BOOL do p or q end
      check
        folds: SEQ.reduce_folds(s, either, False)
      end
      s.reduce(either, False) = s.fold_right(either, False)
    ensure
      holds: Result
    end

  -- B&W §3.2: [(i, j) | i <- [1..4]; j <- [i+1..4]].
  pairs: LIST[TUPLE[NAT, NAT]]
    do
      LIST_OPS.concat_map(LIST_OPS.range(1, 5), fn(i: NAT): LIST[TUPLE[NAT, NAT]] do if i < 5 then LIST_OPS.concat_map(LIST_OPS.range(i + 1, 5), fn(j: NAT): LIST[TUPLE[NAT, NAT]] do LIST.empty.prepend((i, j)) end) else LIST.empty end end)
    ensure
      six: Result.length = 6
    end

  code(xs: LIST[U64]): U64
    do
      LIST_OPS.fold_left(xs, fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0)
    end

  show(xs: LIST[U64]): STRING
    do
      NUMBER_TEXT.nat_to_text(code(xs))
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let xs = LIST_OPS.range(1, 6)
      let small = fn(x: U64): BOOL do x < 3 end
      let ps = pairs
      with IO
        IO.print_line(show(LIST_OPS.take(xs, 2)))
        IO.print_line(show(LIST_OPS.drop(xs, 2)))
        IO.print_line(show(LIST_OPS.take(xs, 9)))
        IO.print_line(NUMBER_TEXT.nat_to_text(LIST_OPS.drop(xs, 9).length))
        IO.print_line(show(LIST_OPS.reverse(xs)))
        IO.print_line(show(LIST_OPS.take_while(xs, small)))
        IO.print_line(show(LIST_OPS.drop_while(xs, small)))
        IO.print_line(show(LIST_OPS.zip_with(xs, LIST_OPS.reverse(xs), fn(a: U64, b: U64): U64 do a *% b end)))
        IO.print_line(NUMBER_TEXT.nat_to_text(LIST_OPS.zip(xs, LIST_OPS.take(xs, 3)).length))
        IO.print_line(show(LIST.empty.prepend(LIST_OPS.last(xs))))
        IO.print_line(show(LIST_OPS.init(xs)))
        IO.print_line(show(LIST_OPS.concat(LIST.empty.prepend(xs).prepend(LIST_OPS.take(xs, 2)))))
        IO.print_line(show(LIST_OPS.iterate_n(fn(x: U64): U64 do x *% 2 end, 1, 6)))
        IO.print_line(if any_seq(SEQ.singleton(False).concat(SEQ.singleton(True))) then "same" else "differs" end)
        IO.print_line(if SEQ.singleton(False).concat(SEQ.singleton(True)).reduce(fn(p: BOOL, q: BOOL): BOOL do p or q end, False) then "1" else "0" end)
        IO.print_line(NUMBER_TEXT.nat_to_text(ps.length))
        IO.print_line(NUMBER_TEXT.nat_to_text(ps.head.first))
        IO.print_line(NUMBER_TEXT.nat_to_text(ps.head.second))
        IO.exit(0)
      end
    end

end
"#;

#[test]
fn every_law_is_used_by_a_client_and_the_operations_run() {
    let p = Project::new("truths", TRUTHS);
    let out = p.check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let run = Command::new(p.dir.join("build").join(format!("main_bin{EXE}"))).output().expect("the program runs");
    assert!(run.status.success(), "{run:?}");
    let printed = String::from_utf8(run.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines,
        [
            "12", "345", "12345", "0", "54321", "12", "345", "58985", "3", "5", "1234", "1212345",
            "124992", "same", "1", "6", "1", "2",
        ],
        "{printed}"
    );
}

/// False versions of the laws and contracts, each refused by the verifier.
const LIES: &str = r#"functions MAIN

feature {ANY}

  -- `take` is total: without `fits`, it may be shorter than `n`.
  long_take(xs: LIST[U64], n: NAT): BOOL
    do
      True
    ensure
      lie: LIST_OPS.take(xs, n).length = n
    end

  same_reverse(xs: LIST[U64]): BOOL
    do
      True
    ensure
      lie: LIST_OPS.reverse(xs) = xs
    end

  -- The reverse of an append is not the appended reverses in the same order.
  in_order(xs: LIST[U64], ys: LIST[U64]): BOOL
    do
      check
        swapped: LIST_OPS.reverse_append(xs, ys)
      end
      True
    ensure
      lie: LIST_OPS.reverse(xs.append(ys)) = LIST_OPS.reverse(xs).append(LIST_OPS.reverse(ys))
    end

  -- A function that is no monoid's: the first duality theorem does not apply.
  first_of(xs: LIST[BOOL]): BOOL
    do
      let first = fn(p: BOOL, q: BOOL): BOOL do p end
      check
        agree: LIST_OPS.fold_duality_first(xs, first, False)
      end
      True
    end

  -- The third theorem with a function that is not the flip.
  unflipped(xs: LIST[BOOL]): BOOL
    do
      let f = fn(x: BOOL, acc: BOOL): BOOL do x and not acc end
      let g = fn(acc: BOOL, x: BOOL): BOOL do acc and not x end
      check
        agree: LIST_OPS.fold_duality_third(xs, f, g, False)
      end
      True
    end

  -- `h` is not `f` then `g`.
  not_composed(xs: LIST[BOOL], f: BOOL -> LIST[BOOL], g: BOOL -> LIST[BOOL]): BOOL
    do
      check
        assoc: LIST_OPS.concat_map_associative(xs, f, g, f)
      end
      True
    end

  -- `and` with `False` as its identity is no monoid.
  no_monoid(s: SEQ[BOOL]): BOOL
    do
      let both = fn(p: BOOL, q: BOOL): BOOL do p and q end
      check
        folds: SEQ.reduce_folds(s, both, False)
      end
      True
    end

  last_of_none(xs: LIST[U64]): U64
    do
      LIST_OPS.last(xs)
    end

  seven: LIST[TUPLE[NAT, NAT]]
    do
      LIST_OPS.concat_map(LIST_OPS.range(1, 5), fn(i: NAT): LIST[TUPLE[NAT, NAT]] do if i < 5 then LIST_OPS.concat_map(LIST_OPS.range(i + 1, 5), fn(j: NAT): LIST[TUPLE[NAT, NAT]] do LIST.empty.prepend((i, j)) end) else LIST.empty end end)
    ensure
      lie: Result.length = 7
    end

end
"#;

#[test]
fn a_false_version_of_each_is_refused() {
    let p = Project::new("lies", LIES);
    let out = p.check();
    let text = all(&out);
    assert!(!out.status.success(), "must fail:\n{text}");
    let errors: Vec<&str> = text.lines().filter(|l| l.contains("[ERROR]")).collect();
    for routine in ["long_take", "same_reverse", "in_order", "first_of", "unflipped", "not_composed", "no_monoid", "last_of_none", "seven"] {
        assert!(
            errors.iter().any(|l| l.contains(&format!("in `{routine}`"))),
            "`{routine}` must be refused:\n{text}"
        );
    }
    assert_eq!(errors.len(), 9, "one refusal each:\n{text}");
}
