mod common;

use ferrosonic::app::client_state::ClientState;
use ferrosonic::app::state::{AppState, Page};
use ferrosonic::config::Config;
use ferrosonic::daemon::state::DaemonState;

use common::song;

fn target_id(daemon: &DaemonState, client: &mut ClientState) -> Option<String> {
    AppState { daemon, client }
        .target_song()
        .map(|song| song.id.clone())
}

#[test]
fn library_uses_focused_song_then_falls_back_to_now_playing() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.now_playing.song = Some(song("playing", "Playing"));
    let mut client = ClientState::default();
    client.page = Page::Library;
    client.artists.songs = vec![song("selected", "Selected")];
    client.artists.selected_song = Some(0);
    client.artists.focus = 1;

    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("selected"));

    client.artists.focus = 0;
    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("playing"));
}

#[test]
fn queue_uses_selected_song_then_falls_back_to_now_playing() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.now_playing.song = Some(song("playing", "Playing"));
    daemon.queue = vec![song("queued", "Queued")];
    let mut client = ClientState::default();
    client.page = Page::Queue;
    client.queue_state.selected = Some(0);

    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("queued"));

    client.queue_state.selected = Some(99);
    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("playing"));
}

#[test]
fn quick_play_uses_focused_song_then_falls_back_to_now_playing() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.now_playing.song = Some(song("playing", "Playing"));
    daemon.library.starred_songs = vec![song("starred", "Starred")];
    let mut client = ClientState::default();
    client.page = Page::QuickPlay;
    client.songs.selected_index = Some(0);
    client.songs.focus = 1;

    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("starred"));

    client.songs.focus = 0;
    assert_eq!(target_id(&daemon, &mut client).as_deref(), Some("playing"));
}

#[test]
fn playlists_uses_focused_song_then_returns_none_without_fallback() {
    let daemon = DaemonState::new(Config::default());
    let mut client = ClientState::default();
    client.page = Page::Playlists;
    client.playlists.songs = vec![song("playlist-song", "Playlist Song")];
    client.playlists.selected_song = Some(0);
    client.playlists.focus = 1;

    assert_eq!(
        target_id(&daemon, &mut client).as_deref(),
        Some("playlist-song")
    );

    client.playlists.focus = 0;
    assert_eq!(target_id(&daemon, &mut client), None);
}

#[test]
fn create_playlist_prompt_owns_song_and_clears_on_close() {
    let mut client = ClientState::default();

    client.open_create_playlist_prompt(song("target", "Target"));

    assert!(client.create_playlist_prompt.active);
    assert!(client.create_playlist_prompt.name.is_empty());
    assert_eq!(
        client
            .create_playlist_prompt
            .song
            .as_ref()
            .map(|song| song.id.as_str()),
        Some("target")
    );

    client.create_playlist_prompt.name = "Road Trip".into();
    client.close_create_playlist_prompt();

    assert!(!client.create_playlist_prompt.active);
    assert!(client.create_playlist_prompt.name.is_empty());
    assert!(client.create_playlist_prompt.song.is_none());
}
