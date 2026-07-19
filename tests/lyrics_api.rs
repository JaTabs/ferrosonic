mod common;

use common::{FakeSubsonic, TestDaemon};
use ferrosonic::ipc::protocol::LyricsResult;
use ferrosonic::subsonic::client::SubsonicClient;
use serde_json::json;
use serial_test::serial;

async fn build_client(fake: &FakeSubsonic) -> SubsonicClient {
    SubsonicClient::new(&fake.url(), "test", &"test".into()).unwrap()
}

#[tokio::test]
async fn get_lyrics_parses_synced_lines_and_metadata() {
    let fake = FakeSubsonic::start().await;
    fake.expect_lyrics(
        "song-1",
        json!({
            "structuredLyrics": [{
                "displayArtist": "Artist",
                "displayTitle": "Song",
                "lang": "en",
                "offset": 125,
                "synced": true,
                "line": [
                    { "start": 1000, "value": "First" },
                    { "start": 2500, "value": "Second" }
                ]
            }]
        }),
    )
    .await;

    let lyrics = build_client(&fake)
        .await
        .get_lyrics_by_song_id("song-1")
        .await
        .unwrap();

    assert_eq!(lyrics.len(), 1);
    assert!(lyrics[0].synced);
    assert_eq!(lyrics[0].offset, 125);
    assert_eq!(lyrics[0].lang.as_deref(), Some("en"));
    assert_eq!(lyrics[0].line[0].start, Some(1000));
    assert_eq!(lyrics[0].line[1].value, "Second");
}

#[tokio::test]
async fn get_lyrics_accepts_unsynced_lines_without_start() {
    let fake = FakeSubsonic::start().await;
    fake.expect_lyrics(
        "song-2",
        json!({
            "structuredLyrics": [{
                "synced": false,
                "line": [
                    { "value": "Plain first line" },
                    { "value": "Plain second line" }
                ]
            }]
        }),
    )
    .await;

    let lyrics = build_client(&fake)
        .await
        .get_lyrics_by_song_id("song-2")
        .await
        .unwrap();

    assert!(!lyrics[0].synced);
    assert_eq!(lyrics[0].line[0].start, None);
    assert_eq!(lyrics[0].line[0].value, "Plain first line");
}

#[tokio::test]
#[serial]
async fn daemon_lyrics_prefers_synced_and_caches_by_song() {
    let td = TestDaemon::new().await;
    td.fake_subsonic
        .expect_open_subsonic_extensions(&["songLyrics"])
        .await;
    td.fake_subsonic
        .expect_lyrics(
            "song-1",
            json!({
                "structuredLyrics": [
                    { "synced": false, "line": [{ "value": "Plain" }] },
                    { "synced": true, "line": [{ "start": 1000, "value": "Timed" }] }
                ]
            }),
        )
        .await;

    let first = td.core.get_lyrics("song-1").await;
    let second = td.core.get_lyrics("song-1").await;

    assert!(matches!(
        first,
        LyricsResult::Available(ref lyrics) if lyrics.synced && lyrics.line[0].value == "Timed"
    ));
    assert_eq!(first, second);
    let requests = td.fake_subsonic.received_requests().await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.url.path() == "/rest/getLyricsBySongId")
            .count(),
        1
    );
}

#[tokio::test]
#[serial]
async fn daemon_lyrics_skips_fetch_when_extension_is_unsupported() {
    let td = TestDaemon::new().await;
    td.fake_subsonic
        .expect_open_subsonic_extensions(&["playbackReport"])
        .await;

    assert_eq!(
        td.core.get_lyrics("song-1").await,
        LyricsResult::Unsupported
    );
    assert!(td
        .fake_subsonic
        .received_requests()
        .await
        .iter()
        .all(|request| request.url.path() != "/rest/getLyricsBySongId"));
}
