"""Generates gtui/src/screen/char_width.gdl, the columns a terminal gives each code point.

    python scripts/gen_char_width.py

The table is generated for Unicode 16.0.0 (layering Unicode 15.1.0 and 16.0.0 additions on
top of Python's `unicodedata`). The rules are those of `wcwidth` as terminals use them:

- 0 columns: nonspacing and enclosing marks (Mn, Me), format characters (Cf) other than
  the soft hyphen and the prepended concatenation marks, and the Hangul medial vowels and
  final consonants (U+1160..U+11FF, U+D7B0..U+D7FF), which join the syllable before them.
- 2 columns: East Asian Wide and Fullwidth (W, F), and the unassigned planes 2 and 3,
  which Unicode reserves for ideographs.
- 1 column: everything else. Control characters never reach a cell (`CELL.shown`), so
  their width does not matter.

The module also provides UAX #29 grapheme-cluster helper predicates (`is_zwj`, `is_vs15`,
`is_vs16`, `is_skin_tone`, `is_regional_indicator`).
"""

import os
import sys
import unicodedata

OUT = os.path.join(os.path.dirname(__file__), "..", "gtui", "src", "screen", "char_width.gdl")

UNICODE_VERSION = "16.0.0"
BLOCK = 4096
FIRST = 0x300  # Below this, every code point is one column.

# Cf characters that take a column: the soft hyphen and the prepended concatenation marks.
CF_SHOWN = {0x00AD, 0x0600, 0x0601, 0x0602, 0x0603, 0x0604, 0x0605, 0x06DD, 0x070F,
            0x0890, 0x0891, 0x08E2, 0x110BD, 0x110CD}

# Unicode 15.1.0 and 16.0.0 additions of zero-width marks (Mn/Me/Cf) when running on older Python unicodedata.
ZERO_16_0_RANGES = (
    (0x0897, 0x0897),    # U+0897 Arabic Pepet (16.0)
    (0x10D69, 0x10D6D),  # U+10D69..U+10D6D Garay vowel signs & nasalization (16.0)
    (0x10EFC, 0x10EFC),  # U+10EFC Arabic Combining Alef Overlay (16.0)
    (0x113B8, 0x113B8),  # U+113B8 Tulu-Tigalari vowel sign (16.0)
    (0x113BB, 0x113C0),  # U+113BB..U+113C0 Tulu-Tigalari vowel signs (16.0)
    (0x113CE, 0x113CE),  # U+113CE Tulu-Tigalari virama (16.0)
    (0x113D0, 0x113D0),  # U+113D0 Tulu-Tigalari conjoiner (16.0)
    (0x113D2, 0x113D2),  # U+113D2 Tulu-Tigalari gemination mark (16.0)
    (0x113E1, 0x113E2),  # U+113E1..U+113E2 Tulu-Tigalari vedic tone marks (16.0)
    (0x11F5A, 0x11F5A),  # U+11F5A Kawi sign nukta (16.0)
    (0x1611E, 0x16129),  # U+1611E..U+16129 Gurung Khema vowel signs (16.0)
    (0x1612D, 0x1612F),  # U+1612D..U+1612F Gurung Khema consonant signs (16.0)
    (0x1E5EE, 0x1E5EF),  # U+1E5EE..U+1E5EF Ol Onal combining signs (16.0)
)

# Unicode 15.1.0 and 16.0.0 additions of East Asian Wide (W) ideographs and emoji.
WIDE_16_0_RANGES = (
    (0x2FFC, 0x2FFF),    # U+2FFC..U+2FFF Ideographic Description Characters (15.1)
    (0x31EF, 0x31EF),    # U+31EF Ideographic Description Character Subtraction (15.1)
    (0x1FA89, 0x1FA89),  # U+1FA89 Harp (16.0)
    (0x1FA8F, 0x1FA8F),  # U+1FA8F Shovel (16.0)
    (0x1FABE, 0x1FABE),  # U+1FABE Leafless Tree (16.0)
    (0x1FAC6, 0x1FAC6),  # U+1FAC6 Fingerprint (16.0)
    (0x1FADC, 0x1FADC),  # U+1FADC Root Vegetable (16.0)
    (0x1FADF, 0x1FADF),  # U+1FADF Splatter (16.0)
    (0x1FAE9, 0x1FAE9),  # U+1FAE9 Face with Bags Under Eyes (16.0)
)

ZERO_16_0 = {cp for lo, hi in ZERO_16_0_RANGES for cp in range(lo, hi + 1)}
WIDE_16_0 = {cp for lo, hi in WIDE_16_0_RANGES for cp in range(lo, hi + 1)}


def width(n: int) -> int:
    if 0xD800 <= n <= 0xDFFF:
        return 1
    if n in ZERO_16_0:
        return 0
    if n in WIDE_16_0:
        return 2
    c = chr(n)
    cat = unicodedata.category(c)
    if cat in ("Mn", "Me") or (cat == "Cf" and n not in CF_SHOWN):
        return 0
    if 0x1160 <= n <= 0x11FF or 0xD7B0 <= n <= 0xD7FF:
        return 0
    if unicodedata.east_asian_width(c) in ("W", "F"):
        return 2
    if 0x20000 <= n <= 0x2FFFD or 0x30000 <= n <= 0x3FFFD:
        return 2
    return 1


