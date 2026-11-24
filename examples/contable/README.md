# Contable — Verified Double-Entry Ledger & Live Budget Studio (`contable`)

A terminal financial ledger and monthly budget studio written in Gardel on [`gtui`](../../gtui) `0.6.0`. Every accounting invariant is proved statically at compile time by Boogie/Z3 under `--strict` with zero `assume` and zero runtime assertion overhead:

* **Conservation of Money (Trial Balance = `$0.00`):** Account balances are stored as signed net debit-minus-credit integer cents (`I64`), so the accounting equation (`Assets + Expenses = Liabilities + Equity + Income`) reduces to `LEDGER_FACTS.sum_balances_math(accts) = 0` in unbounded `MATH_INT`. Every `JOURNAL_ENTRY` statically proves `sum(Debits) = sum(Credits)`, and `LEDGER.post_entry` proves the global trial balance remains `0` after every transaction.
* **Overflow-Free 64-Bit Arithmetic:** Every `ACCOUNT` enforces `[-10^12, +10^12]` cent bounds (`$10B`), and `ACCOUNT.apply_delta` / `LEDGER_FACTS.apply_postings` statically prove no balance can ever overflow `I64`.
* **Immutable Append-Only Audit Trail:** `LEDGER.post_entry` and `LEDGER.reverse_entry` prove `Result.entries.tail = Current.entries` and `Result.entry_count = Current.entry_count + 1`. Posted entries can never be edited or deleted — corrections only happen by appending a balanced compensating reversal entry.
* **Pure Time-Travel Scrubbing:** `LEDGER_HISTORY` records each verified `LEDGER` snapshot so dragging the **History `SLIDER`** (or pressing `←`/`→`) scrubs through every historical balance sheet, monthly budget `GAUGE`, and net-worth `SPARKLINE`.

```text
 File(Alt+F)  Ledger(Alt+L)  Presets(Alt+P)  Theme(Alt+T)  Help
 Contable › Freelance Studio › Budgets            ✓ Trial Balance = $0.00 [PROVEN]
┌ Chart of Accounts ────── 8 accounts ┐┌ Budgets(b) | Entry(e) | Contracts(c) ───┐
│ #  Account         Type     Balance ││ ── Monthly Expense Budgets ──────────── │
│ 0  Checking        Asset  $8,125.00 ││ Rent & Ops  $1,150.00 / $1,500.00       │
│ 1  Tax Reserve     Asset  $1,425.00 ││ [███████████████░░░░░] 76%              │
│ 2  Credit Card     Liab.    $327.50 ││ Cloud & Tools  $185.00 / $300.00        │
│ ...                                 ││ [████████████░░░░░░░░] 61%              │
├ Journal (Immutable Audit Trail) ────┤│ ── Net Worth Trend ($9,222.50) ──────── │
│ #8 2026-11-20 Invoice #102 (Split)  ││ ▁▄▅▅▅▅▅▅█                               │
│ #7 2026-11-15 Client Lunch & Transit││ ── Accounting Equation [PROVEN] ─────── │
│ #6 2026-11-12 REV #5 Duplicate SaaS ││ Assets (DR)        $9,550.00            │
└─────────────────────────────────────┴┴─────────────────────────────────────────┘
┌ Actions & Time-Travel ─────────────────────────────────────────── Snapshot 8/8 ┐
│ [+ New Entry] [↺ Reverse] [◀ Back] [Next ▶]  ━━━━━━━━━━━━━━● 8                 │
└────────────────────────────────────────────────────────────────────────────────┘
 Contable · Freelance Studio   n:new r:reverse ←/→:history ↑/↓:select b/e/c q:quit
```

## Verify, Build, and Run

From the repository root (ensure `boogie` and `Z3_EXE` are configured):

```powershell
gardelc --project examples/contable --strict
./build/contable_bin.exe
```

Inspect the Eiffel-style verified contract interfaces directly:

```powershell
gardelc --project examples/contable --inspect LEDGER --no-verify
gardelc --project examples/contable --inspect JOURNAL_ENTRY --no-verify
```

## 30-Second Sabotage Demo (1-Cent Imbalance Bug)

To see Boogie/Z3 catch a financial bug at compile time:

1. Open [`src/domain/journal_entry.gdl`](src/domain/journal_entry.gdl) and find `make_split_debit2_credit1`.
2. Introduce a 1-cent bug by subtracting `1` from `dr2_amt` when creating `p2`:
   ```eiffel
   let p2 = POSTING.debit(dr2_acct, if dr2_amt > 1 then dr2_amt - 1 else 1 end)
   ```
3. Run `gardelc --project examples/contable --strict`.
4. **Result:** Boogie/Z3 refuses to compile `contable` and pinpoints the violated `JOURNAL_ENTRY` invariant (`balanced` / `debits_eq_credits`).

This negative proof is automated in [`gardelc/tests/contable.rs`](../../gardelc/tests/contable.rs).

## Controls

### Keyboard

| Key | Action |
| :--- | :--- |
| `n` | Open **Post Balanced Entry** modal (`<debit#> <credit#> <amount> [memo]`) |
| `r` | Open **Append Compensating Reversal** confirmation dialog for the selected entry |
| `Up` / `Down` / `j` / `k` | Navigate focused Accounts table, Journal table, or Contracts view |
| `Left` / `Right` / `Home` / `End` | Scrub backward / forward through Ledger History snapshots |
| `b` / `e` / `c` | Switch right pane tabs: **Budgets** / **Entry Detail** / **Contracts** |
| `Tab` | Cycle focus between Accounts, Journal, and Right pane |
| `1` – `3` | Switch starter books (*Freelance Studio*, *Personal Monthly Budget*, *Small Cafe*) |
| `t` | Cycle Theme (`Dark`, `Light`, `Nord`, `Dracula`) |
| `F10` | Open top menu bar |
| `?` | Toggle Help popup |
| `q` | Quit |

### Mouse

* Click top **Menu** items (`File`, `Ledger`, `Presets`, `Theme`, `Help`).
* Click or scroll rows in **Chart of Accounts** and **Journal** tables.
* Click right-pane **Tabs** (`Budgets`, `Entry`, `Contracts`).
* Click action **Buttons** (`+ New Entry`, `↺ Reverse`, `◀ Back`, `Next ▶`) or drag the **History** slider.
* Click a **Toast** notification to dismiss it.

## Tests

```powershell
cargo test --manifest-path gardelc/Cargo.toml --test contable
```
