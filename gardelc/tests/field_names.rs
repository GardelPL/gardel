//! A field named like a built-in method of lists and bytes, `at` or `head`, is read as the field
//! (F.11d). `Current.at` was taken for `LIST.at`, whose argument it has not, and `gardelc`
//! panicked; `Current.head` became `Seq_head(Current)`, which Boogie refused (`E0199`). A field
//! named like a word Boogie reserves, `uses` or `type`, was a parameter of its constructor by
//! that name, and Boogie refused it (`E0199`). A field read through a record bound by `let`,
//! `let h = { Current with .. }` then `h.rest.length`, took `rest` for a list's and `.length` for
//! `Seq_length` (`E0199`).

use std::fs;
use std::process::Command;

const SPOT: &str = "module SPOT

representation {NONE}
  at: NAT
  head: NAT
  uses: MARK
  rest: BYTES

create {ANY}

  make(line: NAT, first: NAT): SPOT
    do
      { at = line, head = first, uses = Used(first), rest = BYTES.empty }
    ensure
      kept_line: Result.line = line
      kept_first: Result.first = first
      kept_mark: Result.marked = first
    end

feature {ANY}

  line: NAT
    do
      Current.at
    end

  first: NAT
    do
      Current.head
    end

  -- A field read through a record bound by `let`, whose type is the module's own.
  waiting: NAT
    do
      let moved = { Current with at = 1 }
      moved.rest.length
    end

  marked: NAT
    do
      match Current.uses
        case Used(n) then n
        case Typed(n) then n
      end
    end

end
";

const MARK: &str = "module MARK

representation {ANY}
  | Used(uses: NAT)
  | Typed(type: NAT)

end
";

const MAIN: &str = "program SPOT_MAIN

feature {ANY}

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let s = SPOT.make(7, 3)
      with IO
        IO.print_line(NUMBER_TEXT.nat_to_text(s.line))
        IO.print_line(NUMBER_TEXT.nat_to_text(s.first))
        IO.print_line(NUMBER_TEXT.nat_to_text(s.marked))
        IO.print_line(NUMBER_TEXT.nat_to_text(s.waiting))
        IO.exit(0)
      end
    end

end
";

#[test]
fn a_field_named_like_a_built_in_or_a_boogie_word_is_the_field() {
    let dir = std::env::temp_dir().join(format!("gardelc_field_names_{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(dir.join("src")).expect("temporary directory");
    fs::write(dir.join("Gardel.toml"), "[project]\nname = \"spot\"\nversion = \"0.1.0\"\n").expect("manifest");
    fs::write(dir.join("src/spot.gdl"), SPOT).expect("source");
    fs::write(dir.join("src/spot_main.gdl"), MAIN).expect("source");
    fs::write(dir.join("src/mark.gdl"), MARK).expect("source");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir)
        .env("GARDEL_CACHE_DIR", dir.join("cache"))
        .args(["--project", ".", "--strict", "--run"])
        .output()
        .expect("gardelc runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    drop(fs::remove_dir_all(&dir));
    assert!(text.contains("0 errors, 0 warnings"), "{text}");
    assert!(String::from_utf8_lossy(&out.stdout).lines().map(str::trim).collect::<Vec<_>>().ends_with(&["7", "3", "3", "0"]), "{text}");
}
