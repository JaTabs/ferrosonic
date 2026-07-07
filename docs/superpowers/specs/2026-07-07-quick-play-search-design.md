# Quick-play search — design

**Fecha:** 2026-07-07 · **Repo:** fork de jaidaken/ferrosonic (master, post-v0.6.1)

## Problema

Hoy, para escuchar una canción concreta: `/` → escribir → Enter (solo cierra el
input) → flechas por el árbol artista → álbum → canción → Enter. El usuario
quiere: escribir el nombre, Enter, y que suene; y al acabar, canciones
aleatorias indefinidamente.

## Lo que ya existe (no se toca)

- **Aleatorio infinito:** `auto_continue` (Settings → Auto-continue, ya en
  `true` en la config del usuario). Al agotarse la cola, el daemon llama a
  `extend_with_random_and_play()` (`daemon/core.rs:497`) y encola canciones
  aleatorias sin repetir las ya reproducidas; se re-dispara cada vez que la
  cola vuelve a agotarse (`daemon/playback_ops.rs:142,171`). Cubre el
  requisito 2 sin cambios.
- **Búsqueda del servidor:** `DaemonRequest::Search` → `search3` de Subsonic.
- **Reproducir una canción de resultados:** `EnqueueSongs { Replace, play_from: 0 }`.

## Cambios (los 3 son mínimos y locales)

### 1. Enter en el filtro reproduce la mejor canción
`handle_library_filter_key` (`app/input_library.rs`), rama `KeyCode::Enter`:

1. Filtro vacío → comportamiento actual (cerrar input).
2. Si no: petición **fresca** `Search { query, artist_count: 0, album_count: 0,
   song_count: 50 }` esperada con `await` (evita la carrera con la búsqueda
   asíncrona en vuelo; contra Navidrome en LAN son milisegundos).
3. Elegir canción: primero título igual (case-insensitive) a la query; si no,
   la primera cuyo título **contenga** la query (misma regla de "match propio"
   que usa el árbol de resultados). Sin match propio no se reproduce nada:
   escribir un nombre de artista/álbum no debe disparar una canción
   impredecible (search3 también devuelve canciones matcheadas vía artista).
4. Con canción: `exit_search()`, notificación "Playing: título", y
   `EnqueueSongs { songs: [canción], mode: Replace { play_from: 0 } }`.
   Al terminar, auto-continue encadena el aleatorio infinito.
5. Sin match propio de canción: comportamiento antiguo (cerrar input y dejar
   los resultados navegables, guardando los frescos en `search_results`) +
   notificación informativa.

### 2. Tab en el filtro = el Enter antiguo
Cerrar el input conservando los resultados para navegar con flechas
(hoy Tab no hace nada mientras se escribe → no rompe nada). Es la vía de
escape para quien busca un artista o álbum.

### 3. Clic en la barra de título del panel abre la búsqueda
`handle_library_click` (`app/mouse_library.rs`): clic en la fila del borde
superior del panel izquierdo (`y == left.y`) → `filter_active = true`
(equivalente a pulsar `/`). Cumple el "dar clic a una parte de la interfaz
y escribir ahí".

### UI
Título del panel en modo búsqueda pasa a incluir el hint:
`Search (query) · Enter: play · Tab: browse`.

## Alternativas descartadas

- **Overlay/popup global de búsqueda** (estilo command palette): mejor UX en
  abstracto, pero mucho más código nuevo (estado, render, enrutado de input y
  ratón) en un codebase ajeno → más riesgo de romper algo, contra el requisito
  explícito "que no se rompa nada más". La caja de búsqueda existente ya es
  visible y clicable con el cambio 3.
- **Reordenar el árbol de resultados (canciones primero):** seguiría exigiendo
  flechas + Enter; no cumple "escribir y Enter".

## Errores y casos borde

- Enter con la búsqueda fresca fallida (daemon caído / servidor fuera): se
  notifica error y se cae al comportamiento antiguo; nunca panic.
- Enter antes de que llegue la búsqueda asíncrona en vuelo: irrelevante, la
  petición fresca es la fuente de verdad (`search_gen` sigue protegiendo al
  hilo asíncrono).
- Escritura rápida + Enter: ídem.

## Tests (patrón TestDaemon + expect_search3 existente)

1. Enter con match exacto → cola = [esa canción], reproducción arranca, filtro
   cerrado y vacío.
2. Enter con match parcial (sin exacto) → suena la primera con match de título.
3. Enter sin canciones → filtro inactivo, resultados conservados, cola intacta.
4. Tab durante el filtro → input cerrado, filtro y resultados conservados.
5. Clic en `y == left.y` de la página Library → `filter_active == true`.
