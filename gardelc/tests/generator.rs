//! `GENERATOR` and `UNFOLD` (Task H.22), through the command line: clients that see no body
//! prove what the laws state, run in C, and a false version of each claim is refused. Since
//! H.22b, the items of a generator of numbers are known to be in range, for any prefix.

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
        let dir = std::env::temp_dir().join(format!("gardelc_generator_{}_{name}", std::process::id()));
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

const TRUTHS: &str = r#"program MAIN

feature {ANY}

  -- iterate (+1) from `from`: the naturals, wrapping at the end of `NAT`.
  naturals(from: NAT): GENERATOR[NAT, NAT]
    do
      GENERATOR.make(from, fn(s: NAT): TUPLE[NAT, NAT] do (s, s +% 1) end)
    end

  -- Fibonacci: the state is the pair of the next two numbers.
  fibs(a: NAT): GENERATOR[TUPLE[NAT, NAT], NAT]
    do
      GENERATOR.make((a, 1), fn(p: TUPLE[NAT, NAT]): TUPLE[NAT, TUPLE[NAT, NAT]] do (p.first, (p.second, p.first +% p.second)) end)
    end

  -- SplitMix64 from a seed: the state is the counter, each item a mix of it.
  splitmix(seed: U64): GENERATOR[U64, U64]
    do
      GENERATOR.make(seed, fn(s: U64): TUPLE[U64, U64] do
        let t = s +% 11400714819323198485
        let a = (t ^ (t >> 30)) *% 13787848793156543929
        let b = (a ^ (a >> 27)) *% 10723151780598845931
        let pair = (b ^ (b >> 31), t)
        pair
      end)
    end

  -- The naturals again, built with a record literal rather than `make`.
  literal_naturals: GENERATOR[NAT, NAT]
    do
      { state = 0, next = fn(s: NAT): TUPLE[NAT, NAT] do (s, s +% 1) end }
    ensure
      starts: Result.state = 0
    end

  -- A retry schedule: 100 ms, doubled each time, at most 10 s.
  delays: GENERATOR[NAT, NAT]
    do
      GENERATOR.iterate(100, fn(d: NAT): NAT do if d >= 5000 then 10000 else d * 2 end end)
    end

  -- The tenth Fibonacci number, from the contracts alone.
  tenth: NAT
    do
      fibs(0).nth(9)
    ensure
      is_34: Result = 34
    end

  -- Every item is a number up to 10000 when the state is and the step keeps it: by induction
  -- on the number of items.
  capped(g: GENERATOR[NAT, NAT], n: NAT): BOOL
    require
      starts: g.state >= 0 and g.state <= 10000
      keeps: forall d: NAT. d <= 10000 implies
        (g.next(d).first >= 0 and g.next(d).first <= 10000 and g.next(d).second >= 0 and g.next(d).second <= 10000)
    variant
      n
    do
      if n = 0 then True else capped(g.tail, n - 1) end
    ensure
      holds: Result
      bounded: forall k: NAT. k < n implies (g.nth(k) >= 0 and g.nth(k) <= 10000)
    end

  -- No delay of the schedule is over 10 s.
  delay_at(k: NAT): NAT
    require
      some: k < 1000000
    do
      check
        capped: capped(delays, k + 1)
      end
      delays.nth(k)
    ensure
      at_most_10s: Result <= 10000
    end

  -- B&W §7.3's sieve, on a prefix: a generator has no `filter`, since its next item that
  -- passes may never come, so the sieve takes the numbers below a bound first.
  sieve(xs: LIST[NAT]): LIST[NAT]
    variant
      xs.length
    do
      if xs.length = 0 then
        LIST.empty
      else
        let p = xs.head
        sieve(LIST_OPS.filter(xs.tail, fn(x: NAT): BOOL do if p = 0 then True else x % p != 0 end end)).prepend(p)
      end
    end

  first_five: LIST[NAT]
    do
      naturals(3).take(5)
    ensure
      length: Result.length = 5
    end

  -- The items of any generator of numbers are in range (H.22b): a prefix of any length, an
  -- item at any index, and those of what follows a prefix, as numbers the client returns.
  prefix(g: GENERATOR[NAT, NAT], n: NAT): LIST[NAT]
    do
      g.take(n)
    end

  item(g: GENERATOR[NAT, NAT], n: NAT): NAT
    do
      g.nth(n)
    end

  later(g: GENERATOR[NAT, NAT], n: NAT): LIST[NAT]
    do
      g.drop(n).tail.take(3)
    end

  found(g: GENERATOR[NAT, NAT]): OPTION[NAT]
    do
      g.find_within(10, fn(x: NAT): BOOL do x > 5 end)
    end

  -- A generator of truth values from a state of numbers.
  flags: GENERATOR[NAT, BOOL]
    do
      GENERATOR.make(0, fn(s: NAT): TUPLE[BOOL, NAT] do (s % 2 = 0, s +% 1) end)
    end

  -- The law clients take generators of `BOOL`.
  at_in_prefix(g: GENERATOR[NAT, BOOL], n: NAT, k: NAT): BOOL
    require
      inside: k < n
    do
      check
        same: GENERATOR.nth_take(g, n, k)
      end
      g.nth(k)
    ensure
      from_prefix: Result = LIST_OPS.at(g.take(n), k)
    end

  split(g: GENERATOR[NAT, BOOL], n: NAT, m: NAT): LIST[BOOL]
    require
      shorter: m <= n
    do
      check
        split: GENERATOR.take_split(g, n, m)
      end
      g.take(n)
    ensure
      split: Result = g.take(m).append(g.drop(m).take(n - m))
    end

  mapped(g: GENERATOR[NAT, BOOL], f: BOOL -> BOOL, n: NAT): LIST[BOOL]
    do
      check
        prefix: GENERATOR.map_take(g, f, n)
      end
      g.map(f).take(n)
    ensure
      prefix: Result = LIST_OPS.map(g.take(n), f)
    end

  -- Pairs of two items from a list: an `UNFOLD` that ends with the list.
  pairs_of(xs: LIST[BOOL]): LIST[LIST[BOOL]]
    do
      UNFOLD.make(xs, fn(ys: LIST[BOOL]): OPTION[TUPLE[LIST[BOOL], LIST[BOOL]]] do
        if ys.length = 0 then None else Some((LIST_OPS.take(ys, 2), LIST_OPS.drop(ys, 2))) end
      end).to_list(fn(ys: LIST[BOOL]): NAT do ys.length end)
    end

  countdown(n: NAT): UNFOLD[NAT, NAT]
    do
      UNFOLD.make(n, fn(k: NAT): OPTION[TUPLE[NAT, NAT]] do if k = 0 then None else Some((k, k - 1)) end end)
    end

  code(xs: LIST[NAT]): U64
    do
      LIST_OPS.fold_left(xs, fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0)
    end

  show(xs: LIST[NAT]): STRING
    do
      NUMBER_TEXT.nat_to_text(code(xs))
    end

  num(n: NAT): STRING
    do
      NUMBER_TEXT.nat_to_text(n)
    end

  -- Each line the program prints, proven on its own. A combinator's items are in range as its
  -- arguments' are (H.22b), so each line prints the value it computes. `zip`'s line compares
  -- it: an item of a zip of pairs at a fixed index took 5M to 30M resources to prove in range.
  line_naturals: STRING do show(naturals(1).take(5)) end
  line_mix_0: STRING do num(LIST_OPS.at(splitmix(0).take(3), 0)) end
  line_mix_1: STRING do num(LIST_OPS.at(splitmix(0).take(3), 1)) end
  line_mix_2: STRING do num(LIST_OPS.at(splitmix(0).take(3), 2)) end
  line_delays_sum: STRING do num(LIST_OPS.fold_left(delays.take(8), fn(acc: NAT, x: NAT): NAT do acc +% x end, 0)) end
  line_delays_last: STRING do num(LIST_OPS.last(delays.take(8))) end
  line_cycle: STRING do show(GENERATOR.cycle(LIST.empty.prepend(3).prepend(2).prepend(1)).take(7)) end
  line_zip_with: STRING do show(naturals(1).zip_with(naturals(10), fn(a: NAT, b: NAT): NAT do a *% b end).take(3)) end
  line_zip: STRING do if naturals(1).zip(fibs(0)).nth(6).second = 8 then "8" else "wrong" end end
  line_scan: STRING do show(naturals(1).scan(fn(acc: NAT, x: NAT): NAT do acc +% x end, 0).take(5)) end
  line_drop: STRING do show(naturals(0).drop(4).take(3)) end
  line_found: STRING do num(naturals(0).find_within(10, fn(x: NAT): BOOL do x > 5 end).unwrap_or(99)) end
  line_not_found: STRING do num(naturals(0).find_within(3, fn(x: NAT): BOOL do x > 5 end).unwrap_or(99)) end
  line_while: STRING do show(naturals(0).take_while_within(10, fn(x: NAT): BOOL do x < 3 end)) end
  line_pairs: STRING do num(pairs_of(LIST.empty.prepend(True).prepend(False).prepend(True).prepend(True).prepend(False)).length) end
  line_paired: STRING do num(LIST_OPS.concat(pairs_of(LIST.empty.prepend(True).prepend(False).prepend(True).prepend(True).prepend(False))).length) end
  line_primes: STRING do num(sieve(LIST_OPS.range(2, 30)).length) end
  line_delay: STRING do num(delay_at(1000)) end
  line_literal: STRING do show(literal_naturals.take(4)) end
  line_flags: STRING do num(flags.take(4).length) end
  line_countdown: STRING do if LIST_OPS.fold_left(countdown(5).take(3), fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0) = 543 then "543" else "wrong" end end
  line_short: STRING do if LIST_OPS.fold_left(countdown(2).take(9), fn(acc: U64, x: U64): U64 do acc *% 10 +% x end, 0) = 21 then "21" else "wrong" end end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(line_naturals)
        IO.print_line(num(tenth))
        IO.print_line(line_mix_0)
        IO.print_line(line_mix_1)
        IO.print_line(line_mix_2)
        IO.print_line(line_delays_sum)
        IO.print_line(line_delays_last)
        IO.print_line(line_cycle)
        IO.print_line(line_zip_with)
        IO.print_line(line_zip)
        IO.print_line(line_scan)
        IO.print_line(line_drop)
        IO.print_line(line_found)
        IO.print_line(line_not_found)
        IO.print_line(line_while)
        IO.print_line(line_pairs)
        IO.print_line(line_paired)
        IO.print_line(line_primes)
        IO.print_line(line_delay)
        IO.print_line(line_literal)
        IO.print_line(line_countdown)
        IO.print_line(line_short)
        IO.print_line(line_flags)
        IO.exit(0)
      end
    end

