//! `LIST.length` in C (Task F.6): each cell keeps the length of the list it starts, so `length`
//! is constant time. The lengths `prepend` and `append` give must be the ones the list has,
//! whichever side is empty, and a list shared by two others keeps its own.

use std::fs;
use std::process::Command;

const PROGRAM: &str = "program MAIN

feature {ANY}

  n(xs: LIST[NAT]): STRING
    do
      NUMBER_TEXT.nat_to_text(xs.length)
    end

  count(xs: LIST[NAT], acc: NAT): NAT
    variant
      xs.length
    do
      if xs.length = 0 then acc else count(xs.tail, acc % 1000000 + 1) end
    end

  build(k: NAT, acc: LIST[NAT]): LIST[NAT]
    variant
      k
    do
      if k = 0 then acc else build(k - 1, acc.prepend(k % 10)) end
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      let e: LIST[NAT] = LIST.empty
      let a = e.prepend(1).prepend(2).prepend(3)
      let b = e.prepend(7).prepend(8)
      let ab = a.append(b)
      let shared = a.prepend(9)
      let long = build(1000, e)
      with IO
        IO.print_line(n(e))
        IO.print_line(n(a))
        IO.print_line(n(ab))
        IO.print_line(n(ab.tail))
        IO.print_line(n(a.append(e)))
        IO.print_line(n(e.append(b)))
        IO.print_line(n(e.append(e)))
        IO.print_line(n(shared))
        IO.print_line(n(a))
        IO.print_line(n(long.append(ab)))
        IO.print_line(NUMBER_TEXT.nat_to_text(count(long, 0)))
        IO.exit(0)
      end
    end

end
";

#[test]
fn a_list_knows_its_length_after_prepend_and_append() {
    let dir = std::env::temp_dir().join(format!("gardelc_list_length_{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(dir.join("src")).expect("temporary directory");
    fs::write(dir.join("Gardel.toml"), "[project]\nname = \"list-length\"\nversion = \"0.1.0\"\n").expect("manifest");
    fs::write(dir.join("src").join("main.gdl"), PROGRAM).expect("source file");
    let out = Command::new(env!("CARGO_BIN_EXE_gardelc"))
        .current_dir(&dir)
        .env("GARDEL_CACHE_DIR", dir.join("cache"))
        .args(["--project", ".", "--strict"])
        .output()
        .expect("gardelc runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("0 errors, 0 warnings"), "{text}");
    let run = Command::new(dir.join("build").join("main_bin.exe")).output().expect("the program runs");
    assert!(run.status.success(), "{run:?}");
    let printed = String::from_utf8(run.stdout).expect("UTF-8 output").replace("\r\n", "\n");
    drop(fs::remove_dir_all(&dir));
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.join(" "), "0 3 5 4 3 2 0 4 3 1005 1000");
}
