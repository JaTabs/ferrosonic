//! Modal text input for creating a playlist around one target song.

use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::state::AppState;
use crate::ui::theme::ThemeColors;

/// Draw the create-playlist name prompt over the frame.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState<'_>, colors: &ThemeColors) {
    let width = 60.min(area.width);
    let height = 3.min(area.height);
    if width < 10 || height < 3 {
        return;
    }
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let input = Paragraph::new(format!(
        "{}\u{2588}",
        state.client.create_playlist_prompt.name
    ))
    .style(Style::default().fg(colors.primary))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors.accent))
            .title(" Create playlist  (Enter: create  Esc: cancel) "),
    );

    frame.render_widget(Clear, rect);
    frame.render_widget(input, rect);
}
