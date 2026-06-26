# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = 閉じる
color = 色
back = ← 戻る
open = 開く
open-folder = フォルダーを開く

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + 新規
new-tip = 選択中のスニペットモードでキャプチャを開始します（タイマー後）
camera = カメラ
camera-tip = スクリーンショットを撮影します（写真）
video = ビデオ
video-tip = 画面（範囲／ウィンドウ／全画面）を .fvid に録画します
mode-tip = + 新規とホットキーでキャプチャする内容
timer-tip = キャプチャ開始までの遅延
color-tip-editor = エディターのツールで使う注釈の色
theme-toggle-tip = ライト／ダークテーマを切り替え

# --- Recording controls ---
rec-stop-tip = 録画を停止して保存
rec-pause-tip = 録画を一時停止／再開

# --- Recent captures ---
recent-heading = 最近のキャプチャ
edit-timeline = 編集（タイムライン）
remove-from-list = リストから削除

# --- Settings ---
settings-heading = 設定
setting-hotkey = キャプチャのホットキー
setting-timer = キャプチャタイマー
setting-default-mode = 既定のスニペットモード
setting-image-format = 既定の画像形式
setting-theme = テーマ
setting-language = UIの言語
setting-save-folder = 保存先フォルダー
change = 変更…
save-folder-tip = キャプチャの保存先を選択
language-note = UIの翻訳は Phase 7 で対応します。ここで言語を選択すると、その選択が保存されます。
settings-capture-heading = キャプチャ
settings-recording-heading = 録画
setting-frame-rate = フレームレート
mic-tip = マイクの音声を録画にミックスします（例：ナレーション用）。
tray-note = システムトレイは Windows と macOS で動作します。Linux 対応は Phase 7 で追加されます。
settings-printscreen-heading = Print Screen
printscreen-tip = Print Screen キーでキャプチャを開始します（任意設定、取り消し可能）
printscreen-macos-note = macOS：システムのスクリーンショットショートカットはアプリで上書きできません。以下の手順を使用してください。
open-system-settings = システム設定を開く

# --- About ---
about-heading = Freally Snipper について
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — All Rights Reserved
about-project-started = プロジェクト開始：2026年6月16日 · 午後2:35 CDT
about-released = v1.0.0 リリース：______

# --- Capture overlay ---
overlay-photo-tip = 写真キャプチャ（スクリーンショット）
overlay-record-tip = 画面録画（再生・書き出しできる .fvid を保存）
overlay-shape-tip = 選択範囲の形状
overlay-color-tip = 注釈の色（自由形式のアウトラインとエディターのツール）
overlay-cancel-tip = キャプチャをキャンセル（Esc）

# --- Player ---
player-restart = ⟲ 最初から
player-restart-tip = 最初から再生
player-edit = ✎ 編集
player-edit-tip = この録画をタイムラインエディターで開く

# --- Timeline editor ---
tl-go-start = 先頭へ移動
tl-step-back = 1フレーム戻る
tl-play-pause = 再生／一時停止（Space）
tl-step-fwd = 1フレーム進む
tl-go-end = 末尾へ移動
tl-split = 分割
tl-split-tip = 再生ヘッド位置でクリップを分割（アクティブなトラック）
tl-ripple-tip = 選択したクリップをリップル削除（隙間を詰めます）
tl-clip = クリップ：
tl-opacity = 不透明度
tl-gain = ゲイン
tl-fade = フェードイン／アウト
