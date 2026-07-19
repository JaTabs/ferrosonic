mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::song;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ferrosonic::app::event_pump::request_lyrics;
use ferrosonic::app::page_state::LyricsStatus;
use ferrosonic::app::{apply_event, App};
use ferrosonic::config::Config;
use ferrosonic::ipc::client::DaemonClient;
use ferrosonic::ipc::protocol::{
    DaemonEvent, DaemonRequest, DaemonResponse, IpcError, LyricsResult,
};
use ferrosonic::subsonic::models::{LyricLine, StructuredLyrics};
use tokio::sync::{broadcast, Mutex};

struct LyricsClient {
    requests: Mutex<Vec<DaemonRequest>>,
    event_tx: broadcast::Sender<DaemonEvent>,
}

impl LyricsClient {
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
impl DaemonClient for LyricsClient {
    async fn request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        self.requests.lock().await.push(req.clone());
        match req {
            DaemonRequest::GetLyrics { song_id } => {
                if song_id == "old" {
                    tokio::time::sleep(Duration::from_millis(60)).await;
                }
                Ok(DaemonResponse::Lyrics {
                    result: LyricsResult::Available(StructuredLyrics {
                        synced: true,
                        line: vec![LyricLine {
                            start: Some(0),
                            value: song_id.clone(),
                        }],
                        ..StructuredLyrics::default()
                    }),
                    song_id,
                })
            }
            _ => Ok(DaemonResponse::Ok),
        }
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

async fn wait_for_ready(app: &App) {
    for _ in 0..50 {
        if matches!(
            app.client_state.read().await.lyrics.status,
            LyricsStatus::Ready(_)
        ) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("lyrics response did not become ready");
}

#[tokio::test]
async fn y_toggles_panel_and_requests_current_song() {
    let client = LyricsClient::new();
    let mut app = App::with_remote_client(client.clone(), Config::new());
    app.daemon_state.write().await.now_playing.song = Some(song("song-1", "Song"));

    app.handle_key(key(KeyCode::Char('y'))).await.unwrap();
    wait_for_ready(&app).await;

    {
        let state = app.client_state.read().await;
        assert!(state.lyrics.open);
        assert_eq!(state.lyrics.song_id.as_deref(), Some("song-1"));
    }
    assert!(client.requests().await.iter().any(|request| matches!(
        request,
        DaemonRequest::GetLyrics { song_id } if song_id == "song-1"
    )));

    app.handle_key(key(KeyCode::Char('y'))).await.unwrap();
    assert!(!app.client_state.read().await.lyrics.open);
}

#[tokio::test]
async fn y_opens_idle_without_a_current_song() {
    let client = LyricsClient::new();
    let mut app = App::with_remote_client(client.clone(), Config::new());

    app.handle_key(key(KeyCode::Char('y'))).await.unwrap();

    let state = app.client_state.read().await;
    assert!(state.lyrics.open);
    assert!(state.lyrics.song_id.is_none());
    assert!(matches!(state.lyrics.status, LyricsStatus::Idle));
    assert!(client.requests().await.is_empty());
}

#[tokio::test]
async fn stale_lyrics_response_cannot_replace_new_song() {
    let client = LyricsClient::new();
    let app = App::with_remote_client(client.clone(), Config::new());
    app.client_state.write().await.lyrics.open = true;
    let client_dyn: Arc<dyn DaemonClient> = client;

    request_lyrics(client_dyn.clone(), app.client_state.clone(), "old".into()).await;
    request_lyrics(client_dyn, app.client_state.clone(), "new".into()).await;
    wait_for_ready(&app).await;
    tokio::time::sleep(Duration::from_millis(80)).await;

    let state = app.client_state.read().await;
    assert_eq!(state.lyrics.song_id.as_deref(), Some("new"));
    assert!(matches!(
        &state.lyrics.status,
        LyricsStatus::Ready(lyrics) if lyrics.line[0].value == "new"
    ));
}

#[tokio::test]
async fn now_playing_change_refreshes_open_panel_and_stop_clears_it() {
    let client = LyricsClient::new();
    let app = App::with_remote_client(client.clone(), Config::new());
    app.client_state.write().await.lyrics.open = true;
    let client_dyn: Arc<dyn DaemonClient> = client.clone();
    let cover_art = common::render::empty_cover_art_state();

    apply_event(
        &app.daemon_state,
        &app.client_state,
        &client_dyn,
        &cover_art,
        DaemonEvent::NowPlayingChanged(Box::new(ferrosonic::daemon::state::NowPlaying {
            song: Some(song("new", "New")),
            ..Default::default()
        })),
    )
    .await;
    wait_for_ready(&app).await;
    assert!(client.requests().await.iter().any(|request| matches!(
        request,
        DaemonRequest::GetLyrics { song_id } if song_id == "new"
    )));

    apply_event(
        &app.daemon_state,
        &app.client_state,
        &client_dyn,
        &cover_art,
        DaemonEvent::NowPlayingChanged(Box::default()),
    )
    .await;

    let state = app.client_state.read().await;
    assert!(state.lyrics.song_id.is_none());
    assert!(matches!(state.lyrics.status, LyricsStatus::Idle));
}
