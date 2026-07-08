//! Volume control end-to-end: the daemon setter, config persistence,
//! mpv re-apply on (re)connect, MPRIS exposure, the now-playing volume
//! slider, and its key/mouse bindings.

mod common;

use common::{FakeMpv, RecordingClient, TestDaemon};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ferrosonic::app::client_state::ClientState;
use ferrosonic::app::App;
use ferrosonic::audio::mpv::MpvController;
use ferrosonic::config::Config;
use ferrosonic::daemon::DaemonState;
use ferrosonic::ipc::protocol::{DaemonEvent, DaemonRequest};
use ferrosonic::ui::widget_now_playing::progress_row_layout;
use ratatui::layout::Rect;
use serde_json::Value;
use serial_test::serial;

fn is_volume_set(cmd: &[Value], vol: f64) -> bool {
    cmd.first().and_then(Value::as_str) == Some("set_property")
        && cmd.get(1).and_then(Value::as_str) == Some("volume")
        && cmd.get(2).and_then(Value::as_f64) == Some(vol)
}

// ---------------------------------------------------------------- config

#[test]
#[serial]
fn config_volume_defaults_to_100_and_roundtrips() {
    let dir = common::tempdir();
    let cfg = Config::new();
    assert_eq!(cfg.volume, 100, "fresh config starts at full volume");

    let path = dir.path().join("config.toml");
    let mut cfg = Config::new();
    cfg.volume = 55;
    cfg.save_to_file(&path).unwrap();
    let loaded = Config::load_from_file(&path).unwrap();
    assert_eq!(loaded.volume, 55, "volume must persist across save/load");
}

// ---------------------------------------------------------------- daemon op

#[tokio::test]
#[serial]
async fn set_volume_persists_applies_and_broadcasts() {
    let td = TestDaemon::new().await;
    let mut rx = td.core.subscribe();

    td.core.set_volume(65).await.unwrap();

    assert_eq!(td.state.read().await.config.volume, 65);
    assert!(
        td.fake_mpv
            .wait_for(1000, |cmds| cmds.iter().any(|c| is_volume_set(c, 65.0)))
            .await,
        "set_volume must reach mpv"
    );

    let cfg = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            match rx.recv().await.expect("event channel open") {
                DaemonEvent::ConfigChanged(cfg) => break cfg,
                _ => continue,
            }
        }
    })
    .await
    .expect("ConfigChanged broadcast within 500ms");
    assert_eq!(cfg.volume, 65, "broadcast config carries the new volume");
}

#[tokio::test]
#[serial]
async fn set_volume_clamps_to_percentage_range() {
    let td = TestDaemon::new().await;
    td.core.set_volume(150).await.unwrap();
    assert_eq!(td.state.read().await.config.volume, 100);
    td.core.set_volume(-3).await.unwrap();
    assert_eq!(td.state.read().await.config.volume, 0);
}

// ------------------------------------------------------- mpv (re)connect

#[tokio::test]
#[serial]
async fn volume_reapplied_on_reconnect() {
    let fake = FakeMpv::start().await;
    let mut ctrl = MpvController::with_socket_path(fake.socket_path.clone());
    ctrl.connect_to_existing().await.unwrap();

    ctrl.set_volume(55).await.unwrap();

    // Model the daemon reconnecting after an mpv crash/respawn: the
    // controller must push the remembered volume again by itself.
    ctrl.connect_to_existing().await.unwrap();

    assert!(
        fake.wait_for(1000, |cmds| {
            cmds.iter().filter(|c| is_volume_set(c, 55.0)).count() >= 2
        })
        .await,
        "volume must be re-applied after a reconnect"
    );
}

#[tokio::test]
#[serial]
async fn apply_config_volume_pushes_persisted_volume_to_mpv() {
    let td = TestDaemon::new().await;
    td.state.write().await.config.volume = 70;

    td.core.apply_config_volume().await;

    assert!(
        td.fake_mpv
            .wait_for(1000, |cmds| cmds.iter().any(|c| is_volume_set(c, 70.0)))
            .await,
        "boot-time apply must push the persisted volume"
    );
}

