# Smart Playlists MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Permitir crear playlists desde el picker y añadir rápidamente la canción objetivo a la última playlist usada mediante `A`.

**Architecture:** Navidrome sigue siendo la fuente de verdad. El daemon ejecuta create/add, persiste `LastPlaylistId` solo después de éxito y emite `ConfigChanged`; la TUI resuelve la canción objetivo y gestiona un prompt mínimo de creación.

**Tech Stack:** Rust 2021, Tokio, serde/toml, ratatui/crossterm, reqwest, wiremock.

## Global Constraints

- No añadir dependencias.
- La canción objetivo es la selección del panel enfocado; fallback a `now_playing.song`.
- `A` crea si no hay playlists, usa `LastPlaylistId` válido o abre picker si falta/es obsoleto.
- `a` conserva el picker y añade `+ New playlist…` como primera fila.
- Nunca persistir `LastPlaylistId` antes de confirmación de Navidrome.
- No generalizar todos los prompts ni refactorizar código ajeno al flujo.
- Las notificaciones de éxito se muestran después de respuesta exitosa.

## File Map

- `src/config/mod.rs`: serialización y round-trip de `LastPlaylistId`.
- `src/subsonic/client.rs`: `create_playlist` devuelve la playlist creada.
- `src/subsonic/models.rs`: payload de create reutilizando `Playlist`.
- `src/daemon/library_ops.rs`: persistencia post-éxito del último destino.
- `src/ipc/protocol.rs`, `src/ipc/client.rs`: respuestas tipadas de create/add.
- `src/app/page_state.rs`, `src/app/client_state.rs`: estado del prompt y helper para abrirlo.
- `src/app/state.rs`: función pura `target_song`.
- `src/app/input.rs`, `src/app/input_playlists.rs`, `src/app/input_library.rs`, `src/app/input_queue.rs`, `src/app/input_songs.rs`: quick-add y picker.
- `src/ui/playlist_picker.rs`, `src/ui/layout.rs`: fila de creación y prompt.
- `src/ui/footer.rs`, `README.md`: ayuda y documentación.
- `tests/playlist_quick_add.rs`, `tests/playlist_editing.rs`, `tests/config_validate_and_save.rs`, `tests/common/fake_subsonic.rs`: cobertura.

---

### Task 1: Persistir `LastPlaylistId`

**Files:**
- Modify: `src/config/mod.rs:17-38,54-156,158-205,217-248,359-383`
- Test: `src/config/mod.rs:840-1188`
- Test: `tests/config_validate_and_save.rs`

**Interfaces:**
- Produces: `Config::last_playlist_id: Option<String>` serializado como `LastPlaylistId`.

- [ ] **Step 1: escribir tests fallidos de default y round-trip**

Añadir en `src/config/mod.rs`:

```rust
#[test]
fn last_playlist_id_defaults_none_and_round_trips() {
    let mut c = Config::default();
    assert_eq!(c.last_playlist_id, None);
    c.last_playlist_id = Some("pl-42".into());
    let f = NamedTempFile::new().unwrap();
    c.save_to_file(f.path()).unwrap();
    assert_eq!(
        Config::load_from_file(f.path()).unwrap().last_playlist_id.as_deref(),
        Some("pl-42")
    );
}
```

- [ ] **Step 2: verificar que falla**

Run: `cargo test last_playlist_id_defaults_none_and_round_trips -- --exact`
Expected: FAIL porque `Config` no tiene `last_playlist_id`.

- [ ] **Step 3: implementar la clave en todas las superficies de config**

Añadir:

```rust
// KNOWN_CONFIG_KEYS
"LastPlaylistId",

// Config
#[serde(rename = "LastPlaylistId", default, skip_serializing_if = "Option::is_none")]
pub last_playlist_id: Option<String>,

// ConfigOnDisk
#[serde(rename = "LastPlaylistId", skip_serializing_if = "Option::is_none")]
last_playlist_id: Option<&'a str>,

// as_on_disk
last_playlist_id: self.last_playlist_id.as_deref(),

// Default
last_playlist_id: None,
```

- [ ] **Step 4: ejecutar tests de configuración**

Run: `cargo test last_playlist_id && cargo test --test config_validate_and_save`
Expected: PASS.

- [ ] **Step 5: commit**

