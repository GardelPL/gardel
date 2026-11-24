# Workbench Tour Example (`gtui_tour`)

A tour of every widget of the [`gtui`](../../gtui) package, laid out as a workbench, in a program that is its state and four routines. It runs in a terminal, takes the keyboard and the mouse, and draws only the cells that change.

- **Left:** the project, under the tabs `Modules(m) | Files(f)`, each a `TREE_VIEW`.
- **Right:** a `BREADCRUMB` path bar for the selected tree node above a source with line numbers, its keywords, strings and comments coloured by a lexer, and one line marked; and a top `MENU` bar on the frame's title row.
- **Bottom:** the tabs `Output(o) | Problems (3)(p)`: a `TEXT_VIEW` of the log, or a `LIST_VIEW` of problems.
- **Over them:** the `MENU` drop-downs (`F10` / `Alt+letter`), the command `PALETTE` (`/`), the help `POPUP` (`?`), and the quit confirmation `DIALOG` when modified.

The pane that has the keys is framed in bold cyan.

## Project

- [`Gardel.toml`](Gardel.toml) depends on `gtui`.
- [`src/gtui_tour.gdl`](src/gtui_tour.gdl) is `reactive program GTUI_TOUR`: `update`, `view`, and the two lines `init` and `step` that start `TUI`.
- [`src/tour_state.gdl`](src/tour_state.gdl) is `TOUR_STATE`: which pane has the keys, the trees, the source, the problems, the log, the palette and the help popup, with `browse` and `in_pane` for keys and `handle_mouse` for the mouse.
- [`src/tour_layout.gdl`](src/tour_layout.gdl) is where the panes go. `update` and `view` both ask it, so the heights a pane pages by are the heights it is drawn in.
- [`src/tour_lexer.gdl`](src/tour_lexer.gdl) gives the spans of a line of source.

## Keys

| Key | What it does |
| :--- | :--- |
| `Tab` | The keys go to the next pane, and from the last to the first |
| `m`, `f` | In the left pane: the Modules or the Files tab |
| `o`, `p` | In the bottom pane: the Output or the Problems tab |
| Arrows, `PageUp`, `PageDown`, `Home`, `End` | Move in the pane that has the keys |
| `Right`, `Left` | In a tree: open a node, or go to its first child; close it, or go to its parent |
| `Enter` | In a tree: open or close the node. In Problems: mark the problem's line in the source |
| `/` | Open the commands; what is typed narrows them; `Enter` runs one, `Escape` closes them |
| `F10`, `Alt+f`/`v`/`h` | Open the `MENU` bar (`File`, `View`, `Help`); arrows/`Enter`/`Escape` navigate |
| `Ctrl+N` | Mark the buffer as modified (confirm `DIALOG` appears on quit) |
| `?` | Show or hide the help |
| `q` | Quit (opens a confirmation `DIALOG` if modified) |

The commands:

| Command | What it does |
| :--- | :--- |
| `/help` | Show the help |
| `/theme [NAME]` | `dark`, `light`, `nord` or `dracula` |
| `/top` | The source from its first line |
| `/clear` | Empty the output |
| `/shell [ARGS]` | Run the program given to the tour, with the terminal its own; the tour comes back when it ends |
| `/quit` | Leave the tour |

## Mouse

The terminal needs to report the mouse with SGR 1006 (Windows Terminal, and most terminals on Linux and macOS, do). The tour asks for button and drag reports on entering, and turns them off on leaving, and while a `/shell` child has the terminal.

| Mouse | What it does |
| :--- | :--- |
| Click a pane | The keys go to it |
| Click a tab | Choose it |
| Click a tree row | Select it; a click on the selected row, or on its `▸` or `▾`, opens or closes the node |
| Wheel over a tree, the source or the problems | Scroll it by three rows |
| Click a problem | Select it and mark its line in the source |
| Click a `MENU` entry or item | Open/switch drop-downs, sub-menus, or run a menu action |
| Click a `BREADCRUMB` crumb | Select that ancestor node in the active tree |
| Click a `DIALOG` button | Confirm (`Yes`) or cancel (`No`) when the quit dialog is open |
| Wheel over the help | Scroll its text |
| Click outside the help or the commands | Close them |

## What is proven

`gardelc --project examples/gtui_tour --strict` proves, from the contracts of `gtui` alone, that every write of a frame lies inside the screen, that the layout's three panes are inside the screen and do not overlap, and that the tabs, trees, lists and views keep their invariants (a tab is chosen, a selection is a row shown, a view's first line is one of its lines) through every key and every mouse event. A click is in at most one pane, as `RECT.has_inside` and `RECT.has_disjoint` carry the layout's facts to cells.

`cargo test --manifest-path gardelc/Cargo.toml --test gtui` runs sessions of the tour through a small VT terminal, with keys and with mouse events, and checks that every character lands inside the screen and that the diffs leave the same cells as a full redraw; and that nothing is drawn while a `/shell` child has the terminal.

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/gtui_tour --strict
./build/gtui_tour_bin.exe                 # no program for /shell
./build/gtui_tour_bin.exe ./some_program  # /shell runs it, attached
```

The executable is built in `build/` under the current directory, so from inside `examples/gtui_tour` it is `build/gtui_tour_bin.exe`.

## Scripted runs and replay

A reactive program can run from a script of events instead of the terminal. Keys are `Key:<name or code>`, the mouse is `Mouse:[Shift+][Alt+][Ctrl+]<Down|Up|Drag>:<button>:<x>:<y>` or `Mouse:WheelUp:<x>:<y>` and `Mouse:WheelDown:<x>:<y>`, from 0:

```powershell
$env:GARDEL_EVENTS = "Resized:90:24 Mouse:Down:0:3:2 Resized:90:24 Key:63 Resized:90:24 Key:113"
./build/gtui_tour_bin.exe
```

The frames are written to standard output as terminal text. `GARDEL_RECORD=<file>` records a live run, mouse events included, and `GARDEL_REPLAY=<file>` runs it again from the log and checks that each command and the exit status are the same.
