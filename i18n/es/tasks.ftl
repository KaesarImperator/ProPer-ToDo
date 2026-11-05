tasks = Tareas
trash = Basura
about = Acerca de

# Content
add-new-task = Añadir nueva tarea
search-tasks = Buscar tareas

# Details
title = Título
details = Detalles
favorite = Favorito
priority = Prioridad
due-date = Fecha de vencimiento
reminder = Recordatorio
notes = Notas
add-notes = Añadir notas

# Empty
no-tasks = No hay tareas
no-tasks-suggestion = Intenta añadir una nueva tarea con el campo de texto de abajo
no-list-selected = No hay lista seleccionada
no-list-suggestion = Crea o selecciona una lista para comenzar

sub-tasks = Subtareas
add-sub-task = Añadir subtarea

# New List Dialog
create-list = Crear nueva lista de tareas

# Rename List Dialog
rename-list = Renombrar lista de tareas

# Rename List Dialog
delete-list = La lista seleccionada será eliminada
delete-list-confirm = ¿Estas seguro de que deseas eliminar esta lista?

# Icon Dialog
icon = Establecer icono...
icon-select = Seleccionar un icono
icon-select-body = Seleccionar un icono para la lista de tareas
search-icons = Buscar iconos...

# Date Dialog
select-date = Selecciona una fecha

# Export Dialog
export = Exportar

# Dialogs
cancel = Cancelar
ok = Ok
copy = Copiar
confirm = Confirmar
save = Guardar
list-name = Nombre de la lista

# Context Pages

## Settings
settings = Ajustes

### Appearance
appearance = Apariencia
theme = Tema
match-desktop = Igualar escritorio
dark = Oscuro
light = Claro

# Menu

## File
file = Archivo
new-window = Nueva ventana
new-list = Nueva lista...
quit = Cerrar

## Edit
edit = Editar
rename = Renombrar...
delete = Eliminar

## View
view = Ver
menu-settings = Ajustes...
menu-about = Acerca de Tareas...
hide-completed = Ocultar completadas

## About
repository = Repositorio
support = Soporte
website = Sitio web

# Sort Menu
sort = Ordenar
sort-name-asc = Nombre A-Z
sort-name-desc = Nombre Z-A
sort-date-asc = Fecha de añadido (Antiguo a Nuevo)
sort-date-desc = Fecha de añadido (Nuevo a Antiguo)

# Error
cause = Causa
oops-something-wrong = ¡Vaya! Algo salió mal.
error-title = Tareas - Error
sync-notices = { $count ->
    [one] 1 aviso
   *[other] { $count } avisos
}
sync-failed = Error al sincronizar: { $error }
caldav = Calendarios CalDAV
caldav-none = Ningún calendario CalDAV conectado.
caldav-count = { $count ->
    [one] 1 conexión
   *[other] { $count } conexiones
}
manage = Gestionar
caldav-add = Añadir conexión
caldav-edit = Editar conexión
caldav-saved = Conexión guardada
caldav-delete-confirm = ¿Eliminar esta conexión? Sus tareas pasan a la papelera; el servidor no se toca.
caldav-name = Nombre
caldav-url = URL del servidor
caldav-username = Usuario
caldav-password = Contraseña
caldav-password-keep = Déjala vacía para conservar la guardada
caldav-test = Probar conexión
caldav-testing = Probando...
caldav-test-ok = { $count ->
    [one] Conectado: 1 calendario con tareas
   *[other] Conectado: { $count } calendarios con tareas
}
caldav-calendar = Calendario
caldav-err-credentials = Usuario o contraseña incorrectos.
caldav-err-timeout = El servidor no responde.
caldav-err-url = La URL no es válida (usa http:// o https://).
caldav-err-no-calendars = El servidor no tiene ningún calendario que admita tareas.
caldav-err-missing-password = Introduce la contraseña.
caldav-err-keyring = No se pudo usar el llavero del sistema: { $detail }
caldav-err-server = El servidor respondió con el error { $status }.
caldav-err-other = Falló la conexión: { $detail }
