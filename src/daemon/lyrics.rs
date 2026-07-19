//! OpenSubsonic lyrics capability, selection, and in-memory cache.

use crate::daemon::core::DaemonCore;
use crate::ipc::protocol::LyricsResult;
use crate::subsonic::models::StructuredLyrics;

/// Prefer synchronized lyrics, then fall back to the first server variant.
pub(crate) fn select_lyrics(items: Vec<StructuredLyrics>) -> Option<StructuredLyrics> {
    items
        .iter()
        .find(|item| item.synced)
        .cloned()
        .or_else(|| items.into_iter().next())
}

impl DaemonCore {
    /// Fetch the best lyrics variant for `song_id`, memoizing capability and result.
    pub async fn get_lyrics(&self, song_id: &str) -> LyricsResult {
        if let Some(hit) = self.lyrics_cache.read().await.get(song_id).cloned() {
            return hit;
        }
        let Some(client) = self.subsonic.read().await.clone() else {
            return LyricsResult::Unavailable;
        };
        let supported = if let Some(value) = *self.song_lyrics_supported.read().await {
            Ok(value)
        } else {
            client
                .get_open_subsonic_extensions()
                .await
                .map(|extensions| extensions.iter().any(|name| name == "songLyrics"))
        };
        if let Ok(value) = supported {
            *self.song_lyrics_supported.write().await = Some(value);
        }
        let result = match supported {
            Ok(false) => LyricsResult::Unsupported,
            Err(error) => {
                tracing::warn!("lyrics extension probe failed: {error}");
                LyricsResult::Unavailable
            }
            Ok(true) => match client.get_lyrics_by_song_id(song_id).await {
                Ok(items) => {
                    select_lyrics(items).map_or(LyricsResult::Empty, LyricsResult::Available)
                }
                Err(error) => {
                    tracing::warn!("lyrics fetch failed for {song_id}: {error}");
                    LyricsResult::Unavailable
                }
            },
        };
        self.lyrics_cache
            .write()
            .await
            .insert(song_id.to_string(), result.clone());
        result
    }
}
