# Step Reordering Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add move-mode reordering of pipeline steps in the TUI, persisted via `step_order` in config.

**Architecture:** Three layers — config (`step_order` field), pipeline (reorder compiled steps), TUI (move mode interaction). Each layer is independently testable.

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, serde/toml

**Spec:** `docs/superpowers/specs/2026-03-10-step-reordering-design.md`

---

## Chunk 1: Config and Pipeline

### Task 1: Add `step_order` to `StepsConfig`

**Files:**
- Modify: `src/config.rs:59-72`

- [ ] **Step 1: Write failing test for step_order serialization**

Add to `src/config.rs` tests module:

```rust
#[test]
fn test_step_order_roundtrip() {
    let mut config = Config::default();
    config.steps.step_order = vec!["po_box".to_string(), "na_check".to_string()];
    let toml_str = config.to_toml();
    assert!(toml_str.contains("step_order"));
    let parsed: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.steps.step_order, vec!["po_box", "na_check"]);
}

#[test]
fn test_step_order_empty_not_serialized() {
    let config = Config::default();
    let toml_str = config.to_toml();
    assert!(!toml_str.contains("step_order"));
}

#[test]
fn test_steps_config_is_empty_with_step_order() {
    let mut sc = StepsConfig::default();
    assert!(sc.is_empty());
    sc.step_order = vec!["na_check".to_string()];
    assert!(!sc.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests -- test_step_order`
Expected: compilation errors — `step_order` field doesn't exist yet.

- [ ] **Step 3: Add `step_order` field to `StepsConfig`**

In `src/config.rs`, add to `StepsConfig` struct (after `pattern_overrides`):

```rust
#[serde(skip_serializing_if = "Vec::is_empty")]
pub step_order: Vec<String>,
```

Update `is_empty()`:

```rust
pub fn is_empty(&self) -> bool {
    self.disabled.is_empty() && self.pattern_overrides.is_empty() && self.step_order.is_empty()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests`
Expected: all config tests pass, including new ones.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add step_order field to StepsConfig"
```

### Task 2: Apply `step_order` in pipeline

**Files:**
- Modify: `src/pipeline.rs:31-67`

- [ ] **Step 1: Write failing test for step_order reordering**

Add to `src/pipeline.rs` tests module:

```rust
#[test]
fn test_config_step_order() {
    let toml_str = r#"
[steps]
step_order = ["pre_direction", "suffix_common", "na_check"]
"#;
    let config: crate::config::Config = toml::from_str(toml_str).unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    // First three should be reordered
    assert_eq!(summaries[0].label, "pre_direction");
    assert_eq!(summaries[1].label, "suffix_common");
    assert_eq!(summaries[2].label, "na_check");
}

#[test]
fn test_config_step_order_unknown_labels_ignored() {
    let toml_str = r#"
[steps]
step_order = ["nonexistent", "na_check", "po_box"]
"#;
    let config: crate::config::Config = toml::from_str(toml_str).unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    // na_check and po_box should be first two (nonexistent ignored)
    assert_eq!(summaries[0].label, "na_check");
    assert_eq!(summaries[1].label, "po_box");
}

