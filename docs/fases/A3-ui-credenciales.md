# A3 — UI de ajustes y credenciales

> **Para el agente:** documento autosuficiente. Refleja el código **después de A2 + A2b**
> (ver `docs/01-roadmap.md`). Si algo no coincide, para y anótalo en `docs/05-bitacora.md`.
> **Prerrequisito:** A2b cerrada (en particular `SyncReport::warnings`, ver abajo).

## Objetivo

Que un usuario configure su Radicale **sin tocar archivos ni variables de entorno**:
URL, usuario, contraseña, probar conexión, elegir calendario, y ver el estado de la
sincronización. La contraseña vive en el keyring del sistema, nunca en `config.ron`.

## Fuera de alcance

- Crear/renombrar/borrar calendarios en el servidor.
- Varios calendarios por conexión (una conexión = un calendario; si quieres otro, otra conexión).
- Flatpak/empaquetado, backup previo → **Fase E**.
- Descubrimiento automático (`.well-known`, autoconfiguración).
- Tocar Google/Microsoft o la pantalla de cuentas COSMIC.

## Lo que dejó A2 (verificado leyendo el código)

| Pieza | Estado real |
|---|---|
| `RadicaleConnection` | En `src/config.rs`; `AppConfig::radicale_connections` (`#[serde(default)]`) |
| Contraseña | `config::radicale_password(&conn)` **síncrona**, lee `PROPER_TODO_RADICALE_PASSWORD`. Marcada `TODO(A3)` |
| Motor | `sync::run_radicale_sync(&store, &[RadicaleConnection]) -> SyncReport`, invocado desde `AccountsAction::SyncNow` (`src/features/accounts/update.rs`) |
| Estado | `AppModel::sync_status: Option<SyncStatus>`; solo se pinta en `accounts/views.rs::page` |
| Limpieza | `purge_orphaned_accounts` ignora a propósito las listas Radicale |

### Cuatro trampas que el brief anterior no cubría (comprobadas)

1. **La pantalla de cuentas es inalcanzable.** `accounts/views.rs::_settings_entry` (con `_`) no se
   usa en ningún sitio; nada abre `ContextPage::Accounts`. Hay que añadir un punto de entrada real
   en la página de ajustes (`src/features/settings/views.rs`).
2. **`accounts::views::page` sale antes de tiempo** si no hay demonio de cuentas o no hay cuentas
   (`return widget::scrollable(section)` antes de pintar el botón de sync). La UI de Radicale **no
   puede colgar de esa página**: va en una página propia.
3. **La sincronización automática solo existe con demonio de cuentas** (`app.rs`, `subscription()`:
   `if self.accounts.is_some() { … if !remote_accounts.is_empty() { every(300s) } }`). Un usuario solo
   con Radicale **nunca sincroniza solo**. Hay que sacar el tick del `if` y activarlo también cuando
   `!config.radicale_connections.is_empty()`.
4. **`SyncNow` solo existe en esa página rota** y en el tick. Necesita un botón accesible.

## Paso 1 — Keyring (`src/shared/secrets.rs`)

Crate: **`oo7`** (Secret Service en Rust, async, funciona también vía portal en Flatpak).
Alternativa si el árbol de dependencias choca con `zbus 5.19` (hoy en `Cargo.lock`): `keyring`
con `async-secret-service` + `tokio`. Prueba con `cargo add oo7 --features tokio` y `cargo build`
**antes** de escribir código; si no compila en ~15 min, pasa a la alternativa y anótalo.

API mínima (async):

```rust
pub async fn save_password(connection_id: Uuid, password: &str) -> Result<(), SecretError>;
pub async fn load_password(connection_id: Uuid) -> Result<Option<String>, SecretError>;
pub async fn delete_password(connection_id: Uuid) -> Result<(), SecretError>;
```