end
"#;

#[test]
fn clients_prove_the_laws_and_the_generators_run() {
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
            "12345",
            "34",
            "16294208416658607535",
            "7960286522194355700",
            "487617019471545679",
            "22700",
            "10000",
            "1231231",
            "1256",
            "8",
            "1370",
            "456",
            "6",
            "99",
            "12",
            "3",
            "5",
            "10",
            "10000",
            "123",
            "543",
            "21",
            "4",
        ],
        "{printed}"
    );
}

const LIES: &str = r#"functions MAIN

feature {ANY}

  -- `s + 1` overflows at the end of `NAT`.
  naturals(from: NAT): GENERATOR[NAT, NAT]
    do
      GENERATOR.make(from, fn(s: NAT): TUPLE[NAT, NAT] do (s, s + 1) end)
    end

  fibs: GENERATOR[TUPLE[NAT, NAT], NAT]
    do
      GENERATOR.make((0, 1), fn(p: TUPLE[NAT, NAT]): TUPLE[NAT, TUPLE[NAT, NAT]] do (p.first, (p.second, p.first +% p.second)) end)
    end

  fifth: NAT
    do
      fibs.nth(4)
    ensure
      lie: Result = 4
    end

  longer(g: GENERATOR[NAT, NAT], n: NAT): BOOL
    do
      True
    ensure
      lie: g.take(n).length = n + 1
    end

  -- The prefix of a mapped generator is not the prefix of another.
  wrong_map(g: GENERATOR[NAT, NAT], f: NAT -> BOOL, h: NAT -> BOOL, n: NAT): BOOL
    do
      check
        prefix: GENERATOR.map_take(g, f, n)
      end
      True
    ensure
      lie: g.map(f).take(n) = LIST_OPS.map(g.take(n), h)
    end

  -- The item at `k` of a prefix too short to hold it.
  outside(g: GENERATOR[NAT, NAT], n: NAT): BOOL
    do
      check
        same: GENERATOR.nth_take(g, n, n)
      end
      True
    end

  counting(from: NAT): GENERATOR[NAT, NAT]
    do
      GENERATOR.make(from, fn(s: NAT): TUPLE[NAT, NAT] do (s, s +% 1) end)
    end

  -- An item's range is its type's, not a bound the step does not keep.
  small(n: NAT): NAT
    require
      some: n < 1000
    do
      counting(0).nth(n)
    ensure
      lie: Result < 100
    end

  -- A generator of `I64`s is not one of `NAT`s.
  signed(g: GENERATOR[I64, I64]): I64
    do
      g.head
    ensure
      lie: Result >= 0
    end

  -- `map`'s items are its function's, in its range.
  mapped_signed: I64
    do
      counting(0).map(fn(x: NAT): I64 do -1 end).head
    ensure
      lie: Result >= 0
    end

  any_cycle(xs: LIST[NAT]): GENERATOR[NAT, NAT]
    do
      GENERATOR.cycle(xs)
    end

  -- A measure that does not shrink: the list may be endless.
  forever(xs: LIST[BOOL]): LIST[BOOL]
    do
      UNFOLD.make(xs, fn(ys: LIST[BOOL]): OPTION[TUPLE[BOOL, LIST[BOOL]]] do
        if ys.length = 0 then None else Some((ys.head, ys)) end
      end).to_list(fn(ys: LIST[BOOL]): NAT do ys.length end)
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
    for routine in ["naturals", "fifth", "longer", "wrong_map", "outside", "small", "signed", "mapped_signed", "any_cycle", "forever"] {
        assert!(
            errors.iter().any(|l| l.contains(&format!("in `{routine}`"))),
            "`{routine}` must be refused:\n{text}"
        );
    }
    assert_eq!(errors.len(), 10, "one refusal each:\n{text}");
    // A refusal for want of time proves nothing false.
    assert!(!errors.iter().any(|l| l.contains("Timeout")), "refused by a counterexample, not a timeout:\n{text}");
}
