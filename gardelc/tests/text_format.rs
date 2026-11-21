//! Formatting and encoders (Task H.6), through the command line: `STRING_BUILDER`,
//! `TEXT_FORMAT`, `HEX`, `BASE64`, `PERCENT` and `NUMBER_TEXT`'s hex. A client that sees no
//! body proves what the contracts and the round-trip laws state, the program runs in C, and a
//! false version of each claim is refused.

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
        let dir = std::env::temp_dir().join(format!("gardelc_text_format_{}_{name}", std::process::id()));
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

  min_i64: I64
    do
      -9223372036854775807 - 1
    end

  -- What a client proves from the contracts alone.

  two: STRING
    do
      STRING_BUILDER.empty.add_char('a').add_char('b').to_string
    ensure
      ab: Result.chars = LIST.empty.prepend('b').prepend('a')
    end

  padded(s: STRING): STRING
    require
      short: s.byte_length <= 100
    do
      TEXT_FORMAT.pad_left(s, 10, ' ')
    ensure
      wide: s.code_point_count < 10 implies Result.code_point_count = 10
      kept: s.code_point_count >= 10 implies Result.chars = s.chars
    end

  same_length(s: STRING): BOOL
    do
      True
    ensure
      lower: TEXT_FORMAT.to_lower(s).byte_length = s.byte_length
      upper: TEXT_FORMAT.to_upper(s).code_point_count = s.code_point_count
    end

  split_back(s: STRING, c: CHAR): BOOL
    do
      True
    ensure
      back: TEXT_FORMAT.join(s.split(c), STRING.from_char(c)).equals(s)
    end

  hex_back(b: BYTES): BOOL
    require
      short: b.length <= 1000000
    do
      True
    ensure
      back: HEX.decode(HEX.encode(b)).is_ok and HEX.decode(HEX.encode(b)).get.model = b.model
    end

  base64_back(b: BYTES, url: BOOL, pad: BOOL): BOOL
    require
      short: b.length <= 1000000
    do
      True
    ensure
      back: BASE64.decode_with(BASE64.encode_with(b, url, pad), url, pad).is_ok and
        BASE64.decode_with(BASE64.encode_with(b, url, pad), url, pad).get.model = b.model
    end

  percent_back(s: STRING): BOOL
    require
      short: s.byte_length <= 1000000
    do
      True
    ensure
      back: PERCENT.decode(PERCENT.encode(s)).is_ok and PERCENT.decode(PERCENT.encode(s)).get.equals(s)
    end

  hex_number_back(n: NAT): RESULT[NUMBER_ERROR, NAT]
    do
      NUMBER_TEXT.parse_hex(NUMBER_TEXT.nat_to_hex(n))
    ensure
      back: Result = Ok(n)
    end

  -- What the program prints.

  why(e: CODEC_ERROR): STRING
    do
      match e
        case BadLength then "bad length"
        case BadSymbol(at) then STRING_BUILDER.empty.add("bad symbol at ").add_nat(at).to_string
        case NotText then "not text"
      end
    end

  bytes_or(r: RESULT[CODEC_ERROR, BYTES]): STRING
    do
      match r
        case Ok(b) then
          match STRING.from_utf8(b)
            case Ok(s) then s
            case Error(_) then "not text"
          end
        case Error(e) then MAIN.why(e)
      end
    end

  text_or(r: RESULT[CODEC_ERROR, STRING]): STRING
    do
      match r
        case Ok(s) then s
        case Error(e) then MAIN.why(e)
      end
    end

  number_why(e: NUMBER_ERROR): STRING
    do
      match e
        case NoDigits then "no digits"
        case NotADigit(at) then STRING_BUILDER.empty.add("not a digit at ").add_nat(at).to_string
        case OutOfRange then "out of range"
      end
    end

  number_or(r: RESULT[NUMBER_ERROR, NAT]): STRING
    do
      match r
        case Ok(n) then NUMBER_TEXT.nat_to_text(n)
        case Error(e) then MAIN.number_why(e)
      end
    end

  b64(s: STRING): STRING
    require
      short: s.byte_length <= 100
    do
      BASE64.encode(s.to_utf8)
    ensure
      short: Result.byte_length <= 200
    end

  line_builder: STRING
    do
      STRING_BUILDER.empty.add("n=").add_nat(18446744073709551615).add_char(' ').add_i64(MAIN.min_i64)
        .add_char('é').add_char('€').add_char('😀').to_string
    end

  line_join: STRING
    do
      let none: LIST[STRING] = LIST.empty
      let parts = none.prepend("z").prepend("").prepend("x")
      if TEXT_FORMAT.join_fits(parts, ", ") then TEXT_FORMAT.join(parts, ", ") else "too long" end
    end

  line_split: STRING
    do
      TEXT_FORMAT.join("a,b,,c".split(','), STRING.from_char(','))
    end

  line_resplit: STRING
    do
      let parts = "a,b,,c".split(',')
      if TEXT_FORMAT.join_fits(parts, " + ") then TEXT_FORMAT.join(parts, " + ") else "too long" end
    end

  line_vectors: STRING
    do
      STRING_BUILDER.empty.add(MAIN.b64("")).add_char('|').add(MAIN.b64("f")).add_char('|').add(MAIN.b64("fo"))
        .add_char('|').add(MAIN.b64("foo")).add_char('|').add(MAIN.b64("foob")).add_char('|')
        .add(MAIN.b64("fooba")).add_char('|').add(MAIN.b64("foobar")).to_string
    end

  two_bytes: BYTES
    do
      let none: LIST[U64] = LIST.empty
      BYTES.from_list(none.prepend(255).prepend(251))
    end

  main(args: SEQ[STRING]): IO[EXIT_CODE]
    do
      with IO
        IO.print_line(MAIN.line_builder)
        IO.print_line(MAIN.two)
        IO.print_line(TEXT_FORMAT.pad_left("42", 6, '0'))
        IO.print_line(TEXT_FORMAT.pad_right("ab", 5, '.'))
        IO.print_line(TEXT_FORMAT.pad_left("toolong", 3, ' '))
        IO.print_line(TEXT_FORMAT.pad_left("é", 3, '·'))
        IO.print_line(TEXT_FORMAT.repeat("ab", 3))
        IO.print_line(TEXT_FORMAT.to_upper("Hello, Wörld!"))
        IO.print_line(TEXT_FORMAT.to_lower("Hello, Wörld!"))
        IO.print_line(MAIN.line_join)
        IO.print_line(MAIN.line_split)
        IO.print_line(MAIN.line_resplit)
        IO.print_line(HEX.encode("Hi!".to_utf8))
        IO.print_line(MAIN.bytes_or(HEX.decode("4869")))
        IO.print_line(MAIN.bytes_or(HEX.decode("486")))
        IO.print_line(MAIN.bytes_or(HEX.decode("4g")))
        IO.print_line(MAIN.line_vectors)
        IO.print_line(BASE64.encode_with(MAIN.two_bytes, False, True))
        IO.print_line(BASE64.encode_with(MAIN.two_bytes, True, False))
        IO.print_line(MAIN.bytes_or(BASE64.decode("Zm9vYmFy")))
        IO.print_line(MAIN.bytes_or(BASE64.decode("Zm9vYmE")))
        IO.print_line(MAIN.bytes_or(BASE64.decode("Zh==")))
        IO.print_line(MAIN.bytes_or(BASE64.decode_with("Zm9vYmE", False, False)))
        IO.print_line(PERCENT.encode("a b/ü~"))
        IO.print_line(MAIN.text_or(PERCENT.decode("caf%C3%A9%21")))
        IO.print_line(MAIN.text_or(PERCENT.decode("%zz")))
        IO.print_line(MAIN.text_or(PERCENT.decode("%FF")))
        IO.print_line(NUMBER_TEXT.nat_to_hex(48879))
        IO.print_line(MAIN.number_or(NUMBER_TEXT.parse_hex("FFFFFFFFFFFFFFFF")))
        IO.print_line(MAIN.number_or(NUMBER_TEXT.parse_hex("10000000000000000")))
        IO.print_line(MAIN.number_or(NUMBER_TEXT.parse_hex("12g")))
        IO.exit(0)
      end
    end

