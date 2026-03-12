# Shared Panel Layout — Design

**Goal:** All three tabs (Steps, Dict, Output) use `render_panel_frame()` for a consistent two-panel layout. Step form is a centered overlay, not a page replacement.

---

## Layout Pattern

All tabs: table on left (55%), detail panel on right (45%). Arrow keys navigate the table, Enter drills into detail or opens overlay.

### Steps Tab
- **Left**: Table with columns: ✓, Label, Type, Input, Output, Pattern (existing table code)
- **Right**: Summary of selected step (type, pattern, output_col, input_col, etc.)
- **Enter**: Opens step form as **centered overlay** (like dict modals), not replacing the table
- **Space**: Toggle enabled/disabled (stays on table)

### Dict Tab
- **Left**: Table with columns: Short, Long, Variants (count), Status
- **Right**: Variant list for selected group (with add/delete)
- **Enter**: Drills into right panel to view/edit variants
- Subtab navigation (Left/Right) switches between tables

### Output Tab
- **Left**: Table with columns: Component, Format, Example
- **Right**: Format picker (Short/Long) with preview
- **Enter**: Drills into right panel to change format

### Step Form Overlay
- Rendered via `centered_rect()` + `Clear` on top of the steps table (same pattern as dict AddShort/EditVariants modals)
- Internal layout unchanged: header + two panels (field list left, editors right)
- Esc closes overlay, returns to steps table
