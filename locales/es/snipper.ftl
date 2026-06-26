# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Cerrar
color = Color
back = ← Atrás
open = Abrir
open-folder = Abrir carpeta

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Nuevo
new-tip = Inicia una captura en el modo de fragmento seleccionado (tras el temporizador)
camera = Cámara
camera-tip = Hacer una captura de pantalla (foto)
video = Vídeo
video-tip = Graba la pantalla (región / ventana / pantalla completa) en un .fvid
mode-tip = Qué capturan + Nuevo y la tecla rápida
timer-tip = Retardo antes de que comience la captura
color-tip-editor = Color de marcado para las herramientas del editor
theme-toggle-tip = Cambiar entre tema claro y oscuro

# --- Recording controls ---
rec-stop-tip = Detener y guardar la grabación
rec-pause-tip = Pausar / reanudar la grabación

# --- Recent captures ---
recent-heading = Capturas recientes
edit-timeline = Editar (línea de tiempo)
remove-from-list = Quitar de la lista

# --- Settings ---
settings-heading = Ajustes
setting-hotkey = Tecla rápida de captura
setting-timer = Temporizador de captura
setting-default-mode = Modo de fragmento predeterminado
setting-image-format = Formato de imagen predeterminado
setting-theme = Tema
setting-language = Idioma de la interfaz
setting-save-folder = Carpeta de destino
change = Cambiar…
save-folder-tip = Elige dónde se guardan las capturas
language-note = La traducción de la interfaz llega en Phase 7; seleccionar un idioma aquí guarda tu elección.
settings-capture-heading = Captura
settings-recording-heading = Grabación
setting-frame-rate = Fotogramas por segundo
mic-tip = Mezcla tu micrófono en la grabación (p. ej. para narrar).
tray-note = La bandeja del sistema funciona en Windows y macOS; la compatibilidad con Linux llega en Phase 7.
settings-printscreen-heading = Imprimir pantalla
printscreen-tip = Usa la tecla Imprimir pantalla para iniciar una captura (opcional, reversible)
printscreen-macos-note = macOS: una app no puede anular los atajos de captura del sistema; sigue los pasos de abajo.
open-system-settings = Abrir Ajustes del sistema

# --- About ---
about-heading = Acerca de Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Todos los derechos reservados
about-project-started = Proyecto iniciado: 16 de junio de 2026 · 2:35 PM CDT
about-released = v1.0.0 publicado: ______

# --- Capture overlay ---
overlay-photo-tip = Captura de foto (captura de pantalla)
overlay-record-tip = Grabación de pantalla (guarda un .fvid que puedes reproducir y exportar)
overlay-shape-tip = Forma de selección
overlay-color-tip = Color de marcado (contorno libre y las herramientas del editor)
overlay-cancel-tip = Cancelar la captura (Esc)

# --- Player ---
player-restart = ⟲ Reiniciar
player-restart-tip = Reproducir desde el inicio
player-edit = ✎ Editar
player-edit-tip = Abrir esta grabación en el editor de línea de tiempo

# --- Timeline editor ---
tl-go-start = Ir al inicio
tl-step-back = Retroceder un fotograma
tl-play-pause = Reproducir / pausar (Espacio)
tl-step-fwd = Avanzar un fotograma
tl-go-end = Ir al final
tl-split = Dividir
tl-split-tip = Divide el clip bajo el cursor de reproducción (pista activa)
tl-ripple-tip = Elimina en cascada el clip seleccionado (cierra el hueco)
tl-clip = Clip:
tl-opacity = Opacidad
tl-gain = Ganancia
tl-fade = Fundido de entrada/salida
