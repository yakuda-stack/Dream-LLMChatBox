#!/usr/bin/env bash
# Dream-LLMChatBox – Installer & Updater (Arch, Fedora, Debian/Ubuntu, openSUSE)
#
#   curl -fsSL https://raw.githubusercontent.com/yakuda-stack/Dream-LLMChatBox/main/install.sh | bash
#   curl -fsSL …/install.sh | bash -s -- uninstall
#
# Run it again = update. Builds the app from source and installs it into the system:
#   /usr/local/bin/dream-llmchatbox
#   /usr/local/share/applications/dream-llmchatbox.desktop
#   /usr/local/share/icons/hicolor/…/dream-llmchatbox.(png|svg)
# Chats and settings (~/.local/share/dream-llmchatbox, ~/.config/dream-llmchatbox) stay untouched.
set -euo pipefail

APP_ID="dream-llmchatbox"
REPO_URL="${DREAM_REPO:-https://github.com/yakuda-stack/Dream-LLMChatBox.git}"
BRANCH="${DREAM_BRANCH:-main}"
SRC_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/$APP_ID-src"
PREFIX="/usr/local"
BIN="$PREFIX/bin/$APP_ID"
DESKTOP="$PREFIX/share/applications/$APP_ID.desktop"
ICON_PNG="$PREFIX/share/icons/hicolor/256x256/apps/$APP_ID.png"
ICON_SVG="$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
DOC_DIR="$PREFIX/share/doc/$APP_ID"
RUST_MIN="1.88"

if [[ -t 1 ]]; then
    B=$'\e[1m'; G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[31m'; N=$'\e[0m'
else
    B=""; G=""; Y=""; R=""; N=""
fi
ok()   { echo "${G}✔${N} $*"; }
warn() { echo "${Y}⚠${N} $*"; }
fail() { echo "${R}✘${N} $*" >&2; exit 1; }
step() { echo; echo "${B}▶ $*${N}"; }

ask() {
    local answer=""
    if [[ -r /dev/tty ]]; then
        read -r -p "$1 [Y/n] " answer </dev/tty || answer=""
    fi
    [[ -z "$answer" || "$answer" =~ ^[YyJj] ]]
}

refresh_caches() {
    command -v update-desktop-database >/dev/null && sudo update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
    command -v gtk-update-icon-cache >/dev/null && sudo gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
}

[[ "$(id -u)" == 0 ]] && fail "Please run this as your normal user, not as root (sudo is asked when needed)."

case "${1:-}" in
    uninstall|--uninstall)
        step "Removing Dream-LLMChatBox"
        sudo rm -f "$BIN" "$DESKTOP" "$ICON_PNG" "$ICON_SVG"
        sudo rm -rf "$DOC_DIR"
        rm -rf "$SRC_DIR"
        refresh_caches
        ok "Removed. Your chats and settings stay in ~/.local/share/$APP_ID and ~/.config/$APP_ID."
        exit 0
        ;;
    "") ;;
    *) fail "Unknown option: $1 (use: uninstall)" ;;
esac

if [[ -x "$BIN" ]]; then
    echo "${B}=== Dream-LLMChatBox: update ===${N}"
else
    echo "${B}=== Dream-LLMChatBox: install ===${N}"
fi

[[ "$(uname -m)" == "x86_64" ]] || warn "Only tested on x86_64 – trying anyway."

if command -v pacman >/dev/null; then
    PM="pacman"; DISTRO="Arch-based"
elif command -v dnf >/dev/null; then
    PM="dnf"; DISTRO="Fedora-based"
elif command -v apt-get >/dev/null; then
    PM="apt"; DISTRO="Debian/Ubuntu-based"
elif command -v zypper >/dev/null; then
    PM="zypper"; DISTRO="openSUSE"
else
    PM="unknown"; DISTRO="unknown"
fi
ok "System: $DISTRO (package manager: $PM)"

if [[ "$PM" == "pacman" ]] && pacman -Qq "$APP_ID" >/dev/null 2>&1; then
    warn "The AUR package '$APP_ID' is already installed. Update it with yay/paru instead."
    ask "Install a second copy to $PREFIX anyway?" || exit 0
fi

pm_install() {
    case "$PM" in
        pacman) sudo pacman -S --needed --noconfirm "$@" ;;
        dnf)    sudo dnf install -y "$@" ;;
        apt)    sudo apt-get update -qq || warn "apt-get update reported an error – continuing."
                sudo apt-get install -y "$@" ;;
        zypper) sudo zypper --non-interactive install "$@" ;;
        *)      return 1 ;;
    esac
}

step "Installing build dependencies (sudo)"
case "$PM" in
    pacman) DEPS=(git base-devel cmake clang pkgconf alsa-lib fontconfig freetype2 libxkbcommon wayland libx11 libxcursor libxi libxrandr) ;;
    dnf)    DEPS=(git gcc gcc-c++ make cmake clang clang-devel pkgconf-pkg-config alsa-lib-devel fontconfig-devel freetype-devel libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel) ;;
    apt)    DEPS=(git curl build-essential cmake clang libclang-dev pkg-config libasound2-dev libfontconfig1-dev libfreetype-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev) ;;
    zypper) DEPS=(git gcc gcc-c++ make cmake clang clang-devel pkg-config alsa-devel fontconfig-devel freetype2-devel libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel) ;;
    *)      DEPS=() ;;
