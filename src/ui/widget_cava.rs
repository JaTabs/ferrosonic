//! Cava audio visualizer widget — renders captured noncurses output

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::app::state::{CavaColor, CavaRow};

/// Widget painting the parsed cava frame into the buffer.
pub struct CavaWidget<'a> {
    screen: &'a [CavaRow],
}

impl<'a> CavaWidget<'a> {
    /// Widget over the latest parsed cava rows.
    #[must_use]
    pub const fn new(screen: &'a [CavaRow]) -> Self {
        Self { screen }
    }
}

const fn cava_color_to_ratatui(c: CavaColor) -> Option<Color> {
    match c {
        CavaColor::Default => None,
        CavaColor::Indexed(i) => Some(Color::Indexed(i)),
        CavaColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

impl Widget for CavaWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.screen.is_empty() {
            return;
        }

        // Bars grow from the bottom of cava's own screen, so a frame that
        // doesn't match the band (startup estimate, pending resize) must
        // stay bottom-aligned: drop the topmost rows when it is taller,
        // pad the top when it is shorter. Never pin it to the top-left.
        let area_h = area.height as usize;
        let skip = self.screen.len().saturating_sub(area_h);
        let pad = area_h.saturating_sub(self.screen.len());

        for (row_idx, cava_row) in self.screen.iter().skip(skip).enumerate() {
            let y = area.y + crate::num::u16_sat(pad + row_idx);
            let mut x = area.x;

            for span in &cava_row.spans {
                for ch in span.text.chars() {
                    if x >= area.x + area.width {
                        break;
                    }
                    let mut style = Style::default();
                    if let Some(fg) = cava_color_to_ratatui(span.fg) {
                        style = style.fg(fg);
                    }
                    if let Some(bg) = cava_color_to_ratatui(span.bg) {
                        style = style.bg(bg);
                    }
                    buf[(x, y)].set_char(ch).set_style(style);
                    x += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CavaWidget, Rect};
    use crate::app::state::{CavaColor, CavaRow, CavaSpan};
    use ratatui::{buffer::Buffer, widgets::Widget};

    fn row(text: &str) -> CavaRow {
        CavaRow {
            spans: vec![CavaSpan {
                text: text.to_string(),
                fg: CavaColor::Default,
                bg: CavaColor::Default,
            }],
        }
    }

    fn render(screen: &[CavaRow], w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        CavaWidget::new(screen).render(area, &mut buf);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn shorter_frame_sticks_to_the_bottom_of_the_band() {
        let lines = render(&[row("ab"), row("cd")], 2, 4);
        assert_eq!(lines, vec!["  ", "  ", "ab", "cd"]);
    }

    #[test]
    fn taller_frame_drops_its_top_rows() {
        let lines = render(&[row("ab"), row("cd"), row("ef")], 2, 2);
        assert_eq!(lines, vec!["cd", "ef"]);
    }

    #[test]
    fn exact_fit_is_unchanged() {
        let lines = render(&[row("ab"), row("cd")], 2, 2);
        assert_eq!(lines, vec!["ab", "cd"]);
    }
}
