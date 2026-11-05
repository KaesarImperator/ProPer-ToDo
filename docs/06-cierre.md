# Cierre del proyecto — 2026-11-05

**Decisión del usuario:** se deja de desarrollar ProPer ToDo y se pasa a **Planify** como app de tareas diaria
(sobre el mismo Radicale). El proyecto queda **congelado, no borrado**.

## Por qué se cierra

- El objetivo era añadir CalDAV a una app que ya existía. Se consiguió (motor de sync + ajustes), pero la app resultante
  sigue siendo la de upstream con una opción más: sin funciones diferenciales.
- Lo que quedaba era pulido (Fase C: 2 fixes; Fase E: backup, Flatpak, 7 días de uso; Fase D: comodidades menores).
- El usuario usa **Cinnamon sobre X11**, no COSMIC: la app corre fuera de su entorno (sin `cosmic-settings-daemon`,
  el tema y la config no siguen al escritorio) y arrastra un coste de toolkit que el pulido no elimina.
- Se detectó un **`panic` de libcosmic** (`src/widget/menu/menu_inner.rs:715`, «range end index 0 out of range for slice of
  length 0») al usar un menú contextual, que cerró la app. Es de upstream (relacionado con #90) y no tiene arreglo en el plan.

## Estado en que queda

| Fase | Estado |
|---|---|
| B — `caldav-client` | ✅ verificado contra Docker y el servidor real |
| A1 — fontanería | ✅ |
| A2 — motor de sync | ✅ pull incremental, push con ETag, conflictos, borrado → papelera |
| A2b — endurecimiento | ✅ timeouts, `warnings`, tests sin residuos, latencia medida |
| A3 — UI y credenciales | 🟡 implementada y con tests; **aceptación manual en la GUI sin completar** |
| C, D, E | ⬜ no se harán |

Verificado: 33 tests de la app (+3 en vivo: keyring, «Probar conexión», e2e), 13 + 4 en `caldav-client`.
**No verificado:** el comportamiento real de la ventana de A3 (las 6 comprobaciones manuales del roadmap) ni el
borrado de la entrada del keyring desde la UI.

## Qué aprovechar si algún día se retoma

- **`caldav-client/`**: crate independiente (sin libcosmic), con timeouts, ETag, `sync-collection` y `CalendarGuard` para
  tests sin residuos. Es lo más reutilizable; valdría publicarlo o usarlo en otra herramienta.
- **El motor de sync** (`src/shared/providers/sync.rs`) y sus decisiones (ETag por encima de timestamps, `dirty` persistido)
  están documentados en la bitácora, sesiones 3–6.
- **Para retomar:** leer `AGENTS.md` → este archivo → `docs/01-roadmap.md`; completar la aceptación manual de A3 y seguir con la
  Fase C. Cuidado con el panic de libcosmic.

## Pendientes de limpieza (del usuario)

- Borrar a mano las tareas de prueba que quedaron en el calendario `d6be8909-…` de Radicale (títulos «Creada por la app»,
  «Sembrada por fuera», «Editada por fuera…»; ~32 recursos antiguos). Los tests actuales ya no dejan residuos.
- Si se hicieron pruebas con la app real: puede haber una conexión de prueba en su config y una entrada
  `kind=caldav-password` en el llavero (se borra eliminando la conexión desde la app, o con Seahorse).
- Opcional: archivar el repositorio en GitHub (acción del usuario; no se ha hecho).

## Si Planify no cubre algo

Anotar aquí qué falta antes de decidir retomar, para que la decisión sea con datos y no con impresión:

- (vacío)
