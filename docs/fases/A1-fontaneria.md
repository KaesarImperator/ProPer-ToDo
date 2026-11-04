# A1 — Fontanería del provider Radicale

> **Estado: ✅ ejecutada el 2026-11-04** (commit `04b3c8c`). Ver
> `docs/05-bitacora.md` §Sesión 2 para lo hecho, las decisiones y las
> limitaciones que quedan para A2. Este documento se conserva como brief
> histórico; no volver a ejecutarlo.

> **Para el agente:** este documento es autosuficiente. No hace falta explorar
> el repo antes de empezar; los archivos y firmas exactas están aquí. Si algo
> no coincide con el código, para y anótalo en `docs/05-bitacora.md`.

## Objetivo

Dejar la app preparada para hablar CalDAV: dependencia, variante de origen,
credenciales y un `RadicaleProvider` que implemente el trait existente.
**Sin UI y sin tocar el motor de sincronización.**

## Fuera de alcance

- Cambios en `sync.rs` más allá de lo necesario para que compile.
- Pantalla de ajustes, keyring, selector de calendario → **A3**.
- Pull/push real, conflictos, papelera → **A2**.

Si te tienta implementar algo de A2 o A3, no lo hagas: rompe la división y
hace que la sesión no se pueda cerrar limpia.

## Estado de partida

- `caldav-client/` completo y verificado (12 unitarios + 4 integración).
- La app compila (`cargo build` ≈ 9 min en frío).
- `src/shared/providers/{provider,model,google,microsoft,sync}.rs` existen.

## Paso 1 — Dependencia

En `Cargo.toml` (raíz):

```toml
[dependencies.caldav-client]
path = "caldav-client"
```

## Paso 2 — `TaskSource::Radicale`

En `src/shared/store/source.rs`, añadir la variante:

```rust
pub enum TaskSource {
    #[default]
    Local,
    Google { account_id: Uuid, remote_id: String },
    Microsoft { account_id: Uuid, remote_id: String },
    Radicale { account_id: Uuid, remote_id: String },
}
```

Y ampliar los dos `match`:

```rust
TaskSource::Radicale { account_id, .. } => Some(*account_id),
TaskSource::Radicale { remote_id, .. } => Some(remote_id.as_str()),
```

**Rompe la compilación a propósito** en los `match` exhaustivos. Los sitios son:

| Archivo | Qué hacer |
|---|---|
| `src/features/lists/nav.rs:13-18` | Añadir `TaskSource::Radicale { .. }` al brazo de icono remoto (reutiliza `"weather-clouds-symbolic"`) |
| `src/shared/providers/sync.rs:167-177` (`make_source`) | Añadir `"radicale" => TaskSource::Radicale { .. }` |

`remote_id` para Radicale es el **href del recurso** (`/cesar/<uuid>/tarea.ics`),
no el UID: es lo único estable y directamente usable en un `GET`/`PUT`.

## Paso 3 — Credenciales

Hoy el trait recibe `token: &str` porque Google/Microsoft usan OAuth Bearer.
Radicale usa HTTP Basic. Cambiar en `src/shared/providers/provider.rs`:

```rust
#[derive(Clone, Debug)]
pub enum ProviderCredentials {
    Bearer(String),
    Basic { username: String, password: String },
}
```

Añadir a `ProviderError`:

```rust
#[error("provider does not support this credential type")]
UnsupportedCredentials,
#[error("operation not supported by this provider")]
Unsupported,
```

Cambiar **todas** las firmas del trait: `token: &str` → `credentials: &ProviderCredentials`.
Son 9 métodos (`create_list`, `delete_list`, `list_lists`, `update_list`,
`create_task`, `delete_task`, `list_tasks`, `update_task`) más `name`.

En `google.rs` y `microsoft.rs`, al principio de cada método:

```rust
let ProviderCredentials::Bearer(token) = credentials else {
    return Err(ProviderError::UnsupportedCredentials);
};
```

En `sync.rs`, donde hoy se obtiene el token, envolverlo:

```rust
let credentials = ProviderCredentials::Bearer(token);
```

y pasar `&credentials` en lugar de `&token`.

## Paso 4 — Campos nuevos en el modelo remoto

`RemoteTask`/`RemoteTaskDraft` solo llevan título, notas, fecha y completado.
Radicale puede transportar más, y perderlo sería una regresión silenciosa.
Añadir a **ambos** structs (con `Default` derivado para no romper llamadores):

```rust
pub priority: Option<u8>,          // 1 = alta, 5 = normal, 9 = baja
pub tags: Vec<String>,
pub percent_complete: Option<u8>,
pub parent_remote_id: Option<String>,
pub recurrence: Option<String>,
```

Google y Microsoft dejan los valores por defecto: no los soportan y no pasa nada.