```bash
git add src/config/mod.rs tests/config_validate_and_save.rs
git commit -m "feat(config): persist last playlist id" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Devolver la playlist creada y persistir solo tras éxito

**Files:**
- Modify: `src/subsonic/models.rs:404-457`
- Modify: `src/subsonic/client.rs:156-170`
- Modify: `src/daemon/library_ops.rs:203-281`
- Modify: `src/ipc/protocol.rs:67-92,223-257`
- Modify: `src/ipc/client.rs:93-107`
- Modify: `tests/common/fake_subsonic.rs:188-202`
- Test: `tests/playlist_editing.rs`

**Interfaces:**
- Produces: `SubsonicClient::create_playlist(&str, &[String]) -> Result<Playlist, SubsonicError>`.
- Produces: `DaemonResponse::PlaylistCreated(Playlist)`.
- Produces: `DaemonCore::create_playlist(...) -> Result<Playlist, Error>`.
- `DaemonCore::playlist_add_song` persiste el destino antes de devolver canciones.

- [ ] **Step 1: cambiar el fixture y escribir tests de ID real/persistencia**

Cambiar `expect_create_playlist` para devolver:

```rust
.respond_with(ok_body(json!({
    "playlist": { "id": "created-1", "name": "Road Trip", "songCount": 1 }
})))
```

Añadir tests en `tests/playlist_editing.rs` que llamen a `core.create_playlist` y `core.playlist_add_song`, y comprueben:

```rust
let created = td.core.create_playlist("Road Trip", &["song-9".into()]).await.unwrap();
assert_eq!(created.id, "created-1");
assert_eq!(td.state.read().await.config.last_playlist_id.as_deref(), Some("created-1"));
```

Para add:

```rust
assert_eq!(td.state.read().await.config.last_playlist_id.as_deref(), Some("pl-1"));
```

Añadir un test con `expect_error("updatePlaylist", 70, "Not found")` que confirme que un ID previo no cambia tras fallo.

- [ ] **Step 2: verificar fallos**

Run: `cargo test --test playlist_editing`
Expected: FAIL en `create_playlist_returns_real_id_and_persists_it`, `add_persists_destination_only_after_success` y `failed_add_keeps_previous_destination` por las firmas/respuestas actuales.

- [ ] **Step 3: parsear el payload de create**

Añadir un payload:

```rust
#[derive(Debug, Deserialize)]
pub struct CreatedPlaylistData {
    pub playlist: Playlist,
}
```

Cambiar el método:

```rust
pub async fn create_playlist(
    &self,
    name: &str,
    song_ids: &[String],
) -> Result<Playlist, SubsonicError> {
    let mut endpoint = format!("createPlaylist?name={}", urlencoding::encode(name));
    for id in song_ids {
        let _ = write!(endpoint, "&songId={}", urlencoding::encode(id));
    }
    let data: CreatedPlaylistData = self.request(&endpoint).await?;
    Ok(data.playlist)
}
```

- [ ] **Step 4: persistir mediante un helper daemon-side**

Añadir en `library_ops.rs`:

```rust
async fn remember_playlist(&self, playlist_id: &str) -> Result<(), Error> {
    {
        let mut state = self.state.write().await;
        state.config.last_playlist_id = Some(playlist_id.to_string());
        state.config.save_default().map_err(Error::Config)?;
    }
    self.emit_config_changed().await;
    Ok(())
}
```

Hacer que create obtenga `Playlist`, llame a `remember_playlist(&playlist.id)` y luego refresque. Hacer que add llame a `remember_playlist(playlist_id)` solo después del `updatePlaylist` exitoso.

- [ ] **Step 5: cablear respuesta IPC**

Añadir `DaemonResponse::PlaylistCreated(Playlist)` y cambiar el router:

```rust
DaemonRequest::CreatePlaylist { name, song_ids } => Ok(
    DaemonResponse::PlaylistCreated(
        core.create_playlist(&name, &song_ids).await.map_err(err)?
    )
),
```

- [ ] **Step 6: ejecutar tests relevantes**

Run: `cargo test --test playlist_editing --test queue_save_playlist --test ipc_roundtrip`
Expected: PASS; adaptar `queue_save_playlist` para aceptar `PlaylistCreated` sin perder su verificación HTTP.

- [ ] **Step 7: commit**

```bash
git add src/subsonic/models.rs src/subsonic/client.rs src/daemon/library_ops.rs src/ipc/protocol.rs src/ipc/client.rs tests/common/fake_subsonic.rs tests/playlist_editing.rs tests/queue_save_playlist.rs
git commit -m "feat(playlists): remember successful destination" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Resolver canción objetivo y crear prompt mínimo

