mod common;

use common::{render, render_styled};
use ferrosonic::app::client_state::ClientState;
use ferrosonic::app::page_state::LyricsStatus;
use ferrosonic::config::Config;
use ferrosonic::daemon::DaemonState;
use ferrosonic::subsonic::models::{LyricLine, StructuredLyrics};
use ferrosonic::ui::widget_lyrics::current_line_index;
use ratatui::style::Modifier;

fn lines() -> Vec<LyricLine> {
    vec![
        LyricLine {
            start: Some(1000),
            value: "First".into(),
        },
        LyricLine {
            start: Some(2500),
            value: "Second".into(),
        },
    ]
}

fn open_client(status: LyricsStatus) -> ClientState {
    let mut client = ClientState::default();
    client.lyrics.open = true;
    client.lyrics.status = status;
    client
}

#[test]
fn current_line_uses_start_plus_signed_offset() {
    let lines = lines();
    assert_eq!(current_line_index(&lines, 500, 0), None);
    assert_eq!(current_line_index(&lines, 1000, 0), Some(0));
    assert_eq!(current_line_index(&lines, 2600, 0), Some(1));
    assert_eq!(current_line_index(&lines, 900, -200), Some(0));
}

#[test]
fn lyrics_panel_renders_each_non_ready_state() {
    let daemon = DaemonState::new(Config::default());
    for (status, expected) in [
        (LyricsStatus::Idle, "Play a song to view lyrics"),
        (LyricsStatus::Loading, "Loading lyrics…"),
        (LyricsStatus::Empty, "No lyrics"),
        (
            LyricsStatus::Unsupported,
            "Lyrics are not supported by this server",
        ),
        (LyricsStatus::Unavailable, "Lyrics unavailable"),
    ] {
        let mut client = open_client(status);
        let screen = render(100, 36, &daemon, &mut client);
        assert!(screen.contains(expected), "missing {expected:?}:\n{screen}");
    }
}

#[test]
fn synced_lyrics_center_and_highlight_current_line() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.now_playing.position = 2.6;
    let lyrics = StructuredLyrics {
        synced: true,
        line: lines(),
        ..StructuredLyrics::default()
    };
    let mut client = open_client(LyricsStatus::Ready(lyrics));
    let accent = client.settings_state.theme_colors().accent;

    let screen = render_styled(100, 36, &daemon, &mut client);
    let rows = screen.rows_with("Second");

    assert_eq!(rows.len(), 1, "screen was:\n{}", screen.text());
    assert!(screen.row_has_fg(rows[0], accent));
    assert!(screen.row_has_modifier_in(rows[0], 0, screen.width(), Modifier::BOLD));
}

#[test]
fn short_terminal_keeps_footer_and_now_playing_visible() {
    let daemon = DaemonState::new(Config::default());
    let mut client = open_client(LyricsStatus::Ready(StructuredLyrics {
        line: lines(),
        ..StructuredLyrics::default()
    }));

    let screen = render(80, 20, &daemon, &mut client);

    assert!(screen.contains("Now Playing"), "screen was:\n{screen}");
    assert!(screen.contains("q:Quit"), "screen was:\n{screen}");
}
