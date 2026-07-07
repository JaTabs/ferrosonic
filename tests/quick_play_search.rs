//! Quick-play search: Enter in the library filter plays the best-matching
//! song immediately; Tab keeps the old confirm-and-browse behaviour; clicking
//! the tree title bar opens the search box.

mod common;

use common::TestDaemon;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ferrosonic::app::App;
use ferrosonic::config::Config;
use ferrosonic::subsonic::models::SearchResult3;
use ratatui::layout::Rect;
use serde_json::Value;
use serial_test::serial;

fn key(code: KeyCode) -> KeyEvent {
    let mut k = KeyEvent::new(code, KeyModifiers::NONE);
    k.kind = KeyEventKind::Press;
    k
}

fn click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

async fn build_app_with_td() -> (App, TestDaemon) {
    let td = TestDaemon::new().await;
    let cfg = td.state.read().await.config.clone();
    let app = App::with_remote_client(
        std::sync::Arc::new(ferrosonic::ipc::InProcessClient::new(td.core.clone())),
        cfg,
    );
    {
        let mut cs = app.client_state.write().await;
        cs.page = ferrosonic::app::state::Page::Library;
        cs.artists.filter_active = true;
    }
    (app, td)
}

async fn type_filter(app: &App, text: &str) {
    let mut cs = app.client_state.write().await;
    cs.artists.filter = text.into();
}

#[tokio::test]
#[serial]
async fn enter_with_exact_title_match_plays_that_song() {
    let (mut app, td) = build_app_with_td().await;
    td.fake_subsonic.expect_ping().await;
    // The exact match is listed second: exactness must beat server order.
    td.fake_subsonic
        .expect_search3(&[], &[], &["Lullaby (Live)", "Lullaby"])
        .await;
    type_filter(&app, "lullaby").await;

    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    let saw_loadfile = td
        .fake_mpv
        .wait_for(2000, |cmds| {
            cmds.iter()
                .any(|c| c.first().and_then(Value::as_str) == Some("loadfile"))
        })
        .await;
    assert!(saw_loadfile, "Enter on a matched song must start playback");

    let ds = td.state.read().await;
    assert_eq!(ds.queue.len(), 1, "queue must hold just the picked song");
    assert_eq!(ds.queue[0].title, "Lullaby");

    let cs = app.client_state.read().await;
    assert!(!cs.artists.filter_active, "search box must close");
    assert!(cs.artists.filter.is_empty(), "filter must reset");
    assert!(cs.artists.search_results.is_none(), "results must clear");
}

#[tokio::test]
#[serial]
async fn enter_without_exact_match_plays_first_title_hit() {
    let (mut app, td) = build_app_with_td().await;
    td.fake_subsonic.expect_ping().await;
    // First song matches only via artist/album; second is the first title hit.
    td.fake_subsonic
        .expect_search3(&[], &[], &["Something Else", "Blue Lullaby Song"])
        .await;
    type_filter(&app, "lullaby").await;

    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    let ds = td.state.read().await;
    assert_eq!(ds.queue.len(), 1);
    assert_eq!(ds.queue[0].title, "Blue Lullaby Song");
}

#[tokio::test]
#[serial]
async fn enter_with_no_title_match_falls_back_to_browsing() {
    let (mut app, td) = build_app_with_td().await;
    // Artist matched, songs matched only via artist name: nothing should play.
    td.fake_subsonic
        .expect_search3(&["The Cure", "Boys Don't Cry", "Lovesong"], &[], &[])
        .await;
    type_filter(&app, "cure").await;

    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    let ds = td.state.read().await;
    assert!(ds.queue.is_empty(), "no title match must not enqueue");

    let cs = app.client_state.read().await;
    assert!(!cs.artists.filter_active, "input closes as before");
    assert_eq!(cs.artists.filter, "cure", "filter stays for browsing");
    let results = cs.artists.search_results.as_ref();
    assert!(
        results.is_some_and(|r| r.artist.len() == 3),
        "fresh results must be kept for arrow-key browsing"
    );
}

#[tokio::test]
#[serial]
async fn enter_with_empty_filter_just_closes_input() {
    let (mut app, td) = build_app_with_td().await;

    app.handle_key(key(KeyCode::Enter)).await.unwrap();

    let ds = td.state.read().await;
    assert!(ds.queue.is_empty());
    let cs = app.client_state.read().await;
    assert!(!cs.artists.filter_active);
}

#[tokio::test]
#[serial]
async fn tab_closes_input_and_keeps_results_for_browsing() {
    let (mut app, _td) = build_app_with_td().await;
    {
        let mut cs = app.client_state.write().await;
        cs.artists.filter = "cure".into();
        cs.artists.search_results = Some(SearchResult3 {
            artist: vec![],
            album: vec![],
            song: vec![],
        });
    }

    app.handle_key(key(KeyCode::Tab)).await.unwrap();

    let cs = app.client_state.read().await;
    assert!(!cs.artists.filter_active, "Tab must close the input");
    assert_eq!(cs.artists.filter, "cure", "filter must survive Tab");
    assert!(
        cs.artists.search_results.is_some(),
        "results must survive Tab"
    );
}

struct AppFixture {
    app: App,
    _tempdir: tempfile::TempDir,
}

async fn build_mouse_app() -> AppFixture {
    let tempdir = common::tempdir();
    std::env::set_var("FERROSONIC_CONFIG_DIR", tempdir.path());
    let mut config = Config::new();
    config.daemon = false;
    let app = App::new(config);
    {
        let mut cs = app.client_state.write().await;
        cs.page = ferrosonic::app::state::Page::Library;
        cs.layout.header = Rect::new(0, 0, 80, 1);
        cs.layout.content = Rect::new(0, 1, 80, 20);
        cs.layout.content_left = Some(Rect::new(0, 1, 40, 20));
        cs.layout.content_right = Some(Rect::new(40, 1, 40, 20));
        cs.layout.now_playing = Rect::new(0, 21, 80, 7);
    }
    AppFixture {
        app,
        _tempdir: tempdir,
    }
}

#[tokio::test]
#[serial]
async fn click_on_tree_title_bar_opens_search() {
    let mut fx = build_mouse_app().await;

    fx.app.handle_mouse(click(10, 1)).await.unwrap();

    let cs = fx.app.client_state.read().await;
    assert!(
        cs.artists.filter_active,
        "clicking the tree title bar must open the search box"
    );
}

#[tokio::test]
#[serial]
async fn click_below_title_bar_still_selects_rows() {
    let mut fx = build_mouse_app().await;
    {
        let mut ds = fx.app.daemon_state.write().await;
        ds.library.artists = vec![ferrosonic::subsonic::models::Artist {
            id: "a0".into(),
            name: "Alpha".into(),
            album_count: Some(1),
            cover_art: None,
        }];
    }

    fx.app.handle_mouse(click(10, 2)).await.unwrap();

    let cs = fx.app.client_state.read().await;
    assert!(
        !cs.artists.filter_active,
        "row clicks must not open the search box"
    );
    assert_eq!(cs.artists.selected_index, Some(0));
}
