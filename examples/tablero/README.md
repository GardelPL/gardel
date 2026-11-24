# Tablero — Parallel Verified Constrained N-Queens Solver (`tablero`)

An interactive terminal constraint solver written in Gardel on [`gtui`](../../gtui) `0.6.0`. **Tablero** showcases Gardel's **automatic lock-free parallelism (C11 + OpenMP)** alongside compile-time Boogie/Z3 **soundness** and **termination** proofs with zero `assume` and zero runtime assertion overhead:

* **Verified Soundness (`is_valid_solution`):** `QUEENS_SOLVER.solve` and `solve_range` statically prove that whenever `Some(sol)` is returned, `sol` has `N` queens with no row, column, or diagonal attacks, respects every user-pinned queen, and avoids every blocked square.
* **Guaranteed Termination (`variant`):** `solve_range` proves termination via `(board.size - placed.length) * 16 + (hi - lo)`.
* **Automatic Lock-Free Parallelism (OpenMP):** `QUEENS_SOLVER.solve_range` splits candidate columns `[lo..hi)` into two independent recursive `let` bindings (`left_res` and `right_res`), which `gardelc` automatically compiles into `#pragma omp task` / `#pragma omp taskwait` with zero mutexes, zero data races, and bit-for-bit deterministic output (`SOLVE_OUTCOME.merge` always selects the lexicographically first valid solution).
* **Interactive TrueColor Board:** Click (or use Arrow keys + `Enter` / `x`) to pin queens (`♛`) or block squares (`✕`), toggle live attack-ray highlighting, count all valid solutions across `N = 4..10`, and inspect the proven coordinates table and short-form contracts.

## Verify, Build, and Run

From the repository root (ensure `boogie` and `Z3_EXE` are configured):

```powershell
gardelc --project examples/tablero --strict
./build/tablero_bin.exe
```

Inspect the Eiffel-style verified contract interfaces directly:

```powershell
gardelc --project examples/tablero --inspect QUEENS_SOLVER --no-verify
gardelc --project examples/tablero --inspect BOARD --no-verify
```

## 30-Second Sabotage Demo (Soundness Bug)

To see Boogie/Z3 catch an invalid solver branch at compile time:

1. Open [`src/domain/queens_solver.gdl`](src/domain/queens_solver.gdl) and find `solve_range`.
2. Remove the pairwise attack guard `and QUEENS_FACTS.safe_against(r, c, placed)`:
   ```eiffel
   if QUEENS_FACTS.cell_allowed(board, r, c) then
   ```
3. Run `gardelc --project examples/tablero --strict`.
4. **Result:** Boogie/Z3 refuses to compile `tablero` and pinpoints the violated `sound` postcondition (`QUEENS_FACTS.is_valid_solution(board, sol)`).

This negative proof is automated in [`gardelc/tests/tablero.rs`](../../gardelc/tests/tablero.rs).

## Controls

### Keyboard

| Key | Action |
| :--- | :--- |
| `Space` | Run Parallel OpenMP Solver |
| `Arrows` | Move board cursor |
| `Enter` | Toggle Pinned Queen (`♛`) at cursor |
| `x` | Toggle Blocked Cell (`✕`) at cursor |
| `1` – `4` | Load presets (*Classic 8×8*, *Pinned Corner 8×8*, *Blocked Center 8×8*, *Conflict 6×6 UNSAT*) |
| `c` | Clear all pins and blocks |
| `r` | Reset active preset |
| `p` / `s` / `v` | Switch right pane tabs: **Proofs** / **Solution** / **Contracts** |
| `t` | Cycle Theme (`Dark`, `Light`, `Nord`, `Dracula`) |
| `F10` | Open top menu bar |
| `?` | Toggle Help popup |
| `q` | Quit |

### Mouse

* **Left-click** any square on the board to toggle a pinned queen (`♛`).
* **Right-click** any square to toggle a blocked cell (`✕`).
* Click `[▶ Solve]`, `[Clear Pins]`, `[Reset]`, or drag the **Size `N`** slider (`4..10`).
* Toggle **Show pinned queen attack rays** and **Count all solutions (OpenMP)** checkboxes.

## Tests

```powershell
cargo test --manifest-path gardelc/Cargo.toml --test tablero
```
