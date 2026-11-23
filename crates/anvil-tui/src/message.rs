//! Bottom message band for transient status and notifications.

use super::*;
use anvil_session::ui::NotificationSeverity;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Paragraph;

const MAX_MESSAGE_ROWS: usize = 3;

struct Message {
    marker: &'static str,
    color: Color,
    text: String,
}

fn warning_marker() -> &'static str {
    warning_marker_for(crate::icon_theme::ascii_mode())
}

fn warning_marker_for(ascii: bool) -> &'static str {
    if ascii { "!" } else { "▲" }
}

pub(crate) fn height(model: &Model, available: u16) -> u16 {
    let rows = messages(model).len();
    u16::try_from(rows.min(MAX_MESSAGE_ROWS))
        .unwrap_or(0)
        .min(available.saturating_sub(1))
}

/// The rows occupied by notifications inside the message band.
pub(crate) fn notification_area(model: &Model, area: Rect) -> Option<Rect> {
    let total = usize::from(height(model, area.height));
    let primary = usize::from(
        model.status.busy.is_some()
            || (matches!(model.ui.input_mode, InputMode::Normal) && !model.status.text.is_empty()),
    );
    let notifications = notification_messages(model)
        .len()
        .min(total.saturating_sub(primary));
    let height = u16::try_from(notifications).unwrap_or(0);
    (height > 0).then(|| Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(height),
        width: area.width,
        height,
    })
}

pub(crate) fn render(model: &Model, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let lines = messages(model)
        .into_iter()
        .take(usize::from(area.height))
        .map(|message| {
            Line::from(vec![
                Span::styled(
                    format!(" {} ", message.marker),
                    Style::default().fg(message.color),
                ),
                Span::styled(message.text, Style::default().fg(theme::text_color())),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).style(theme::status()), area);
}

fn messages(model: &Model) -> Vec<Message> {
    let mut messages = Vec::new();
    if let Some(busy) = &model.status.busy {
        messages.push(Message {
            marker: model.spinner_glyph(),
            color: theme::accent(),
            text: format!("{busy}…"),
        });
    } else if matches!(model.ui.input_mode, InputMode::Normal) && !model.status.text.is_empty() {
        let text = model.status.text.clone();
        let (marker, color) = if text.starts_with("E:") {
            ("!", theme::error())
        } else if text.starts_with("W:") {
            (warning_marker(), theme::warning())
        } else {
            (crate::art::DOT, theme::muted())
        };
        messages.push(Message {
            marker,
            color,
            text,
        });
    }
    let remaining = MAX_MESSAGE_ROWS.saturating_sub(messages.len());
    messages.extend(notification_messages(model).into_iter().take(remaining));
    messages
}

fn notification_messages(model: &Model) -> Vec<Message> {
    model
        .ui
        .notifications
        .items()
        .iter()
        .rev()
        .take(MAX_MESSAGE_ROWS)
        .map(|notification| {
            let (marker, color) = match notification.severity {
                NotificationSeverity::Error => ("!", theme::error()),
                NotificationSeverity::Warning => (warning_marker(), theme::warning()),
                NotificationSeverity::Info => ("i", theme::accent()),
            };
            let suffix = if notification.count > 1 {
                format!(" ×{}", notification.count)
            } else {
                String::new()
            };
            Message {
                marker,
                color,
                text: format!("{}: {}{suffix}", notification.source, notification.message),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_editor::EditorState;

    #[test]
    fn warning_marker_has_ascii_fallback() {
        assert_eq!(warning_marker_for(true), "!");
        assert_eq!(warning_marker_for(false), "▲");
    }

    #[test]
    fn caps_rows_and_reserves_content() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        for index in 0..5 {
            model.notify(
                NotificationSeverity::Info,
                "test",
                format!("message {index}"),
            );
        }
        assert_eq!(height(&model, 20), 3);
        assert_eq!(height(&model, 2), 1);
    }

    #[test]
    fn transient_status_messages_use_the_band() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.status.text = "E: boom".to_owned();
        assert_eq!(height(&model, 20), 1);
    }

    #[test]
    fn busy_state_remains_visible_outside_normal_mode() {
        let mut model = Model::new(EditorState::from_text("x"), None);
        model.ui.input_mode = InputMode::Insert;
        model.status.busy = Some("copy".to_owned());
        assert_eq!(height(&model, 20), 1);
    }
}
