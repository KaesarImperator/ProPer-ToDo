# ProPer ToDo — contexto para agentes

App de tareas para escritorio **COSMIC**, fork de [`cosmic-utils/cosmic-ext-tasks`](https://github.com/cosmic-utils/cosmic-ext-tasks), con **sincronización CalDAV contra un Radicale propio**.

> Este archivo es el punto de entrada para cualquier sesión nueva. Si eres un agente: lee esto, luego `docs/00-estado-actual.md`, y trabaja **una fase a la vez** según `docs/01-roadmap.md`. No leas todo el repo de golpe.

## Estado

La fuente única del estado de las fases es **[`docs/01-roadmap.md`](docs/01-roadmap.md)**; el detalle
de lo último hecho está en `docs/00-estado-actual.md` y `docs/05-bitacora.md`.
A fecha 2026-11-05: B, A1, A2 y A2b cerradas; **A3 implementada** (falta la aceptación manual en la GUI, ver roadmap); después, Fase C.

## Cómo arrancar una fase

El usuario dice **"Arranca C"** (o E). El agente entonces:

1. Lee `docs/fases/<FASE>-*.md` (p. ej. `A3-ui-credenciales.md`; A2b no tiene brief: está en el roadmap) — es autosuficiente, no hace falta explorar el repo.
2. Lee `docs/00-estado-actual.md` para confirmar que no cambió nada desde entonces.
3. Trabaja **solo** el alcance de esa fase. Lo que esté "fuera de alcance" en el brief no se toca.
4. Al terminar: tests verdes → entrada en `docs/05-bitacora.md` → actualizar `docs/01-roadmap.md` (casillas y estado) y `00-estado-actual.md` → commit → push.

Si el brief no coincide con el código, **parar y anotarlo** en la bitácora en vez de improvisar.

## Mapa del repo

```
├── src/                  # app COSMIC (upstream, Rust + libcosmic)
│   └── shared/providers/ # trait RemoteTaskProvider + Google/Microsoft
├── caldav-client/        # ⭐ crate nuestro, sin libcosmic, testeable en segundos
├── docs/                 # planificación y bitácora (leer antes de tocar código)
├── i18n/  res/  justfile
└── Cargo.toml            # crate de la app (NO es workspace)
```

`caldav-client/` es un crate independiente. Sigue sin formar workspace con la app; desde A1 entra como dependencia `path` en `Cargo.toml` (con su propio `target/` y `Cargo.lock` implícito, por eso `cargo test` ahí sigue siendo de segundos).

## Construir y probar

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# Crate CalDAV (rápido, sin libcosmic)
cd caldav-client && cargo test

# App completa (requiere dependencias de sistema, ver docs/03-radicale.md)
cargo build
```

## Convenciones

- Español en docs y comentarios de commit; inglés en identificadores y código.
- Cada fase termina con: tests en verde + entrada en `docs/05-bitacora.md` + commit.
- Los `.md` de `docs/` se actualizan **antes** de dar una fase por cerrada.
- No reescribir archivos upstream sin necesidad: cada cambio en `src/` debe quedar justificado en la bitácora.

## Bloqueos conocidos

Ninguno. El entorno está completo: Rust, dependencias de libcosmic, Docker y acceso SSH a GitHub.

## Credenciales de prueba

El servidor Radicale real y su contraseña viven en `~/.config/proper-todo/test.env`
(fuera del repo). Ver `docs/03-radicale.md` §5. Nunca commitear ese archivo.
