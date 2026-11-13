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

`examples/todo_tui` is a complete program, and `examples/gtui_tour` uses every widget: three framed panes, tabs, trees, a source with line numbers and coloured keywords, problems that mark a line, and a command palette.

## The loop

`TUI[S]` holds the application's state, its `update` and `view`, and the screen as the terminal shows it.

- `TUI.start(s, update, view)` takes the alternate screen, hides the cursor, asks the terminal's size and reads a key.
- Every event but `Shutdown` goes to `update`, which gives `Next(state, cmds)` or `Quit(code, after)`. `TUI_NEXT.next(s)` and `quit(code)` are the short forms.
- After each event, `view` draws the state on a blank screen of the terminal's size. The first frame and every resize clear the screen and draw it whole. Any other step writes only the cells that changed, in one `Write`.
- A key is followed by a read of the next, until keys end.
- `update`'s commands are sent before the frame. `Print`, `Write`, `TerminalSize`, `ReadLine` and `ReadKey` are dropped (`TUI_COMMANDS.own`), as `TUI` owns the screen and the keyboard.
- `Quit`, and `Shutdown` with exit status 1, put the terminal's screen and cursor back and stop. The commands of `Quit` run after that, so a `Print` there is seen.
- A `RunAttached` among `update`'s commands gives the terminal to a child: `TUI` puts the screen back before it, and draws nothing until every child it sent has ended (`TUI.attached` counts them). The last `AttachedEnded` takes the alternate screen again, and the `Resized` the host sends after it draws the screen whole.

A view that gives back a screen of another size than the one it was given is not drawn; the blank one is (`SCREEN.fitted`). Every routine of `SCREEN` keeps the size.

## The screen

`SCREEN` is a grid of `CELL`s, at most 65535 by 65535. A cell is a character and a `STYLE`: any of bold, dim, underlined and reversed, and a foreground and a background `COLOR`.

- `COLOR` is the terminal's own (`COLOR.default`) or one of the 16 standard colours, `COLOR.red` to `COLOR.bright_white`, or `COLOR.standard(n)` for `n` from 0 to 15. What each looks like is the terminal's theme.
- `STYLE.plain.fg(COLOR.cyan).bold` and `STYLE.plain.bg(COLOR.blue)` build styles; `same` compares all of a style, its colours included.

