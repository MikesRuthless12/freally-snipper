# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Закрити
color = Колір
back = ← Назад
open = Відкрити
open-folder = Відкрити теку

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Створити
new-tip = Почати захоплення у вибраному режимі знімка (після таймера)
camera = Камера
camera-tip = Зробити знімок екрана (фото)
video = Відео
video-tip = Записати екран (область / вікно / весь екран) у .fvid
mode-tip = Що захоплюють + Створити та гаряча клавіша
timer-tip = Затримка перед початком захоплення
color-tip-editor = Колір розмітки для інструментів редактора
theme-toggle-tip = Перемкнути світлу/темну тему

# --- Recording controls ---
rec-stop-tip = Зупинити та зберегти запис
rec-pause-tip = Призупинити / відновити запис

# --- Recent captures ---
recent-heading = Нещодавні захоплення
edit-timeline = Редагувати (монтажний стіл)
remove-from-list = Вилучити зі списку

# --- Settings ---
settings-heading = Налаштування
setting-hotkey = Гаряча клавіша захоплення
setting-timer = Таймер захоплення
setting-default-mode = Типовий режим знімка
setting-image-format = Типовий формат зображення
setting-theme = Тема
setting-language = Мова інтерфейсу
setting-save-folder = Тека збереження
change = Змінити…
save-folder-tip = Виберіть, куди зберігати захоплення
language-note = Переклад інтерфейсу з'явиться у Phase 7; вибір мови тут зберігає ваш вибір.
settings-capture-heading = Захоплення
settings-recording-heading = Запис
setting-frame-rate = Частота кадрів
mic-tip = Додати мікрофон до запису (наприклад, для озвучення).
tray-note = Системний лоток працює у Windows і macOS; підтримка Linux з'явиться у Phase 7.
settings-printscreen-heading = Print Screen
printscreen-tip = Використовувати клавішу Print Screen для початку захоплення (за вибором, оборотно)
printscreen-macos-note = macOS: застосунок не може перевизначити системні комбінації для знімків — скористайтеся кроками нижче.
open-system-settings = Відкрити системні налаштування

# --- About ---
about-heading = Про Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Усі права захищено
about-project-started = Проєкт розпочато: June 16th, 2026 · 2:35 PM CDT
about-released = v1.0.0 випущено: ______

# --- Capture overlay ---
overlay-photo-tip = Захоплення фото (знімок екрана)
overlay-record-tip = Запис екрана (зберігає .fvid, який можна відтворити та експортувати)
overlay-shape-tip = Форма виділення
overlay-color-tip = Колір розмітки (довільний контур + інструменти редактора)
overlay-cancel-tip = Скасувати захоплення (Esc)

# --- Player ---
player-restart = ⟲ Спочатку
player-restart-tip = Відтворити з початку
player-edit = ✎ Редагувати
player-edit-tip = Відкрити цей запис у редакторі монтажного столу

# --- Timeline editor ---
tl-go-start = Перейти на початок
tl-step-back = Назад на один кадр
tl-play-pause = Відтворити / пауза (Space)
tl-step-fwd = Уперед на один кадр
tl-go-end = Перейти в кінець
tl-split = Розділити
tl-split-tip = Розділити кліп під повзунком (активна доріжка)
tl-ripple-tip = Видалити вибраний кліп зі зсувом (закриває проміжок)
tl-clip = Кліп:
tl-opacity = Непрозорість
tl-gain = Підсилення
tl-fade = Наростання/згасання
