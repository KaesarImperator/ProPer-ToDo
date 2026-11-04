# ProPer ToDo — contexto para agentes

App de tareas para escritorio **COSMIC**, fork de [`cosmic-utils/cosmic-ext-tasks`](https://github.com/cosmic-utils/cosmic-ext-tasks), con **sincronización CalDAV contra un Radicale propio**.

> Este archivo es el punto de entrada para cualquier sesión nueva. Si eres un agente: lee esto, luego `docs/00-estado-actual.md`, y trabaja **una fase a la vez** según `docs/01-roadmap.md`. No leas todo el repo de golpe.

## Estado (2026-11-04)

| Fase | Descripción | Estado |
|---|---|---|
| **B** | Crate aislado `caldav-client` + tests | ✅ Completa — 12 unitarios + 4 integración contra Radicale 3.9 |
| **A** | `RadicaleProvider` dentro de la app + sync | ⬜ Pendiente |
| **C** | Fixes upstream #164 y #115 | ⬜ Pendiente |

## Mapa del repo

```
├── src/                  # app COSMIC (upstream, Rust + libcosmic)
│   └── shared/providers/ # trait RemoteTaskProvider + Google/Microsoft
├── caldav-client/        # ⭐ crate nuestro, sin libcosmic, testeable en segundos
├── docs/                 # planificación y bitácora (leer antes de tocar código)
├── i18n/  res/  justfile
└── Cargo.toml            # crate de la app (NO es workspace)
```

`caldav-client/` es un crate independiente. No está en un workspace con la app todavía; en la fase A se añadirá como dependencia `path`.

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

- La clave SSH `~/.ssh/github_proper` aún no está autorizada en GitHub → no se puede hacer push (ver `docs/03-radicale.md` §Push).
- Faltan paquetes de sistema para compilar libcosmic (`libxkbcommon-dev`, `libwayland-dev`, `libudev-dev`, `libdbus-1-dev`, `libssl-dev`, `libfontconfig-dev`, `libfreetype-dev`).
- No hay URL ni credenciales del Radicale del usuario todavía → el test de integración se salta solo.
