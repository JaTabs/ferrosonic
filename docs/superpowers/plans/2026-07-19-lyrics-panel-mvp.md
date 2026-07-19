# Lyrics Panel MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mostrar letras obtenidas de Navidrome en un panel inferior alternable con `y`, resaltando la línea sincronizada actual.

**Architecture:** El daemon consulta `getLyricsBySongId`, comprueba la extensión `songLyrics` y cachea por canción. La TUI solicita letras asíncronamente, descarta respuestas obsoletas por `song_id` y mantiene únicamente estado visual local.

**Tech Stack:** Rust 2021, Tokio, serde, ratatui/crossterm, reqwest, wiremock.

## Global Constraints

- Fuente única: servidor OpenSubsonic/Navidrome; no LRCLIB directo ni tags locales.
- Sin dependencias nuevas, caché en disco, traducciones, karaoke enhanced ni persistencia del panel.
- `y` alterna el panel fuera de modales/campos de texto.
- Panel inferior de 10 filas objetivo, reducido de forma segura en terminales bajas.
- Letras sincronizadas auto-scroll; no hay scroll manual para letras no sincronizadas en este MVP.
- Toda respuesta lleva `song_id` y solo se aplica si sigue vigente.

## File Map

- `src/subsonic/models.rs`: modelos `LyricsListData`, `StructuredLyrics`, `LyricLine`.
- `src/subsonic/client.rs`: endpoint `get_lyrics_by_song_id`.
- `src/daemon/core.rs`: caché de letras y capacidad `songLyrics` memoizada.
- Create `src/daemon/lyrics.rs`: selección de entrada, soporte y fetch/cache.
- `src/daemon/mod.rs`: registrar módulo.
- `src/daemon/settings_ops.rs`: invalidar capacidad/caché al cambiar de servidor.
- `src/ipc/protocol.rs`, `src/ipc/client.rs`: request/response.
- `src/app/page_state.rs`, `src/app/client_state.rs`: `LyricsState`.
- `src/app/input.rs`: toggle y petición.
- `src/app/event_pump.rs`: nueva petición al cambiar canción y limpieza en stop.
- Create `src/ui/widget_lyrics.rs`: render y cálculo de línea.
- `src/ui/mod.rs`, `src/ui/layout.rs`: módulo y banda inferior.
- `src/ui/footer.rs`, `README.md`: ayuda/documentación.
- Create `tests/lyrics_api.rs`, `tests/lyrics_input.rs`, `tests/lyrics_render.rs`; modify `tests/common/fake_subsonic.rs`.

---

### Task 1: Modelos y endpoint OpenSubsonic

**Files:**
- Modify: `src/subsonic/models.rs`
- Modify: `src/subsonic/client.rs:10-15,237-244`
- Modify: `tests/common/fake_subsonic.rs`
- Create: `tests/lyrics_api.rs`

**Interfaces:**
- Produces: `LyricLine { start: Option<u64>, value: String }`.
- Produces: `StructuredLyrics { lang, synced, offset, line, ... }` serializable por IPC.
- Produces: `SubsonicClient::get_lyrics_by_song_id(&str) -> Result<Vec<StructuredLyrics>, SubsonicError>`.

- [ ] **Step 1: fixtures y tests de parseo fallidos**

Añadir a `FakeSubsonic`:

```rust
pub async fn expect_lyrics(&self, song_id: &str, lyrics: Value) {
    Mock::given(method("GET"))
        .and(path("/rest/getLyricsBySongId"))
        .and(wiremock::matchers::query_param("id", song_id))
        .respond_with(ok_body(json!({ "lyricsList": lyrics })))
        .mount(&self.server)
        .await;
}
```

Crear tests synced y unsynced. Fixture synced:

```rust
json!({ "structuredLyrics": [{
    "displayArtist": "Artist",
    "displayTitle": "Song",
    "lang": "en",
    "offset": 125,
    "synced": true,
    "line": [
        { "start": 1000, "value": "First" },
        { "start": 2500, "value": "Second" }
    ]
}]})
```

