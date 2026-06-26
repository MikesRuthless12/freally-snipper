# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Chiudi
color = Colore
back = ← Indietro
open = Apri
open-folder = Apri cartella

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Nuovo
new-tip = Avvia una cattura nella modalità di acquisizione selezionata (dopo il timer)
camera = Fotocamera
camera-tip = Acquisisci uno screenshot (foto)
video = Video
video-tip = Registra lo schermo (area / finestra / schermo intero) in un .fvid
mode-tip = Cosa cattura + Nuovo e la scorciatoia
timer-tip = Ritardo prima dell'avvio della cattura
color-tip-editor = Colore di markup per gli strumenti dell'editor
theme-toggle-tip = Alterna tema chiaro/scuro

# --- Recording controls ---
rec-stop-tip = Arresta e salva la registrazione
rec-pause-tip = Metti in pausa / riprendi la registrazione

# --- Recent captures ---
recent-heading = Catture recenti
edit-timeline = Modifica (timeline)
remove-from-list = Rimuovi dall'elenco

# --- Settings ---
settings-heading = Impostazioni
setting-hotkey = Scorciatoia di cattura
setting-timer = Timer di cattura
setting-default-mode = Modalità di acquisizione predefinita
setting-image-format = Formato immagine predefinito
setting-theme = Tema
setting-language = Lingua dell'interfaccia
setting-save-folder = Cartella di salvataggio
change = Cambia…
save-folder-tip = Scegli dove salvare le catture
language-note = La traduzione dell'interfaccia arriverà nella Phase 7; selezionando qui una lingua la tua scelta viene salvata.
settings-capture-heading = Cattura
settings-recording-heading = Registrazione
setting-frame-rate = Frequenza fotogrammi
mic-tip = Includi il microfono nella registrazione (ad es. per narrare).
tray-note = L'icona nella barra delle applicazioni funziona su Windows e macOS; il supporto Linux arriverà nella Phase 7.
settings-printscreen-heading = Stamp
printscreen-tip = Usa il tasto Stamp per avviare una cattura (opzionale, reversibile)
printscreen-macos-note = macOS: le scorciatoie di screenshot di sistema non possono essere sovrascritte da un'app — segui i passaggi qui sotto.
open-system-settings = Apri Impostazioni di sistema

# --- About ---
about-heading = Informazioni su Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Tutti i diritti riservati
about-project-started = Progetto avviato: 16 giugno 2026 · 14:35 CDT
about-released = v1.0.0 rilasciato: ______

# --- Capture overlay ---
overlay-photo-tip = Cattura foto (screenshot)
overlay-record-tip = Registrazione dello schermo (salva un .fvid riproducibile ed esportabile)
overlay-shape-tip = Forma della selezione
overlay-color-tip = Colore di markup (contorno a mano libera + strumenti dell'editor)
overlay-cancel-tip = Annulla la cattura (Esc)

# --- Player ---
player-restart = ⟲ Riavvia
player-restart-tip = Riproduci dall'inizio
player-edit = ✎ Modifica
player-edit-tip = Apri questa registrazione nell'editor della timeline

# --- Timeline editor ---
tl-go-start = Vai all'inizio
tl-step-back = Indietro di un fotogramma
tl-play-pause = Riproduci / pausa (Spazio)
tl-step-fwd = Avanti di un fotogramma
tl-go-end = Vai alla fine
tl-split = Dividi
tl-split-tip = Dividi la clip sotto l'indicatore di riproduzione (traccia attiva)
tl-ripple-tip = Elimina con ripple la clip selezionata (chiude lo spazio vuoto)
tl-clip = Clip:
tl-opacity = Opacità
tl-gain = Guadagno
tl-fade = Dissolvenza in entrata/uscita
