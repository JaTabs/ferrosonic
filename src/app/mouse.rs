use crossterm::event::{self, MouseButton, MouseEventKind};

use crate::error::Error;

use super::{App, AppState, DaemonRequest, EnqueueMode, LayoutAreas, Page};

impl App {
    /// Route one mouse event to header buttons or the active page.
    ///
    /// # Errors
    /// Returns an `Error` if the daemon request fails.
    pub async fn handle_mouse(&mut self, mouse: event::MouseEvent) -> Result<(), Error> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.handle_mouse_click(x, y).await,
            // Dragging only drives the volume slider; dragging across the
            // progress bar must not spam seeks.
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(vol) = self.volume_slider_hit(x, y).await {
                    return self
                        .client
                        .request(DaemonRequest::SetVolume(vol))
                        .await
                        .map(|_| ())
                        .map_err(Error::from);
                }
                Ok(())
            }
            MouseEventKind::ScrollUp => {
                if self.over_now_playing(y).await {
                    return self.step_volume(5).await;
                }
                self.handle_mouse_scroll_up().await
            }
            MouseEventKind::ScrollDown => {
                if self.over_now_playing(y).await {
                    return self.step_volume(-5).await;
                }
                self.handle_mouse_scroll_down().await
            }
            _ => Ok(()),
        }
    }

    /// Target volume for a pointer at `(x, y)`, when it lands on the
    /// volume bar of the now-playing progress row.
    async fn volume_slider_hit(&self, x: u16, y: u16) -> Option<i32> {
        let (area, time_width) = {
            let ds = self.daemon_state.read().await;
            let cs = self.client_state.read().await;
            let np = &ds.now_playing;
            let time = format!("{} / {}", np.format_position(), np.format_duration());
            (cs.layout.now_playing, crate::num::u16_sat(time.len()))
        };
        if area.height < 2 || y != area.y + area.height - 2 {
            return None;
        }
        let row = crate::ui::widget_now_playing::progress_row_layout(
            area.width.saturating_sub(2),
            time_width,
        )?;
        let vol_start = row.vol_bar_start?;
        let rel_x = x.checked_sub(area.x + 1)?;
        if rel_x < vol_start || rel_x >= vol_start + row.vol_bar_width {
            return None;
        }
        let fraction = f64::from(rel_x - vol_start) / f64::from(row.vol_bar_width - 1);
        // f64->i32 `as` saturates; fraction is 0.0..=1.0.
        #[allow(clippy::cast_possible_truncation)]
        Some((fraction * 100.0).round() as i32)
    }

    async fn over_now_playing(&self, y: u16) -> bool {
        let area = self.client_state.read().await.layout.now_playing;
        area.height > 0 && y >= area.y && y < area.y + area.height
    }

    async fn step_volume(&self, delta: i32) -> Result<(), Error> {
        let cur = i32::from(self.daemon_state.read().await.config.volume);
        let target = (cur + delta).clamp(0, 100);
        if target == cur {
            return Ok(());
        }
        self.client
            .request(DaemonRequest::SetVolume(target))
            .await
            .map(|_| ())
            .map_err(Error::from)
    }

    // Cohesive single match/render; splitting would fragment one logical unit.
    #[allow(clippy::too_many_lines)]
    // significant_drop_tightening: tokio guard held to scope; not tightened (early-drop is borrow-blocked, spans a trailing await, or saves nothing before return).
    #[allow(clippy::significant_drop_tightening)]
    async fn handle_mouse_click(&mut self, x: u16, y: u16) -> Result<(), Error> {
        use crate::ui::header::{Header, HeaderRegion};

        let ds = self.daemon_state.read().await;

        let mut cs = self.client_state.write().await;

        let state = AppState {
            daemon: &ds,
            client: &mut cs,
        };
        let layout = state.client.layout.clone();
        let page = state.client.page;
        let duration = state.daemon.now_playing.duration;
        let time_width = crate::num::u16_sat(
            format!(
                "{} / {}",
                state.daemon.now_playing.format_position(),
                state.daemon.now_playing.format_duration()
            )
            .len(),
        );
        let _ = state;
        drop(cs);
        drop(ds);

        if y >= layout.header.y && y < layout.header.y + layout.header.height {
            if let Some(region) = Header::region_at(layout.header, x, y) {
                match region {
                    HeaderRegion::Tab(tab_page) => {
                        let ds = self.daemon_state.read().await;
                        let mut cs = self.client_state.write().await;
                        let state = AppState {
                            daemon: &ds,
                            client: &mut cs,
                        };
                        state.client.page = tab_page;
                    }
                    HeaderRegion::PrevButton => {
                        return self
                            .client
                            .request(DaemonRequest::Previous)
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                    HeaderRegion::PlayButton => {
                        return self
                            .client
                            .request(DaemonRequest::TogglePause)
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                    HeaderRegion::PauseButton => {
                        return self
                            .client
                            .request(DaemonRequest::TogglePause)
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                    HeaderRegion::StopButton => {
                        // Toolbar Stop clears the queue and stops; MPRIS Stop
                        // (DaemonRequest::Stop) keeps the track per spec.
                        return self
                            .client
                            .request(DaemonRequest::ClearQueue)
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                    HeaderRegion::NextButton => {
                        return self
                            .client
                            .request(DaemonRequest::Next)
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                }
            }
            return Ok(());
        }

        if y >= layout.now_playing.y && y < layout.now_playing.y + layout.now_playing.height {
            if let Some(vol) = self.volume_slider_hit(x, y).await {
                return self
                    .client
                    .request(DaemonRequest::SetVolume(vol))
                    .await
                    .map(|_| ())
                    .map_err(Error::from);
            }
            let area = layout.now_playing;
            if area.height >= 2 && y == area.y + area.height - 2 && duration > 0.0 {
                let row = crate::ui::widget_now_playing::progress_row_layout(
                    area.width.saturating_sub(2),
                    time_width,
                );
                if let (Some(row), Some(rel_x)) = (row, x.checked_sub(area.x + 1)) {
                    if row.bar_width > 0
                        && rel_x >= row.bar_start
                        && rel_x < row.bar_start + row.bar_width
                    {
                        let fraction = f64::from(rel_x - row.bar_start) / f64::from(row.bar_width);
                        let seek_pos = fraction * duration;
                        let _ = self
                            .client
                            .request(DaemonRequest::Seek(seek_pos))
                            .await
                            .map(|_| ())
                            .map_err(Error::from);
                    }
                }
            }
            return Ok(());
        }

        if y >= layout.content.y && y < layout.content.y + layout.content.height {
            return self.handle_content_click(x, y, page, &layout).await;
        }

        Ok(())
    }

    async fn handle_content_click(
        &mut self,
        x: u16,
        y: u16,
        page: Page,
        layout: &LayoutAreas,
    ) -> Result<(), Error> {
        match page {
            Page::QuickPlay => self.handle_quick_play_click(x, y, layout).await,
            Page::Library => self.handle_library_click(x, y, layout).await,
            Page::Queue => self.handle_queue_click(y, layout).await,
            Page::Playlists => self.handle_playlists_click(x, y, layout).await,
            _ => Ok(()),
        }
    }

    // significant_drop_tightening: tokio guard held to scope; not tightened (early-drop is borrow-blocked, spans a trailing await, or saves nothing before return).
    #[allow(clippy::significant_drop_tightening)]
    async fn handle_quick_play_click(
        &mut self,
        x: u16,
        y: u16,
        layout: &LayoutAreas,
    ) -> Result<(), Error> {
        use crate::app::models::SongOption;
        let (Some(left), Some(right)) = (layout.content_left, layout.content_right) else {
            return Ok(());
        };

        let in_pane = |r: ratatui::layout::Rect| {
            x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        };

        if in_pane(left) {
            let row_in_pane = y.saturating_sub(left.y + 1) as usize;
            let option = match row_in_pane {
                0 => Some(SongOption::Starred),
                1 => Some(SongOption::Random),
                _ => None,
            };
            if let Some(option) = option {
                let already;
                {
                    let ds = self.daemon_state.read().await;
                    let mut cs = self.client_state.write().await;
                    let state = AppState {
                        daemon: &ds,
                        client: &mut cs,
                    };
                    already = state.client.songs.selected_option.as_ref() == Some(&option);
                    state.client.songs.selected_option = Some(option.clone());
                    state.client.songs.focus = 0;
                }
                if !already {
                    let req = match option {
                        SongOption::Starred => DaemonRequest::RefreshStarred,
                        SongOption::Random => DaemonRequest::RefreshRandom,
                    };
                    let _ = self.client.request(req).await;
                }
            }
            return Ok(());
        }

        if !in_pane(right) {
            return Ok(());
        }

        let row_in_pane = y.saturating_sub(right.y + 1) as usize;
        let ds = self.daemon_state.read().await;
        let mut cs = self.client_state.write().await;
        let state = AppState {
            daemon: &ds,
            client: &mut cs,
        };
        let item_index = state.client.songs.scroll_offset + row_in_pane;
        if item_index >= state.songs_list().len() {
            return Ok(());
        }
        state.client.songs.focus = 1;
        let was_selected = state.client.songs.selected_index == Some(item_index);
        state.client.songs.selected_index = Some(item_index);

        let is_second_click = was_selected
            && self
                .last_click
                .is_some_and(|(lx, ly, t)| lx == x && ly == y && t.elapsed().as_millis() < 500);

        if is_second_click {
            let songs = state.songs_list().to_vec();
            let _ = state;
            drop(cs);
            drop(ds);
            self.last_click = Some((x, y, std::time::Instant::now()));
            return self
                .client
                .request(DaemonRequest::EnqueueSongs {
                    songs,
                    mode: EnqueueMode::Replace {
                        play_from: Some(item_index),
                    },
                })
                .await
                .map(|_| ())
                .map_err(Error::from);
        }

        self.last_click = Some((x, y, std::time::Instant::now()));
        Ok(())
    }

    async fn handle_queue_click(&mut self, y: u16, layout: &LayoutAreas) -> Result<(), Error> {
        let ds = self.daemon_state.read().await;
        let mut cs = self.client_state.write().await;
        let state = AppState {
            daemon: &ds,
            client: &mut cs,
        };
        let content = layout.content;

        let row_in_viewport = y.saturating_sub(content.y + 1) as usize;
        let item_index = state.client.queue_state.scroll_offset + row_in_viewport;

        if item_index < state.daemon.queue.len() {
            let was_selected = state.client.queue_state.selected == Some(item_index);
            state.client.queue_state.selected = Some(item_index);

            let is_second_click = was_selected
                && self
                    .last_click
                    .is_some_and(|(_, ly, t)| ly == y && t.elapsed().as_millis() < 500);

            if is_second_click {
                let _ = state;
                drop(cs);
                drop(ds);
                self.last_click = Some((0, y, std::time::Instant::now()));
                return self
                    .client
                    .request(DaemonRequest::PlayQueueIndex(item_index))
                    .await
                    .map(|_| ())
                    .map_err(Error::from);
            }
        }

        self.last_click = Some((0, y, std::time::Instant::now()));
        Ok(())
    }

    // significant_drop_tightening: tokio guard held to scope; not tightened (early-drop is borrow-blocked, spans a trailing await, or saves nothing before return).
    #[allow(clippy::significant_drop_tightening)]
    async fn handle_mouse_scroll_up(&self) -> Result<(), Error> {
        let ds = self.daemon_state.read().await;
        let mut cs = self.client_state.write().await;
        let state = AppState {
            daemon: &ds,
            client: &mut cs,
        };
        match state.client.page {
            Page::Library => {
                if state.client.artists.focus == 0 {
                    if let Some(sel) = state.client.artists.selected_index {
                        if sel > 0 {
                            state.client.artists.selected_index = Some(sel - 1);
                        }
                    }
                } else if let Some(sel) = state.client.artists.selected_song {
                    if sel > 0 {
                        state.client.artists.selected_song = Some(sel - 1);
                    }
                }
            }
            Page::Queue => {
                if let Some(sel) = state.client.queue_state.selected {
                    if sel > 0 {
                        state.client.queue_state.selected = Some(sel - 1);
                    }
                } else if !state.daemon.queue.is_empty() {
                    state.client.queue_state.selected = Some(0);
                }
            }
            Page::QuickPlay if state.client.songs.focus == 1 => {
                if let Some(sel) = state.client.songs.selected_index {
                    if sel > 0 {
                        state.client.songs.selected_index = Some(sel - 1);
                    }
                } else if !state.songs_list().is_empty() {
                    state.client.songs.selected_index = Some(0);
                }
            }
            Page::Playlists => {
                if state.client.playlists.focus == 0 {
                    if let Some(sel) = state.client.playlists.selected_playlist {
                        if sel > 0 {
                            state.client.playlists.selected_playlist = Some(sel - 1);
                        }
                    }
                } else if let Some(sel) = state.client.playlists.selected_song {
                    if sel > 0 {
                        state.client.playlists.selected_song = Some(sel - 1);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // significant_drop_tightening: tokio guard held to scope; not tightened (early-drop is borrow-blocked, spans a trailing await, or saves nothing before return).
    #[allow(clippy::significant_drop_tightening)]
    async fn handle_mouse_scroll_down(&self) -> Result<(), Error> {
        let ds = self.daemon_state.read().await;
        let mut cs = self.client_state.write().await;
        let state = AppState {
            daemon: &ds,
            client: &mut cs,
        };
        match state.client.page {
            Page::Library => {
                if state.client.artists.focus == 0 {
                    let tree_items = crate::ui::pages::library::build_tree_items(&state);
                    let max = tree_items.len().saturating_sub(1);
                    if let Some(sel) = state.client.artists.selected_index {
                        if sel < max {
                            state.client.artists.selected_index = Some(sel + 1);
                        }
                    } else if !tree_items.is_empty() {
                        state.client.artists.selected_index = Some(0);
                    }
                } else {
                    let max = state.client.artists.songs.len().saturating_sub(1);
                    if let Some(sel) = state.client.artists.selected_song {
                        if sel < max {
                            state.client.artists.selected_song = Some(sel + 1);
                        }
                    } else if !state.client.artists.songs.is_empty() {
                        state.client.artists.selected_song = Some(0);
                    }
                }
            }
            Page::Queue => {
                let max = state.daemon.queue.len().saturating_sub(1);
                if let Some(sel) = state.client.queue_state.selected {
                    if sel < max {
                        state.client.queue_state.selected = Some(sel + 1);
                    }
                } else if !state.daemon.queue.is_empty() {
                    state.client.queue_state.selected = Some(0);
                }
            }
            Page::QuickPlay if state.client.songs.focus == 1 => {
                let max = state.songs_list().len().saturating_sub(1);
                if let Some(sel) = state.client.songs.selected_index {
                    if sel < max {
                        state.client.songs.selected_index = Some(sel + 1);
                    }
                } else if !state.songs_list().is_empty() {
                    state.client.songs.selected_index = Some(0);
                }
            }
            Page::Playlists => {
                if state.client.playlists.focus == 0 {
                    let max = state.daemon.library.playlists.len().saturating_sub(1);
                    if let Some(sel) = state.client.playlists.selected_playlist {
                        if sel < max {
                            state.client.playlists.selected_playlist = Some(sel + 1);
                        }
                    } else if !state.daemon.library.playlists.is_empty() {
                        state.client.playlists.selected_playlist = Some(0);
                    }
                } else {
                    let max = state.client.playlists.songs.len().saturating_sub(1);
                    if let Some(sel) = state.client.playlists.selected_song {
                        if sel < max {
                            state.client.playlists.selected_song = Some(sel + 1);
                        }
                    } else if !state.client.playlists.songs.is_empty() {
                        state.client.playlists.selected_song = Some(0);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