Afirmar `start == Some(1000)`, `offset == 125`, y que una línea unsynced sin `start` parsea como `None`.

- [ ] **Step 2: verificar fallo**

Run: `cargo test --test lyrics_api`
Expected: FAIL por modelos/método ausentes.

- [ ] **Step 3: implementar modelos tolerantes**

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricLine {
    #[serde(default)]
    pub start: Option<u64>,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredLyrics {
    #[serde(default, rename = "displayArtist")]
    pub display_artist: Option<String>,
    #[serde(default, rename = "displayTitle")]
    pub display_title: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub line: Vec<LyricLine>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LyricsList {
    #[serde(default, rename = "structuredLyrics")]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

#[derive(Debug, Default, Deserialize)]
pub struct LyricsListData {
    #[serde(default, rename = "lyricsList")]
    pub lyrics_list: LyricsList,
}
```

- [ ] **Step 4: implementar endpoint**

```rust
pub async fn get_lyrics_by_song_id(
    &self,
    id: &str,
) -> Result<Vec<StructuredLyrics>, SubsonicError> {
    let endpoint = format!("getLyricsBySongId?id={}", urlencoding::encode(id));
    let data: LyricsListData = self.request(&endpoint).await?;
    Ok(data.lyrics_list.structured_lyrics)
}
```

- [ ] **Step 5: ejecutar tests y commit**

Run: `cargo test --test lyrics_api --test subsonic_client_endpoints`
Expected: PASS.

```bash
git add src/subsonic tests/common/fake_subsonic.rs tests/lyrics_api.rs
git commit -m "feat(lyrics): add OpenSubsonic client models" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Fetch, selección y caché daemon-side

**Files:**
- Modify: `src/daemon/core.rs:135-190,199-260`
- Create: `src/daemon/lyrics.rs`
- Modify: `src/daemon/mod.rs`
- Modify: `src/ipc/protocol.rs`
- Modify: `src/ipc/client.rs:44-187`
- Test: `tests/lyrics_api.rs`

**Interfaces:**
- Produces: `LyricsResult::{Available(StructuredLyrics), Empty, Unsupported, Unavailable}`.
- Produces: `DaemonCore::get_lyrics(&str) -> LyricsResult`.
- Produces: `DaemonRequest::GetLyrics { song_id }`.
- Produces: `DaemonResponse::Lyrics { song_id, result }`.

- [ ] **Step 1: tests fallidos de selección, soporte y caché**

Añadir test puro para preferencia:

```rust
let selected = select_lyrics(vec![unsynced(), synced()]).unwrap();
assert!(selected.synced);
```

Test daemon con `expect_open_subsonic_extensions(&["songLyrics"])`, `expect_lyrics`, dos llamadas a `get_lyrics("song-1")` y conteo de requests:

```rust
let count = requests.iter()
    .filter(|r| r.url.path() == "/rest/getLyricsBySongId")
    .count();
assert_eq!(count, 1);
```

Test sin extensión espera `Unsupported` y cero requests al endpoint.

- [ ] **Step 2: verificar fallo**

Run: `cargo test --test lyrics_api daemon_lyrics`
Expected: FAIL por módulo/tipos ausentes.

- [ ] **Step 3: definir resultado IPC estable**

En `protocol.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LyricsResult {
    Available(StructuredLyrics),
    Empty,
    Unsupported,
    Unavailable,
}
```

Añadir las variantes request/response exactas descritas en Interfaces.

- [ ] **Step 4: añadir cachés mínimas al core**

En `DaemonCore`:

```rust
pub(super) lyrics_cache: RwLock<std::collections::HashMap<String, LyricsResult>>,
pub(super) song_lyrics_supported: RwLock<Option<bool>>,
```

Inicializar con `HashMap::new()` y `None`. No incorporarlas al snapshot ni a `DaemonState`. Al completar con éxito el primer `getOpenSubsonicExtensions`, guardar `Some(true/false)`; las canciones siguientes reutilizan ese valor. En `update_server_config`, limpiar `lyrics_cache` y volver `song_lyrics_supported` a `None` para no mezclar capacidades ni letras de dos servidores.

- [ ] **Step 5: implementar `src/daemon/lyrics.rs`**

```rust
pub(crate) fn select_lyrics(items: Vec<StructuredLyrics>) -> Option<StructuredLyrics> {
    items.iter().find(|item| item.synced).cloned().or_else(|| items.into_iter().next())
}

impl DaemonCore {
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
            client.get_open_subsonic_extensions().await.map(|exts| {
                exts.iter().any(|name| name == "songLyrics")
            })
        };
        if let Ok(value) = supported {
            *self.song_lyrics_supported.write().await = Some(value);
        }
        let result = match supported {
            Ok(false) => LyricsResult::Unsupported,
            Err(e) => { tracing::warn!("lyrics extension probe failed: {e}"); LyricsResult::Unavailable }
            Ok(true) => match client.get_lyrics_by_song_id(song_id).await {
                Ok(items) => select_lyrics(items).map_or(LyricsResult::Empty, LyricsResult::Available),
                Err(e) => { tracing::warn!("lyrics fetch failed for {song_id}: {e}"); LyricsResult::Unavailable }
            },
        };
        self.lyrics_cache.write().await.insert(song_id.to_string(), result.clone());
        result
    }
}
```


- [ ] **Step 6: cablear router IPC**

```rust
DaemonRequest::GetLyrics { song_id } => Ok(DaemonResponse::Lyrics {
    result: core.get_lyrics(&song_id).await,
    song_id,
}),
```

- [ ] **Step 7: tests y commit**

Run: `cargo test --test lyrics_api --test ipc_roundtrip --test ipc_frame_proptest`
Expected: PASS; ampliar generadores exhaustivos para la variante nueva.

```bash
git add src/daemon src/ipc tests/lyrics_api.rs tests/ipc_roundtrip.rs tests/ipc_frame_proptest.rs
git commit -m "feat(lyrics): fetch and cache server lyrics" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Estado TUI, toggle y descarte de respuestas obsoletas

**Files:**
- Modify: `src/app/page_state.rs`
- Modify: `src/app/client_state.rs`
- Modify: `src/app/input.rs:53-311`
- Modify: `src/app/event_pump.rs:44-212`
- Create: `tests/lyrics_input.rs`
- Modify: `tests/apply_event_variants.rs`

**Interfaces:**
- Produces: `LyricsState { open, song_id, status }`.
- Produces: `LyricsStatus::{Idle, Loading, Ready(StructuredLyrics), Empty, Unsupported, Unavailable}`.
- Produces: helper async `request_lyrics(client, daemon_state, client_state, song_id)`.

- [ ] **Step 1: tests fallidos de toggle**

Con RecordingClient que responda `DaemonResponse::Lyrics`, comprobar:

```rust
press(&mut app, key(KeyCode::Char('y'))).await;
assert!(app.client_state.read().await.lyrics.open);
assert!(client.sent().iter().any(|r| matches!(r,
    DaemonRequest::GetLyrics { song_id } if song_id == "song-1"
)));
```

Segundo `y` cierra. Sin canción abre panel en Idle y no envía request. Con quit/delete prompt activo, `y` conserva el comportamiento del modal.

- [ ] **Step 2: definir estado TUI**

```rust
#[derive(Debug, Clone, Default)]
pub struct LyricsState {
    pub open: bool,
    pub song_id: Option<String>,
    pub status: LyricsStatus,
}

#[derive(Debug, Clone, Default)]
pub enum LyricsStatus {
    #[default]
    Idle,
    Loading,
    Ready(StructuredLyrics),
    Empty,
    Unsupported,
    Unavailable,
}
```

- [ ] **Step 3: implementar petición no bloqueante**

Crear helper que marque Loading, haga `tokio::spawn`, pida `GetLyrics` y, al volver, aplique solo si `lyrics.open && lyrics.song_id.as_deref() == Some(response_song_id)`.

El match de resultado asigna el `LyricsStatus` equivalente. No retener locks durante `.await`.

- [ ] **Step 4: cablear `y` después de overlays/text inputs y antes de page handlers**

```rust
(KeyCode::Char('y'), KeyModifiers::NONE) => {
    let next_open = !state.client.lyrics.open;
    state.client.lyrics.open = next_open;
    if !next_open {
        return Ok(());
    }
    let song_id = state.daemon.now_playing.song.as_ref().map(|s| s.id.clone());
    // drop locks y lanzar helper si hay id
}
```

- [ ] **Step 5: refrescar al cambiar canción**

Ampliar `apply_now_playing_changed` para recibir `client_state`. Antes de reemplazar el estado, obtener el nuevo ID. Tras actualizar daemon y cover art:

- si `np.song` es `None`, dejar `lyrics.song_id = None` y `Idle`;
- si panel abierto y el ID difiere, llamar al helper asíncrono.

Actualizar la llamada en `apply_event` y tests de variantes.

- [ ] **Step 6: comprobar stale response**

En test, hacer que una respuesta para `old` llegue después de cambiar `lyrics.song_id` a `new`; afirmar que el estado no cambia a Ready(old).

- [ ] **Step 7: tests y commit**

Run: `cargo test --test lyrics_input --test apply_event_variants --test input_global_keys --test input_quit_confirm`
Expected: PASS.

```bash
git add src/app tests/lyrics_input.rs tests/apply_event_variants.rs
git commit -m "feat(lyrics): add asynchronous panel state" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Widget sincronizado y layout seguro

**Files:**
- Create: `src/ui/widget_lyrics.rs`
- Modify: `src/ui/mod.rs:1-19`
- Modify: `src/ui/layout.rs:1-153`
- Create: `tests/lyrics_render.rs`
- Modify: `tests/ui_layout_branches.rs`

**Interfaces:**
- Produces: `current_line_index(lines: &[LyricLine], position_ms: u64, offset_ms: i64) -> Option<usize>`.
- Produces: `LyricsWidget::new(&LyricsState, position_seconds, colors)`.
- Añade `LayoutAreas::lyrics: Option<Rect>` si los tests/mouse hit-testing necesitan observar el área; si no, mantenerla local a draw.

- [ ] **Step 1: tests fallidos de índice**

```rust
assert_eq!(current_line_index(&lines, 500, 0), None);
assert_eq!(current_line_index(&lines, 1000, 0), Some(0));
assert_eq!(current_line_index(&lines, 2600, 0), Some(1));
assert_eq!(current_line_index(&lines, 900, -200), Some(0));
```

El tiempo efectivo es `start + offset`; usar aritmética firmada y saturar a cero.

- [ ] **Step 2: implementar función pura**

```rust
pub fn current_line_index(
    lines: &[LyricLine],
    position_ms: u64,
    offset_ms: i64,
) -> Option<usize> {
    lines.iter().enumerate().rev().find_map(|(i, line)| {
        let start = line.start?;
        let effective = i128::from(start) + i128::from(offset_ms);
        let effective = u128::try_from(effective.max(0)).unwrap_or(0);
        (effective <= u128::from(position_ms)).then_some(i)
    })
}
```

- [ ] **Step 3: tests de render por estado**

Con `tests/common/render.rs`, comprobar texto para Idle/Loading/Empty/Unsupported/Unavailable y que una letra Ready contiene líneas. Con `render_styled`, afirmar que la fila activa usa `colors.accent` o BOLD.

- [ ] **Step 4: implementar widget**

Renderizar `Block` con título `Lyrics (y: close)`. Para synced:

1. calcular índice actual;
2. calcular `visible_rows = area.height.saturating_sub(2)`;
3. `start = current.saturating_sub(visible_rows / 2)`;
4. construir solo el rango visible;
5. aplicar estilo destacado a la línea actual.

Para unsynced, empezar en 0. Para estados usar exactamente los mensajes del spec.

- [ ] **Step 5: integrar layout**

Calcular `lyrics_h` solo si `state.client.lyrics.open`:

```rust
let fixed = 1 + now_playing_h + 2;
let available = area.height.saturating_sub(fixed);
let lyrics_h = available.saturating_sub(content_min).min(10);
```

Solo renderizar banda si `lyrics_h >= 3`; si no, mantener panel abierto pero omitir banda hasta que crezca la terminal. Insertar la banda entre content y now-playing tanto con cava como sin cava.

- [ ] **Step 6: tests de terminal baja/cava/art**

Probar 120x40 y 80x20 con panel; afirmar que no hay panic, `now_playing.height` conserva el valor y el texto de footer sigue presente. Activar cava y cover art en casos separados.

- [ ] **Step 7: tests y commit**

Run: `cargo test --test lyrics_render --test ui_layout_branches --test ui_smoke --test form_pages_dont_blank`
Expected: PASS.

```bash
git add src/ui src/app/state.rs tests/lyrics_render.rs tests/ui_layout_branches.rs
git commit -m "feat(lyrics): render synced lyrics panel" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: README, validación integral, instalación y entrega

**Files:**
- Modify: `src/ui/footer.rs:57-125`
- Modify: `README.md`
- Modify: `/home/tabs/Biblioteca/Notas/Notas/Proyectos futuros/ferrosonic-letras-y-playlists.md` después de desplegar.

**Interfaces:**
- Consumes: `y`, `A`, `a` finales.
- Produces: documentación y artefactos de entrega.

- [ ] **Step 1: ayuda y README**

Añadir `y: Lyrics` al footer global. Actualizar el bloque de fork para enumerar quick-play, seeking, volume, smart playlist add y lyrics panel. Mantener enlace visible a `https://github.com/jaidaken/ferrosonic` e invitar a patrocinar/apoyar al creador original mediante los enlaces que el upstream publique; no inventar un enlace de donación.

- [ ] **Step 2: formateo y pruebas completas**

Run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Expected: todos exit 0. Si clippy con `--all-features` activa una combinación no soportada por el repo, usar el comando documentado existente y registrar la desviación en el PR.

- [ ] **Step 3: validación end-to-end local**

Invocar la skill `verify` y conducir el binario real contra Navidrome:

1. canción con lyrics synced: `y` muestra panel y el highlight avanza;
2. canción sin letras: `No lyrics`;
3. cambiar canción rápidamente: no aparecen letras de la anterior;
4. cero playlists: `A`, nombre, playlist con una canción;
5. segunda canción: `A` cae en la misma playlist;
6. `a`: `+ New playlist…` y playlists existentes;
7. borrar el último destino en Navidrome: `A` abre picker;
8. refrescar Symfonium y confirmar la playlist.

- [ ] **Step 4: instalar el binario validado**

```bash
install -m755 target/release/ferrosonic ~/.local/bin/ferrosonic
```

Verificar: `~/.local/bin/ferrosonic --version` exit 0.

- [ ] **Step 5: actualizar nota del vault**

Marcar checklist, teclas finales, commits y resultados runtime en `ferrosonic-letras-y-playlists.md`; actualizar también la nota `ferrosonic-fork-quick-play` mediante `guardar-en-obsidian`.

- [ ] **Step 6: commit final de docs**

```bash
git add README.md src/ui/footer.rs
git commit -m "docs: describe fork lyrics and playlist features" -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

- [ ] **Step 7: push y draft PR**

```bash
git push -u origin feature/lyrics-and-smart-playlists
gh pr create --draft \
  --base feature/quick-play-search \
  --head feature/lyrics-and-smart-playlists \
  --title "feat: add lyrics panel and smarter playlist flows" \
  --body-file "$CLAUDE_JOB_DIR/tmp/ferrosonic-pr.md"
```

El cuerpo debe resumir comportamiento, pruebas, validación runtime y límites del MVP, y terminar con:

```markdown
🤖 Generated with [Claude Code](https://claude.com/claude-code)
```
