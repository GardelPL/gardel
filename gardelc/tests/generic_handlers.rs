//! A generic module that stores functions over its type parameter, as `grest`'s routes will
//! hold handlers over a service's state (F.6). A client chains calls on it and proves facts
//! about them through the contracts alone, and the program computes the same in C.
//!
//! Two faults kept this from verifying:
//! - an axiom of a routine whose type parameter appears only in its result, such as
//!   `ROUTER[S].empty`, was stated of `ROUTER[int]` alone, as only its trigger carried the
//!   result type;
//! - Boogie 3.4.3's type axiom for a generic function with a scalar parameter before another
//!   one, such as `on(k: NAT, h: (s: S, n: NAT) -> OUTCOME[S])`, guards the wrong argument, so
//!   no axiom fired on `on(on(..))`. `gardelc` now moves scalar parameters last in Boogie.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct World {
    dir: PathBuf,
}

impl World {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("gardelc_generic_{}_{name}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        let world = Self { dir };
        world.write("lib/Gardel.toml", "[project]\nname = \"glib\"\nversion = \"0.1.0\"\n");
        world.write("app/Gardel.toml", "[project]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nglib = { path = \"../lib\" }\n");
        world.write("lib/src/outcome.gdl", OUTCOME);
        world.write("lib/src/route.gdl", ROUTE);
        world.write("lib/src/router.gdl", ROUTER);
        world.write("app/src/counter.gdl", COUNTER);
        world
    }

    fn write(&self, path: &str, text: &str) {
        let p = self.dir.join(path);
        fs::create_dir_all(p.parent().expect("a parent")).expect("directory");
        fs::write(p, text).expect("file");
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gardelc"))
            .current_dir(self.dir.join("app"))
            .env("GARDEL_CACHE_DIR", self.dir.join("cache"))
            .args(["--project", ".", "--strict"])
            .output()
            .expect("gardelc runs")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.dir));
    }
}

fn all(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

const OUTCOME: &str = "module OUTCOME[S]

representation {NONE}
  next: S
  said: NAT

create {ANY}

  make(s: S, n: NAT): OUTCOME[S]
    do
      { next = s, said = n }
    ensure
      said_set: Result.answer = n
    end

feature {ANY}

  state: S
    do
      Current.next
    end

  answer: NAT
    do
      Current.said
    end

end
";

const ROUTE: &str = "module ROUTE[S]

representation {NONE}
  key: NAT
  run: (s: S, n: NAT) -> OUTCOME[S]

create {ANY}

  make(k: NAT, h: (s: S, n: NAT) -> OUTCOME[S]): ROUTE[S]
    do
      { key = k, run = h }
    ensure
      key_set: Result.route_key = k
    end

feature {ANY}

  route_key: NAT
    do
      Current.key
    end

  respond(s: S, n: NAT): OUTCOME[S]
    do
      Current.run(s, n)
    end

end
";

const ROUTER: &str = "module ROUTER[S]

representation {NONE}
  table: LIST[ROUTE[S]]
  n_routes: NAT

invariant
  counted: n_routes = table.length
  bounded: n_routes <= 64

create {ANY}

  empty: ROUTER[S]
    do
      { table = LIST.empty, n_routes = 0 }
    ensure
      none: Result.route_count = 0
    end

  find(rs: LIST[ROUTE[S]], s: S, k: NAT, n: NAT): OUTCOME[S]
    variant
      rs.length
    do
      if rs.length = 0 then
        OUTCOME.make(s, 404)
      else if rs.head.route_key = k then
        rs.head.respond(s, n)
      else
        ROUTER.find(rs.tail, s, k, n)
      end end
    ensure
      missing: rs.length = 0 implies Result.answer = 404
    end

feature {ANY}

  route_count: NAT
    do
      Current.n_routes
    ensure
      bounded: Result <= 64
    end

  on(k: NAT, h: (s: S, n: NAT) -> OUTCOME[S]): ROUTER[S]
    require
      room: Current.route_count < 64
    do
      { table = Current.table.append(LIST.singleton(ROUTE.make(k, h))), n_routes = Current.n_routes + 1 }
    ensure
      grew: Result.route_count = Current.route_count + 1
    end

  dispatch(s: S, k: NAT, n: NAT): OUTCOME[S]
    do
      ROUTER.find(Current.table, s, k, n)
    ensure
      no_routes: Current.route_count = 0 implies Result.answer = 404
    end

end
";

const COUNTER: &str = "module COUNTER

representation {NONE}
  total: NAT

invariant
  capped: total <= 1000

create {ANY}

  zero: COUNTER
    do
      { total = 0 }
    ensure
      empty: Result.value = 0
    end

feature {ANY}

  value: NAT
    do
      Current.total
    ensure
      capped: Result <= 1000
    end

  plus(n: NAT): COUNTER
    do
      if n <= 1000 - Current.total then { total = Current.total + n } else Current end
    ensure
      grew_or_same: Result.value >= Current.value
    end

end
";

fn app(claims: &str) -> String {
    format!(
        "program APP

feature {{ANY}}

  peek(c: COUNTER, n: NAT): OUTCOME[COUNTER]
    do
      OUTCOME.make(c, c.value)
    end

  add(c: COUNTER, n: NAT): OUTCOME[COUNTER]
    do
      let next = c.plus(n)
      OUTCOME.make(next, next.value)
    end

  routes: ROUTER[COUNTER]
    do
      ROUTER.empty.on(1, peek).on(2, add)
    ensure
{claims}
    end

  -- What the routes answer to three requests in turn: add 5, add 7, then peek.
  answers: OUTCOME[COUNTER]
    do
      let r = routes
      let a = r.dispatch(COUNTER.zero, 2, 5)
      let b = r.dispatch(a.state, 2, 7)
      r.dispatch(b.state, 1, 0)
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(NUMBER_TEXT.nat_to_text(routes.dispatch(COUNTER.zero, 2, 5).answer))
        IO.print_line(NUMBER_TEXT.nat_to_text(answers.answer))
        IO.print_line(NUMBER_TEXT.nat_to_text(routes.dispatch(answers.state, 3, 0).answer))
        IO.exit(0)
      end
    end

end
"
    )
}

/// A chain of generic calls whose handlers come after a number verifies, through the
/// contracts alone, and the program computes the answers in C.
#[test]
fn a_chain_of_generic_calls_with_handlers_verifies_and_runs() {
    let world = World::new("chain");
    world.write("app/src/app.gdl", &app(
        "      two: Result.route_count = 2
      empty_none: ROUTER.empty.on(1, peek).route_count = 1",
    ));
    let out = world.check();
    assert!(out.status.success() && all(&out).contains("0 errors, 0 warnings"), "{}", all(&out));
    let run = Command::new(world.dir.join("app/build/app_bin.exe"))
        .stdout(Stdio::piped())
        .output()
        .expect("the program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout).split_whitespace().collect::<Vec<_>>(), ["5", "12", "404"]);
}

#[test]
fn a_claim_past_a_chain_of_generic_calls_is_refused() {
    let world = World::new("lie");
    world.write("app/src/app.gdl", &app("      claim: Result.route_count = 3"));
    let out = world.check();
    assert!(!out.status.success(), "{}", all(&out));
    assert!(all(&out).contains("condition `claim` failed"), "{}", all(&out));
}
