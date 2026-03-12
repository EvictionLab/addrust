use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::address::COL_DEFS;
use crate::pattern::{self, PatternSegment};
use crate::step::{OutputCol, StepDef};

use super::meta::{self, PropKey, TABLE_DESCRIPTIONS};
use super::widgets;
use super::App;

// Re-export types used by mod.rs and tabs.rs
pub(crate) use types::*;

mod types {
    /// Which panel is open.
    #[derive(Debug, Clone)]
    pub(crate) enum PanelKind {
        Step(StepPanelState),
        Dict(DictPanelState),
    }

    /// Focus state within the panel.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum PanelFocus {
        /// Navigating the field list with up/down.
        Navigating,
        /// Editing a single-value field inline (cursor position, buffer text).
        InlineEdit { cursor: usize, buffer: String },
        /// Navigating items in an expanded dropdown.
        Dropdown { cursor: usize },
        /// Editing an item within a dropdown (item index, cursor, buffer).
        DropdownEdit { item: usize, cursor: usize, buffer: String },
    }

    /// Step panel state.
    /// Note: step type is stored in `def.step_type` — no separate field.
    /// When cycling types with Left/Right, update `def.step_type` directly.
    #[derive(Debug, Clone)]
    pub(crate) struct StepPanelState {
        /// Index into App.steps, or None for new step.
        pub(crate) step_index: Option<usize>,
        /// Working copy of the step definition (includes step_type).
        pub(crate) def: crate::step::StepDef,
        /// Which fields are visible (computed from def.step_type).
        pub(crate) visible_fields: Vec<StepField>,
        /// Cursor position in visible_fields.
        pub(crate) field_cursor: usize,
        /// Current focus.
        pub(crate) focus: PanelFocus,
        /// Parsed pattern segments for drill-down.
        pub(crate) pattern_segments: Vec<crate::pattern::PatternSegment>,
        /// Whether this is a new step.
        pub(crate) is_new: bool,
        /// Show discard confirmation.
        pub(crate) show_discard_prompt: bool,
    }

    /// Dictionary panel state.
    #[derive(Debug, Clone)]
    pub(crate) struct DictPanelState {
        /// Index into the current dict_entries vec.
        pub(crate) entry_index: usize,
        /// Working copies.
        pub(crate) short: String,
        pub(crate) long: String,
        pub(crate) variants: Vec<(String, bool)>,  // (text, enabled)
        /// Which field is focused: 0=short, 1=long, 2=variants.
        pub(crate) field_cursor: usize,
        /// Current focus.
        pub(crate) focus: PanelFocus,
        /// Whether this is a new entry.
        pub(crate) is_new: bool,
    }

    /// Fields in the step panel.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum StepField {
        Label,
        Pattern,
        Table,
        OutputCol,
        SkipIfFilled,
        Replacement,
        InputCol,
        Mode,
    }
}

/// Compute a centered overlay rect with auto-height.
/// Width is 70% of area, height is `content_lines` + 5 (header 3 + footer 2),
/// clamped to area height - 4.
pub(crate) fn centered_overlay(area: Rect, content_lines: u16) -> Rect {
    let width = (area.width * 70 / 100).max(50).min(100).min(area.width);
    let height = (content_lines + 5).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}
