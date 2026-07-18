# Ferrosonic: MVP de letras y playlists

Fecha: 2026-07-19
Estado: aprobado
Base: `feature/quick-play-search`
Rama: `feature/lyrics-and-smart-playlists`

## Objetivo

Añadir dos mejoras al fork `JaTabs/ferrosonic`: creación y quick-add de playlists persistidas en Navidrome, y un panel inferior de letras servido por OpenSubsonic `getLyricsBySongId`. El resultado será un MVP pequeño, ajustable después de probarlo.

## Playlists

- `A`: usa la canción seleccionada en el panel enfocado o, si no existe, la canción en reproducción.
- Sin playlists: abre un prompt y crea una playlist con esa canción.
- Con `LastPlaylistId` válido: añade directamente.
- Sin último ID o con uno borrado: abre el picker.
- `a`: picker existente con `+ New playlist…` como primera fila.
- El daemon persiste `LastPlaylistId` solo después de éxito confirmado.
- `SubsonicClient::create_playlist` devuelve la `Playlist` creada para obtener su ID real sin inferirlo por nombre.
- Las notificaciones de éxito se muestran después de confirmar la operación.

## Letras

- `y`: abre o cierra un panel inferior; los modales existentes conservan prioridad.
- Al abrir o cambiar de canción, la TUI solicita letras asíncronamente.
- `GetLyrics { song_id }` devuelve `Lyrics { song_id, result }`; la TUI descarta respuestas que ya no correspondan a la canción actual.
- Caché en memoria del daemon por `song_id`, sin TTL ni persistencia.
- Se prefiere la primera entrada sincronizada; si no existe, la primera disponible.
- Letras sincronizadas: línea activa resaltada y centrada usando posición y offset.
- Letras no sincronizadas: texto desde el principio; sin scroll manual en el MVP.
- Estados explícitos: loading, no lyrics, unsupported y unavailable.
- Al detener la reproducción se limpia el contenido.

## Configuración y estado

- Añadir `LastPlaylistId` a `Config`, `ConfigOnDisk`, defaults y `KNOWN_CONFIG_KEYS`.
- No persistir el estado abierto/cerrado del panel de letras.
- Mantener el estado visual de letras en `ClientState`, no en `DaemonState` serializado.
- El daemon actualiza y guarda `LastPlaylistId` tras create/add y emite `ConfigChanged`.

## Layout

- Nueva banda de letras entre contenido y now-playing.
- Altura objetivo de 10 filas, reducida en terminales bajas antes de perjudicar header, footer o now-playing.
- Compatible con cava y cover art.
- Nuevo `widget_lyrics.rs` para estados, texto y highlight.

## Fuera de alcance

Karaoke por sílabas, traducciones, LRCLIB directo, tags locales, caché en disco, altura configurable, persistencia del panel, scroll manual, multi-select, smart playlists, cambios en Symfonium y refactors generales.

## Errores

- Nombre vacío: no crear.
- Sin canción: `Nothing to add`.
- Playlist obsoleta: limpiar ID y abrir picker.
- Fallo de create/add: no cambiar `LastPlaylistId`.
- Sin extensión: `Lyrics not supported by server`.
- Respuesta vacía: `No lyrics`.
- Red, timeout o parseo: `Lyrics unavailable` y detalle solo en tracing.

## Pruebas

- Round-trip de `LastPlaylistId` y claves conocidas.
- Resolución de canción objetivo por página/foco y fallback.
- Quick-add con cero playlists, ID válido e ID obsoleto.
- Picker con fila de creación.
- Parseo synced/unsynced/empty y servidor sin extensión.
- Caché: dos consultas del mismo ID hacen una llamada HTTP.
- Create devuelve ID real; add/create exitoso persiste; fallo no persiste.
- Cálculo de línea actual con offset y límites.
- Descarte de respuesta obsoleta.
- Toggle y layout con cava/cover art y terminal baja.

## README y entrega

Actualizar `README.md` para presentar el repositorio como un fork con mejoras surgidas del uso real, enumerar las funciones propias, acreditar y enlazar `jaidaken/ferrosonic`, e invitar a apoyar al creador original. Publicar la rama en `JaTabs/ferrosonic` y abrir un draft PR.

## Validación

Ejecutar `cargo fmt --check`, `cargo test`, clippy compatible con el repo y `cargo build --release`. Después validar contra Navidrome la creación, quick-add, picker, visibilidad en Symfonium, letras sincronizadas y estados sin letras/error. Instalar en `~/.local/bin/ferrosonic` únicamente tras superar la validación.

## Criterio de aceptación

Una canción seleccionada o en reproducción puede crear y alimentar playlists persistidas mediante `A`/`a`; `y` muestra letras del servidor en un panel inferior con seguimiento de línea cuando están sincronizadas; no hay regresiones en navegación, cava, cover art, reproducción ni configuración.
