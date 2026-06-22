# Freally Snipper — English (source locale for the Freally Translator).
#
# Extracted from the egui UI as Phase-7 i18n groundwork. The app does NOT yet load
# these strings (that wiring — a loader + replacing the inline literals — is Phase 7);
# this catalog exists so the Translator can produce every-language versions now.
# Keys are grouped by UI surface. Pure-icon buttons (⏮ ◀ ▶| ⏭ 🗑) are intentionally
# omitted — there is no text to translate.

# --- Common ---
close = Fechar
color = Cor
back = ← Voltar
open = Abrir
open-folder = Abrir pasta

# --- Home window / toolbar ---
app-title = Freally Snipper
new = + Novo
new-tip = Iniciar uma captura no modo de snippet selecionado (após o cronômetro)
camera = Câmera
camera-tip = Tirar uma captura de tela (foto)
video = Vídeo
video-tip = Gravar a tela (região / janela / tela cheia) em um .fvid
mode-tip = O que + Novo e a tecla de atalho capturam
timer-tip = Atraso antes do início da captura
color-tip-editor = Cor de marcação para as ferramentas do editor
theme-toggle-tip = Alternar entre tema claro e escuro

# --- Recording controls ---
rec-stop-tip = Parar e salvar a gravação
rec-pause-tip = Pausar / retomar a gravação

# --- Recent captures ---
recent-heading = Capturas recentes
edit-timeline = Editar (linha do tempo)
remove-from-list = Remover da lista

# --- Settings ---
settings-heading = Configurações
setting-hotkey = Tecla de atalho de captura
setting-timer = Cronômetro de captura
setting-default-mode = Modo de snippet padrão
setting-image-format = Formato de imagem padrão
setting-theme = Tema
setting-language = Idioma da interface
setting-save-folder = Pasta para salvar
change = Alterar…
save-folder-tip = Escolha onde as capturas são salvas
language-note = A tradução da interface chega na Phase 7; selecionar um idioma aqui salva sua escolha.
settings-capture-heading = Captura
settings-recording-heading = Gravação
setting-frame-rate = Taxa de quadros
mic-tip = Misture o microfone na gravação (por exemplo, para narrar).
tray-note = A bandeja do sistema funciona no Windows e no macOS; o suporte ao Linux chega na Phase 7.
settings-printscreen-heading = Print Screen
printscreen-tip = Use a tecla Print Screen para iniciar uma captura (opcional, reversível)
printscreen-macos-note = macOS: os atalhos de captura de tela do sistema não podem ser substituídos por um aplicativo — use as etapas abaixo.
open-system-settings = Abrir Configurações do Sistema

# --- About ---
about-heading = Sobre o Freally Snipper
about-copyright = © Mike Weaver <mythodikalone@gmail.com> — Todos os direitos reservados
about-project-started = Projeto iniciado: 16 de junho de 2026 · 14h35 CDT
about-released = v1.0.0 lançado: ______

# --- Capture overlay ---
overlay-photo-tip = Captura de foto (captura de tela)
overlay-record-tip = Gravação de tela (salva um .fvid que você pode reproduzir + exportar)
overlay-shape-tip = Forma da seleção
overlay-color-tip = Cor de marcação (contorno livre + ferramentas do editor)
overlay-cancel-tip = Cancelar a captura (Esc)

# --- Player ---
player-restart = ⟲ Reiniciar
player-restart-tip = Reproduzir desde o início
player-edit = ✎ Editar
player-edit-tip = Abrir esta gravação no editor de linha do tempo

# --- Timeline editor ---
tl-go-start = Ir para o início
tl-step-back = Voltar um quadro
tl-play-pause = Reproduzir / pausar (Espaço)
tl-step-fwd = Avançar um quadro
tl-go-end = Ir para o fim
tl-split = Dividir
tl-split-tip = Dividir o clipe sob o cursor de reprodução (faixa ativa)
tl-ripple-tip = Excluir com ondulação o clipe selecionado (fecha o espaço)
tl-clip = Clipe:
tl-opacity = Opacidade
tl-gain = Ganho
tl-fade = Fade de entrada/saída
