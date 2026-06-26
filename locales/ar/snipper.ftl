# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = إغلاق
color = اللون
back = ← رجوع
open = فتح
open-folder = فتح المجلد

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + جديد
new-tip = ابدأ التقاطًا بوضع القصاصة المحدد (بعد المؤقّت)
camera = كاميرا
camera-tip = التقاط لقطة شاشة (صورة)
video = فيديو
video-tip = سجّل الشاشة (منطقة / نافذة / كامل الشاشة) إلى ملف .fvid
mode-tip = ما يلتقطه زرّ + جديد ومفتاح الاختصار
timer-tip = مهلة قبل بدء الالتقاط
color-tip-editor = لون التحديد لأدوات المحرّر
theme-toggle-tip = التبديل بين السمة الفاتحة والداكنة

# --- Recording controls ---
rec-stop-tip = إيقاف التسجيل وحفظه
rec-pause-tip = إيقاف مؤقّت / استئناف التسجيل

# --- Recent captures ---
recent-heading = الالتقاطات الأخيرة
edit-timeline = تحرير (الخط الزمني)
remove-from-list = إزالة من القائمة

# --- Settings ---
settings-heading = الإعدادات
setting-hotkey = مفتاح اختصار الالتقاط
setting-timer = مؤقّت الالتقاط
setting-default-mode = وضع القصاصة الافتراضي
setting-image-format = صيغة الصورة الافتراضية
setting-theme = السمة
setting-language = لغة الواجهة
setting-save-folder = مجلد الحفظ
change = تغيير…
save-folder-tip = اختر مكان حفظ الالتقاطات
language-note = تصل ترجمة الواجهة في Phase 7؛ اختيار لغة هنا يحفظ تفضيلك.
settings-capture-heading = الالتقاط
settings-recording-heading = التسجيل
setting-frame-rate = معدّل الإطارات
mic-tip = ادمج صوت الميكروفون في التسجيل (مثلًا للتعليق الصوتي).
tray-note = تعمل علبة النظام على Windows وmacOS؛ يصل دعم Linux في Phase 7.
settings-printscreen-heading = زر Print Screen
printscreen-tip = استخدم مفتاح Print Screen لبدء الالتقاط (اختياري وقابل للتراجع)
printscreen-macos-note = macOS: لا يمكن لأي تطبيق تجاوز اختصارات لقطة الشاشة بالنظام — اتّبع الخطوات أدناه.
open-system-settings = فتح إعدادات النظام

# --- About ---
about-heading = حول Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — جميع الحقوق محفوظة
about-project-started = بدأ المشروع: June 16th, 2026 · 2:35 PM CDT
about-released = إصدار v1.0.0: ______

# --- Capture overlay ---
overlay-photo-tip = التقاط صورة (لقطة شاشة)
overlay-record-tip = تسجيل الشاشة (يحفظ ملف .fvid يمكنك تشغيله وتصديره)
overlay-shape-tip = شكل التحديد
overlay-color-tip = لون التحديد (مخطّط حرّ وأدوات المحرّر)
overlay-cancel-tip = إلغاء الالتقاط (Esc)

# --- Player ---
player-restart = ⟲ إعادة التشغيل
player-restart-tip = التشغيل من البداية
player-edit = ✎ تحرير
player-edit-tip = افتح هذا التسجيل في محرّر الخط الزمني

# --- Timeline editor ---
tl-go-start = الانتقال إلى البداية
tl-step-back = التراجع إطارًا واحدًا
tl-play-pause = تشغيل / إيقاف مؤقّت (Space)
tl-step-fwd = التقدّم إطارًا واحدًا
tl-go-end = الانتقال إلى النهاية
tl-split = تقسيم
tl-split-tip = قسّم المقطع تحت رأس التشغيل (المسار النشط)
tl-ripple-tip = حذف متتابع للمقطع المحدد (يُغلق الفجوة)
tl-clip = المقطع:
tl-opacity = العتامة
tl-gain = الكسب
tl-fade = تلاشٍ للداخل/للخارج
