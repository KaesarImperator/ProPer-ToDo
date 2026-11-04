# A2 — Motor de sincronización con Radicale

> **Para el agente:** documento autosuficiente. Refleja el código **después de A1**
> (commit `04b3c8c`). Si algo no coincide, para y anótalo en `docs/05-bitacora.md`.

## Objetivo

Que la app sincronice de verdad con Radicale: traer cambios, subir cambios,
detectar borrados y resolver conflictos sin perder datos.

## Fuera de alcance

- UI de ajustes y keyring → **A3**. En A2 la contraseña sale de una variable de entorno.
- Crear/renombrar/borrar calendarios desde la app → no está en el roadmap.
- `VALARM` / recordatorios, subtareas remotas completas, recurrencia avanzada.

## Lo que dejó A1 (verificado)

| Pieza | Estado real |
|---|---|
| `TaskSource::Radicale { account_id, remote_id }` | Existe; `remote_id` = **href** del `.ics` |
| `ProviderCredentials::{Bearer, Basic}` | Existe; `Basic` tiene `#[allow(dead_code)]` |
| `RemoteTask` / `RemoteTaskDraft` | Ya llevan `priority`, `tags`, `percent_complete`, `parent_remote_id`, `recurrence` |
| `RadicaleProvider` | Implementa `list_lists`, `list_tasks`, `create_task`, `update_task`, `delete_task` |
| `provider_for("radicale")` | Devuelve `None` con TODO de A2 |
| `List` | **No** tiene `sync_token` |
| `AppConfig` | **No** tiene conexiones Radicale |
| Tests | 9 en la app (inline, no hay `src/lib.rs`), 12 en `caldav-client` |

Deuda declarada por A1 que A2 debe cerrar:

1. `update_task` hace GET+PUT porque el trait no transporta ETag.
2. `put_vtodo` deriva el href del UID → un VTODO creado por otro cliente acabaría duplicado.
3. `sync.rs` construye los drafts con `..Default::default()`, así que **se pierden prioridad y etiquetas** en cada push.
4. `apply_remote_to_local` solo aplica título, notas, fecha y completado.

## Problema central: Radicale no es una cuenta de COSMIC

`run_sync` itera `accounts.list_enabled_accounts(Service::Tasks)` y usa
`accounts_core::models::Account`. Radicale no aparece ahí y **no se debe
fabricar un `Account` falso**: `Account` viene de otro crate y tiene campos
OAuth que no aplican.

Solución: un modelo de conexión propio y un motor de sync paralelo que
**reutiliza** la misma función `sync_list_tasks`.

## Paso 1 — Modelo de conexión

En `src/config.rs`:

```rust
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RadicaleConnection {
    pub id: Uuid,
    pub name: String,
    /// URL base con la que se construye el CalDavClient, p. ej.
    /// http://192.168.0.35:5232/cesar/
    pub base_url: String,
    pub username: String,
    /// href del calendario. Si es None, se descubre el primero con VTODO.
    pub calendar_href: Option<String>,
}
```

Y en `AppConfig`:

```rust
#[serde(default)]
pub radicale_connections: Vec<RadicaleConnection>,
```

La contraseña **no** va aquí. En A2, temporalmente:

```rust
/// TODO(A3): sustituir por el keyring del sistema.
pub fn radicale_password(_connection: &RadicaleConnection) -> Option<String> {
    std::env::var("PROPER_TODO_RADICALE_PASSWORD").ok()
}
```

## Paso 2 — Token de sync en la lista

En `src/features/lists/list.rs`, añadir a `List`:

```rust
/// Último `sync-token` conocido del calendario remoto. None = sync completo.
#[serde(default)]
pub sync_token: Option<String>,
```

Actualizar `Default` y `List::new` (`None`).

## Paso 3 — ETag y href explícito

### 3.1 `caldav-client`

Añadir a `src/client.rs`:

```rust
/// Escribe un VTODO en un href concreto (no derivado del UID).
pub async fn put_vtodo_at(
    &self,
    href: &str,
    vtodo: &Vtodo,
    etag: Option<&str>,
) -> Result<String>
```

y hacer que `put_vtodo` delegue:

```rust
pub async fn put_vtodo(&self, calendar_href: &str, vtodo: &Vtodo, etag: Option<&str>) -> Result<String> {
    self.put_vtodo_at(&Self::vtodo_href(calendar_href, &vtodo.uid), vtodo, etag).await
}
```

### 3.2 Modelo

`RemoteTask` y `RemoteTaskDraft` ganan:

```rust
pub etag: Option<String>,
```

`From<&RemoteTask> for RemoteTaskDraft` lo copia.

### 3.3 Provider

- `vtodo_to_remote(href, etag, vtodo)` — añadir el parámetro y rellenarlo.
- `list_tasks` pasa `item.etag.clone()`.
- `update_task` usa `draft.etag`:

