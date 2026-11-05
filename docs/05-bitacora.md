# Bitácora

Registro cronológico. Se añade una entrada al final de cada sesión, antes del commit.
Formato: objetivo → hecho → decisiones → problemas → siguiente paso.

---

## Sesión 1 — 2026-11-04

**Objetivo:** evaluar `cosmic-ext-tasks` como base y arrancar el cliente CalDAV.

### Hecho

- Clonado el fork `KaesarImperator/ProPer-ToDo`; `origin` y `upstream` configurados. Fork verificado idéntico a upstream.
- Analizadas las 14 issues abiertas de upstream → `04-issues-upstream.md`.
- Instalado Rust vía rustup (`cargo 1.99.1`) sin sudo.
- Creado el crate `caldav-client/` con cuatro módulos:
  - `ical.rs` — RFC 5545: unfold, propiedades con parámetros entrecomillados, escape/unescape, folding a 75 octetos.
  - `vtodo.rs` — struct `Vtodo`, parseo/serialización, conservación de propiedades desconocidas en `extra`.
  - `xml.rs` — cuerpos PROPFIND/REPORT y parseo de `multistatus`, incluyendo borrados de `sync-collection`.
  - `client.rs` — descubrimiento, listado de calendarios, CRUD con ETag, `sync-collection`.
- 12 tests unitarios en verde (`cargo test`).
- Radicale 3.9 levantado en Docker y **4 tests de integración en verde**:
  descubrimiento, ciclo de vida completo (crear → listar → leer → actualizar con ETag → 412 con ETag obsoleto → borrar idempotente) y sync incremental.
- **Verificado además contra el servidor real del usuario** (`http://192.168.0.35:5232/cesar/…`, calendario `R-Profesional`): 4/4 en verde, sin dejar tareas residuales.
- Descubrimiento desde URL raíz verificado: cae a `/{usuario}/` cuando el servidor no resuelve `current-user-principal`.
- `caldav-client/README.md` con ejemplo de uso y `examples/probe.rs` para sondeo manual.
- Documentación de proyecto completa (`AGENTS.md` + `docs/`).
- Push a GitHub funcionando; commit `7d11e41` publicado.

### Decisiones

1. **Cliente propio** en vez de una crate externa: compila en ~2 s y se testea sin libcosmic.
2. **Parser iCalendar propio** en vez de la crate `ical`: evita dependencia poco mantenida y permite preservar propiedades desconocidas.
3. **Orden de fases B → A → C**, no A → B → C: B es prerrequisito técnico de A.
4. **Fork, no PR**: upstream lleva 2 años sin CalDAV y su foco está en Google/Microsoft.
5. **ETag + `sync-collection`** en vez del LWW por timestamp del motor actual.
6. **Fallback de principal**: si `current-user-principal` no resuelve, usar `/{usuario}/`. Necesario para Radicale con `auth.type = none`.

### Problemas

- Push bloqueado temporalmente por errores `Internal Server Error` / `fatal error in commit_refs` de GitHub; resuelto reintentando (era transitorio).
- Faltan 7 paquetes `-dev` para compilar libcosmic → **resuelto**: el usuario los instaló.

### Siguiente paso

**A1 — Fontanería del provider Radicale.** La fase A se partió en A1/A2/A3 para
que cada sesión cierre limpia. Brief autosuficiente en `docs/fases/A1-fontaneria.md`:
basta con decir "Arranca A1".

### Notas de entorno (costaron tiempo, documentadas en `03-radicale.md`)

- La imagen `tomsquest/docker-radicale` ignora `RADICALE_RIGHTS_TYPE`: hay que montar `config` + `rights` propios.
- `permissions: RW` no basta: las minúsculas (`RrWw`) son las que autorizan escribir ítems.
- `MKCOL` sin cuerpo crea una colección genérica, no un calendario; hay que enviar el XML con `resourcetype: calendar`.
- `MKCALENDAR` devuelve 403 en Radicale 3.9.

### Siguiente paso

Fase A: integración en la app (requiere resolver antes el sudo para libcosmic).

---

## Sesión 2 — 2026-11-04 (A1: fontanería del provider)

**Objetivo:** dejar la app preparada para hablar CalDAV (dependencia, origen,
credenciales y provider) sin tocar UI ni motor de sincronización.

### Hecho

- `Cargo.toml`: `caldav-client` entra como dependencia `path`.
- `TaskSource::Radicale { account_id, remote_id }` en `src/shared/store/source.rs`;
  `remote_id` es el **href** del recurso, no el UID. Añadido a los dos `match`
  (`account_id()`, `remote_id()`) y a los exhaustivos de `nav.rs` (icono de nube)
  y `sync.rs::make_source` (`"radicale"`).
