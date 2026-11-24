# CLRS Studio — Verified Algorithm Visualizer (`clrs_studio`)

An interactive terminal visualizer for algorithms from *Introduction to Algorithms* (CLRS), written in Gardel on [`gtui`](../../gtui) `0.6.0`. Rather than treating correctness as an afterthought, `clrs_studio` spotlights Gardel's compile-time Boogie/Z3 proofs directly inside the UI:

* **CLRS Ch. 2.1 Insertion Sort:** Fine-grained 4-phase stepper (`PickKey` → `CompareShift` → `InsertKey` → `Done`) with statically proven outer/inner loop invariants, **true permutation preservation (`count_of: MATH_INT`, inspired by [Firulete](https://github.com/GardelPL/Firulete))**, strict termination (`work_left`), and final `is_sorted(A)` postcondition — all proof lemmas live inside erased `check` blocks with zero `assume` and zero runtime assertion overhead.
* **CLRS Ch. 22.3 Dijkstra's Shortest Path (4th ed.):** Adjacency-matrix stepper (`ExtractMin` → `RelaxEdge` → `Done`) with weighted directed graph diagram + live distance/priority-queue table, inspired by [Microsoft Research's AutoCLRS](https://risemsr.github.io/blog/2026-03-06-autoclrs/). Statically proves `dist[source] = 0`, bounded distances & parent pointers, exact settled-set count, strict termination, and an **AutoCLRS-style complexity bound (`ticks <= n * 9`)**.
* **Visual-to-Contract Coupling:** The proven sorted prefix in the bar chart shares the same green accent as its `✓ [PROVEN]` invariant badges, and the termination metric `work_left` renders as a live `GAUGE` draining to `0`.
* **Pure Time-Travel Stepping:** `SORT_HISTORY` materializes the entire trace using `work_left` as its proven termination `variant`, powering **Step Forward**, **Step Back**, **Auto-Play** (`SetTimer`/`Tick`), and **Timeline Scrubbing (`SLIDER`)**.
* **Synchronized Code & Short Form:** Switch tabs between **Invariants (`i`)**, syntax-highlighted **Code (`c`)** (with the active phase line marked), and the real `gardelc --inspect INSERTION_SORT` **Short Form (`s`)**.

```text
 File  Algorithm  Input  Theme  Help
┌ Insertion Sort — CLRS Ch. 2.1 ──────────┐┌ Invariants(i) │ Code(c) │ Short(s) ┐
│         ██                              ││ ✓ sorted_with_hole(0..j)  [PROVEN] │
│   ██    ██          ██                  ││ ✓ length_matches (n = len)[PROVEN] │
│   ██    ██    ██    ██                  ││ ✓ sum_conserved (exact)   [PROVEN] │
│   ██    ██    ██    ██    ██            ││ ✓ 0 <= hole <= j <= n     [PROVEN] │
│   ██    ██    ██    ██    ██    ██      ││ ✓ work_left strictly ↓    [PROVEN] │
│   2     4     5     6     1     3       ││ ── Variant (work_left) ─────────── │
│   0     1     2     ^cmp  ^key  5       ││ [██████████░░░░░░░░] 112 / 179 → 0 │
│   ■ proven sorted  ■ key  ■ comparing   ││ ── Live State ──────────────────── │
│  [x] Highlight proven region            ││ Phase         CompareShift         │
│  [x] Show bar values                    ││ j / hole / key  4 / 3 / 1          │
└─────────────────────────────────────────┘└────────────────────────────────────┘
┌ Transport ────────────────────── Step 6/21 ─────────────────────────── Speed ─┐
│ [ ◀ Back ] [ Play  ] [ Step ▶ ] [ Reset ]  ━━━━●──────── 6/21   ━━━●── 5     │
└───────────────────────────────────────────────────────────────────────────────┘
 CLRS Studio · CLRS [5,2,4,6,1,3]    Space:play ←/→:step r:reset 1-5:preset q:quit
```

## Verify, Build, and Run

From the repository root (see [`SETUP.md`](../../SETUP.md) for configuring `gardelc` and `Z3_EXE`):

```powershell
gardelc --project examples/clrs_studio --strict
./build/clrs_studio_bin.exe
```

Inspect the Eiffel-style verified contract interfaces directly:

```powershell
gardelc --project examples/clrs_studio --inspect INSERTION_SORT --no-verify
gardelc --project examples/clrs_studio --inspect DIJKSTRA --no-verify
```

## 30-Second Sabotage Demo

To see Gardel's compile-time proofs in action:

1. Open [`src/algo/insertion_sort.gdl`](src/algo/insertion_sort.gdl) and find `step_compare_shift`.
2. Introduce a classic 1-character bug by changing `>` to `<`:
   ```eiffel
   if Current.hole > 0 and LIST_OPS.at(Current.items, Current.hole - 1) < Current.key then
   ```
3. Run `gardelc --project examples/clrs_studio --strict`.
4. **Result:** Boogie/Z3 refuses to build the TUI and pinpoints the violated CLRS invariants (`greater_inner_right` / `insert_ready`).

This negative proof is also automated in [`gardelc/tests/clrs_studio.rs`](../../gardelc/tests/clrs_studio.rs).

## Controls

### Keyboard

| Key | Action |
| :--- | :--- |
| `Space` | Play / Pause deterministic auto-play |
| `a` | Switch Algorithm (**Insertion Sort Ch. 2.1** ↔ **Dijkstra Ch. 24.3**) |
| `Right` / `n` / `l` | Step Forward |
| `Left` / `p` / `h` | Step Back |
| `r` / `Home` | Reset to step 0 |
| `End` | Jump directly to `Done` |
| `i` / `c` / `s` | Switch right pane: **Invariants** / **Code** / **Short Form** |
| `1` – `5` | Load presets (5 Insertion Sort arrays or 3 Dijkstra graphs incl. CLRS Fig. 24.6) |
| `u` | Open Custom Input modal (`0..999`, 1–32 integers) |
| `t` | Cycle Theme (`Dark`, `Light`, `Nord`, `Dracula`) |
| `+` / `=`, `-` | Increase / decrease auto-play speed |
| `F10` | Open top menu bar |
| `Tab` | Switch pane focus |
| `?` | Toggle Help popup |
| `q` | Quit |

### Mouse

* Click top **Menu** items (`File`, `Algorithm`, `Input`, `Theme`, `Help`).
* Click **Tabs** (`Invariants`, `Code`, `Short Form`).
* Click transport **Buttons** (`◀ Back`, `Play`/`Pause`, `Step ▶`, `Reset`).
* Click or drag the **Timeline** slider to scrub backward/forward through time, or the **Speed** slider.
* Click **Checkboxes** to toggle proven-region highlighting and bar value labels.
* Click a **Toast** notification to dismiss it.

## Tests

```powershell
cargo test --manifest-path gardelc/Cargo.toml --test clrs_studio
```
