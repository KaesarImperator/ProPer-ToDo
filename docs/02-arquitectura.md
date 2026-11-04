# Arquitectura

## Vista general

```
┌──────────────────────────────────────────┐
│ App COSMIC (src/)                        │
│  store RON  ──  features/*  ──  app.rs   │
│         │                                │
│         └── shared/providers/sync.rs     │
│                    │                     │
│              RemoteTaskProvider          │
│              ├── google.rs               │
│              ├── microsoft.rs            │
│              └── radicale.rs  ◄── A1     │
└────────────────────┬─────────────────────┘
                     │
        ┌────────────▼─────────────┐
        │ caldav-client (crate)    │
        │  client / xml / vtodo    │
        └────────────┬─────────────┘
                     │ HTTP Basic
              ┌──────▼──────┐
              │  Radicale   │
              └─────────────┘
```

`caldav-client` **no conoce** los tipos de la app (`Task`, `List`, `Store`). Solo habla CalDAV. El mapeo vive en `src/shared/providers/radicale.rs`.

## Por qué un cliente propio

| Alternativa | Por qué no |
|---|---|
| Crate `caldav` de crates.io | Poco mantenida, arrastra su propio modelo de dominios |
| `reqwest` + `ical` a pelo en la app | Acopla el parseo a libcosmic: cada test necesita compilar la app |
| Reutilizar `google.rs` | Google Tasks no es CalDAV: modelo y auth distintos |

El crate propio compila en ~2 s y se testea sin servidor gráfico.

## Modelo de datos: Task ↔ VTODO

| `Task` (app) | VTODO | Notas |
|---|---|---|
| `id: Uuid` | `UID` | El UUID de la app es el UID; estable para siempre |
| `title` | `SUMMARY` | escapado |
| `notes` | `DESCRIPTION` | escapado, `\n` → `\\n` |
| `due_date: Date` | `DUE;VALUE=DATE` | solo fecha, sin hora |
| `completion_date: Timestamp` | `COMPLETED` | UTC `YYYYMMDDTHHMMSSZ` |
| `is_completed()` | `STATUS:COMPLETED` / `NEEDS-ACTION` | |
| `priority: High/Normal/Low` | `PRIORITY` 1 / 5 / 9 | RFC 5545: 1 = máxima, 9 = mínima |
| `tags: Vec<String>` | `CATEGORIES` | separadas por coma, escapadas |
| `parent_id: Uuid` | `RELATED-TO;RELTYPE=PARENT` | UID del padre |
| `recurrence` | `RRULE` | |
| `creation_date` | `CREATED` | |
| `reminder_date` | `VALARM` con `TRIGGER` | fase A, opcional |
| `remote_updated_at` | `LAST-MODIFIED` | base del LWW |
| `favorite`, `today`, `expanded`, `sort_order` | — | solo locales |
| `sub_task_ids` | — | derivable de `parent_id`; se mantiene local |

Regla: **nunca borrar propiedades desconocidas**. `Vtodo.extra` las conserva para que un round-trip no destruya datos de Thunderbird/Nextcloud.

## Autenticación

`RemoteTaskProvider` recibe `credentials: &ProviderCredentials` (cambio hecho en
A1). Google y Microsoft exigen `Bearer` y devuelven `UnsupportedCredentials` con
cualquier otra variante; `sync.rs` envuelve el token OAuth en
`ProviderCredentials::Bearer` antes de llamar. Radicale usa **HTTP Basic** y su
`CalDavClient` se construye una vez por conexión, no por llamada.

```rust
pub enum ProviderCredentials {
    Bearer(String),
    Basic { username: String, password: String },
}
```

Motivos:

- No se puede reutilizar `accounts_core` (demonio de cuentas COSMIC): está atado a OAuth y a proveedores registrados.
- Un token OAuth no representa una contraseña de Radicale.
- Guardar la contraseña en `AppConfig` (RON en claro) es inaceptable → keyring del sistema (`secret-service` vía crate `oo7` o `keyring`).

## Algoritmo de sincronización

### Pull

1. Leer `sync_token` local del calendario (persistido junto a la lista).
2. Si no hay token → `REPORT calendar-query` completo, guardar token actual.
3. Si hay token → `REPORT sync-collection`:
   - `changed`: hrefs + ETag
   - `removed`: hrefs con 404
4. Para cada `changed`:
   - Si el ETag local coincide → ignorar (nada cambió).
   - Si no → `GET` del `.ics`, parsear, aplicar a la tarea local, guardar ETag y token nuevos.
5. Para cada `removed` → mover la tarea local a la papelera (no borrado duro, coherente con el comportamiento de la app).

### Push

1. Tareas con `dirty == true` y `source = Radicale`.
2. Sin `remote_id` → `PUT` con `If-None-Match: *` (crear; si existe, no pisa).
3. Con `remote_id` → `PUT` con `If-Match: <etag local>`.
4. `412 Precondition Failed` → conflicto real:
   - `GET` de la versión remota
   - comparar `LAST-MODIFIED` con `remote_updated_at` local
   - si el remoto es más nuevo → aplicar remoto y marcar la tarea para avisar al usuario
   - si el local es más nuevo → reintentar el `PUT` con el ETag recién leído
5. `DELETE` siempre con `If-Match`.

### Por qué no solo last-write-wins por timestamp

El motor actual (`sync.rs`) usa `remote_updated_at` sin ETag. Con Google/Microsoft funciona porque el servidor es la única fuente de verdad y los relojes están sincronizados. Con Radicale el usuario edita desde el móvil, el escritorio y Thunderbird: sin `If-Match` dos clientes se pisan y **el último `PUT` borra el trabajo del otro sin dejar rastro**.

## Persistencia local

La app guarda RON: `lists.ron`, `states.ron`, y un `.ron` por tarea dentro de `{base_dir}/{list_uuid}/{task_uuid}.ron`.

No se cambia el formato. Los campos nuevos (`etag`) van con `#[serde(default)]` para que los datos existentes migren sin romperse.

## Descubrimiento en Radicale

Radicale expone:

```
GET  /.well-known/caldav        → redirige a /{usuario}/
PROPFIND /{usuario}/            → current-user-principal
PROPFIND /{usuario}/            → calendar-home-set (normalmente el mismo)
PROPFIND /{usuario}/  Depth:1   → colecciones; las de tareas llevan
                                  <c:supported-calendar-component-set><c:comp name="VTODO"/>
```

`caldav-client` ya implementa ese flujo con fallback: si `current-user-principal` no aparece, usa la URL base como principal.

## Limitaciones conocidas

- `sync-collection` requiere Radicale ≥ 3.1; si falla, hay que caer a listado completo (pendiente, fase A).
- No se soportan calendarios compartidos ni delegación.
- Un calendario CalDAV se mapea a una `List` de la app; no hay jerarquía de listas en CalDAV.
