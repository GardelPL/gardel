# Widgets & Controls Studio (`gtui_widgets`)

A unified interactive showcase of every widget added in [`gtui`](../../gtui) `0.6.0`: `MENU` (bar, drop-downs, sub-menus, separators, disabled items, shortcuts), `BREADCRUMB`, `DIVIDER` (labeled horizontal + vertical), `CHECKBOX`, `SLIDER`, `BUTTON`, `DIALOG` (modal confirmation and alert), and `TOAST` (4 severity levels, 6 screen positions, click-to-dismiss).

- **Top:** `MENU` bar (`File`, `Controls`, `Toast`, `Help`) with drop-downs and a `Theme ▸` sub-menu.
- **Below Menu:** `BREADCRUMB` path bar (`Studio › Appearance › Preview`).
- **Left Pane (`Controls`):**
  - Labeled horizontal `DIVIDER`s (`Options`, `Tuning`, `Actions`).
  - Three `CHECKBOX`es (`Wrap preview`, `Line numbers`, `Bold heading`).
  - Two `SLIDER`s (`Width: 20..80`, `Indent: 0..8`).
  - Three `BUTTON`s (`[ Apply ]`, `[ Toast ]`, `[ Reset ]`).
- **Center:** Vertical `DIVIDER` separating the controls from the preview.
- **Right Pane (`Live Preview`):** A live `PARAGRAPH` preview reflecting the breadcrumb section, checkboxes, sliders, theme, and toast settings.
- **Overlays:** `TOAST` notification queue (cycleable across all 6 positions) and modal `DIALOG` (on Reset, About, or Quit).

## Keys

| Key | What it does |
| :--- | :--- |
| `F10`, `Alt+f`/`c`/`t`/`h` | Open the `MENU` bar; arrows/`Enter`/`Escape` navigate |
| `Tab` / `Shift+Tab` | Move focus across the 8 form controls (checkboxes, sliders, buttons) |
| `Space` / `Enter` | Toggle focused `CHECKBOX`, press focused `BUTTON`, or step focused `SLIDER` |
| `Left` / `Right` / `Home` / `End` | Adjust focused `SLIDER`, or select a crumb in `BREADCRUMB` |
| `i`, `s`, `w`, `e` | Push an Info, Success, Warning, or Error `TOAST` |
| `p` | Cycle `TOAST` position (top-right, bottom-right, bottom-center, bottom-left, top-left, top-center) |
| `c` | Clear all `TOAST`s |
| `t` | Cycle `THEME` (`dark`, `nord`, `dracula`, `light`) |
| `r` | Open the Reset confirmation `DIALOG` |
| `q` | Open the Quit confirmation `DIALOG` |

## Mouse

| Mouse | What it does |
| :--- | :--- |
| Click a `MENU` entry or item | Open/switch drop-downs, sub-menus, or run a menu command |
| Click a `BREADCRUMB` crumb | Select that section in the preview |
| Click a `CHECKBOX` | Focus and toggle it |
| Click or drag on a `SLIDER` | Focus and set its value |
| Click a `BUTTON` | Focus and activate `Apply`, `Toast`, or `Reset` |
| Click a `TOAST` | Dismiss that notification |
| Click a `DIALOG` button | Confirm or cancel the active modal dialog (outside clicks are swallowed) |

## Verify, build and run

From the repository root:

```powershell
gardelc --project examples/gtui_widgets --strict
./build/gtui_widgets_bin.exe
```