- `ProviderCredentials::{Bearer, Basic}` en `provider.rs`; las 9 firmas del trait
  pasan de `token: &str` a `credentials: &ProviderCredentials`. Google y
  Microsoft devuelven `UnsupportedCredentials` si no reciben un Bearer; su
  comportamiento con Bearer no cambia.
- `ProviderError`: nuevas variantes `UnsupportedCredentials`, `Unsupported` y
  `CalDav(#[from] caldav_client::CalDavError)`.
- `RemoteTask`/`RemoteTaskDraft`: `priority`, `tags`, `percent_complete`,
  `parent_remote_id`, `recurrence`, con `Default` derivado. Google/Microsoft los
  dejan por defecto; los dos literales de `sync.rs` usan `..Default::default()`,
  así que el motor no cambia.
- `src/shared/providers/radicale.rs`: `RadicaleProvider` sobre `CalDavClient`
  (construido por conexión, no por llamada), con el mapeo `Vtodo` ↔ `RemoteTask`
  del brief. `create_list`/`update_list`/`delete_list` → `Unsupported`.
- `provider_for("radicale")` → `None` con el TODO de A2 (no son cuentas de COSMIC).
- **Tests:** 9 en la app (5 previos + 4 nuevos del mapeo) y 12 en `caldav-client`,
  todos en verde. `cargo build` compila; queda **un** warning, el preexistente de
  `src/features/trash/trash.rs:55` (`RequestEmptyTrash`), ajeno a esta fase.

### Decisiones

1. **`ProviderError::CalDav`** además de las dos variantes que pedía el brief: sin
   ella habría que mapear errores de iCalendar/XML a `Api { status: 0 }` y se
   perdería el motivo real. Es un añadido mínimo, no un cambio de alcance.
2. **`#[allow(dead_code)]`** en `ProviderCredentials::Basic` y en `RadicaleProvider`:
   en A1 nada los construye todavía; evita warnings nuevos sin fingir uso.
3. **`PRIORITY:0` → `None`** al leer (RFC 5545 lo define como "sin prioridad") y
   se omite al escribir.
4. **`apply_draft` mantiene `COMPLETED` coherente**: lo sella al completar y lo
   borra al reabrir, para no dejar una tarea "no completada" con fecha de fin.
5. **`delete_task` idempotente**: un `404` en el `GET` previo del ETag se trata
   como éxito, igual que `CalDavClient::delete_vtodo`.

### Problemas y limitaciones conocidas

- `update_task` hace GET+PUT (el trait no transporta ETag) y `put_vtodo` deriva el
  href del UID: un VTODO creado por otro cliente con otro nombre de fichero se
  escribiría en un href nuevo. A2 pasa el href y el ETag en el draft.
- Sin tests de red en esta fase: el mapeo es puro y se prueba sin servidor; la
  integración real ya está cubierta por `caldav-client/tests/radicale.rs`.

### Siguiente paso

**A2 — Motor de sync:** registrar las conexiones Radicale fuera del framework de
cuentas, pull con `sync-token`, push con `If-Match` y ETag en el draft.

---

## Sesión 3 — 2026-11-05 (A2: motor de sync)

**Objetivo:** que la app sincronice de verdad con Radicale: pull incremental,
push con ETag, conflictos y borrados.

### Hecho

- `RadicaleConnection` + `AppConfig::radicale_connections` (`#[serde(default)]`;
  test con un `config.ron` anterior). Contraseña solo por
  `PROPER_TODO_RADICALE_PASSWORD` (TODO A3: keyring).
- `List::sync_token`; `Task::remote_etag`; `etag` en `RemoteTask`/`RemoteTaskDraft`.
- `caldav-client`: `put_vtodo_at`.
- Trait: `sync_changes`, `get_task` (y `current_sync_token`) con defaults para
  Google/Microsoft. `RadicaleProvider` los implementa (GET por entrada si no
  viene `calendar-data`).
- `sync.rs`: `SyncTarget`, `run_radicale_sync` (motor paralelo), `sync_list_tasks`
  con token, `full_scan_pull`, `push_task` con resolución de conflictos,
  `draft_from_task` completo, `apply_remote_to_local` ampliado, `remote_wins`.
- `accounts/update.rs`: `SyncNow` lanza ambos motores y fusiona los informes
  (ya no es no-op si no hay demonio de cuentas COSMIC).
- Test e2e `radicale_engine_end_to_end` (`--ignored`) contra el servidor real:
  pull completo, push, pasada sin cambios (0/0), edición externa, conflicto,
  push simple, borrado remoto → papelera. Verde.

### Decisiones