#[test]
fn test_config_step_order_missing_labels_appended() {
    // Only specify a partial order — remaining steps keep relative default order
    let toml_str = r#"
[steps]
step_order = ["suffix_common", "na_check"]
"#;
    let config: crate::config::Config = toml::from_str(toml_str).unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    assert_eq!(summaries[0].label, "suffix_common");
    assert_eq!(summaries[1].label, "na_check");
    // Remaining steps should follow in their default relative order
    // city_state_zip is 2nd in defaults, po_box is 3rd, etc.
    assert_eq!(summaries[2].label, "city_state_zip");
    assert_eq!(summaries[3].label, "po_box");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pipeline::tests -- test_config_step_order`
Expected: FAIL — reordering not implemented yet, default order returned.

- [ ] **Step 3: Implement step reordering in `from_steps_config()`**

In `src/pipeline.rs`, in `from_steps_config()`, add reordering after `compile_steps` and before the disabled loop. Replace lines 53-60 with:

```rust
        let mut steps = compile_steps(&defs.step, &tables);

        // Apply step_order reordering
        if !config.steps.step_order.is_empty() {
            let order = &config.steps.step_order;
            // Build position map: label -> index in step_order
            let pos_map: std::collections::HashMap<&str, usize> = order
                .iter()
                .enumerate()
                .map(|(i, label)| (label.as_str(), i))
                .collect();

            // Partition into ordered (in step_order) and unordered (not in step_order)
            let mut ordered: Vec<(usize, crate::step::Step)> = Vec::new();
            let mut unordered: Vec<crate::step::Step> = Vec::new();
            for step in steps {
                if let Some(&pos) = pos_map.get(step.label()) {
                    ordered.push((pos, step));
                } else {
                    unordered.push(step);
                }
            }
            ordered.sort_by_key(|(pos, _)| *pos);
            steps = ordered.into_iter().map(|(_, s)| s).collect();
            steps.extend(unordered);
        }

        // Apply disabled list
        for step in &mut steps {
            if config.steps.disabled.contains(&step.label().to_string()) {
                step.set_enabled(false);
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pipeline::tests`
Expected: all pipeline tests pass, including new step_order tests.

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat: apply step_order reordering in pipeline"
```

## Chunk 2: TUI Move Mode

### Task 3: Add move mode state and keybindings

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Add move mode fields to `App` struct**

In `src/tui.rs`, add two fields to the `App` struct (after `steps_list_state` at line 107):

```rust
    /// If Some, we're in move mode — value is the index of the step being moved.
    moving_step: Option<usize>,
    /// Original index before move started, for Esc cancel.
    moving_step_origin: Option<usize>,
```

Initialize both to `None` in `App::new()` (after `steps_list_state` initialization, around line 152):

```rust
            moving_step: None,
            moving_step_origin: None,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles (fields added but not yet used, may get warnings).

- [ ] **Step 3: Add move mode key handling**

In `src/tui.rs`, modify `handle_rules_key()` (line 478). The function currently handles `Down/j`, `Up/k`, `Space`, and `Enter`. Add move mode handling at the top of the function, before the existing match:

```rust
fn handle_rules_key(app: &mut App, code: KeyCode) {
    let len = app.steps.len();
    if len == 0 {
        return;
    }

    // Move mode: step is grabbed, arrow keys reposition it
    if let Some(moving_idx) = app.moving_step {
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                if moving_idx + 1 < len {
                    app.steps.swap(moving_idx, moving_idx + 1);
                    let new_idx = moving_idx + 1;
                    app.moving_step = Some(new_idx);
                    app.steps_list_state.select(Some(new_idx));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if moving_idx > 0 {
                    app.steps.swap(moving_idx, moving_idx - 1);
                    let new_idx = moving_idx - 1;
                    app.moving_step = Some(new_idx);
                    app.steps_list_state.select(Some(new_idx));
                }
            }
            KeyCode::Enter => {
                app.moving_step = None;
                app.moving_step_origin = None;
                app.dirty = true;
            }
            KeyCode::Esc => {
                // Cancel: remove from current position, re-insert at origin
                if let Some(origin) = app.moving_step_origin {
                    let step = app.steps.remove(moving_idx);
                    app.steps.insert(origin, step);
                    app.steps_list_state.select(Some(origin));
                }
                app.moving_step = None;
                app.moving_step_origin = None;
            }
            _ => {} // All other keys ignored in move mode
        }
        return;
    }

    // Normal mode (existing code)
    match code {
        // ... existing match arms unchanged ...
```

Add the `m` key to the existing normal-mode match (add a new arm before the `_ => {}` catch-all):

```rust
        KeyCode::Char('m') => {
            if let Some(i) = app.steps_list_state.selected() {
                app.moving_step = Some(i);
                app.moving_step_origin = Some(i);
            }
        }
```

- [ ] **Step 4: Block tab switching and global keys during move mode**

In `run_loop()` (line 397), the normal-mode match at line 436 handles `q/Esc`, `Tab/BackTab`, and `s` globally before delegating to tab handlers. Move mode needs to intercept these. Add a check before the normal mode block (after the input_mode check at line 430):

```rust
            // Move mode: only step handler processes keys
            if app.moving_step.is_some() && app.active_tab == Tab::Steps {
                handle_rules_key(app, key.code);
                continue;
            }
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 6: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add move mode keybindings for step reordering"
```

### Task 4: Add move mode visual feedback

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Update step list rendering for move mode highlight**

In `render_steps()` (line 955), modify the item rendering loop. The current code builds `ListItem` with styles based on enabled/disabled state. Add move-mode styling. Change the `.map(|r| {` closure to include the index and check for move mode:

```rust
    let items: Vec<ListItem> = app
        .steps
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let is_moving = app.moving_step == Some(idx);
            let check = if r.enabled { " " } else { "x" };
            let style = if is_moving {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if !r.enabled {
                Style::new().fg(Color::DarkGray)
            } else if r.enabled != r.default_enabled {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            let check_style = if is_moving {
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if r.enabled {
                Style::new().fg(Color::Green)
            } else {
                Style::new().fg(Color::Red)
            };
            let pattern_style = if is_moving {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new().fg(Color::DarkGray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", check), check_style),
                Span::styled(format!("{:30} ", r.label), style),
                Span::styled(format!("{:8} ", r.action_desc), if is_moving { style } else { Style::new().fg(Color::DarkGray) }),
                Span::styled(&r.pattern_template, pattern_style),
            ]))
        })
        .collect();
```

- [ ] **Step 2: Update status bar for move mode**

In `render()` (line 864), modify the status bar section (lines 898-904). Replace with:

```rust
    // Status bar
    let dirty_indicator = if app.dirty { " [modified]" } else { "" };
    let status_text = if app.moving_step.is_some() {
        format!(" ↑↓: move | Enter: confirm | Esc: cancel{}", dirty_indicator)
    } else {
        format!(
            " Tab: switch | j/k: navigate | Space: toggle | m: move | Enter: edit | s: save | q: quit{}",
            dirty_indicator
        )
    };
    let status = Paragraph::new(status_text)
        .style(Style::new().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(status, status_area);
```

Note: also added `m: move` to the normal-mode status bar so the feature is discoverable.

- [ ] **Step 3: Verify it compiles and run tests**

Run: `cargo check && cargo test`
Expected: compiles, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add move mode visual feedback in TUI"
```

### Task 5: Fix config round-trip for reordered steps

**Files:**
- Modify: `src/tui.rs`

- [ ] **Step 1: Fix `App::new()` — use label-based lookup instead of positional zip**

In `App::new()` (line 132), replace the step-building code (lines 138-147):

```rust
        // Build step states from step summaries
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.step_summaries();
        let config_summaries = pipeline.step_summaries();

        // Use label-based lookup for default_enabled (not positional zip)
        let default_enabled_map: std::collections::HashMap<&str, bool> = default_summaries
            .iter()
            .map(|s| (s.label.as_str(), s.enabled))
            .collect();

        let steps: Vec<StepState> = config_summaries
            .iter()
            .map(|current| {
                let default_enabled = default_enabled_map
                    .get(current.label.as_str())
                    .copied()
                    .unwrap_or(true);
                StepState {
                    label: current.label.clone(),
                    group: current.step_type.clone(),
                    action_desc: current.step_type.clone(),
                    pattern_template: current.pattern_template.clone().unwrap_or_default(),
                    enabled: current.enabled,
                    default_enabled,
                }
            })
            .collect();
```

- [ ] **Step 2: Fix `to_config()` — use label-based lookup for pattern overrides**

In `to_config()` (line 289), replace the pattern overrides section (lines 301-312):

```rust
        // Pattern overrides: compare by label (not position) since steps may be reordered
        let default_pipeline = Pipeline::default();
        let default_summaries = default_pipeline.step_summaries();
        let default_patterns: std::collections::HashMap<&str, &str> = default_summaries
            .iter()
            .map(|s| (s.label.as_str(), s.pattern_template.as_deref().unwrap_or("")))
            .collect();

        for step in &self.steps {
            let default_template = default_patterns
                .get(step.label.as_str())
                .copied()
                .unwrap_or("");
            if step.pattern_template != default_template {
                config.steps.pattern_overrides.insert(
                    step.label.clone(),
                    step.pattern_template.clone(),
                );
            }
        }
```

- [ ] **Step 3: Add step_order to `to_config()`**

After the pattern overrides section in `to_config()`, add step order detection (before the `// Dictionaries` section):

```rust
        // Step order: only store if different from default
        let default_order: Vec<&str> = default_summaries.iter().map(|s| s.label.as_str()).collect();
        let current_order: Vec<&str> = self.steps.iter().map(|s| s.label.as_str()).collect();
        if current_order != default_order {
            config.steps.step_order = self.steps.iter().map(|s| s.label.clone()).collect();
        }
```

- [ ] **Step 4: Verify it compiles and run tests**

Run: `cargo check && cargo test`
Expected: compiles, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "feat: fix config round-trip for reordered steps"
```

### Task 6: Integration test

**Files:**
- Modify: `src/pipeline.rs` (add test)

- [ ] **Step 1: Write integration test for full round-trip**

Add to `src/pipeline.rs` tests module:

```rust
#[test]
fn test_step_order_with_disabled_and_overrides() {
    // Combine all three config features
    let toml_str = r#"
[steps]
disabled = ["na_check"]
step_order = ["po_box", "na_check", "city_state_zip"]

[steps.pattern_overrides]
po_box = '(?i)P\.?\s*O\.?\s*BOX\s+(\w+)'
"#;
    let config: crate::config::Config = toml::from_str(toml_str).unwrap();
    let p = Pipeline::from_config(&config);
    let summaries = p.step_summaries();
    // po_box first, na_check second (disabled), city_state_zip third
    assert_eq!(summaries[0].label, "po_box");
    assert_eq!(summaries[1].label, "na_check");
    assert!(!summaries[1].enabled); // na_check is disabled
    assert_eq!(summaries[2].label, "city_state_zip");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib pipeline::tests::test_step_order_with_disabled_and_overrides`
Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all tests pass (unit, integration, golden).

- [ ] **Step 4: Commit**

```bash
git add src/pipeline.rs
git commit -m "test: add integration test for step_order with disabled and overrides"
```
