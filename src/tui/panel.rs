#![allow(dead_code)]

use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::widgets::Block;
use ratatui::Frame;

use super::widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    Table,
    Detail,
}

/// Render a two-panel layout: table on left, detail on right.
/// Returns the inner areas for table and detail content.
pub fn render_panel_frame(
    frame: &mut Frame,
    area: Rect,
    focus: PanelFocus,
    title: &str,
) -> (Rect, Rect) {
    let [table_area, detail_area] = Layout::horizontal([
        Constraint::Percentage(55),
        Constraint::Percentage(45),
    ])
    .areas(area);

    let table_block = Block::bordered()
        .title(title)
        .border_style(widgets::focus_border(focus == PanelFocus::Table));
    frame.render_widget(table_block, table_area);

    let detail_block = Block::bordered()
        .border_style(widgets::focus_border(focus == PanelFocus::Detail));
    frame.render_widget(detail_block, detail_area);

    let table_inner = table_area.inner(Margin::new(1, 1));
    let detail_inner = detail_area.inner(Margin::new(1, 1));
    (table_inner, detail_inner)
}

/// Handle arrow key navigation for a panel.
/// Returns true if the key was consumed.
pub fn handle_panel_nav(
    focus: &mut PanelFocus,
    selected: &mut usize,
    item_count: usize,
    code: crossterm::event::KeyCode,
) -> bool {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Up => {
            if *focus == PanelFocus::Table && item_count > 0 {
                *selected = if *selected == 0 {
                    item_count - 1
                } else {
                    *selected - 1
                };
            }
            true
        }
        KeyCode::Down => {
            if *focus == PanelFocus::Table && item_count > 0 {
                *selected = (*selected + 1) % item_count;
            }
            true
        }
        KeyCode::Right | KeyCode::Enter => {
            if *focus == PanelFocus::Table {
                *focus = PanelFocus::Detail;
            }
            true
        }
        KeyCode::Left | KeyCode::Esc => {
            if *focus == PanelFocus::Detail {
                *focus = PanelFocus::Table;
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
