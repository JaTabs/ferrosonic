//! Lyrics panel with synchronized-line highlighting and automatic centering.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::state::{LyricsState, LyricsStatus};
use crate::subsonic::models::LyricLine;
use crate::ui::theme::ThemeColors;

/// Last line whose effective timestamp (`start + offset`) has been reached.
#[must_use]
pub fn current_line_index(lines: &[LyricLine], position_ms: u64, offset_ms: i64) -> Option<usize> {
    lines.iter().enumerate().rev().find_map(|(index, line)| {
        let start = line.start?;
        let effective = i128::from(start) + i128::from(offset_ms);
        let effective = u128::try_from(effective.max(0)).unwrap_or(0);
        (effective <= u128::from(position_ms)).then_some(index)
    })
}

/// Bordered lyrics pane driven entirely by TUI-local state.
pub struct LyricsWidget<'a> {
    state: &'a LyricsState,
    position_seconds: f64,
    colors: ThemeColors,
}

impl<'a> LyricsWidget<'a> {
    /// Build a lyrics widget for the current playback position.
    #[must_use]
    pub const fn new(state: &'a LyricsState, position_seconds: f64, colors: ThemeColors) -> Self {
        Self {
            state,
            position_seconds,
            colors,
        }
    }

    fn position_ms(&self) -> u64 {
        if !self.position_seconds.is_finite() || self.position_seconds <= 0.0 {
            return 0;
        }
        std::time::Duration::from_secs_f64(self.position_seconds)
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

impl Widget for LyricsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.colors.border_unfocused))
            .title(" Lyrics (y: close) ");
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.is_empty() {
            return;
        }

        let message = match &self.state.status {
            LyricsStatus::Idle => Some("Play a song to view lyrics"),
            LyricsStatus::Loading => Some("Loading lyrics…"),
            LyricsStatus::Empty => Some("No lyrics"),
            LyricsStatus::Unsupported => Some("Lyrics are not supported by this server"),
            LyricsStatus::Unavailable => Some("Lyrics unavailable"),
            LyricsStatus::Ready(_) => None,
        };
        if let Some(message) = message {
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::default().fg(self.colors.muted))
                .render(inner, buf);
            return;
        }

        let LyricsStatus::Ready(lyrics) = &self.state.status else {
            return;
        };
        let current = lyrics
            .synced
            .then(|| current_line_index(&lyrics.line, self.position_ms(), lyrics.offset))
            .flatten();
        let visible_rows = usize::from(inner.height);
        let start = current.map_or(0, |index| index.saturating_sub(visible_rows / 2));
        let lines = lyrics
            .line
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(index, lyric)| {
                let style = if Some(index) == current {
                    Style::default()
                        .fg(self.colors.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(self.colors.primary)
                };
                Line::styled(lyric.value.clone(), style)
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buf);
    }
}