def ranges():
    """The runs of code points whose width is not 1, as [first, last, width]."""
    out = []
    for n in range(FIRST, 0x110000):
        w = width(n)
        if w == 1:
            continue
        if out and out[-1][1] == n - 1 and out[-1][2] == w:
            out[-1][1] = n
        else:
            out.append([n, n, w])
    return out


def blocks(rs, w):
    """For each block with runs of width `w`: the block, and its runs or None when one
    run covers it all."""
    out = {}
    for lo, hi, ww in rs:
        if ww != w:
            continue
        for b in range(lo // BLOCK, hi // BLOCK + 1):
            out.setdefault(b, []).append((lo, hi))
    for b, runs in out.items():
        first, last = b * BLOCK, b * BLOCK + BLOCK - 1
        if any(lo <= first and last <= hi for lo, hi in runs):
            out[b] = None
    return dict(sorted(out.items()))


def hexes(lo, hi):
    return f"U+{lo:04X}" if lo == hi else f"U+{lo:04X}..U+{hi:04X}"


def test(lo, hi):
    return f"n = {lo}" if lo == hi else f"(n >= {lo} and n <= {hi})"


def feature(name, doc, bs):
    lines = [f"  -- {doc}", f"  {name}(n: NAT): BOOL", "    do", f"      match n / {BLOCK}"]
    for b, runs in bs.items():
        lines.append(f"        case {b} then  -- {hexes(b * BLOCK, b * BLOCK + BLOCK - 1)}")
        if runs is None:
            lines.append("          True")
            continue
        tests = [test(lo, hi) for lo, hi in runs]
        col = max(len(t) for t in tests) + 3
        for i, (t, (lo, hi)) in enumerate(zip(tests, runs)):
            lead = "          " if i == 0 else "          or "
            text = f"{lead}{t}"
            pad = " " * max(1, len("          or ") + col - len(text))
            lines.append(f"{text}{pad}-- {hexes(lo, hi)}")
    lines += ["        case _ then", "          False", "      end", "    end"]
    return "\n".join(lines)


def check(zero, wide):
    """Every code point gets from the table the width the Unicode data gives it."""
    def has(bs, n):
        b = n // BLOCK
        if b not in bs:
            return False
        return bs[b] is None or any(lo <= n <= hi for lo, hi in bs[b])

    for n in range(0x110000):
        got = 1 if n < FIRST else 0 if has(zero, n) else 2 if has(wide, n) else 1
        assert got == width(n), f"U+{n:04X}: table {got}, Unicode {width(n)}"


def main():
    rs = ranges()
    zero, wide = blocks(rs, 0), blocks(rs, 2)
    check(zero, wide)
    nzero = sum(1 for r in rs if r[2] == 0)
    nwide = len(rs) - nzero
    text = f"""-- GENERATED by scripts/gen_char_width.py from Unicode {UNICODE_VERSION}; do not edit.
--
-- The columns a terminal gives a character: 0 for a combining mark or a zero-width format
-- character, which is drawn on the cell before it; 2 for an East Asian wide or fullwidth
-- character, an ideograph or most emoji; 1 for the rest. {nzero} runs of zero-width and {nwide}
-- runs of wide code points, each tested in its block of {BLOCK} code points.
functions CHAR_WIDTH

feature {{ANY}}

  of(c: CHAR): NAT
    do
      CHAR_WIDTH.of_code(c.code)
    ensure
      at_most_two: Result <= 2
    end

  of_code(n: NAT): NAT
    do
      if n < {FIRST} then
        1
      else if CHAR_WIDTH.is_zero(n) then
        0
      else if CHAR_WIDTH.is_wide(n) then
        2
      else
        1
      end end end
    ensure
      at_most_two: Result <= 2
    end

  -- Zero-Width Joiner (U+200D), which glues the following character into the current grapheme cluster.
  is_zwj(c: CHAR): BOOL
    do
      c.code = 8205
    end

  -- Variation Selector-15 (U+FE0E, text presentation selector).
  is_vs15(c: CHAR): BOOL
    do
      c.code = 65038
    end

  -- Variation Selector-16 (U+FE0F, emoji presentation selector, promoting a 1-column base to 2 columns).
  is_vs16(c: CHAR): BOOL
    do
      c.code = 65039
    end

  -- Emoji skin-tone modifier (U+1F3FB..U+1F3FF, Fitzpatrick Types 1-2..6).
  is_skin_tone(c: CHAR): BOOL
    do
      c.code >= 127995 and c.code <= 127999
    end

  -- Regional Indicator Symbol (U+1F1E6..U+1F1FF), which pairs with a preceding unpaired Regional Indicator into a 2-column flag.
  is_regional_indicator(c: CHAR): BOOL
    do
      c.code >= 127462 and c.code <= 127487
    end

{feature("is_zero", "Combining marks, zero-width format characters and Hangul jamo that join the syllable before them.", zero)}

{feature("is_wide", "East Asian wide and fullwidth characters, most emoji, and the ideograph planes 2 and 3.", wide)}

end
"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    lines = text.count("\n")
    print(f"{OUT}: {len(rs)} runs ({nzero} zero, {nwide} wide), {lines} lines, "
          f"Unicode {UNICODE_VERSION}, every code point checked")


if __name__ == "__main__":
    sys.exit(main())
