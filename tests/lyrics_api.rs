mod common;

use common::FakeSubsonic;
use ferrosonic::subsonic::client::SubsonicClient;
use serde_json::json;

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
