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
- Descubrimiento desde URL raíz verificado: cae a `/{usuario}/` cuando el servidor no resuelve `current-user-principal`.
- `caldav-client/README.md` con ejemplo de uso y `examples/probe.rs` para sondeo manual.
- Documentación de proyecto completa (`AGENTS.md` + `docs/`).

### Decisiones

1. **Cliente propio** en vez de una crate externa: compila en ~2 s y se testea sin libcosmic.
2. **Parser iCalendar propio** en vez de la crate `ical`: evita dependencia poco mantenida y permite preservar propiedades desconocidas.
3. **Orden de fases B → A → C**, no A → B → C: B es prerrequisito técnico de A.
4. **Fork, no PR**: upstream lleva 2 años sin CalDAV y su foco está en Google/Microsoft.
5. **ETag + `sync-collection`** en vez del LWW por timestamp del motor actual.
6. **Fallback de principal**: si `current-user-principal` no resuelve, usar `/{usuario}/`. Necesario para Radicale con `auth.type = none`.

### Problemas

- La clave SSH `github_proper` no está autorizada en GitHub (`Permission denied (publickey)`). Push bloqueado.
- Faltan 7 paquetes `-dev` para compilar libcosmic. Requiere sudo, pendiente.
- Sin URL/credenciales del Radicale real del usuario: verificado solo contra Docker.

### Notas de entorno (costaron tiempo, documentadas en `03-radicale.md`)

- La imagen `tomsquest/docker-radicale` ignora `RADICALE_RIGHTS_TYPE`: hay que montar `config` + `rights` propios.
- `permissions: RW` no basta: las minúsculas (`RrWw`) son las que autorizan escribir ítems.
- `MKCOL` sin cuerpo crea una colección genérica, no un calendario; hay que enviar el XML con `resourcetype: calendar`.
- `MKCALENDAR` devuelve 403 en Radicale 3.9.

### Siguiente paso

Fase A: integración en la app (requiere resolver antes el sudo para libcosmic).

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
