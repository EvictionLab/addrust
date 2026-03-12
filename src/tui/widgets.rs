#![allow(dead_code)]

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn selected_style(selected: bool) -> Style {
    if selected {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    }
}

pub fn focus_border(focused: bool) -> Style {
    Style::new().fg(if focused { Color::Cyan } else { Color::DarkGray })
}

pub fn checkbox(checked: bool) -> &'static str {
    if checked { "[x]" } else { "[ ]" }
}

pub fn cursor_line<'a>(text: &'a str, cursor: usize) -> Line<'a> {
    let pos = cursor.min(text.len());
    let (before, after) = text.split_at(pos);
    Line::from(vec![
        Span::styled(before, Style::new().fg(Color::White)),
        Span::styled(
            if after.is_empty() { "_".to_string() } else { after[..1].to_string() },
            Style::new().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(
            if after.len() > 1 { after[1..].to_string() } else { String::new() },
            Style::new().fg(Color::White),
        ),
    ])
}

pub fn truncate(text: &str, width: usize) -> String {
    if text.len() <= width {
        text.to_string()
    } else if width > 3 {
        format!("{}...", &text[..width - 3])
    } else {
        text[..width].to_string()
    }
}
