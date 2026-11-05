# Roadmap

> **PROYECTO CERRADO el 2026-11-05** — ver [`06-cierre.md`](06-cierre.md). Lo que sigue es el plan original, conservado como historial.

**Esta es la única fuente de verdad del estado de las fases** (`AGENTS.md` y `00-estado-actual.md` solo enlazan aquí).
**Una fase por sesión.** Cada fase tiene criterio de aceptación explícito; no se pasa a la siguiente sin cumplirlo.

**Orden vigente:** B ✅ → A1 ✅ → A2 ✅ → **A2b** (endurecimiento) → **A3** (UI + keyring) → **C** (fixes de uso diario) → **E** (dogfooding y empaquetado) → D (backlog).

Criterio de ordenación: primero lo que puede dejar la app colgada o mentirosa (timeouts, avisos), luego lo que la hace usable (A3),
luego quitar asperezas (C), luego uso real antes de añadir funciones (E antes de D).

---

## Fase B — Cliente CalDAV aislado ✅

**Objetivo:** un crate que hable CalDAV correctamente, sin depender de libcosmic, testeable en segundos.

### Tareas

- [x] Estructura del crate `caldav-client/` (sin workspace; dependencia `path` de la app desde A1)
- [x] `ical.rs`: unfold RFC 5545, parseo de propiedades y parámetros, escape/unescape, folding a 75 octetos
- [x] `vtodo.rs`: struct `Vtodo`, parseo y serialización, preservación de propiedades desconocidas
- [x] `xml.rs`: cuerpos PROPFIND/REPORT + parseo de `multistatus` (incluye borrados de `sync-collection`)
- [x] `client.rs`: descubrimiento (`current-user-principal`, `calendar-home-set`), listado de calendarios, CRUD de VTODO con ETag, `sync-collection`
- [x] 12 tests unitarios en verde
- [x] `tests/radicale.rs`: integración real, marcada `#[ignore]` (4 tests)
- [x] `caldav-client/README.md` con ejemplo de uso
- [x] Verificado contra Radicale 3.9 en Docker (4/4 tests de integración)

### Criterio de aceptación

1. ✅ `cargo test` pasa sin servidor.
2. ✅ `cargo test -- --ignored` pasa contra Radicale: crear → listar → leer → actualizar con ETag → detectar conflicto (412) → borrar → borrar otra vez (idempotente).
3. ✅ El payload generado lo acepta Radicale sin normalizaciones raras (round-trip verificado).

**Pendiente de la fase:** nada. Verificado contra Radicale 3.9 en Docker **y** contra el servidor real del usuario (`R-Profesional`), 4/4 tests en verde en ambos.

---

## Fase A — Integración en la app

Partida en tres sub-fases para que cada sesión cierre limpia. Briefs detallados en `docs/fases/`.

### A1 — Fontanería ✅

Brief: **[`fases/A1-fontaneria.md`](fases/A1-fontaneria.md)**

- [x] Dependencia `path` a `caldav-client`
- [x] `TaskSource::Radicale { account_id, remote_id }`
- [x] `ProviderCredentials` (Bearer / Basic) y refactor del trait
- [x] Campos nuevos en `RemoteTask`/`RemoteTaskDraft`
- [x] `src/shared/providers/radicale.rs` + mapeo `Vtodo` ↔ `RemoteTask`
- [x] Tests unitarios del mapeo (4)

**Aceptación:** ✅ compila, tests verdes (9 en la app, 12 en `caldav-client`), Google/Microsoft sin cambios de comportamiento.

### A2 — Motor de sync ✅

Brief: **[`fases/A2-sync.md`](fases/A2-sync.md)**

- [x] `RadicaleConnection` en `AppConfig` (contraseña por entorno, keyring en A3)
- [x] `List::sync_token`
- [x] `put_vtodo_at` + `etag` en `RemoteTask`/`RemoteTaskDraft`
- [x] `sync_changes`/`get_task` en el trait, con default para Google/Microsoft
- [x] `run_radicale_sync` reutilizando `sync_list_tasks` vía `SyncTarget`
- [x] Pull incremental con `sync-token`
- [x] Resolución de conflictos (412) con reintento único
- [x] Borrado remoto → papelera local
- [x] `draft_from_task` completo (prioridad, etiquetas, recurrencia)

**Aceptación:** ✅ (verificado por el e2e; la cifra "< 5 s" no se midió) crear en la app aparece en el móvil en < 5 s y viceversa; editar en ambos lados no pierde datos en silencio; borrar en un lado no resucita la tarea; el segundo sync sin cambios no reescribe nada.

### A2b — Endurecimiento ✅

Sin brief propio: es una lista corta de deudas detectadas al cerrar A2 (ver bitácora, sesión 3 y 4).
Todo lo de aquí **debe cerrarse antes de A3**, que depende del segundo punto.

- [x] **Timeouts** en `caldav-client` (`CONNECT_TIMEOUT` 10 s, `REQUEST_TIMEOUT` 30 s, `with_timeouts`; test con servidor colgado)
- [x] **`SyncReport::warnings`** separado de `errors`: los conflictos resueltos pasan a avisos; `had_errors()` solo mira `errors`
- [x] **e2e sin residuos:** el test usa el calendario de pruebas designado (`d6be8909-…`, ver `docs/03-radicale.md`),
      UIDs con prefijo `proper-todo-e2e-` y un guard `Drop` que borra todo lo suyo aunque el test falle
