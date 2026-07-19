mod common;

use std::sync::Arc;

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ferrosonic::app::client_state::ClientState;
use ferrosonic::app::state::{AppState, Page};
use ferrosonic::app::App;
use ferrosonic::config::Config;
use ferrosonic::daemon::state::DaemonState;
use ferrosonic::ipc::client::DaemonClient;
use ferrosonic::ipc::protocol::{DaemonEvent, DaemonRequest, DaemonResponse, IpcError};
use ferrosonic::subsonic::models::Playlist;
use tokio::sync::{broadcast, Mutex};

use common::{render, song};

struct QuickAddClient {
    requests: Mutex<Vec<DaemonRequest>>,
    event_tx: broadcast::Sender<DaemonEvent>,
}

impl QuickAddClient {
    fn new() -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(8);
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            event_tx,
        })
    }

    async fn requests(&self) -> Vec<DaemonRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl DaemonClient for QuickAddClient {
    async fn request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        let response = match &req {
            DaemonRequest::AddSongToPlaylist { .. } => DaemonResponse::PlaylistSongs(Vec::new()),
            DaemonRequest::CreatePlaylist { name, .. } => {
                DaemonResponse::PlaylistCreated(Playlist {
                    id: "created-1".into(),
                    name: name.clone(),
                    owner: None,
                    song_count: Some(1),
                    duration: None,
                    cover_art: None,
                    public: None,
                    comment: None,
                })
            }
            _ => DaemonResponse::Ok,
        };
        self.requests.lock().await.push(req);
        Ok(response)
    }

    fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.event_tx.subscribe()
    }
}

fn key(code: KeyCode) -> KeyEvent {
    let mut key = KeyEvent::new(code, KeyModifiers::NONE);
    key.kind = KeyEventKind::Press;
    key
}

fn playlist(id: &str, name: &str) -> Playlist {
    Playlist {
        id: id.into(),
        name: name.into(),
        owner: None,
        song_count: Some(0),
        duration: None,
        cover_art: None,
        public: None,
        comment: None,
    }
}

async fn app_with_playing_song(playlists: Vec<Playlist>) -> (App, Arc<QuickAddClient>) {
    let client = QuickAddClient::new();
    let app = App::with_remote_client(client.clone(), Config::new());
    {
        let mut daemon = app.daemon_state.write().await;
        daemon.now_playing.song = Some(song("playing", "Playing"));
        daemon.library.playlists = playlists;
    }
    (app, client)
}

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

#[tokio::test]
async fn shift_a_opens_create_prompt_when_there_are_no_playlists() {
    let (mut app, _) = app_with_playing_song(Vec::new()).await;

    app.handle_key(key(KeyCode::Char('A'))).await.unwrap();

    let client = app.client_state.read().await;
    assert!(client.create_playlist_prompt.active);
    assert_eq!(
        client
            .create_playlist_prompt
            .song
            .as_ref()
            .map(|song| song.id.as_str()),
        Some("playing")
    );
}

#[tokio::test]
async fn shift_a_adds_to_valid_last_playlist() {
    let (mut app, client) = app_with_playing_song(vec![playlist("pl-1", "Mix")]).await;
    app.daemon_state.write().await.config.last_playlist_id = Some("pl-1".into());

    app.handle_key(key(KeyCode::Char('A'))).await.unwrap();

    assert!(client.requests().await.iter().any(|request| matches!(
        request,
        DaemonRequest::AddSongToPlaylist {
            playlist_id,
            song_id
        } if playlist_id == "pl-1" && song_id == "playing"
    )));
}

#[tokio::test]
async fn shift_a_opens_picker_when_last_playlist_is_missing_or_stale() {
    for last in [None, Some("gone".to_string())] {
        let (mut app, _) = app_with_playing_song(vec![playlist("pl-1", "Mix")]).await;
        app.daemon_state.write().await.config.last_playlist_id = last;

        app.handle_key(key(KeyCode::Char('A'))).await.unwrap();

        assert!(app.client_state.read().await.playlist_picker.active);
    }
}

#[tokio::test]
async fn lowercase_a_uses_now_playing_and_opens_create_or_picker() {
    let (mut app, _) = app_with_playing_song(Vec::new()).await;
    app.handle_key(key(KeyCode::Char('a'))).await.unwrap();
    assert!(app.client_state.read().await.create_playlist_prompt.active);

    let (mut app, _) = app_with_playing_song(vec![playlist("pl-1", "Mix")]).await;
    app.handle_key(key(KeyCode::Char('a'))).await.unwrap();
    let client = app.client_state.read().await;
    assert!(client.playlist_picker.active);
    assert_eq!(
        client
            .playlist_picker
            .song
            .as_ref()
            .map(|song| song.id.as_str()),
        Some("playing")
    );
}

#[tokio::test]
async fn picker_first_row_opens_prompt_and_second_row_adds_to_first_playlist() {
    let (mut app, client) = app_with_playing_song(vec![playlist("pl-1", "Mix")]).await;
    app.handle_key(key(KeyCode::Char('a'))).await.unwrap();

    app.handle_key(key(KeyCode::Enter)).await.unwrap();
    {
        let state = app.client_state.read().await;
        assert!(!state.playlist_picker.active);
        assert!(state.create_playlist_prompt.active);
    }

    app.client_state
        .write()
        .await
        .close_create_playlist_prompt();
    app.handle_key(key(KeyCode::Char('a'))).await.unwrap();
    app.handle_key(key(KeyCode::Down)).await.unwrap();
    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    assert!(client.requests().await.iter().any(|request| matches!(
        request,
        DaemonRequest::AddSongToPlaylist {
            playlist_id,
            song_id
        } if playlist_id == "pl-1" && song_id == "playing"
    )));
}

#[tokio::test]
async fn create_prompt_sends_trimmed_name_and_closes_after_success() {
    let (mut app, client) = app_with_playing_song(Vec::new()).await;
    app.handle_key(key(KeyCode::Char('a'))).await.unwrap();
    for ch in "  Road Trip  ".chars() {
        app.handle_key(key(KeyCode::Char(ch))).await.unwrap();
    }

    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    assert!(client.requests().await.iter().any(|request| matches!(
        request,
        DaemonRequest::CreatePlaylist { name, song_ids }
            if name == "Road Trip" && song_ids == &["playing"]
    )));
    assert!(!app.client_state.read().await.create_playlist_prompt.active);
}

#[test]
fn picker_and_create_prompt_render_their_distinct_actions() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.library.playlists = vec![playlist("pl-1", "Mix")];
    let mut client = ClientState::default();
    client.open_playlist_picker(song("target", "Target"));

    let picker = render(90, 28, &daemon, &mut client);
    assert!(picker.contains("+ New playlist…"), "screen was:\n{picker}");
    assert!(picker.contains("Mix"), "screen was:\n{picker}");

    client.playlist_picker.active = false;
    client.open_create_playlist_prompt(song("target", "Target"));
    client.create_playlist_prompt.name = "Road Trip".into();

    let prompt = render(90, 28, &daemon, &mut client);
    assert!(prompt.contains("Create playlist"), "screen was:\n{prompt}");
    assert!(prompt.contains("Road Trip"), "screen was:\n{prompt}");
}

#[test]
fn footer_documents_quick_and_chosen_playlist_actions() {
    let daemon = DaemonState::new(Config::default());
    let mut client = ClientState::default();
    client.page = Page::Library;

    let screen = render(200, 28, &daemon, &mut client);

    assert!(screen.contains("A:Quick playlist"), "screen was:\n{screen}");
    assert!(
        screen.contains("a:Choose playlist"),
        "screen was:\n{screen}"
    );
}