esac
if (( ${#DEPS[@]} )); then
    pm_install "${DEPS[@]}" || fail "Could not install the build dependencies: ${DEPS[*]}"
    ok "Build dependencies"
else
    warn "Unknown package manager. Please install yourself: git, cmake, clang, pkg-config, ALSA, fontconfig, xkbcommon, wayland and X11 dev packages."
fi

step "Checking Rust (needs $RUST_MIN or newer)"
[[ -d "$HOME/.cargo/bin" ]] && PATH="$HOME/.cargo/bin:$PATH"

rust_ok() {
    command -v cargo >/dev/null && command -v rustc >/dev/null || return 1
    local have
    have="$(rustc --version | awk '{print $2}')"
    [[ "$(printf '%s\n%s\n' "$RUST_MIN" "$have" | sort -V | head -1)" == "$RUST_MIN" ]]
}

if ! rust_ok; then
    if command -v rustup >/dev/null; then
        rustup update stable
        rustup default stable >/dev/null
    elif ! command -v cargo >/dev/null && [[ "$PM" != "apt" ]]; then
        case "$PM" in
            pacman) pm_install rust ;;
            dnf)    pm_install rust cargo ;;
            zypper) pm_install rust cargo ;;
        esac
    fi
fi
if ! rust_ok; then
    warn "Rust is missing or too old. The official installer rustup installs a current Rust into ~/.cargo (no sudo)."
    ask "Install Rust with rustup now?" || fail "Rust $RUST_MIN+ is needed: https://rustup.rs"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    PATH="$HOME/.cargo/bin:$PATH"
    rust_ok || fail "Rust is still too old: $(rustc --version 2>/dev/null)"
fi
ok "$(rustc --version)"

step "Getting the source code"
if [[ -d "$SRC_DIR/.git" ]]; then
    git -C "$SRC_DIR" fetch --depth 1 origin "$BRANCH"
    git -C "$SRC_DIR" reset --hard -q FETCH_HEAD
else
    rm -rf "$SRC_DIR"
    mkdir -p "$(dirname "$SRC_DIR")"
    git clone --depth 1 --branch "$BRANCH" "$REPO_URL" "$SRC_DIR"
fi
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$SRC_DIR/Cargo.toml" | head -1)"
ok "Version $VERSION ($SRC_DIR)"

step "Building (takes a few minutes the first time)"
(cd "$SRC_DIR" && cargo build --release --locked)
ok "Built"

step "Installing into the system (sudo)"
sudo install -Dm755 "$SRC_DIR/target/release/$APP_ID" "$BIN"
sudo install -Dm644 "$SRC_DIR/packaging/$APP_ID.desktop" "$DESKTOP"
sudo install -Dm644 "$SRC_DIR/assets/icon.png" "$ICON_PNG"
sudo install -Dm644 "$SRC_DIR/assets/icon.svg" "$ICON_SVG"
sudo install -Dm644 "$SRC_DIR/LICENSE" "$DOC_DIR/LICENSE"
sudo install -Dm644 "$SRC_DIR/README.md" "$DOC_DIR/README.md"
refresh_caches
ok "$BIN"
ok "App menu: Dream-LLMChatBox"

step "Ollama"
if command -v ollama >/dev/null; then
    ok "Ollama is installed"
else
    warn "Ollama is not installed (needed for local models)."
    if ask "Install Ollama now with the official script (ollama.com)?"; then
        curl -fsSL https://ollama.com/install.sh | sh
    else
        echo "  Later: inside the app the button bottom left says \"Install\"."
    fi
fi

step "Optional programs"
case "$PM" in
    pacman) OPT="sudo pacman -S --needed poppler unzip tesseract ffmpeg pandoc libnotify xdg-utils  (read aloud: yay -S piper-tts-bin)" ;;
    dnf)    OPT="sudo dnf install poppler-utils unzip tesseract ffmpeg pandoc libnotify xdg-utils" ;;
    apt)    OPT="sudo apt install poppler-utils unzip tesseract-ocr ffmpeg pandoc libnotify-bin xdg-utils" ;;
    zypper) OPT="sudo zypper install poppler-tools unzip tesseract-ocr ffmpeg pandoc libnotify-tools xdg-utils" ;;
    *)      OPT="poppler, unzip, tesseract, ffmpeg, pandoc, libnotify, xdg-utils" ;;
esac
echo "  PDF, DOCX, OCR, audio/video, documents, notifications:"
echo "  ${B}$OPT${N}"

echo
echo "${G}${B}✔ Done! Dream-LLMChatBox $VERSION is installed.${N}"
echo "  Start:   ${B}$APP_ID${N}  (or from the app menu)"
echo "  Update:  run the same line again"
echo "  Remove:  ${B}curl -fsSL https://raw.githubusercontent.com/yakuda-stack/Dream-LLMChatBox/main/install.sh | bash -s -- uninstall${N}"