**Files:**
- Modify: `src/app/state.rs:141-156`
- Modify: `src/app/page_state.rs:118-163`
- Modify: `src/app/client_state.rs:14-63`
- Test/Create: `tests/playlist_quick_add.rs`

**Interfaces:**
- Produces: `AppState::target_song(&self) -> Option<&Child>`.
- Produces: `CreatePlaylistPrompt { active, name, song }`.
- Produces: `ClientState::open_create_playlist_prompt(Child)` y `close_create_playlist_prompt()`.

- [ ] **Step 1: escribir test tabular de target**

Crear `tests/playlist_quick_add.rs` con casos Library/Queue/QuickPlay/Playlists y fallback. Ejemplo:

```rust
#[test]
fn library_uses_focused_song_then_falls_back_to_now_playing() {
    let mut daemon = DaemonState::new(Config::default());
    daemon.now_playing.song = Some(song("playing"));
    let mut client = ClientState::default();
    client.page = Page::Library;
    client.artists.songs = vec![song("selected")];
    client.artists.selected_song = Some(0);
    client.artists.focus = 1;
    let state = AppState { daemon: &daemon, client: &mut client };
    assert_eq!(state.target_song().unwrap().id, "selected");
}
```

Añadir caso `focus = 0` que espere `playing`.

- [ ] **Step 2: verificar fallo**

Run: `cargo test --test playlist_quick_add target_song`
Expected: FAIL por método ausente.

- [ ] **Step 3: implementar resolución pura**

```rust
pub fn target_song(&self) -> Option<&Child> {
    let selected = match self.client.page {
        Page::Library if self.client.artists.focus == 1 => self.client.artists.selected_song
            .and_then(|i| self.client.artists.songs.get(i)),
        Page::Queue => self.client.queue_state.selected
            .and_then(|i| self.daemon.queue.get(i)),
        Page::QuickPlay if self.client.songs.focus == 1 => self.client.songs.selected_index
            .and_then(|i| self.songs_list().get(i)),
        Page::Playlists if self.client.playlists.focus == 1 => self.client.playlists.selected_song
            .and_then(|i| self.client.playlists.songs.get(i)),
        _ => None,
    };
    selected.or(self.daemon.now_playing.song.as_ref())
}
```

- [ ] **Step 4: añadir estado de prompt**

```rust
#[derive(Debug, Clone, Default)]
pub struct CreatePlaylistPrompt {
    pub active: bool,
    pub name: String,
    pub song: Option<Child>,
}
```

Incorporarlo a `ClientState` y añadir helpers que limpien nombre/song al cerrar.

- [ ] **Step 5: tests y commit**

Run: `cargo test --test playlist_quick_add`
Expected: PASS.

```bash
git add src/app/state.rs src/app/page_state.rs src/app/client_state.rs tests/playlist_quick_add.rs
git commit -m "feat(playlists): resolve quick-add target" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Picker con nueva playlist y quick-add global

**Files:**
- Modify: `src/app/input.rs:53-311`
- Modify: `src/app/input_playlists.rs:281-435`
- Modify: `src/app/input_library.rs:566-575`
- Modify: `src/app/input_queue.rs:222-231`
- Modify: `src/app/input_songs.rs:114-123`
- Modify: `src/ui/playlist_picker.rs:14-61`
- Modify: `src/ui/layout.rs:146-153`
- Test: `tests/playlist_quick_add.rs`
- Test: `tests/playlist_editing.rs`

**Interfaces:**
- Consumes: `AppState::target_song`, `CreatePlaylistPrompt` y respuestas de Task 2.
- Produces: `App::handle_create_playlist_prompt_key` y `App::quick_add_to_playlist`.

- [ ] **Step 1: tests fallidos de rutas UX**

Añadir tests para:

```rust
// cero playlists
press(&mut app, KeyCode::Char('A')).await;
assert!(app.client_state.read().await.create_playlist_prompt.active);

// último válido
app.daemon_state.write().await.config.last_playlist_id = Some("pl-1".into());
press(&mut app, KeyCode::Char('A')).await;
assert!(client.sent().iter().any(|r| matches!(r,
    DaemonRequest::AddSongToPlaylist { playlist_id, song_id }
    if playlist_id == "pl-1" && song_id == "selected"
)));

