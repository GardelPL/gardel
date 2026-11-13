# Gardel terminal interfaces (`gtui`)

A verified library for full-screen terminal programs: a screen of cells, layout, widgets, and an Elm-style loop that draws only what changed. It runs on the terminal host of Task F.9, so the same program writes the same bytes on Linux and Windows.

```eiffel
reactive program TODO_TUI

feature {ANY}

  update(s: TODO_STATE, e: EVENT): TUI_NEXT[TODO_STATE]
    do ... end

  view(s: TODO_STATE, blank: SCREEN): SCREEN
    do ... end

  init(args: SEQ[STRING]): STEP[TUI[TODO_STATE]]
    do
      TUI.start(TODO_STATE.empty, update, view)
    end

  step(t: TUI[TODO_STATE], e: EVENT): STEP[TUI[TODO_STATE]]
    do
      t.step(e)
    end

end
```

`examples/todo_tui` is a complete program.

## The loop

`TUI[S]` holds the application's state, its `update` and `view`, and the screen as the terminal shows it.

- `TUI.start(s, update, view)` takes the alternate screen, hides the cursor, asks the terminal's size and reads a key.
- Every event but `Shutdown` goes to `update`, which gives `Next(state, cmds)` or `Quit(code, after)`. `TUI_NEXT.next(s)` and `quit(code)` are the short forms.
- After each event, `view` draws the state on a blank screen of the terminal's size. The first frame and every resize clear the screen and draw it whole. Any other step writes only the cells that changed, in one `Write`.
- A key is followed by a read of the next, until keys end.
- `update`'s commands are sent before the frame. `Print`, `Write`, `TerminalSize`, `ReadLine` and `ReadKey` are dropped (`TUI_COMMANDS.own`), as `TUI` owns the screen and the keyboard.
- `Quit`, and `Shutdown` with exit status 1, put the terminal's screen and cursor back and stop. The commands of `Quit` run after that, so a `Print` there is seen.

A view that gives back a screen of another size than the one it was given is not drawn; the blank one is (`SCREEN.fitted`). Every routine of `SCREEN` keeps the size.

## The screen

`SCREEN` is a grid of `CELL`s, at most 65535 by 65535. A cell is a character and a `STYLE` (any of bold, dim, underlined and reversed).

- `s.text(clip, x, y, text, style)` draws text on row `y` from column `x`, and `s.fill(clip, c, style)` fills a rectangle. Both change only the cells inside both `clip` and the screen, so a widget given a rectangle cannot draw outside it.
- A cell never holds a control character (`CELL`'s invariant): one becomes `?`, and a tab a space. So text cannot move the terminal's cursor.
- One code point is one cell. A character a terminal shows two cells wide, or none (a combining mark), is not accounted for, and would shift the rest of its row.

`SCREEN_DIFF.changes(old, new)` gives the writes that turn one screen into another of the same size: each write is a run of changed cells on one row, and runs closer than four unchanged cells are joined, since moving the cursor costs more bytes than writing a few cells again. `VT_TEXT.frame` turns writes into text: a cursor move for each, and an SGR sequence where the style changes.

## Layout

`RECT` is a rectangle of cells. Its cuts split it in two along a side: `take_top(n)` and `drop_top(n)`, and the same for `bottom`, `left` and `right`, and `inset(n)`.

`LAYOUT.rows(r, sizes)` and `LAYOUT.columns(r, sizes)` split a rectangle along one axis, one child per `SIZE`: `SIZE.fixed(n)` cells, or `SIZE.flex(w)`, a share of what the fixed sizes leave in proportion to its weight from 1 to 1000. The last flexible child takes what rounding leaves. When the sizes ask for more than there is, children are cut short in order.

## Widgets

Each widget is a value with a `draw(screen, rect, ...)` that gives a new screen, and an `on_key` that gives a new value. The application keeps them in its state.

| Widget | What it is |
| :--- | :--- |
| `TEXT_VIEW` | Lines shown from a first line, scrolled by the arrows, Page Up, Page Down, Home and End |
| `LIST_VIEW` | Items with one selected, drawn across its row in a style of its own, and kept on the page shown |
| `STATUS_BAR` | One row with text on the left and on the right |
| `INPUT_LINE` | A line being typed, with a cursor, up to 4096 characters; the part around the cursor is shown when it is too long |

`TEXT_VIEW` and `LIST_VIEW` take `rows`, the height they are drawn in, which pages them. `update` does not know the layout, so an application keeps the height it gets from `Resized`, as `todo_tui` does.

## What is proven

- **Every write stays inside the screen.** `SCREEN_DIFF.changes` and `whole` ensure `SCREEN_WRITE.all_within(Result, width, height)`: each write is on a row of the screen and ends at or before its right edge. `TUI.frame_writes` gives it for every frame, against the terminal's last reported size.
- **Children stay inside their parent and do not overlap.** `RECT.cut_top`, `cut_bottom`, `cut_left` and `cut_right`: both pieces are inside, they are disjoint, and they hold all the rows or columns. `LAYOUT.columns` and `rows` give one child per size, laid along the axis in order (`LAYOUT.across` and `down`); `LAYOUT.across_inside` and `down_inside` put each child inside the parent, and `LAYOUT_APART.columns_apart` and `rows_apart` make any two disjoint. `RECT.inside_trans` and `disjoint_inside` carry this through nested layouts.
- **The widgets' invariants.** A list's selection is an item, or 0 when it is empty, and on the page shown. A text view's first line is one of its lines. An input line has at most 4096 characters.

Not proven: that the writes of a diff turn the old screen into the new one, and that the bytes `VT_TEXT` writes put each cell where its write says. `tests/gtui.rs` checks both by running a session of `todo_tui` through a small VT terminal: every character lands inside the screen, and after the session the cells are the same, styles included, as a full redraw gives.

## Cost

A full redraw of a 200 by 60 screen, where every cell changes, takes about 7 ms, measured by `tests/gtui.rs` with `gtui/tests`'s bench, against a frame of 16 ms. Each frame builds its screen from lists of cells, compares each row once, and writes the changed cells through `BYTES_BUILDER`.