// ---------------------------------------------------------------- MPRIS

#[tokio::test]
#[serial]
async fn mpris_property_snapshot_carries_volume() {
    let td = TestDaemon::new().await;
    td.state.write().await.config.volume = 40;
    let snap = ferrosonic::mpris::server::build_property_snapshot(&td.state).await;
    assert!(
        (snap.volume - 0.4).abs() < 1e-9,
        "MPRIS volume is the 0.0..=1.0 mirror of config, got {}",
        snap.volume
    );
}

// ---------------------------------------------------------------- render

fn playing_state(volume: u8) -> DaemonState {
    let mut config = Config::new();
    config.volume = volume;
    let mut daemon = DaemonState::new(config);
    daemon.now_playing.song = Some(common::song("s1", "Song One"));
    daemon.now_playing.duration = 240.0;
    daemon.now_playing.position = 60.0;
    daemon
}

#[test]
#[serial]
fn volume_slider_renders_next_to_progress_bar() {
    let daemon = playing_state(65);
    let mut client = ClientState::default();
    let text = common::render(90, 30, &daemon, &mut client);
    let row = text
        .lines()
        .find(|l| l.contains("01:00 / 04:00"))
        .expect("progress row rendered");
    assert!(row.contains('♪'), "volume icon on progress row: {row}");
    assert!(row.contains("65%"), "volume percentage shown: {row}");
}

#[test]
#[serial]
fn volume_slider_hidden_on_narrow_terminal() {
    let daemon = playing_state(65);
    let mut client = ClientState::default();
    let text = common::render(40, 30, &daemon, &mut client);
    assert!(
        !text.contains('♪'),
        "narrow terminals keep the full-width progress bar"
    );
}

// ---------------------------------------------------------------- keys

struct AppFixture {
    app: App,
    client: std::sync::Arc<RecordingClient>,
    _tempdir: tempfile::TempDir,
}

