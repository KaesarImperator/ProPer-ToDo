# Estado actual

**Última actualización:** 2026-11-04

## Resumen

Objetivo: app de tareas COSMIC que sincronice con un servidor **Radicale** propio vía CalDAV (VTODO).

Base elegida: fork de `cosmic-utils/cosmic-ext-tasks` (Rust + libcosmic, GPL-3.0). Se eligió porque ya trae motor de sincronización con proveedores remotos (Google Tasks y Microsoft To Do) y un trait `RemoteTaskProvider` que CalDAV puede implementar.

## Hecho

- Fork creado y clonado en el workspace. `origin` → fork, `upstream` → cosmic-utils.
- Fork verificado **idéntico a upstream** al inicio (0 commits adelante).
- Toolchain Rust instalado vía rustup (`cargo 1.99.1`).
- Crate `caldav-client/` creado y funcionando:
  - `ical.rs` — lectura/escritura RFC 5545 (unfold, params, escape, folding)
  - `vtodo.rs` — mapeo VTODO ↔ struct, conserva propiedades desconocidas
  - `xml.rs` — cuerpos PROPFIND/REPORT y parseo de `multistatus`
  - `client.rs` — descubrimiento, CRUD con ETag, `sync-collection`
  - **12 tests unitarios en verde**
- Análisis de las 14 issues abiertas de upstream (ver `04-issues-upstream.md`).

## En curso

Nada. **Fase B cerrada.** Siguiente: fase A (integración en la app).

## Pendiente inmediato

1. Obtener la contraseña de `cesar` (vía `~/.config/proper-todo/test.env`) y repetir los tests de integración contra el servidor real.
2. Validar que la app compila en esta máquina (build en curso).
3. Empezar la fase A: dependencia `path`, `TaskSource::Radicale`, refactor de credenciales.

## Resuelto

- ✅ Push a GitHub funcionando (SSH). Commit `7d11e41` publicado.
- ✅ Dependencias de sistema de libcosmic instaladas.

## Decisiones ya tomadas

Ver `02-arquitectura.md` §Decisiones. Resumen:

- Cliente CalDAV propio y mínimo en vez de depender de una crate externa.
- Autenticación HTTP Basic propia, fuera del framework OAuth de COSMIC.
- Sync con ETag + `sync-collection`, no solo last-write-wins por timestamp.
- Fork en vez de esperar a que upstream acepte CalDAV.

## Riesgos abiertos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| `libcosmic` cambia y rompe el build | Medio | Fijar `Cargo.lock`; el crate CalDAV es independiente |
| Radicale sin `sync-collection` (versiones viejas) | Bajo | Fallback a listado completo con ETag |
| Sandbox Flatpak sin red | Medio | Declarar permiso de red en el manifiesto al empaquetar |
| Conflictos de edición desde el móvil | Alto | `If-Match` + resolución explícita (fase A) |
