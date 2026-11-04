# Issues de upstream — análisis

Estado a 2026-11-04. 14 issues abiertas en `cosmic-utils/cosmic-ext-tasks`.

Veredicto general: **ninguna es bloqueante grave** (sin pérdida de datos ni crashes constantes). Tres molestan en el uso diario.

## Tabla

| # | Título | Gravedad | ¿Afecta UX? | Fase |
|---|---|---|---|---|
| 164 | Suprimir tecleando abre el diálogo de borrar lista | 🔴 Alta | Sí, constante | **C** |
| 115 | Input de subtarea parece enfocado pero no escribe | 🔴 Alta | Sí, bloquea subtareas | **C** |
| 90 | Menú contextual no abre en listas con scroll | 🔴 Alta | Sí, bloquea ⋮ | upstream |
| 119 | Estado del padre no refleja subtareas | 🟠 Media | Sí, confusión | D |
| 28 | Exportar no copia al portapapeles | 🟠 Media | Solo al exportar | D |
| 153 | No hay iconos para elegir | 🟡 Baja | Cosmético | D |
| 152 | Toast de borrado demasiado ancho | 🟡 Baja | Cosmético (tapa Undo) | D |
| 166 | Tareas diarias que se reinician | ⚪ Feature | — | D |
| 151 | Versión imprimible de pendientes | ⚪ Feature | — | — |
| 150 | Página de tarea dedicada en vez de sidebar | ⚪ Feature | — | D |
| 126 | Crear subtarea con Enter | ⚪ Feature | — | — |
| 156 | Backup/restore local | ⚪ Feature | — | D |
| 21 | Importar listas Markdown | ⚪ Feature | — | D |
| 6 | Integrar servicios de terceros (CalDAV) | ⚪ Feature | — | **A** |

## Detalle de las que importan

### #164 — `Delete` abre "borrar lista"

No es un fallo de UI, es un atajo global sin guarda de foco. `src/shared/navigation/core/init.rs:131`:

```rust
bind!([], Key::Named(Named::Delete), DeleteList);
```

En portátiles `Fn+Backspace` genera `Delete`, así que salta al escribir. Abre diálogo de confirmación, así que **no destruye nada**, pero interrumpe constantemente.

Arreglo: ignorar el atajo cuando el widget enfocado es un `text_input`/`text_editor`. ~15 líneas.

### #115 — Input de subtarea "enfocado" pero no escribe

El cursor parpadea sin foco real. Hay gestión manual de foco con `LAST_FOCUS_UPDATE` y comentarios admitiendo el problema en `src/features/lists/content.rs:428-451`. Workaround: segundo clic.

### #90 — Menú ⋮ muerto con scroll

**Bug de `libcosmic`, no de Tasks.** Confirmado por el mantenedor con panic en `menu_inner.rs:661`:

```
range end index 0 out of range for slice of length 0
```

El reporter añadió "Edit: This issue has been fixed" (2026-08). Probablemente se resuelve actualizando `libcosmic`. Mientras tanto impide borrar/renombrar tareas fuera de la vista.

### #119 — Estado del padre vs subtareas

Requiere checkbox con estado indeterminado, que `libcosmic` no tiene (issue abierta desde 2022: pop-os/libcosmic#24). El mantenedor acepta al menos la propagación simple (marcar padre marca hijos).

### #28 — Exportar no copia

Dos años abierto. El mantenedor confirmó en 2025 que ocurre **también sin sandbox**. Usa `cli-clipboard`; probablemente necesita `wl-clipboard`/`xclip` en Wayland/X11.

### #6 — Integración con servicios de terceros

La issue que motiva este fork. 8 comentarios, varios pidiendo **CalDAV explícitamente**. El mantenedor respondió en 2026-08: *"Working on it"* enlazando a una rama `feat/accounts`.

Esa rama ya está en `main`: `src/shared/providers/{provider,google,microsoft,sync}.rs` existen y el motor de sync funciona. **CalDAV no está**, y la arquitectura es OAuth-céntrica (ver `02-arquitectura.md` §Autenticación).

## Conclusión para el proyecto

- No hay nada que impida usar la app como base.
- Las fases C arreglan las dos asperezas que más duelen (164, 115).
- #90 se resuelve actualizando `libcosmic`, no parcheando.
- Ninguna issue de upstream cubre CalDAV: es trabajo nuestro.