end
"#;

#[test]
fn clients_prove_the_round_trips_and_the_text_is_right() {
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
            "n=18446744073709551615 -9223372036854775808é€😀",
            "ab",
            "000042",
            "ab...",
            "toolong",
            "··é",
            "ababab",
            "HELLO, WöRLD!",
            "hello, wörld!",
            "x, , z",
            "a,b,,c",
            "a + b +  + c",
            "486921",
            "Hi",
            "bad length",
            "bad symbol at 1",
            // RFC 4648 §10.
            "|Zg==|Zm8=|Zm9v|Zm9vYg==|Zm9vYmE=|Zm9vYmFy",
            "+/8=",
            "-_8",
            "foobar",
            "bad length",
            "bad symbol at 1",
            "fooba",
            "a%20b%2F%C3%BC~",
            "café!",
            "bad symbol at 0",
            "not text",
            "beef",
            "18446744073709551615",
            "out of range",
            "not a digit at 2",
        ],
        "{printed}"
    );
}

const LIES: &str = r#"functions MAIN

feature {ANY}

  -- `to_lower` keeps the byte length.
  longer(s: STRING): NAT
    do
      TEXT_FORMAT.to_lower(s).byte_length
    ensure
      wrong: Result = s.byte_length + 1
    end

  -- Hex text is twice as long as its bytes.
  half(b: BYTES): NAT
    require
      some: b.length > 0 and b.length <= 1000
    do
      HEX.encode(b).byte_length
    ensure
      wrong: Result = b.length
    end

  -- The builder's text is what was added.
  other_char: STRING
    do
      STRING_BUILDER.empty.add_char('a').to_string
    ensure
      wrong: Result.chars = LIST.singleton('b')
    end

  -- `PERCENT.encode` needs a bound, for its result to fit.
  unbounded(s: STRING): STRING
    do
      PERCENT.encode(s)
    end

  -- The hex text of a number reads back as that number, not the next.
  next(n: NAT): RESULT[NUMBER_ERROR, NAT]
    require
      small: n < 1000
    do
      NUMBER_TEXT.parse_hex(NUMBER_TEXT.nat_to_hex(n))
    ensure
      wrong: Result = Ok(n + 1)
    end

  -- Padding gives the width, not one more.
  wider(s: STRING): NAT
    require
      short: s.byte_length <= 100 and s.code_point_count < 5
    do
      TEXT_FORMAT.pad_left(s, 5, ' ').code_point_count
    ensure
      wrong: Result = 6
    end

  -- A split joined back is the text, not one scalar longer.
  rejoined(s: STRING): NAT
    do
      TEXT_FORMAT.join(s.split(','), STRING.from_char(',')).code_point_count
    ensure
      wrong: Result = s.code_point_count + 1
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
    for routine in ["longer", "half", "other_char", "unbounded", "next", "wider", "rejoined"] {
        assert!(
            errors.iter().any(|l| l.contains(&format!("in `{routine}`"))),
            "`{routine}` must be refused:\n{text}"
        );
    }
    assert_eq!(errors.len(), 7, "one refusal each:\n{text}");
    // A refusal for want of time proves nothing false.
    assert!(!errors.iter().any(|l| l.contains("Timeout")), "refused by a counterexample, not a timeout:\n{text}");
}
