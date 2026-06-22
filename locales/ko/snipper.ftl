# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = 닫기
color = 색상
back = ← 뒤로
open = 열기
open-folder = 폴더 열기

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + 새로 만들기
new-tip = 선택한 스니펫 모드로 (타이머 후) 캡처를 시작합니다
camera = 카메라
camera-tip = 스크린샷(사진)을 찍습니다
video = 비디오
video-tip = 화면(영역 / 창 / 전체 화면)을 .fvid로 녹화합니다
mode-tip = + 새로 만들기와 단축키로 캡처할 대상
timer-tip = 캡처 시작 전 지연 시간
color-tip-editor = 편집기 도구의 마크업 색상
theme-toggle-tip = 밝은/어두운 테마 전환

# --- Recording controls ---
rec-stop-tip = 녹화를 중지하고 저장합니다
rec-pause-tip = 녹화 일시정지 / 재개

# --- Recent captures ---
recent-heading = 최근 캡처
edit-timeline = 편집(타임라인)
remove-from-list = 목록에서 제거

# --- Settings ---
settings-heading = 설정
setting-hotkey = 캡처 단축키
setting-timer = 캡처 타이머
setting-default-mode = 기본 스니펫 모드
setting-image-format = 기본 이미지 형식
setting-theme = 테마
setting-language = UI 언어
setting-save-folder = 저장 폴더
change = 변경…
save-folder-tip = 캡처를 저장할 위치를 선택합니다
language-note = UI 번역은 Phase 7에서 제공됩니다. 여기서 언어를 선택하면 설정이 저장됩니다.
settings-capture-heading = 캡처
settings-recording-heading = 녹화
setting-frame-rate = 프레임 속도
mic-tip = 마이크를 녹화에 함께 믹스합니다(예: 내레이션 추가).
tray-note = 시스템 트레이는 Windows와 macOS에서 작동합니다. Linux 지원은 Phase 7에서 제공됩니다.
settings-printscreen-heading = Print Screen
printscreen-tip = Print Screen 키로 캡처를 시작합니다(선택 사항, 되돌릴 수 있음)
printscreen-macos-note = macOS: 시스템 스크린샷 단축키는 앱에서 재정의할 수 없습니다 — 아래 단계를 따르세요.
open-system-settings = 시스템 설정 열기

# --- About ---
about-heading = Freally Snipper 정보
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — All Rights Reserved
about-project-started = 프로젝트 시작: 2026년 6월 16일 · 오후 2:35 CDT
about-released = v1.0.0 출시: ______

# --- Capture overlay ---
overlay-photo-tip = 사진 캡처(스크린샷)
overlay-record-tip = 화면 녹화(재생 및 내보내기가 가능한 .fvid로 저장)
overlay-shape-tip = 선택 모양
overlay-color-tip = 마크업 색상(자유형 윤곽선 + 편집기 도구)
overlay-cancel-tip = 캡처 취소(Esc)

# --- Player ---
player-restart = ⟲ 다시 시작
player-restart-tip = 처음부터 재생
player-edit = ✎ 편집
player-edit-tip = 이 녹화를 타임라인 편집기에서 엽니다

# --- Timeline editor ---
tl-go-start = 처음으로 이동
tl-step-back = 한 프레임 뒤로
tl-play-pause = 재생 / 일시정지(Space)
tl-step-fwd = 한 프레임 앞으로
tl-go-end = 끝으로 이동
tl-split = 분할
tl-split-tip = 재생 헤드 위치에서 클립을 분할합니다(활성 트랙)
tl-ripple-tip = 선택한 클립을 리플 삭제합니다(빈 공간을 닫음)
tl-clip = 클립:
tl-opacity = 불투명도
tl-gain = 게인
tl-fade = 페이드 인/아웃
