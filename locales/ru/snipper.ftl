# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Закрыть
color = Цвет
back = ← Назад
open = Открыть
open-folder = Открыть папку

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Создать
new-tip = Начать захват в выбранном режиме (после таймера)
camera = Камера
camera-tip = Сделать снимок экрана (фото)
video = Видео
video-tip = Записать экран (область / окно / весь экран) в .fvid
mode-tip = Режим захвата для «+ Создать» и горячей клавиши
timer-tip = Задержка перед началом захвата
color-tip-editor = Цвет разметки для инструментов редактора
theme-toggle-tip = Переключить светлую/тёмную тему

# --- Recording controls ---
rec-stop-tip = Остановить и сохранить запись
rec-pause-tip = Приостановить / возобновить запись

# --- Recent captures ---
recent-heading = Недавние захваты
edit-timeline = Изменить (таймлайн)
remove-from-list = Удалить из списка

# --- Settings ---
settings-heading = Настройки
setting-hotkey = Горячая клавиша захвата
setting-timer = Таймер захвата
setting-default-mode = Режим захвата по умолчанию
setting-image-format = Формат изображения по умолчанию
setting-theme = Тема
setting-language = Язык интерфейса
setting-save-folder = Папка сохранения
change = Изменить…
save-folder-tip = Выберите, куда сохранять захваты
language-note = Перевод интерфейса появится в Phase 7; выбор языка здесь сохраняет ваши настройки.
settings-capture-heading = Захват
settings-recording-heading = Запись
setting-frame-rate = Частота кадров
mic-tip = Добавить звук с микрофона в запись (например, для озвучивания).
tray-note = Системный трей работает в Windows и macOS; поддержка Linux появится в Phase 7.
settings-printscreen-heading = Print Screen
printscreen-tip = Использовать клавишу Print Screen для запуска захвата (по желанию, обратимо)
printscreen-macos-note = macOS: приложение не может переопределить системные сочетания для снимков экрана — используйте шаги ниже.
open-system-settings = Открыть «Системные настройки»

# --- About ---
about-heading = О Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Все права защищены
about-project-started = Проект начат: 16 июня 2026 г. · 2:35 PM CDT
about-released = v1.0.0 выпущена: ______

# --- Capture overlay ---
overlay-photo-tip = Фотозахват (снимок экрана)
overlay-record-tip = Запись экрана (сохраняется .fvid для воспроизведения и экспорта)
overlay-shape-tip = Форма выделения
overlay-color-tip = Цвет разметки (произвольный контур + инструменты редактора)
overlay-cancel-tip = Отменить захват (Esc)

# --- Player ---
player-restart = ⟲ Restart
player-restart-tip = Воспроизвести с начала
player-edit = ✎ Edit
player-edit-tip = Открыть эту запись в редакторе таймлайна

# --- Timeline editor ---
tl-go-start = В начало
tl-step-back = На один кадр назад
tl-play-pause = Воспроизвести / пауза (Space)
tl-step-fwd = На один кадр вперёд
tl-go-end = В конец
tl-split = Разделить
tl-split-tip = Разделить клип под указателем (активная дорожка)
tl-ripple-tip = Удалить выбранный клип со сдвигом (закрывает промежуток)
tl-clip = Клип:
tl-opacity = Непрозрачность
tl-gain = Усиление
tl-fade = Появление/затухание