- Atributos del item: `application=dev.edfloreshz.Tasks`, `kind=caldav-password`,
  `connection-id=<uuid>`. Etiqueta legible: `"ProPer ToDo — <nombre de la conexión>"`.
- Si no hay Secret Service: error claro y **no** guardar la conexión (no hay fallback a texto plano).
- Nunca `Debug`/log de la contraseña; el borrador de formulario no deriva `Debug`
  (impl manual que la oculte).

Cambio en el motor: `run_radicale_sync` pasa a resolver la contraseña así —
1. `PROPER_TODO_RADICALE_PASSWORD` si está definida (solo para tests/CI; documentarlo),
2. si no, `secrets::load_password(conn.id)`.

Elimina `config::radicale_password` y su `TODO(A3)`. Si falta contraseña, aviso en
`report.errors` con texto accionable ("falta la contraseña de «X»: edítala en Ajustes").

## Paso 2 — Página «Calendarios CalDAV»

Nueva feature `src/features/caldav/{mod,action,update,views}.rs`, con la misma forma que
`features/accounts`. Nuevo `ContextPage::Caldav` en `app.rs` y entrada en Ajustes.

### Estado (en `AppModel`)

```rust
pub struct CaldavForm {           // sin #[derive(Debug)]
    editing: Option<Uuid>,        // None = conexión nueva
    name: String, base_url: String, username: String, password: String,
    test: TestState,
}
pub enum TestState {
    Idle, Testing,
    Ok { calendars: Vec<CalendarChoice>, selected: usize },
    Failed(String),
}
pub struct CalendarChoice { href: String, name: String }
```

### Mensajes (`CaldavAction`)

`Edit(Option<Uuid>)`, `FieldChanged(Field, String)`, `TestConnection`,
`TestFinished(Result<Vec<CalendarChoice>, String>)`, `SelectCalendar(usize)`, `Save`,
`SaveFinished(Result<RadicaleConnection, String>)`, `Delete(Uuid)`, `DeleteFinished(Uuid)`.

### Comportamiento

- **Probar conexión:** `CalDavClient::new(url, user, pass)` (ya trae timeouts) →
  `list_calendars()` → filtrar `supports_vtodo`. Mapear errores a mensajes humanos:
  `Unauthorized` → "usuario o contraseña incorrectos"; `Http` con `is_timeout()` → "el servidor no
  responde"; `Url` → "URL no válida"; `Status` → código + sugerencia. Sin calendarios con VTODO →
  mensaje propio, no un desplegable vacío.
- **Guardar** (solo habilitado con `TestState::Ok`): primero `secrets::save_password`; si falla, no
  se toca la config. Luego escribir `radicale_connections` con el mismo mecanismo que usa
  `SetLocallyEnabled` para `disabled_accounts` (busca `config_set`/handler en `accounts/update.rs`;
  no inventes otro). `calendar_href` = el elegido (siempre `Some`: A3 elimina el "primero con VTODO").
- **Editar:** precarga nombre/URL/usuario; la contraseña **no** se precarga (campo vacío = conservar la
  guardada; no vacío = reemplazar). Cambiar URL/usuario/calendario obliga a "Probar" otra vez y, al
  guardar, **pone `sync_token = None`** en la lista local enlazada (re-escaneo completo).
- **Eliminar:** diálogo de confirmación (reutiliza el patrón de diálogos existente). Acciones, en este
  orden: quitar de config → `secrets::delete_password` (error no fatal, se loguea) → mandar a la
  papelera la lista local y sus tareas (`store.trash().trash_list(list.id)` para la lista con
  `TaskSource::Radicale { account_id == conn.id }`). **No** se toca el servidor.
- Campo contraseña con `text_input::secure_input` de libcosmic (comprueba la firma en la versión del
  `Cargo.lock`, no la de la documentación).
- i18n: claves nuevas en `i18n/en/*.ftl` y `i18n/es/*.ftl`; el resto de idiomas puede caer a `en`
  (no los rellenes a mano).

## Paso 3 — Sincronización accesible y estado visible

