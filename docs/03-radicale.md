# Entorno: Radicale, toolchain y push a GitHub

## 1. Toolchain

Rust ya está instalado vía rustup en `~/.cargo`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version   # 1.99.1
```

Para que sea permanente, añade a `~/.bashrc`:

```bash
. "$HOME/.cargo/env"
```

## 2. Dependencias de sistema para compilar la app

`caldav-client` **no las necesita**. Solo hacen falta para la app COSMIC (fase A en adelante):

```bash
sudo apt install -y libxkbcommon-dev libwayland-dev libudev-dev \
  libdbus-1-dev libssl-dev libfontconfig-dev libfreetype-dev
```

Verificación: `cargo build` en la raíz del repo.

## 3. Radicale local para pruebas

Docker funciona en esta máquina. **No uses las variables de entorno del
contenedor para `rights`**: la imagen `tomsquest/docker-radicale` solo
renderiza `server`, `auth`, `storage` y `headers`, así que un
`RADICALE_RIGHTS_TYPE=...` se ignora en silencio y todo devuelve 403.

Monta una configuración propia:

```bash
mkdir -p /tmp/radicale-conf

cat > /tmp/radicale-conf/config <<'EOF'
[server]
hosts = 0.0.0.0:5232

[auth]
type = none

[storage]
filesystem_folder = /data/collections

[rights]
type = from_file
file = /config/rights

[logging]
level = info
EOF

cat > /tmp/radicale-conf/rights <<'EOF'
[root]
user: .+
collection: .*
permissions: RrWw
EOF

docker run -d --name proper-radicale \
  -p 5232:5232 \
  -v proper-radicale-data:/data/collections \
  -v /tmp/radicale-conf:/config:ro \
  tomsquest/docker-radicale:latest
```

Dos detalles que cuestan tiempo si no se saben:

1. **`permissions: RrWw`** — las mayúsculas son para la colección y las
   minúsculas para los ítems. Con solo `RW` el `MKCOL` funciona pero cada
   `PUT` de un `.ics` devuelve `403 Forbidden`.
2. **`MKCOL` a secas crea una colección genérica**, no un calendario: no
   anuncia `resourcetype: calendar` ni `supported-calendar-component-set`, y
   la app no la verá. Hay que crear el calendario con cuerpo:

```bash
curl -u testuser:testuser -X MKCOL \
  -H "Content-Type: application/xml" \
  --data '<?xml version="1.0" encoding="utf-8"?>
<mkcol xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <set><prop>
    <resourcetype><collection/><C:calendar/></resourcetype>
    <C:supported-calendar-component-set><C:comp name="VTODO"/></C:supported-calendar-component-set>
  </prop></set>
</mkcol>' \
  http://localhost:5232/testuser/agenda/
```

`MKCALENDAR` no está soportado por Radicale 3.9 (devuelve 403).

Comprobación:

```bash
curl -u testuser:testuser -X PROPFIND -H "Depth: 1" \
  -H "Content-Type: application/xml" \
  --data '<?xml version="1.0"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:resourcetype/><d:displayname/><c:supported-calendar-component-set/></d:prop></d:propfind>' \
  http://localhost:5232/testuser/
```

Debe aparecer `agenda` con `<C:calendar/>` y `<C:comp name="VTODO"/>`.

### Notas de comportamiento observadas en Radicale 3.9

- Con `auth.type = none` cualquier usuario vale, pero **hay que enviar
  credenciales igualmente**: sin `Authorization`, la petición es anónima y se
  rechaza con 401.
- `current-user-principal` no se resuelve desde `/`, así que
  `CalDavClient` cae a `/{usuario}/` usando el nombre de usuario configurado.
  Verificado: apuntando a `http://localhost:5232/` descubre `/testuser/` y sus
  calendarios.
- Un calendario recién creado y **nunca escrito** puede no exponer
  `sync-token`. El test de integración siembra una tarea para forzarlo.
- `sync-collection` sí está soportado y devuelve los borrados como
  `<response>` con `404`.

### Sondeo manual

```bash
cd caldav-client
RADICALE_URL=http://localhost:5232/testuser/agenda/ \
RADICALE_USER=testuser RADICALE_PASS=testuser \
cargo run --example probe
```

Imprime principal, home, calendarios, sync-token y VTODOs encontrados.

### Apagar y limpiar

```bash
docker rm -f proper-radicale
docker volume rm proper-radicale-data
```

## 4. Tests de integración

```bash
cd caldav-client

# Solo unitarios (no requiere servidor)
cargo test

# Integración contra Radicale
export RADICALE_URL=http://localhost:5232/testuser/tareas/
export RADICALE_USER=testuser
export RADICALE_PASS=testuser
cargo test --test radicale -- --ignored --nocapture
```

Si las variables no están definidas, los tests se saltan solos con un mensaje.

## 5. Servidor Radicale del usuario

Pendiente de configurar. Cuando esté:

```bash
export RADICALE_URL=http://<host>:5232/<usuario>/<coleccion>/
export RADICALE_USER=<usuario>
export RADICALE_PASS=<contraseña>
```

No commitear nunca estas credenciales. En la app irán al keyring del sistema.

## 6. Push a GitHub (SSH)

La clave existe pero **aún no está autorizada** en GitHub:

```
~/.ssh/github_proper      (privada)
~/.ssh/github_proper.pub  (pública)
Fingerprint: SHA256:1Ies6uGXBK0Tk4u9ynq5L9vBD19r4d592mFs/9RhdcI
```

Pasos:

1. Pegar el contenido de `github_proper.pub` en <https://github.com/settings/keys> → *New SSH key* (tipo **Authentication**, no Deploy key).
2. Añadir entrada a `~/.ssh/config`:

```
Host github.com
  HostName github.com
  User git
  IdentityFile ~/.ssh/github_proper
  IdentitiesOnly yes
```

3. Comprobar: `ssh -T git@github.com` debe responder `Hi KaesarImperator!`.
4. Apuntar `origin` al remoto SSH:

```bash
git remote set-url --push origin git@github.com:KaesarImperator/ProPer-ToDo.git
```

Si `ssh -T` devuelve `Permission denied (publickey)`, la clave no está guardada en la cuenta correcta: revisar que el fingerprint de la lista de GitHub coincida con el de arriba.

### Alternativa sin SSH

Token fine-grained con `Contents: Read and write` sobre el repo, guardado localmente:

```bash
git config --global credential.helper store
# el primer push pedirá usuario y token; queda en ~/.git-credentials
```

## 7. Flujo de trabajo con upstream

```bash
git fetch upstream
git log --oneline HEAD..upstream/main   # qué hay nuevo
git rebase upstream/main                # opcional
```

Mantener los cambios propios en commits pequeños y separados por fase, para que un rebase contra upstream no duela.
