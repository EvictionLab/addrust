use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub fn checkbox(checked: bool) -> &'static str {
    if checked { "[x]" } else { "[ ]" }
}

/// Render text with a visible cursor, scrolling horizontally to keep cursor in view.
/// `max_width` is the available character width (0 = no scrolling).
pub fn cursor_line(text: &str, cursor: usize, max_width: usize) -> Line<'static> {
    let pos = cursor.min(text.len());

    // Compute scroll offset to keep cursor visible
    let scroll = if max_width == 0 || text.len() + 1 <= max_width {
        0
    } else {
        // Keep cursor roughly centered, but clamped
        let margin = max_width / 4;
        if pos < max_width.saturating_sub(margin) {
            0
        } else {
            (pos + margin).saturating_sub(max_width).min(text.len().saturating_sub(max_width.saturating_sub(1)))
        }
    };

    let end = text.len().min(scroll + max_width.max(text.len() + 1));
    let visible = &text[scroll..end];
    let cursor_in_visible = pos - scroll;

    let (before, after) = visible.split_at(cursor_in_visible);
    let mut spans = Vec::new();

    if scroll > 0 {
        spans.push(Span::styled("…", Style::new().fg(Color::DarkGray)));
        // Adjust: skip first char of before since we used it for the ellipsis
        if before.len() > 1 {
            spans.push(Span::styled(before[1..].to_string(), Style::new().fg(Color::White)));
        }
    } else {
        spans.push(Span::styled(before.to_string(), Style::new().fg(Color::White)));
    }

    spans.push(Span::styled(
        if after.is_empty() { "_".to_string() } else { after[..1].to_string() },
        Style::new().fg(Color::Black).bg(Color::White),
    ));

    if after.len() > 1 {
        let rest = &after[1..];
        if end < text.len() {
            // More text beyond visible area
            if rest.len() > 1 {
                spans.push(Span::styled(rest[..rest.len()-1].to_string(), Style::new().fg(Color::White)));
            }
            spans.push(Span::styled("…", Style::new().fg(Color::DarkGray)));
        } else {
            spans.push(Span::styled(rest.to_string(), Style::new().fg(Color::White)));
        }
    }

    Line::from(spans)
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
