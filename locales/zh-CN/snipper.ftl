# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = 关闭
color = 颜色
back = ← 返回
open = 打开
open-folder = 打开文件夹

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + 新建
new-tip = 在所选截取模式下开始捕获（计时结束后）
camera = 拍照
camera-tip = 截取屏幕截图（照片）
video = 录像
video-tip = 将屏幕（区域 / 窗口 / 全屏）录制为 .fvid
mode-tip = + 新建和热键捕获的内容
timer-tip = 捕获开始前的延迟
color-tip-editor = 编辑器工具的标注颜色
theme-toggle-tip = 切换浅色/深色主题

# --- Recording controls ---
rec-stop-tip = 停止并保存录像
rec-pause-tip = 暂停 / 继续录像

# --- Recent captures ---
recent-heading = 最近的捕获
edit-timeline = 编辑（时间轴）
remove-from-list = 从列表中移除

# --- Settings ---
settings-heading = 设置
setting-hotkey = 捕获热键
setting-timer = 捕获计时器
setting-default-mode = 默认截取模式
setting-image-format = 默认图片格式
setting-theme = 主题
setting-language = 界面语言
setting-save-folder = 保存文件夹
change = 更改…
save-folder-tip = 选择捕获内容的保存位置
language-note = 界面翻译将在 Phase 7 推出；在此选择语言会保存您的选择。
settings-capture-heading = 捕获
settings-recording-heading = 录像
setting-frame-rate = 帧率
mic-tip = 将麦克风混入录像（例如用于配音）。
tray-note = 系统托盘支持 Windows 和 macOS；Linux 支持将在 Phase 7 推出。
settings-printscreen-heading = Print Screen
printscreen-tip = 使用 Print Screen 键开始捕获（可选启用，可还原）
printscreen-macos-note = macOS：系统截图快捷键无法被应用覆盖——请按以下步骤操作。
open-system-settings = 打开系统设置

# --- About ---
about-heading = 关于 Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — 保留所有权利
about-project-started = 项目启动：2026 年 6 月 16 日 · 下午 2:35 CDT
about-released = v1.0.0 发布：______

# --- Capture overlay ---
overlay-photo-tip = 照片捕获（截图）
overlay-record-tip = 屏幕录制（保存可播放并导出的 .fvid）
overlay-shape-tip = 选区形状
overlay-color-tip = 标注颜色（自由轮廓 + 编辑器工具）
overlay-cancel-tip = 取消捕获（Esc）

# --- Player ---
player-restart = ⟲ 重新播放
player-restart-tip = 从头播放
player-edit = ✎ 编辑
player-edit-tip = 在时间轴编辑器中打开此录像

# --- Timeline editor ---
tl-go-start = 跳到开头
tl-step-back = 后退一帧
tl-play-pause = 播放 / 暂停（Space）
tl-step-fwd = 前进一帧
tl-go-end = 跳到结尾
tl-split = 分割
tl-split-tip = 在播放头处分割片段（当前轨道）
tl-ripple-tip = 波纹删除所选片段（自动闭合空隙）
tl-clip = 片段：
tl-opacity = 不透明度
tl-gain = 增益
tl-fade = 淡入/淡出