- Mover el indicador (`sync_status`) y el botón **Sincronizar** a un componente reutilizable
  (`features/accounts/views.rs::sync_row(app)`), y mostrarlo **tanto** en la página CalDAV como en
  la de cuentas (que sigue igual).
- Arreglar la suscripción periódica (trampa 3). El tick es de 300 s; documenta que es fijo.
- Mostrar **avisos separados de errores** usando `SyncReport::warnings` (A2b): los conflictos
  resueltos van como aviso informativo ("se conservó la versión remota de «X»"), no como error rojo.
  `SyncStatus::Idle` gana `warnings: usize`; hoy solo tiene `had_errors`.
- Mostrar el último error de sync (texto de `report.errors[0]`, truncado) bajo el botón, para que el
  usuario no tenga que mirar logs.
- Los errores de red en un tick automático **no** abren toasts (ruido cada 5 min); el toast solo
  para sync manual.

## Paso 4 — Tests

Inline (no hay `src/lib.rs`):

1. `friendly_error` mapea cada `CalDavError` al mensaje esperado (función pura, sin red).
2. Validación del formulario: URL vacía/ inválida, nombre vacío, `Save` bloqueado sin test OK.
3. Round-trip de config con una conexión con `calendar_href: Some(..)` (ya existe; amplía para
   comprobar que **no** aparece ningún campo `password` en el RON serializado).
4. Con `PROPER_TODO_RADICALE_PASSWORD` definida, el motor la usa sin tocar el keyring.
5. `#[ignore]`: ciclo `save → load → delete` contra el Secret Service real (en esta máquina
   `gnome-keyring` está activo). Limpia siempre (guard `Drop`).
6. La decisión "¿arranca el tick periódico?" extraída a función pura
   (`fn periodic_sync_enabled(has_daemon_accounts: bool, caldav_connections: usize) -> bool`) y testeada.

## Criterios de aceptación

1. Desde cero (config sin conexiones, **sin** variable de entorno, **sin** demonio de cuentas),
   un usuario añade su Radicale desde Ajustes, prueba, elige calendario, guarda, y sus tareas aparecen.
2. `grep -r "<contraseña>" ~/.config/cosmic/dev.edfloreshz.Tasks/` no encuentra nada.
3. Con la app abierta y sin pulsar nada, un cambio hecho por fuera llega en ≤ 5 min.
4. Contraseña errónea → mensaje comprensible en la propia pantalla, conexión **no** guardada.
5. Servidor apagado → "Probar" y "Sincronizar" terminan en ≤ 30 s con error legible (sin colgarse).
6. Eliminar una conexión deja las tareas en la papelera y la entrada desaparece del keyring
   (`secret-tool search kind caldav-password`).
7. `cargo build` sin warnings nuevos; `cargo test --bin tasks` y `cd caldav-client && cargo test` verdes.

## Trampas

- Todo lo del keyring es **async y puede tardar** (desbloqueo de gnome-keyring): nunca desde `view()`
  ni bloqueando `update()`; siempre vía `cosmic::task::future`.
- `AppConfig` sigue siendo `CosmicConfigEntry` versionado: **no** añadas campos a
  `RadicaleConnection` sin `#[serde(default)]`.
- No reescribas `accounts/views.rs::page`; solo extrae `sync_row`.
- No migres la variable de entorno a la config: es solo para tests.
- El e2e (`radicale_engine_end_to_end`) debe seguir verde: lo ejecuta A2b con el calendario de pruebas.

## Checkpoint de sesión

Orden de corte si el contexto se agota: **Paso 1 (keyring + motor)** con commit → **Paso 3
(tick + sync_row)** con commit → Paso 2 (formulario) → Paso 4. Cada corte compila y deja tests verdes.

## Comandos

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build
cargo test --bin tasks
cd caldav-client && cargo test
secret-tool search kind caldav-password      # verificar el keyring a mano
```