async fn build_app() -> AppFixture {
    let tempdir = common::tempdir();
    std::env::set_var("FERROSONIC_CONFIG_DIR", tempdir.path());
    let client = RecordingClient::new();
    let config = Config::new();
    let app = App::with_remote_client(client.clone(), config);
    {
        let mut cs = app.client_state.write().await;
        cs.layout.header = Rect::new(0, 0, 80, 1);
        cs.layout.content = Rect::new(0, 1, 80, 20);
        cs.layout.now_playing = Rect::new(0, 21, 80, 7);
    }
    AppFixture {
        app,
        client,
        _tempdir: tempdir,
    }
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

async fn volume_requests(client: &RecordingClient) -> Vec<i32> {
    client
        .requests()
        .await
        .into_iter()
        .filter_map(|r| match r {
            DaemonRequest::SetVolume(v) => Some(v),
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn minus_key_steps_volume_down_and_plus_is_capped() {
    let mut fx = build_app().await;
    fx.app.handle_event(key(KeyCode::Char('-'))).await.unwrap();
    // Already at the ceiling (100): '+' must not send a redundant request.
    fx.app.handle_event(key(KeyCode::Char('+'))).await.unwrap();
    assert_eq!(volume_requests(&fx.client).await, vec![95]);
}

#[tokio::test]
#[serial]
async fn plus_and_equals_keys_step_volume_up() {
    let mut fx = build_app().await;
    fx.app.daemon_state.write().await.config.volume = 50;
    fx.app.handle_event(key(KeyCode::Char('+'))).await.unwrap();
    fx.app.handle_event(key(KeyCode::Char('='))).await.unwrap();
    // The daemon mirror stays at 50 (RecordingClient echoes nothing), so
    // both presses compute 50 + 5.
    assert_eq!(volume_requests(&fx.client).await, vec![55, 55]);
}

// ---------------------------------------------------------------- mouse

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

/// Layout is seeded as now_playing = (0, 21, 80, 7): inner width 78,
/// progress row at y = 26, inner x origin 1.
async fn seeded_mouse_app() -> AppFixture {
    let fx = build_app().await;
    {
        let mut ds = fx.app.daemon_state.write().await;
        ds.now_playing.song = Some(common::song("s1", "Song One"));
        ds.now_playing.duration = 240.0;
        ds.now_playing.position = 60.0;
    }
    fx
}

fn volume_bar_x() -> (u16, u16) {
    // "01:00 / 04:00" is 13 columns wide.
    let row = progress_row_layout(78, 13).expect("row wide enough");
    let start = row.vol_bar_start.expect("volume visible at width 78");
    (1 + start, row.vol_bar_width)
}

#[tokio::test]
#[serial]
async fn click_on_volume_bar_ends_sets_volume() {
    let mut fx = seeded_mouse_app().await;
    let (x0, w) = volume_bar_x();
    let click = |x| mouse(MouseEventKind::Down(MouseButton::Left), x, 26);
    fx.app.handle_mouse(click(x0)).await.unwrap();
    fx.app.handle_mouse(click(x0 + w - 1)).await.unwrap();
    assert_eq!(volume_requests(&fx.client).await, vec![0, 100]);
}

#[tokio::test]
#[serial]
async fn drag_on_volume_bar_sets_volume() {
    let mut fx = seeded_mouse_app().await;
    let (x0, w) = volume_bar_x();
    let mid = x0 + w / 2;
    fx.app
        .handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), mid, 26))
        .await
        .unwrap();
    let reqs = volume_requests(&fx.client).await;
    assert_eq!(reqs.len(), 1, "drag over the bar sends one update");
    assert!(
        (40..=60).contains(&reqs[0]),
        "mid-bar drag lands near 50%, got {}",
        reqs[0]
    );
}

#[tokio::test]
#[serial]
async fn drag_outside_volume_bar_is_ignored() {
    let mut fx = seeded_mouse_app().await;
    // Progress-bar cell: dragging there must not seek or set volume.
    fx.app
        .handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 20, 26))
        .await
        .unwrap();
    assert!(fx.client.requests().await.is_empty());
}

#[tokio::test]
#[serial]
async fn scroll_over_now_playing_steps_volume() {
    let mut fx = seeded_mouse_app().await;
    fx.app.daemon_state.write().await.config.volume = 50;
    fx.app
        .handle_mouse(mouse(MouseEventKind::ScrollUp, 40, 23))
        .await
        .unwrap();
    fx.app
        .handle_mouse(mouse(MouseEventKind::ScrollDown, 40, 23))
        .await
        .unwrap();
    assert_eq!(volume_requests(&fx.client).await, vec![55, 45]);
}

#[tokio::test]
#[serial]
async fn scroll_over_content_does_not_touch_volume() {
    let mut fx = seeded_mouse_app().await;
    fx.app
        .handle_mouse(mouse(MouseEventKind::ScrollUp, 40, 10))
        .await
        .unwrap();
    assert!(volume_requests(&fx.client).await.is_empty());
}

// ---------------------------------------------------------------- seek via shared geometry

#[tokio::test]
#[serial]
async fn click_on_progress_bar_still_seeks() {
    let mut fx = seeded_mouse_app().await;
    let row = progress_row_layout(78, 13).expect("layout");
    let bar_mid = 1 + row.bar_start + row.bar_width / 2;
    fx.app
        .handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), bar_mid, 26))
        .await
        .unwrap();
    let seeks: Vec<f64> = fx
        .client
        .requests()
        .await
        .into_iter()
        .filter_map(|r| match r {
            DaemonRequest::Seek(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(seeks.len(), 1, "one seek per click");
    assert!(
        (100.0..=140.0).contains(&seeks[0]),
        "mid-bar click seeks near half of 240s, got {}",
        seeks[0]
    );
}