1. **`Task.dirty` pasa de `#[serde(skip)]` a `#[serde(default)]`** (archivo
   upstream, `task.rs`). El motor lee de disco, así que con `skip` ninguna
   edición local figuraba como pendiente y nunca se subía. Cambio justificado.
2. **`remote_newer` usa el ETag** cuando ambos lados lo tienen; los timestamps
   (1 s de precisión) perdían ediciones externas hechas en el mismo segundo.
3. En conflicto con `seen_remote`, el ETag guardado decide si el servidor avanzó;
   si sí, gana el remoto y se avisa en `report.errors`. El 412 se resuelve una vez.

### Problemas y limitaciones conocidas

- Las ejecuciones fallidas del e2e (antes de los dos arreglos) dejaron tareas de
  prueba en el calendario real (`Creada por la app`, `Sembrada por fuera`,
  `Editada por fuera…`). Pendiente de borrar a mano; el test pasa limpio ya.
- Los avisos de conflicto viajan en `report.errors`: se mezclan con errores reales.
- `caldav-client` no fija timeouts de red.
- Sin UI: la única forma de crear una conexión es editar `config.ron`.

### Siguiente paso

**A3 — UI de ajustes + keyring.**

---

## Sesión 4 — 2026-11-05 (replanificación, timeouts y brief de A3)

**Objetivo:** adaptar la planificación a la evaluación de la sesión 3, cerrar los timeouts y dejar escrito el brief de A3.

### Hecho

- `caldav-client`: `CONNECT_TIMEOUT` (10 s), `REQUEST_TIMEOUT` (30 s) y `CalDavClient::with_timeouts`;
  test `hung_server_times_out` (13 tests unitarios).
- Roadmap reordenado: **A2b** (endurecimiento) → A3 → C → **E** (dogfooding/empaquetado) → D.
  El roadmap pasa a ser la única fuente de verdad del estado; `AGENTS.md` y `00-estado-actual.md` enlazan en vez de duplicar.
- `docs/fases/A3-ui-credenciales.md` escrito tras leer el código.
- Calendario `d6be8909-…` designado para pruebas (decisión del usuario).

### Hallazgos al preparar A3 (no estaban en el brief original)

1. `accounts/views.rs::_settings_entry` no se usa: la pantalla de cuentas es inalcanzable desde la UI.
2. `accounts::views::page` retorna antes de pintar el botón de sync si no hay demonio/cuentas.
3. El tick de sync de 300 s solo se suscribe con demonio de cuentas **y** cuentas remotas: con solo Radicale nunca sincroniza solo.
4. El manifiesto Flatpak no tiene `--share=network` ni acceso al Secret Service.
5. `purge_orphaned_accounts` ya ignora las listas Radicale (comprobado).

### Siguiente paso

**A2b** (warnings separados, e2e sin residuos, medir latencia) y después **A3**.

---

## Sesión 5 — 2026-11-05 (A2b: endurecimiento)

**Objetivo:** cerrar las deudas de A2 antes de A3.

### Hecho

- `SyncReport::warnings`: los conflictos resueltos ya no son errores. `had_errors()` solo mira `errors`;
  `merge` combina ambos; `SyncNow` registra los avisos con `tracing::warn!`. Test nuevo
  (`warnings_do_not_count_as_errors_and_merge`). `SyncStatus` no cambia hasta A3.
- `caldav_client::testing::CalendarGuard`: foto de los recursos del calendario al crearse y borrado de todo lo
  nuevo en `Drop` (hilo propio con runtime, porque `Drop` puede ejecutarse dentro de tokio). Cubre lo creado por
  el test, por la app bajo prueba o por el "cliente ajeno". Módulo `#[doc(hidden)]` siempre compilado (sin feature).
- Usado en el e2e de la app (UID de siembra `proper-todo-e2e-…`) y en los tests `full_vtodo_lifecycle` e
  `incremental_sync_reports_changes` del crate.
- Latencia medida en el e2e con límite de 5 s: push ≈ 0,29 s, pull ≈ 0,28 s (red local).

### Verificación

- 20 tests de la app + 1 e2e, 13 del crate + 4 de integración, todos en verde.
- El calendario de pruebas tenía 32 recursos antes y 32 después de ejecutar ambas suites (son residuos de las
  sesiones anteriores, que borra el usuario).
- **No verificado:** el borrado en `Drop` ante un fallo forzado a mitad de test (se confía en el desenrollado de pánico).

### Siguiente paso

**A3** — `docs/fases/A3-ui-credenciales.md`.

---

## Plantilla para la próxima entrada

```md
## Sesión N — AAAA-MM-DD

**Objetivo:**

### Hecho

### Decisiones

### Problemas

### Siguiente paso
```
