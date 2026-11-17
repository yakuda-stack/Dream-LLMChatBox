#!/bin/bash
# Dream-LLMChatBox installer – downloads the newest AppImage from GitHub
#
#   curl -fsSL https://raw.githubusercontent.com/yakuda-stack/Dream-LLMChatBox/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/yakuda-stack/Dream-LLMChatBox/main/install.sh | bash -s -- --uninstall
#
# Installs to ~/.local/share/dream-llmchatbox/app/ with the command ~/.local/bin/dream-llmchatbox
# and an app menu entry. No sudo needed. Your chats and settings are never touched.

set -euo pipefail

REPO="yakuda-stack/Dream-LLMChatBox"
ID="dream-llmchatbox"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$DATA/$ID/app"
APPIMAGE="$APP_DIR/Dream-LLMChatBox-x86_64.AppImage"
BIN_DIR="$HOME/.local/bin"
LINK="$BIN_DIR/$ID"
DESKTOP="$DATA/applications/$ID.desktop"
ICON="$DATA/icons/hicolor/256x256/apps/$ID.png"

say() { printf '\033[1;35m▸\033[0m %s\n' "$*"; }
fail() { printf '\033[1;31m✘\033[0m %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$LINK" "$DESKTOP" "$ICON"
    rm -rf "$APP_DIR"
    command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" 2>/dev/null || true
    say "Dream-LLMChatBox removed. Your chats and settings are still in $DATA/$ID and ~/.config/$ID"
    say "Delete them too:  rm -rf \"$DATA/$ID\" ~/.config/$ID"
    exit 0
fi

[ "$(uname -m)" = "x86_64" ] || fail "Only x86_64 is supported (this is $(uname -m))."
command -v curl >/dev/null || fail "curl is missing."

if command -v pacman >/dev/null && { command -v yay >/dev/null || command -v paru >/dev/null; }; then
    say "Tip: on Arch / CachyOS you can also use the AUR package:  yay -S $ID"
fi

say "Looking for the newest release of $REPO ..."
URL="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -o '"browser_download_url": *"[^"]*x86_64\.AppImage"' \
    | head -1 | sed 's/.*"\(https[^"]*\)"/\1/')"
[ -n "$URL" ] || fail "No AppImage found in the newest release."
VERSION="$(basename "$URL" | sed -n 's/^Dream-LLMChatBox-\(.*\)-x86_64\.AppImage$/\1/p')"

say "Downloading Dream-LLMChatBox ${VERSION:-} ..."
mkdir -p "$APP_DIR" "$BIN_DIR"
TMP="$(mktemp "$APP_DIR/download.XXXXXX")"
trap 'rm -f "$TMP"' EXIT
curl -fL --progress-bar "$URL" -o "$TMP"
head -c 4 "$TMP" | grep -q "ELF" || fail "The download is not an AppImage."
chmod +x "$TMP"
mv "$TMP" "$APPIMAGE"
ln -sf "$APPIMAGE" "$LINK"

mkdir -p "$(dirname "$DESKTOP")"
cat > "$DESKTOP" << EOF
[Desktop Entry]
Type=Application
Name=Dream-LLMChatBox
Comment=Chat with local AI models (Ollama)
Exec="$APPIMAGE"
Icon=$ID
Terminal=false
Categories=Utility;Development;
StartupWMClass=$ID
EOF
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" 2>/dev/null || true

if ! "$APPIMAGE" --version >/dev/null 2>&1; then
    say "Note: the AppImage needs FUSE. If it does not start, install fuse (e.g. sudo pacman -S fuse2 / sudo apt install libfuse2)"
fi

say "Installed: $APPIMAGE"
case ":$PATH:" in
    *":$BIN_DIR:"*) say "Start it from the app menu or with:  $ID" ;;
    *) say "Start it from the app menu or with:  $LINK  (add ~/.local/bin to your PATH for just '$ID')" ;;
esac

if ! command -v ollama >/dev/null; then
    say "Ollama is not installed yet. The app can install it for you (button bottom left), or run:"
    say "  curl -fsSL https://ollama.com/install.sh | sh"
fi
say "Update later: run this installer again. Remove: add  --uninstall"