```rust
let etag = draft.etag.as_deref();
self.client.put_vtodo_at(remote_id, &vtodo, etag).await?;
```

Si `draft.etag` es `None`, hacer el `GET` previo (comportamiento actual) para
no perder la protección contra escrituras concurrentes.

## Paso 4 — Sync incremental en el trait

En `src/shared/providers/model.rs`:

```rust
#[derive(Clone, Debug, Default)]
pub struct RemoteChanges {
    pub changed: Vec<RemoteTask>,
    /// `remote_id` (href) de los recursos que ya no existen.
    pub removed: Vec<String>,
    pub next_token: Option<String>,
}
```

En el trait, dos métodos nuevos con implementación por defecto para no tocar
Google/Microsoft:

```rust
async fn sync_changes(
    &self,
    _credentials: &ProviderCredentials,
    _list_remote_id: &str,
    _sync_token: &str,
) -> ProviderResult<Option<RemoteChanges>> {
    Ok(None) // el proveedor no soporta sync incremental
}

async fn get_task(
    &self,
    _credentials: &ProviderCredentials,
    _list_remote_id: &str,
    _remote_id: &str,
) -> ProviderResult<RemoteTask> {
    Err(ProviderError::Unsupported)
}
```

`RadicaleProvider`:

- `sync_changes` → `client.sync_collection(calendar, token)`.
  **Ojo:** las entradas que vuelven sin `calendar-data` (Radicale solo manda href
  y ETag) hay que resolverlas con `client.get_vtodo(href)`.
- `get_task` → `client.get_vtodo(remote_id)`.

## Paso 5 — Refactor de `sync.rs`

### 5.1 Sustituir `&Account` por un objetivo neutro

```rust
#[derive(Clone, Copy, Debug)]
pub struct SyncTarget<'a> {
    pub id: Uuid,
    pub provider_id: &'a str,
    pub display_name: &'a str,
}
```

Cambiar `sync_account`, `sync_list_tasks` y `matches_account` para usarlo.
En `run_sync`, construir:

```rust
let target = SyncTarget {
    id: account.id,
    provider_id: &account.provider,
    display_name: &account.display_name,
};
```

`make_source` ya recibe `provider_id`; añadir el brazo `"radicale"`.

### 5.2 Motor paralelo

```rust
pub async fn run_radicale_sync(
    store: &Store,
    connections: &[RadicaleConnection],
) -> SyncReport
```

Para cada conexión:

1. `let Some(password) = radicale_password(conn) else { error y continue }`.
2. `RadicaleProvider::from_credentials(&conn.base_url, &ProviderCredentials::Basic { username, password })`.
3. Resolver el calendario: `conn.calendar_href` o el primero de `list_lists` que
   el cliente confirme con VTODO (usa `CalDavClient::list_calendars` directamente,
   porque `RemoteList` no expone `supports_vtodo`).
4. Buscar la `List` local con `TaskSource::Radicale` y `account_id == conn.id`.
   Si no existe, crearla (`List::new(name)`, `source = Radicale { .. }`, `save`).
5. Llamar a `sync_list_tasks(store, provider, target, credentials, list.id, &href, list.sync_token.as_deref())`.
6. Guardar el `next_token` devuelto en `list.sync_token`.

### 5.3 `sync_list_tasks` con token

Firma nueva (añade el token):

```rust
async fn sync_list_tasks(
    store: &Store,
    provider: &dyn RemoteTaskProvider,
    target: SyncTarget<'_>,
    credentials: &ProviderCredentials,
    list_id: Uuid,
    list_remote_id: &str,
    sync_token: Option<&str>,
) -> Result<(SyncReport, Option<String>), ProviderError>
```

Lógica de pull:

```
if let Some(token) = sync_token {
    match provider.sync_changes(credentials, list_remote_id, token).await? {
        Some(changes) => {
            // aplicar solo changes.changed
            // mover a papelera solo changes.removed
            // next_token = changes.next_token
        }
        None => full_scan()   // Google/Microsoft y servidores sin sync-collection
    }
} else {
    full_scan()
}
```

`full_scan` es el bloque actual de `list_tasks` + detección de borrados por
diferencia. **No dupliques código**: extrae el "aplicar un remoto a local" a una
función `apply_remote(...)` reutilizada por ambos caminos.

### 5.4 Conflictos (412)

En el push de una tarea `dirty`, `update_task` puede devolver
`ProviderError::CalDav(CalDavError::PreconditionFailed(_))`. Entonces:

1. `provider.get_task(...)` para traer la versión remota fresca (con su ETag).
2. Comparar `remote.updated_at` con `local.remote_updated_at`:
   - remoto más nuevo (o local sin timestamp) → `apply_remote`, contar como pull,
     y **añadir aviso** a `report.errors` con texto tipo
     `"conflicto en 'título': se conservó la versión remota"`.
   - local más nuevo → reintentar **una sola vez** con `draft.etag = remote.etag`.
     Si vuelve a fallar, error al report y seguir.