- `s.text(clip, x, y, text, style)` draws text on row `y` from column `x`, and `s.fill(clip, c, style)` fills a rectangle. Both change only the cells inside both `clip` and the screen, so a widget given a rectangle cannot draw outside it.
- A cell never holds a control character (`CELL`'s invariant): one becomes `?`, and a tab a space. So text cannot move the terminal's cursor.
- One code point is one cell. A character a terminal shows two cells wide, or none (a combining mark), is not accounted for, and would shift the rest of its row.

`SCREEN_DIFF.changes(old, new)` gives the writes that turn one screen into another of the same size: each write is a run of changed cells on one row, and runs closer than four unchanged cells are joined, since moving the cursor costs more bytes than writing a few cells again. `VT_TEXT.frame` turns writes into text: a cursor move for each, and an SGR sequence where the style changes, which resets the pen and then sets the attributes and the colours that are not the terminal's own (30 to 37 and 90 to 97 for the foreground, 40 to 47 and 100 to 107 for the background).

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
| `FRAME` | A box on the edge of a rectangle with rounded corners and a title in its top edge; `inner(r)` holds the pane, and `title_area(r)` a row of tabs |
| `FOCUS` | Which of `n` panes has the keys; Tab moves it to the next, and from the last to the first |
| `TABS` | A row of titles such as `Text(t) \| Short(s)`, one chosen, by its key or by Tab |
| `TREE_VIEW` | Nodes at depths, opened and closed with `▾` and `▸`, on a `LIST_VIEW`'s selection and paging |
| `PALETTE` | Commands drawn over the screen, narrowed by what is typed after `/`, each with what it does |

`TEXT_VIEW`, `LIST_VIEW`, `TREE_VIEW` and `PALETTE` take `rows`, the height they are drawn in, which pages them. `update` does not know the layout, so an application keeps the size it gets from `Resized` and finds the height from the same layout `view` uses, as `todo_tui` and `gtui_tour` do.

**Panes.** A pane is a `FRAME` and what is drawn in its `inner`. The application gives each key to the pane `FOCUS` names, and draws that pane's frame in a style of its own, such as `STYLE.plain.fg(COLOR.cyan).bold`. When a pane has tabs, draw them in the frame's `title_area`, and let `tabs.takes(k)` decide whether a key is theirs, so that Tab can stay `FOCUS`'s.

**Source.** `TEXT_VIEW.draw_code(s, r, st, gutter, marked, mark, spans)` draws the lines with their numbers, right-aligned, in `gutter`; the line `marked` (an `OPTION[NAT]`, from 0) across its row in `mark`; and over each line the `SPAN`s that `spans(i, line)` gives, each a run of characters in a style of its own. `spans` is the application's, a lexer for instance, and is asked only for the lines drawn. `showing(n, rows)` scrolls as little as shows line `n`.

**Trees.** A tree is a list of `TREE_NODE`s in order, each a label at a depth: a node's children are the deeper nodes after it. Right opens the node selected or moves to its first child; Left closes it or moves to its parent. `selected_node` gives the index of the node selected, by which the application finds what it stands for; `toggled`, `set_open` and `with_nodes` keep the selection on its row.

**Commands.** `PALETTE.make(entries)` with `PALETTE_ENTRY.make("run", "[ARGS] run the program")`. What is typed after `/` narrows the list to the commands whose name starts with its first word, and the rest is `arguments`. The arrows and the page keys move in the list and other keys edit the line; Enter and Escape are the application's, which reads `chosen` and `arguments`. `place(area, width, height)` gives where to draw it, at the bottom of `area`.

## What is proven

- **Every write stays inside the screen.** `SCREEN_DIFF.changes` and `whole` ensure `SCREEN_WRITE.all_within(Result, width, height)`: each write is on a row of the screen and ends at or before its right edge. `TUI.frame_writes` gives it for every frame, against the terminal's last reported size.
- **Children stay inside their parent and do not overlap.** `RECT.cut_top`, `cut_bottom`, `cut_left` and `cut_right`: both pieces are inside, they are disjoint, and they hold all the rows or columns. `LAYOUT.columns` and `rows` give one child per size, laid along the axis in order (`LAYOUT.across` and `down`); `LAYOUT.across_inside` and `down_inside` put each child inside the parent, and `LAYOUT_APART.columns_apart` and `rows_apart` make any two disjoint. `RECT.inside_trans` and `disjoint_inside` carry this through nested layouts.
- **The widgets' invariants.** A list's selection is an item, or 0 when it is empty, and on the page shown. A text view's first line is one of its lines. An input line has at most 4096 characters. A tree shows one list row per row it holds, and a palette one per command it shows. A tab bar has one key per title and its choice is a tab; a focus is on a pane.
- **Where things go.** `FRAME.inner(r)` and `title_area(r)` lie inside `r`, and `PALETTE.place(area, w, h)` inside `area`. `TEXT_VIEW.showing(n, rows)` shows line `n` when there is one and `rows` is not 0.

Not proven: that the writes of a diff turn the old screen into the new one, and that the bytes `VT_TEXT` writes put each cell where its write says. `tests/gtui.rs` checks both by running sessions of `todo_tui` and `gtui_tour` through a small VT terminal: every character lands inside the screen, and after the session the cells are the same, styles and colours included, as a full redraw gives. That a tree hides what is inside a closed node, that a palette narrows, and that nothing is drawn while a child has the terminal are tested too.

## Cost

A full redraw of a 200 by 60 screen, where every cell changes, takes about 7 ms, measured by `tests/gtui.rs` with `gtui/tests`'s bench, against a frame of 16 ms. Each frame builds its screen from lists of cells, compares each row once, and writes the changed cells through `BYTES_BUILDER`.
