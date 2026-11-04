# caldav-client

Cliente CalDAV mínimo, centrado en **VTODO**, pensado para Radicale.

Sin dependencias de libcosmic: compila en segundos y se testea sin servidor
gráfico. Es la base del `RadicaleProvider` de la app (ver `docs/02-arquitectura.md`).

## Qué hace

- Descubrimiento: `current-user-principal` → `calendar-home-set` → colecciones
- Listado de calendarios con `supported-calendar-component-set`
- `REPORT calendar-query` filtrado por `VTODO`
- CRUD con `If-Match` / `If-None-Match` (concurrencia optimista real)
- `sync-collection` incremental con `sync-token`
- iCalendar propio: unfold, escape, folding a 75 octetos, propiedades desconocidas preservadas

## Uso

```rust
use caldav_client::{CalDavClient, Vtodo};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let client = CalDavClient::new("http://localhost:5232/user/", "user", "pass")?;

let calendars = client.list_calendars().await?;
let calendar = calendars.iter().find(|c| c.supports_vtodo).unwrap();

let mut task = Vtodo::new("mi-uuid", "Comprar leche");
task.priority = Some(1);

let etag = client.put_vtodo(&calendar.href, &task, None).await?;

let mut task = client.get_vtodo(&format!("{}mi-uuid.ics", calendar.href)).await?.vtodo;
task.summary = "Comprar leche y pan".into();

// Falla con PreconditionFailed si otro cliente lo cambió mientras tanto.
client.put_vtodo(&calendar.href, &task, Some(&etag)).await?;
# Ok(())
# }
```

## Tests

```bash
cargo test                    # 12 unitarios, sin servidor
cargo test -- --ignored       # integración, requiere RADICALE_* (ver docs/03-radicale.md)
```

## Límites conocidos

- Un calendario = una lista. CalDAV no tiene jerarquía de listas.
- Sin `VALARM` todavía (recordatorios: fase A).
- Sin soporte de calendarios compartidos ni delegación.
