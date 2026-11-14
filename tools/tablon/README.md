# Tablon

Tablon is a workbench for a Gardel project, written in Gardel on `gtui` and `gjson`. It browses
a project's modules and files, shows each module's short form and parts, checks the project and
takes you from a failure to its line, builds it, runs it, and opens a file in your editor. It
runs in a terminal.

```
┌ Modules(m) │ Files(f) ─┐┌ CELL - Text ──────────────────────────────────────────┐
│▾ project               ││ Text(t) │ Short(s) │ Features(f) │ Creators(n) │ ... │
│  ▾ modules             ││  1 -- A cell that holds a number up to 1000.           │
│      CELL              ││  2 module CELL satisfies MEASURED                       │
│  ▾ functions           ││  ...                                                    │
│      USE_CELL          ││                                                         │
└────────────────────────┘└─────────────────────────────────────────────────────────┘
┌ Output(o) │ Problems (1)(p) │ Build(b) │ Run(r) ──────────────────────────────────┐
│src/cell.gdl:31  error  Formal verification condition `same` failed in `measure`   │
└───────────────────────────────────────────────────────────────────────────────────┘
 Tab panes  / commands  ^K check  ^B build  ^R run  ^E edit  q quit    2 modules, 1 error
```

## Building it

From `tools/tablon`:

```
gardelc --project . --strict
```

This verifies Tablon and writes `build/tablon_bin` (`tablon_bin.exe` on Windows). `gardelc`
writes `build/` under the directory it runs in, so run it from `tools/tablon` to have the
executable there. On Windows, close a Tablon that runs before building it again: the linker
cannot replace an executable in use.

## Running it

```
tools/tablon/build/tablon_bin [DIR] [GARDELC] [EDITOR]
```

- `DIR` is the project to open, the directory that holds its `Gardel.toml`. It is `.` when not
  given.
- `GARDELC` is the compiler. It is `gardelc` on the `PATH` when not given. Give its path when it
  is not on the `PATH`, such as `gardelc/target/release/gardelc.exe`.
- `EDITOR` is the command line that `/edit` runs on a file, such as `vi`, `nano`, `notepad` or
  `"code --wait"`. It is `vi` when not given. The file's path is added after its words.

For example, to browse `gtui` with the compiler built in this repository:

```
tools/tablon/build/tablon_bin gtui gardelc/target/release/gardelc
```

Tablon needs `Z3_EXE` set as `gardelc` does to check, build or run, but not to browse: at the
start it runs `gardelc --inspect --json-diagnostics --no-verify`, which type checks the project
and reads its modules without proving them. While it runs, the status bar says `inspecting...`,
and every key works. On a large project the inspection takes some seconds.

To open another project while Tablon runs, use `/open DIR`. To start a module, use
`/new NAME`: it writes `src/name.gdl`, a `functions` module named `NAME` with no feature yet,
opens it, and reads the modules again. Edit it with `/edit` (Ctrl-E).

## The panes

- **Left, the project.** Two tabs:
  - **Modules** (`m`): the project's package first, then each dependency, and under each its
    modules by kind: reactive programs, programs, modules and functions. Enter on a module
    chooses it.
  - **Files** (`f`): the files under `src`, directories first. Enter opens a file: its module
    when it has one, and as text otherwise.
- **Right, the module chosen.** Ten views, each with its key:

  | Key | View | What it shows |
  |-----|------|---------------|
  | `t` | Text | The source, with line numbers and Gardel's syntax coloured. |
  | `s` | Short | The short form: the module's contracts without their bodies. |
  | `f` | Features | Its features, one a line. Enter shows the feature's line in Text. |
  | `n` | Creators | Its creators. Enter shows the line. |
  | `l` | Laws | Its laws. Enter shows the line. |
  | `i` | Invariants | Its invariant clauses. Enter shows the line. |
  | `a` | Concepts | The concepts it satisfies. |
  | `d` | Instances | Each concept it satisfies, with the other modules that satisfy it. |
  | `c` | Clients | The modules that use it. Enter chooses one. |
  | `u` | Suppliers | The modules it uses. Enter chooses one. |

- **Bottom, the output.** Four tabs:
  - **Output** (`o`): what Tablon and `gardelc` said, as it came.
  - **Problems** (`p`): the errors and warnings of the last inspection, check or build, each
    with its file and line. The tab shows how many there are, and is chosen when there is an
    error. Enter shows the line in Text, marked, and chooses its module.
  - **Build** (`b`): the compiler's steps, then the executable or why there is none.
  - **Run** (`r`): the program's output, standard output and standard error as they come. While
    it runs, the last row is its input line (see below).
