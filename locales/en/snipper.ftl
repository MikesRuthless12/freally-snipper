# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Close
color = Color
back = ← Back
open = Open
open-folder = Open folder

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + New
new-tip = Start a capture in the selected snippet mode (after the timer)
camera = Camera
camera-tip = Take a screenshot (photo)
video = Video
video-tip = Record the screen (region / window / full screen) to a .fvid
mode-tip = What + New and the hotkey capture
timer-tip = Delay before the capture starts
color-tip-editor = Markup colour for the editor's tools
theme-toggle-tip = Toggle light/dark theme

# --- Recording controls ---
rec-stop-tip = Stop and save the recording
rec-pause-tip = Pause / resume the recording

# --- Recent captures ---
recent-heading = Recent captures
edit-timeline = Edit (timeline)
remove-from-list = Remove from list

# --- Settings ---
settings-heading = Settings
setting-hotkey = Capture hotkey
setting-timer = Capture timer
setting-default-mode = Default snippet mode
setting-image-format = Default image format
setting-theme = Theme
setting-language = UI language
setting-save-folder = Save folder
change = Change…
save-folder-tip = Choose where captures are saved
language-note = UI translation arrives in Phase 7; selecting a language here saves your choice.
settings-capture-heading = Capture
settings-recording-heading = Recording
setting-frame-rate = Frame rate
mic-tip = Mix your microphone into the recording (e.g. to narrate).
tray-note = System tray runs on Windows and macOS; Linux support arrives in Phase 7.
settings-printscreen-heading = Print Screen
printscreen-tip = Use the Print Screen key to start a capture (opt-in, reversible)
printscreen-macos-note = macOS: the system screenshot shortcuts can't be overridden by an app — use the steps below.
open-system-settings = Open System Settings

# --- About ---
about-heading = About Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — All Rights Reserved
about-project-started = Project started: June 16th, 2026 · 2:35 PM CDT
about-released = v1.0.0 released: ______

# --- Capture overlay ---
overlay-photo-tip = Photo capture (screenshot)
overlay-record-tip = Screen recording (saves a .fvid you can play + export)
overlay-shape-tip = Selection shape
overlay-color-tip = Markup colour (freeform outline + the editor's tools)
overlay-cancel-tip = Cancel the capture (Esc)

# --- Player ---
player-restart = ⟲ Restart
player-restart-tip = Play from the start
player-edit = ✎ Edit
player-edit-tip = Open this recording in the timeline editor

# --- Timeline editor ---
tl-go-start = Go to start
tl-step-back = Step back one frame
tl-play-pause = Play / pause (Space)
tl-step-fwd = Step forward one frame
tl-go-end = Go to end
tl-split = Split
tl-split-tip = Split the clip under the playhead (active track)
tl-ripple-tip = Ripple-delete the selected clip (closes the gap)
tl-clip = Clip:
tl-opacity = Opacity
tl-gain = Gain
tl-fade = Fade in/out
