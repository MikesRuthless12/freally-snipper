# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Fermer
color = Couleur
back = ← Retour
open = Ouvrir
open-folder = Ouvrir le dossier

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Nouveau
new-tip = Démarrer une capture dans le mode sélectionné (après le minuteur)
camera = Appareil photo
camera-tip = Prendre une capture d'écran (photo)
video = Vidéo
video-tip = Enregistrer l'écran (région / fenêtre / plein écran) dans un .fvid
mode-tip = Ce que capturent + Nouveau et le raccourci
timer-tip = Délai avant le début de la capture
color-tip-editor = Couleur d'annotation pour les outils de l'éditeur
theme-toggle-tip = Basculer entre le thème clair et sombre

# --- Recording controls ---
rec-stop-tip = Arrêter et enregistrer la capture
rec-pause-tip = Mettre en pause / reprendre l'enregistrement

# --- Recent captures ---
recent-heading = Captures récentes
edit-timeline = Modifier (montage)
remove-from-list = Retirer de la liste

# --- Settings ---
settings-heading = Paramètres
setting-hotkey = Raccourci de capture
setting-timer = Minuteur de capture
setting-default-mode = Mode de capture par défaut
setting-image-format = Format d'image par défaut
setting-theme = Thème
setting-language = Langue de l'interface
setting-save-folder = Dossier d'enregistrement
change = Modifier…
save-folder-tip = Choisir où enregistrer les captures
language-note = La traduction de l'interface arrive en Phase 7 ; choisir une langue ici enregistre votre préférence.
settings-capture-heading = Capture
settings-recording-heading = Enregistrement
setting-frame-rate = Fréquence d'images
mic-tip = Intégrer votre microphone à l'enregistrement (par ex. pour commenter).
tray-note = La zone de notification fonctionne sous Windows et macOS ; la prise en charge de Linux arrive en Phase 7.
settings-printscreen-heading = Impr. écran
printscreen-tip = Utiliser la touche Impr. écran pour démarrer une capture (optionnel, réversible)
printscreen-macos-note = macOS : les raccourcis de capture du système ne peuvent pas être remplacés par une application — suivez les étapes ci-dessous.
open-system-settings = Ouvrir les Réglages système

# --- About ---
about-heading = À propos de Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Tous droits réservés
about-project-started = Projet démarré : 16 juin 2026 · 14 h 35 CDT
about-released = v1.0.0 publié : ______

# --- Capture overlay ---
overlay-photo-tip = Capture photo (capture d'écran)
overlay-record-tip = Enregistrement d'écran (crée un .fvid que vous pouvez lire et exporter)
overlay-shape-tip = Forme de sélection
overlay-color-tip = Couleur d'annotation (contour libre + outils de l'éditeur)
overlay-cancel-tip = Annuler la capture (Échap)

# --- Player ---
player-restart = ⟲ Recommencer
player-restart-tip = Lire depuis le début
player-edit = ✎ Modifier
player-edit-tip = Ouvrir cet enregistrement dans l'éditeur de montage

# --- Timeline editor ---
tl-go-start = Aller au début
tl-step-back = Reculer d'une image
tl-play-pause = Lecture / pause (Espace)
tl-step-fwd = Avancer d'une image
tl-go-end = Aller à la fin
tl-split = Diviser
tl-split-tip = Diviser le clip sous la tête de lecture (piste active)
tl-ripple-tip = Supprimer le clip sélectionné en cascade (referme l'espace)
tl-clip = Clip :
tl-opacity = Opacité
tl-gain = Gain
tl-fade = Fondu d'entrée/sortie