- **The status bar**: the keys that work now on the left; what runs, or the number of modules
  and of problems, on the right.

The pane that has the keys is framed in cyan. Tab moves the keys to the next pane.

## Keys

| Key | Where | What it does |
|-----|-------|--------------|
| Tab | anywhere | The keys to the next pane. |
| Up, Down, PageUp, PageDown, Home, End | a tree, a list or a text | Move or scroll. |
| Right, Left | a tree | Open or close a node. |
| Enter | a tree, a list, Problems | Open what is chosen, or go to its line. |
| a tab's key | its pane | Choose that tab, such as `s` for Short on the right. |
| `/` | anywhere | Open the commands. |
| Ctrl-K | anywhere | Check (`/check`). |
| Ctrl-B | anywhere | Build (`/build`). |
| Ctrl-R | anywhere | Build and run (`/run`). |
| Ctrl-E | anywhere | Edit the file shown (`/edit`). |
| Ctrl-X | anywhere | Stop what runs (`/stop`). |
| `q` | anywhere but the input line | Quit. |

Ctrl-C is not one of them: as in any terminal program, it ends Tablon.

## Commands

`/` opens the palette. Type to narrow it, Up and Down to choose, Enter to run, Escape to close.
Words after the command's name are its arguments, such as `/run 3 4`.

| Command | What it does |
|---------|--------------|
| `/check` | Verify the project with `gardelc --inspect --json-diagnostics`. Its errors and warnings go to Problems, and the modules are read again. |
| `/build` | Verify and compile the project with `gardelc --json-diagnostics`. Its steps go to Build, its errors to Problems. |
| `/run [ARGS]` | Build, then run the program with `ARGS`, split at spaces. |
| `/stop` | Stop the program run, or else the build, or else the check. |
| `/edit [EDITOR]` | Open the file shown in the editor, then read it again and check the project. With `EDITOR`, that editor is used from then on. |
| `/new NAME` | Make the module `NAME`: write `src/name.gdl`, in lowercase, unless a file is there already, then open it and read the modules again. `NAME` is a letter, then letters, digits and `_`. |
| `/inspect` | Read the project's modules again, without proving them. |
| `/open DIR` | Browse the project in `DIR` instead. |
| `/quit` | Leave Tablon. |

None of them waits: each runs `gardelc` or the program as a process, the status bar says what
runs (`checking...`, `building...`, `running...`), and the keys keep working. Only one check,
one build and one program run at a time.

## Running a program

`/run` (or Ctrl-R) builds the project first, so the program run is always the source as it is
now, and runs it only when the build makes an executable. Tablon then runs it one of two ways:

- **An ordinary program** runs with its output read into Run. Its standard output is a pipe, not
  a terminal, so what it prints may come only when its buffer fills or it ends: a prompt such as
  `Name? ` with no line end can show after the line you type. Tablon gives the keys to its input
  line: type a line and press Enter to send it to the program's standard input. Ctrl-D ends the
  input, Escape gives the keys back to the tabs (Enter in Run types again), and Ctrl-X stops the
  program. The line Run shows when the program ends gives its exit code.
- **A reactive program**, which draws on the terminal itself, is run **attached**: Tablon gives
  it the terminal, and draws again when it ends. Leave the program as it leaves; Tablon cannot
  stop it, as the terminal is the program's.

## Editing

Tablon shows source but does not edit it yet (that is Task F.16). `/edit` (or Ctrl-E) runs your
editor on the file shown, attached to the terminal. When the editor ends, Tablon reads the file
again and checks the project, so Problems shows what the edit fixed or broke.

## How it is made

`src/tablon.gdl` is the reactive program: its `update` hands each event to `TABLON_STATE`
(`src/ui/tablon_state.gdl`), and its `view` draws the state. `src/model` reads `gardelc`'s JSON
(`INSPECTION`, `DIAGNOSTICS`) and makes the trees and the views; `src/ui` has the layout, the
lexer that colours the source, and the state. Every file read, listing and process is a command
whose answer is an event, so Tablon never waits on one.

`gardelc/tests/tablon.rs` tests Tablon by scripted sessions, which give it the events a session
would have (`GARDEL_EVENTS`) and read the screen it draws. A session recorded with
`GARDEL_RECORD=FILE` replays with `GARDEL_REPLAY=FILE`, with no process run.
