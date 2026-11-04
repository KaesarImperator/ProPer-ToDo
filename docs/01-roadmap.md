# Roadmap

Tres fases. **Una fase por sesión.** Cada fase tiene criterio de aceptación explícito; no se pasa a la siguiente sin cumplirlo.

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

### A2 — Motor de sync ⬜

- [ ] Registrar conexiones Radicale fuera del framework de cuentas de COSMIC
- [ ] Pull con `sync-token`, push con `If-Match`
- [ ] ETag en `RemoteTaskDraft` para que el update sea atómico
- [ ] Resolución de conflictos (412)
- [ ] Borrado remoto → papelera local

**Aceptación:** crear en la app aparece en el móvil en < 5 s y viceversa; editar en ambos lados no pierde datos en silencio; borrar en un lado no resucita la tarea.

### A3 — UI y credenciales ⬜

- [ ] Pantalla de ajustes: URL, usuario, contraseña, "Probar conexión", selector de calendario
- [ ] Contraseña en keyring del sistema (`secret-service`), nunca en `config.ron`
- [ ] Estado de sync visible (última vez, errores)

**Aceptación:** un usuario puede configurar su Radicale sin tocar archivos ni variables de entorno.

---

## Fase C — Fixes upstream ⬜

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
