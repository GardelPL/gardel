# Verification Dashboard Example (`gtui_dashboard`)

A dashboard of the verification of a few `gtui` modules, which shows the widgets of the [`gtui`](../../gtui) package that the tour does not: a table with a scrollbar, gauges, a sparkline, a paragraph, and a themed frame whose borders join. It runs in a terminal, takes the keyboard and the mouse, and draws only the cells that change.

- **Top:** the tabs `Overview(1) | Tasks(2) | Events(3) | Unicode(4)`. The tab chosen titles the left pane and changes the notes below.
- **Left:** `Modules`, a `TABLE` of modules with their verification conditions, time and state (some of it in emoji and flags, to show UAX #29 clusters), and a vertical `SCROLLBAR` on its right edge.
- **Right:** `Telemetry`, two `GAUGE`s (the coverage of the module selected, and a cache hit rate) and a `SPARKLINE` of solver samples. The two panes share a border, which `FRAME.merge_borders` joins into `┬` and `┴`.
- **Bottom:** `Inspector`, a `PARAGRAPH` about the module selected, wrapped and aligned as chosen, separated by a vertical `DIVIDER` from two `CHECKBOX`es (`Auto-refresh`, `Show finished`), a labeled horizontal `DIVIDER` (`Rate`), and a `SLIDER` (`1..10`); plus a `STATUS_BAR`.
- **Over them:** `TOAST` notifications in the top-right corner and the help `POPUP` (`?`).

## Project

- [`Gardel.toml`](Gardel.toml) depends on `gtui`.
- [`src/gtui_dashboard.gdl`](src/gtui_dashboard.gdl) is `reactive program GTUI_DASHBOARD`: `update`, `view`, and the two lines `init` and `step` that start `TUI`.
- [`src/dash_state.gdl`](src/dash_state.gdl) is `DASH_STATE`: the table, the tabs, the samples, the theme, border, wrap and alignment chosen, the inspector and the help popup, with `on_key` and `on_mouse`.
- [`src/dash_layout.gdl`](src/dash_layout.gdl) is where everything goes: the tabs bar, the two panes, the inspector, the status bar, and the rectangles of the table and its scrollbar. `update` and `view` both ask it, so a click is looked for where a widget is drawn.

## Keys

| Key | What it does |
| :--- | :--- |
| `1` to `4` | The Overview, Tasks, Events or Unicode tab |
| `Tab` | The keys go to the next pane |
| Arrows, `PageUp`, `PageDown`, `Home`, `End` | Select a module in the table |
| `Space` | Record a solver sample: the sparkline moves on |
| `t` | The theme: dark, nord, dracula, light |
| `b` | The border: rounded, plain, double, thick |
| `w` | The inspector's wrap: word, char, none |
| `a` | The inspector's alignment: left, center, right |
| `r`, `f` | Toggle `Auto-refresh` / `Show finished` `CHECKBOX`es (and push a `TOAST`) |
| `+` / `=`, `-` | Adjust the `Rate` `SLIDER` (`1..10`) |
| `s`, `Enter` | Push a status `TOAST` notification (success / info / warning) |
| `?` | Show or hide the help (`Escape` and `Enter` close it; arrows scroll it) |
| `q` | Quit |

## Mouse

The terminal needs to report the mouse with SGR 1006 (Windows Terminal, and most terminals on Linux and macOS, do). The dashboard asks for button and drag reports on entering, and turns them off on leaving.

| Mouse | What it does |
| :--- | :--- |
| Click a tab | Choose it |
| Click a table row | Select the module |
| Wheel over the table | Move the selection by three rows |
| Click or drag on the scrollbar | Jump to that place in the table |
| Wheel over the inspector | Scroll its notes by three lines |
| Click a `CHECKBOX` | Toggle `Auto-refresh` or `Show finished` |
| Click or drag on the `SLIDER` | Set the refresh rate |
| Click a `TOAST` notification | Dismiss it |
| Click a pane's frame | The keys go to it |
| Wheel over the help | Scroll its text |
| Click outside the help | Close it |

## What is proven

`gardelc --project examples/gtui_dashboard --strict` proves, from the contracts of `gtui` alone, that every write of a frame lies inside the screen, that the layout gives the four rows and two panes it should, and that the table's selection is a row shown, the scrollbar's position is inside the table, and a paragraph's scroll stays within its last page, through every key and every mouse event.

`cargo test --manifest-path gardelc/Cargo.toml --test gtui` runs sessions of the dashboard through a small VT terminal, with keys and with mouse events, and checks the table, the tabs, the scrollbar, the inspector's scroll and the help's dismissal, that every character lands inside the screen, and that the diffs leave the same cells as a full redraw.

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/gtui_dashboard --strict
./build/gtui_dashboard_bin.exe
```

The executable is built in `build/` under the current directory, so from inside `examples/gtui_dashboard` it is `build/gtui_dashboard_bin.exe`.

## Scripted runs and replay

A reactive program can run from a script of events instead of the terminal. Keys are `Key:<name or code>`, the mouse is `Mouse:[Shift+][Alt+][Ctrl+]<Down|Up|Drag>:<button>:<x>:<y>` or `Mouse:WheelUp:<x>:<y>` and `Mouse:WheelDown:<x>:<y>`, from 0:

```powershell
$env:GARDEL_EVENTS = "Resized:80:16 Mouse:Down:0:16:0 Resized:80:16 Mouse:Down:0:5:5 Resized:80:16 Key:113"
./build/gtui_dashboard_bin.exe
```

The frames are written to standard output as terminal text. `GARDEL_RECORD=<file>` records a live run, mouse events included, and `GARDEL_REPLAY=<file>` runs it again from the log and checks that each command and the exit status are the same.