- [x] **Medir el criterio "< 5 s":** el e2e cronometra push→visible por fuera y falla por encima de 5 s
- [x] Helper de test para el calendario de pruebas (crear/limpiar) reutilizable por `caldav-client/tests/radicale.rs`

**Aceptación:** ✅ (push 0,29 s y pull 0,28 s medidos; calendario idéntico antes/después) `cargo test -- --ignored` (app y crate) pasa y deja el calendario de pruebas exactamente como estaba; un fallo a mitad no deja residuos.

### A3 — UI y credenciales 🟡 implementada, falta aceptación manual en la GUI

Brief: **[`fases/A3-ui-credenciales.md`](fases/A3-ui-credenciales.md)**

- [x] `src/shared/secrets.rs` (keyring, `oo7`) y el motor lee la contraseña de ahí (la variable de entorno queda solo para tests)
- [x] Página «Calendarios CalDAV» independiente del demonio de cuentas: URL, usuario, contraseña, **Probar conexión**, selector de calendario, editar, eliminar
- [x] Punto de entrada real en Ajustes (la pantalla de cuentas actual es inalcanzable)
- [x] Sync periódico también sin demonio de cuentas; botón «Sincronizar» y estado (última vez, errores, avisos) reutilizables
- [x] Mensajes de error humanos (credenciales, timeout, URL, sin calendarios VTODO)

**Aceptación manual pendiente (requiere ver la ventana; no se pudo automatizar):**

1. Desde cero (sin variable de entorno ni demonio de cuentas): Ajustes → Calendarios CalDAV → Gestionar → Añadir → Probar → elegir calendario → Guardar; las tareas aparecen.
2. Tras guardar: `grep -ri password ~/.config/cosmic/dev.edfloreshz.Tasks/` no devuelve nada.
3. Con la app abierta y sin pulsar nada, un cambio externo llega en ≤ 5 min.
4. Contraseña errónea → mensaje claro en la pantalla y la conexión no se guarda.
5. Servidor apagado → «Probar»/«Sincronizar» terminan en ≤ 30 s con error legible.
6. Eliminar → tareas en la papelera y entrada fuera del llavero (p. ej. con Seahorse/`secret-tool`, que no está instalado aquí).

Ya verificado por tests: 1–2 a nivel de lógica (`to_connection`, RON sin «password»), 4–5 a nivel de función contra el servidor real.

**Aceptación:** un usuario configura su Radicale sin tocar archivos ni variables de entorno; la contraseña no aparece en `config.ron`; servidor apagado ⇒ error legible en ≤ 30 s.

---

## Fase C — Fixes upstream ⬜ (tras A3)

**Objetivo:** quitar las asperezas de uso diario antes de dogfooding.

### Tareas

- [ ] **#164** — La tecla `Delete` abre el diálogo de borrar lista mientras escribes.
      Causa: `bind!([], Key::Named(Named::Delete), DeleteList)` en `src/shared/navigation/core/init.rs:131`, sin comprobar foco.
      Arreglo: ignorar el atajo si el widget enfocado es un campo de texto.
- [ ] **#115** — El input de subtarea muestra cursor pero no recibe foco.
      Causa: gestión manual de foco con `LAST_FOCUS_UPDATE` en `src/features/lists/content.rs:428-451`.
- [ ] Regresión manual de ambos.

### Criterio de aceptación

Escribir una tarea con `Delete`/`Fn+Backspace` en el título no abre ningún diálogo, y "Add subtask" permite escribir sin segundo clic.

---

## Fase E — Dogfooding y empaquetado ⬜

**Objetivo:** que la app sea de fiar para el uso diario antes de añadir funciones.

### Tareas

- [ ] **Backup previo a la primera sincronización** de cada conexión (copia del store local a `…/backups/`), con aviso en la UI
- [ ] Flatpak: `--share=network` y acceso al Secret Service en `dev.edfloreshz.Tasks.json` (hoy no hay ni red; el keyring falla en sandbox sin esto)
- [ ] Uso real durante **7 días** como lista de tareas principal, con diario de incidencias en la bitácora
- [ ] Prueba con el móvil (cliente CalDAV real) editando a la vez
- [ ] Decisión sobre la deriva de upstream: ¿rebase periódico o fork definitivo? (`sync.rs` ya es 14× el original)
- [ ] Limpieza de warnings de clippy propios (los heredados de upstream se dejan)

### Criterio de aceptación

7 días seguidos sin pérdida de datos ni sync colgada, y 0 incidencias abiertas de severidad alta. Solo entonces se abre la Fase D.

---

## Fase D — Mejoras (backlog, sin fecha)

Ordenadas por valor para este proyecto:

1. **#119** Estado parcial del padre según subtareas (requiere checkbox indeterminado en libcosmic)
2. **#156** Backup/restore local (JSON export/import)
3. **#150** Página de tarea dedicada en vez de sidebar
4. **#152** Ancho máximo del toast de borrado
5. **#153** Selector de iconos para listas
6. **#28** Exportar al portapapeles (bug de `cli-clipboard`)
7. **#21** Importar listas Markdown
8. **#166** Tareas diarias recurrentes