3. Nunca reintentar en bucle.

Extrae la decisión a una función pura para poder testearla:

```rust
fn remote_wins(local: Option<Timestamp>, remote: Option<Timestamp>) -> bool
```

## Paso 6 — Mapeo completo `Task` ↔ draft

Hoy los dos sitios de `sync.rs` que construyen `RemoteTaskDraft` usan
`..Default::default()`. Centraliza en:

```rust
fn draft_from_task(task: &Task, parent_remote_id: Option<String>) -> RemoteTaskDraft
```

| Campo | Regla |
|---|---|
| `priority` | `High → 1`, `Normal → 5`, `Low → 9` |
| `tags` | `task.tags.clone()` |
| `parent_remote_id` | href remoto del padre, si el padre ya está sincronizado |
| `recurrence` | días de `Recurrence` → `FREQ=WEEKLY;BYDAY=MO,TU,…`; sin días → `None` |
| `percent_complete` | `None` (la app no tiene estado parcial) |
| `etag` | `None` aquí; lo rellena el motor |

Y ampliar `apply_remote_to_local`:

| Remoto | Local |
|---|---|
| `priority` 1-4 | `Priority::High` |
| `priority` 5 | `Priority::Normal` |
| `priority` 6-9 | `Priority::Low` |
| `tags` | `task.tags` |
| `recurrence` (RRULE `BYDAY`) | `Recurrence` con esos días; si no se entiende, dejar como está |

## Paso 7 — Tests

No existe `src/lib.rs`: los tests de la app van **inline** (`#[cfg(test)] mod tests`
dentro de `sync.rs`). No intentes crear `tests/` en la raíz, no compilará.

Mínimo exigido:

1. `draft_from_task` mapea prioridad, etiquetas y recurrencia.
2. `remote_wins` cubre: remoto más nuevo, local más nuevo, ambos `None`, uno `None`.
3. Prioridad remota → enum (1-4 High, 5 Normal, 6-9 Low).
4. `Recurrence` con dos días → `FREQ=WEEKLY;BYDAY=MO,WE`; sin días → `None`.
5. `sync_token` sobrevive a serializar/deserializar una `List` (RON).

## Criterios de aceptación

1. Crear una tarea en la app → aparece en Radicale (compruébalo con `examples/probe`).
2. Crear una tarea por fuera (curl) → aparece en la app tras sincronizar.
3. Editar lo mismo en ambos lados: no se pierde nada en silencio; o gana el
   remoto con aviso, o se reintenta con el ETag fresco.
4. Borrar en el servidor → la tarea va a la papelera local, no desaparece.
5. Un segundo sync sin cambios **no** reescribe tareas (el `sync-token` funciona;
   se ve en los logs como 0 pulled / 0 pushed).
6. Google y Microsoft siguen compilando y `cargo test` pasa.

## Trampas

- **No hay `src/lib.rs`.** Tests inline.
- `AppConfig` es `CosmicConfigEntry` versionado (`CONFIG_VERSION = 1`). Añade
  campos con `#[serde(default)]` y **prueba con un `config.ron` existente**. Si
  cosmic-config falla al cargar, sube `CONFIG_VERSION` a 2 y migra; no lo hagas
  "por si acaso".
- La contraseña nunca en `config.ron`. En A2 solo por entorno.
- `sync_collection` puede devolver entradas sin `calendar-data`: hay que hacer `GET`.
- Los `href` de Radicale son relativos (`/cesar/<uuid>/x.ics`); usa
  `CalDavClient::absolute`, no concatenes.
- `put_vtodo` deriva el nombre del fichero del UID. Para recursos ajenos, usa
  `put_vtodo_at`.
- El bucle de `run_sync` **no** debe intentar sincronizar Radicale: son motores
  separados que comparten `sync_list_tasks`.
- No metas UI en esta fase.

## Checkpoint de sesión

Si el contexto se agota, cierra después del **paso 5.2** (motor que hace pull
completo) con commit y entrada en la bitácora, dejando push/conflictos para
A2b. No dejes `sync.rs` a medio refactor sin compilar.

## Comandos

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd /home/cesar/Documentos/AppsIA/ProPer_ToDo

cargo build          # ~1-2 min en caliente
cargo test --bin tasks
cd caldav-client && cargo test
```

Prueba manual contra el servidor real:

```bash
set -a; . ~/.config/proper-todo/test.env; set +a
export RADICALE_URL=http://192.168.0.35:5232/cesar/d6be8909-c122-e21e-91fd-304a1e7d2313/
export PROPER_TODO_RADICALE_PASSWORD="$RADICALE_PASS"
cd caldav-client && cargo run --example probe
```