// stale
app.daemon_state.write().await.config.last_playlist_id = Some("gone".into());
press(&mut app, KeyCode::Char('A')).await;
assert!(app.client_state.read().await.playlist_picker.active);
```

Añadir test picker: `selected == 0` + Enter abre prompt; la primera playlist real usa índice visual 1.

- [ ] **Step 2: verificar fallos**

Run: `cargo test --test playlist_quick_add --test playlist_editing`
Expected: FAIL en rutas nuevas.

- [ ] **Step 3: hacer que el prompt posea las teclas antes del routing global**

En `handle_key`, inmediatamente después de quit prompt y antes del picker:

```rust
if state.client.create_playlist_prompt.active {
    drop(cs);
    drop(ds);
    return self.handle_create_playlist_prompt_key(key).await;
}
```

El handler acepta Esc, Backspace, Char y Enter. Enter recorta nombre, mantiene el prompt abierto si está vacío y, si es válido, envía `CreatePlaylist { name, song_ids: vec![song.id] }`. Solo tras `PlaylistCreated` muestra `Created playlist: {name}` y cierra.

- [ ] **Step 4: implementar `A` global**

Añadir un arm antes de delegar a páginas:

```rust
(KeyCode::Char('A'), _) => {
    let song = state.target_song().cloned();
    let playlists = &state.daemon.library.playlists;
    let last = state.daemon.config.last_playlist_id.clone();
    // 0 => prompt; last válido => request; resto => picker
}
```

Para add válido, esperar `PlaylistSongs(_)`; después notificar. En error usar `notify_error("Failed to add song to playlist")`.

- [ ] **Step 5: convertir `a` en ruta global con el mismo target**

Añadir junto a `A` un arm global para `KeyCode::Char('a')`: resolver `state.target_song().cloned()`, notificar `Nothing to add` si no existe y llamar a un helper TUI pequeño `open_playlist_picker_or_create(song)`. El helper abre el prompt si la lista está vacía y el picker si contiene playlists. Eliminar los cuatro arms duplicados de `a` en Library, Queue, QuickPlay y Playlists para que todas las páginas obtengan el mismo fallback a now-playing.

- [ ] **Step 6: adaptar picker a índice sintético 0**

Renderizar primero:

```rust
std::iter::once(ListItem::new("+ New playlist…"))
    .chain(playlists.iter().map(...))
```

Usar `count = playlists.len() + 1`; Enter en 0 abre prompt, Enter en `n > 0` usa `playlists[n - 1]`.

- [ ] **Step 7: renderizar prompt**

Crear una función pequeña en `playlist_picker.rs` o un nuevo `ui/create_playlist_prompt.rs` solo si el archivo supera una responsabilidad clara. Dibujar modal centrado con título `New playlist`, buffer y ayudas Enter/Esc. Llamarlo desde `layout.rs` después del picker para que el prompt tenga prioridad visual.

- [ ] **Step 8: ejecutar tests de input y render**

Run: `cargo test --test playlist_quick_add --test playlist_editing --test input_library_keys --test input_queue_full --test input_songs_full --test playlists_render`
Expected: PASS.

- [ ] **Step 9: commit**

```bash
git add src/app src/ui tests/playlist_quick_add.rs tests/playlist_editing.rs
git commit -m "feat(playlists): add create and quick-add flows" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Ayuda, README y validación de playlists

**Files:**
- Modify: `src/ui/footer.rs:57-125`
- Modify: `README.md:1-15,190-290`
- Test: tests afectados por footer/snapshots.

**Interfaces:**
- Consumes: teclas finales `A` y `a`.
- Produces: documentación del flujo estable.

- [ ] **Step 1: actualizar ayuda**

Añadir `A: Quick playlist` en global y `a: Choose playlist` en páginas musicales cuando quepa; mantener los hints prioritarios si el ancho trunca.

- [ ] **Step 2: actualizar README**

Reescribir el bloque inicial para decir que es un fork no oficial con mejoras nacidas del uso real; enlazar upstream y añadir una llamada explícita a apoyar al creador original. Documentar:

```markdown
- `A`: add the selected song (or now playing) to the last-used playlist.
- `a`: choose a playlist or create a new one.
```

No afirmar soporte de letras todavía; eso entra en el segundo plan.

- [ ] **Step 3: pruebas y validación**

Run: `cargo fmt --check && cargo test --test playlist_quick_add --test playlist_editing --test queue_save_playlist && cargo test`
Expected: PASS completo.

- [ ] **Step 4: commit**

```bash
git add src/ui/footer.rs README.md tests/snapshots
git commit -m "docs: explain fork playlist improvements" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```