## Paso 5 — `src/shared/providers/radicale.rs`

```rust
pub struct RadicaleProvider {
    client: CalDavClient,
}
```

El `CalDavClient` se construye por conexión (URL + credenciales), no por llamada.

Implementación de cada método:

| Método del trait | Implementación |
|---|---|
| `name()` | `"radicale"` |
| `list_lists` | `client.list_calendars()` → `RemoteList { remote_id: href, title: display_name.unwrap_or(href) }` |
| `list_tasks(calendar)` | `client.list_vtodos(calendar)` → `RemoteTask` (ver mapeo) |
| `create_task` | `Vtodo::new(uuid_v4, title)`, `client.put_vtodo(calendar, &v, None)`, devolver `RemoteTask` con `remote_id = CalDavClient::vtodo_href(calendar, &uid)` |
| `update_task(calendar, remote_id, draft)` | `client.get_vtodo(remote_id)` → aplicar draft → `put_vtodo(..., Some(&etag))` |
| `delete_task(calendar, remote_id)` | `client.get_vtodo` para el ETag → `client.delete_vtodo(remote_id, Some(&etag))` |
| `create_list` / `update_list` / `delete_list` | `Err(ProviderError::Unsupported)` — el motor actual no las usa (ver doc del trait) |

Nota: `update_task` hace GET+PUT porque el trait no transporta ETag. Es
correcto pero no atómico; A2 lo arreglará pasando el ETag en el draft.

### Mapeo `Vtodo` ↔ `RemoteTask`

| `Vtodo` | `RemoteTask` |
|---|---|
| `uid` | — (se usa `href` como `remote_id`) |
| `summary` | `title` |
| `description` | `notes` (vacío → `""`) |
| `due` | `due_date` |
| `status == Completed` o `percent_complete == 100` | `completed` |
| `last_modified` | `updated_at` |
| `priority` | `priority` |
| `categories` | `tags` |
| `percent_complete` | `percent_complete` |
| `parent_uid` | `parent_remote_id` |
| `rrule` | `recurrence` |

A la inversa, al escribir: `status = Completed` + `percent_complete = 100` si
`completed`; `NEEDS-ACTION` si no.

## Paso 6 — Registrar el provider

En `sync.rs::provider_for`, añadir el brazo. Como `RadicaleProvider` necesita
credenciales que aún no existen en el bucle de sync (no es una cuenta de
COSMIC), **devuelve `None` por ahora** y deja el TODO explícito:

```rust
// A2: las conexiones Radicale no son cuentas de COSMIC; se registrarán
// aparte y se inyectarán aquí con sus credenciales.
```

No inventes un `Account` falso: el bucle de `run_sync` itera
`accounts.list_enabled_accounts(Service::Tasks)`, que nunca devolverá Radicale.
Ese es el problema que resuelve A2.

## Paso 7 — Tests

En `radicale.rs`, tests unitarios (sin red) del mapeo:

1. `RemoteTask` → `Vtodo` → `RemoteTask` conserva título, notas, fecha,
   prioridad, etiquetas y `completed`.
2. `completed = true` produce `STATUS:COMPLETED` y `PERCENT-COMPLETE:100`.
3. Un `Vtodo` con `percent_complete: 50` y `IN-PROCESS` **no** se marca completado.
4. `description = None` no genera `DESCRIPTION:` vacío.

## Criterios de aceptación

1. `cargo build` compila sin warnings nuevos.
2. `cargo test` pasa (los tests nuevos incluidos).
3. Google y Microsoft siguen compilando y su comportamiento no cambia.
4. `cargo test` de `caldav-client/` sigue en verde.
5. Entrada nueva en `docs/05-bitacora.md` y commit con prefijo `feat(radicale):`.

## Trampas conocidas

- `TaskSource` se serializa en RON: no renombrar variantes existentes, solo añadir.
- No guardar la contraseña en `AppConfig` (acaba en claro en `config.ron`). En A1
  las credenciales solo viajan en memoria.
- `RemoteTaskDraft` se construye en `sync.rs`; si añades campos sin `Default`,
  romperás ese archivo. Deriva `Default`.
- El `href` que devuelve Radicale es relativo (`/cesar/<uuid>/x.ics`).
  `CalDavClient::absolute()` lo resuelve; no lo concatenes a mano.
- No uses `MKCALENDAR` con Radicale 3.9: devuelve 403.

## Comandos

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd /home/cesar/Documentos/AppsIA/ProPer_ToDo

cargo build
cargo test

cd caldav-client && cargo test
```

Integración contra el servidor real (si hace falta):

```bash
set -a; . ~/.config/proper-todo/test.env; set +a
export RADICALE_URL=http://192.168.0.35:5232/cesar/d6be8909-c122-e21e-91fd-304a1e7d2313/
cargo test --test radicale -- --ignored
```
