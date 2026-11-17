#!/bin/bash
# Dream-LLMChatBox — AppImage Builder (gleicher Aufbau wie bei LinuxVR-ViewShot)
# Benötigt: cargo, cmake, clang, wget (appimagetool + Runtime werden automatisch geladen)
# Verwendung:  bash scripts/build_appimage.sh   (egal von wo aus)
#
# Ergebnis:    build/Dream-LLMChatBox-<version>-x86_64.AppImage (+ .zsync)
#              Die alte AppImage + .zsync bleiben liegen, bis die neue fertig ist –
#              alles andere in build/ wird bei jedem Lauf geleert.
#
# Die App ist ein einzelnes Rust-Programm. Die Bibliotheken für Fenster, Schrift
# und Ton (Wayland/X11, fontconfig, ALSA) bringt jedes Desktop-Linux mit – sie
# werden absichtlich NICHT eingepackt, sonst passen sie nicht zum Grafiktreiber.

set -e

cd "$(dirname "$0")"
for _ in 1 2 3; do
    [ -f Cargo.toml ] && [ -d src ] && break
    cd ..
done
if [ ! -f Cargo.toml ] || [ ! -d src ]; then
    echo "FEHLER: Projekt-Root nicht gefunden (Cargo.toml fehlt)."
    exit 1
fi

APP="Dream-LLMChatBox"
ID="dream-llmchatbox"
VERSION="$(sed -n '/^\[package\]/,/^\[/{s/^version *= *"\([^"]*\)".*/\1/p}' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "FEHLER: version in Cargo.toml nicht gefunden."
    exit 1
fi
ARCH="x86_64"
OUT_DIR="$(pwd)/build"
BUILD_DIR="$OUT_DIR/AppDir"
OUT="$OUT_DIR/${APP}-${VERSION}-${ARCH}.AppImage"
NEW_DIR="$OUT_DIR/new"
NEW="$NEW_DIR/$(basename "$OUT")"

echo "=== Dream-LLMChatBox AppImage Builder ==="
echo "Version: $VERSION"
echo ""

for f in assets/icon.png assets/icon.svg packaging/$ID.desktop CHANGELOG.md README.md LICENSE Cargo.lock; do
    if [ ! -e "$f" ]; then
        echo "FEHLER: $f nicht gefunden — bitte aus dem Projekt-Root bauen."
        exit 1
    fi
done
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || {
    echo "FEHLER: cargo fehlt (sudo pacman -S rust)."
    exit 1
}

echo "[0/5] Leere build/ ..."
mkdir -p "$OUT_DIR"
find "$OUT_DIR" -mindepth 1 -maxdepth 1 ! -name '*.AppImage' ! -name '*.zsync' -exec rm -rf {} +

if [ -z "${APPIMAGETOOL:-}" ]; then
    APPIMAGETOOL="/tmp/appimagetool-new-${ARCH}"
    if [ ! -s "$APPIMAGETOOL" ]; then
        echo "[Info] Lade appimagetool (github.com/AppImage/appimagetool)..."
        wget -q "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage" \
            -O "$APPIMAGETOOL" || {
            echo "FEHLER: appimagetool konnte nicht geladen werden."
            rm -f "$APPIMAGETOOL"
            exit 1
        }
    fi
    if ! head -c 4 "$APPIMAGETOOL" | grep -q "ELF"; then
        echo "FEHLER: $APPIMAGETOOL ist keine ELF-Datei — Download kaputt."
        rm -f "$APPIMAGETOOL"
        exit 1
    fi
    chmod +x "$APPIMAGETOOL"
fi
export APPIMAGE_EXTRACT_AND_RUN=1

RUNTIME_URL="https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-${ARCH}"
RUNTIME="/tmp/appimage-runtime-${ARCH}"
if [ ! -s "$RUNTIME" ]; then
    echo "[Info] Lade statische AppImage-Runtime (fuse2+fuse3)..."
    wget -q "$RUNTIME_URL" -O "$RUNTIME" || {
        echo "FEHLER: Runtime konnte nicht geladen werden ($RUNTIME_URL)."
        exit 1
    }
fi
if ! head -c 4 "$RUNTIME" | grep -q "ELF"; then
    echo "FEHLER: $RUNTIME ist keine ELF-Datei — Download kaputt."
    rm -f "$RUNTIME"
    exit 1
fi
chmod +x "$RUNTIME"

echo "[1/5] Baue Dream-LLMChatBox (cargo build --release)..."
cargo build --release --locked
BIN="target/release/$ID"
[ -s "$BIN" ] || { echo "FEHLER: $BIN fehlt nach cargo build."; exit 1; }

if command -v objdump >/dev/null; then
    GLIBC_MAX="$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)"
    echo "[Info] Programm braucht mindestens glibc $GLIBC_MAX"
fi

echo "[2/5] Erstelle AppDir..."
mkdir -p "$BUILD_DIR/usr/bin" "$BUILD_DIR/usr/share/applications" \
         "$BUILD_DIR/usr/share/icons/hicolor/256x256/apps" \
         "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps" \
         "$BUILD_DIR/usr/share/doc/$ID"
install -m 755 "$BIN" "$BUILD_DIR/usr/bin/$ID"
strip "$BUILD_DIR/usr/bin/$ID" 2>/dev/null || true
cp assets/icon.png "$BUILD_DIR/usr/share/icons/hicolor/256x256/apps/$ID.png"
cp assets/icon.svg "$BUILD_DIR/usr/share/icons/hicolor/scalable/apps/$ID.svg"
cp assets/icon.png "$BUILD_DIR/$ID.png"
cp "packaging/$ID.desktop" "$BUILD_DIR/usr/share/applications/$ID.desktop"
cp "packaging/$ID.desktop" "$BUILD_DIR/$ID.desktop"
cp README.md CHANGELOG.md LICENSE "$BUILD_DIR/usr/share/doc/$ID/"

cat > "$BUILD_DIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/dream-llmchatbox" "$@"
APPRUN
chmod +x "$BUILD_DIR/AppRun"

UPDATE_INFO="gh-releases-zsync|yakuda-stack|Dream-LLMChatBox|latest|Dream-LLMChatBox-*${ARCH}.AppImage.zsync"

chmod -R a+rX "$BUILD_DIR"

echo "[3/5] Baue AppImage..."
mkdir -p "$NEW_DIR"
(cd "$NEW_DIR" && ARCH="$ARCH" "$APPIMAGETOOL" --runtime-file "$RUNTIME" \
    -u "$UPDATE_INFO" "$BUILD_DIR" "$NEW")

echo ""
echo "[4/5] Prüfe das Ergebnis..."
if head -c 400000 "$NEW" | strings | grep -q "libfuse\.so\.2"; then
    echo "WARNUNG: Die AppImage verweist noch auf libfuse.so.2 — Runtime nicht statisch."
else
    echo "✔ Runtime ist statisch (läuft mit fuse2 UND fuse3)"
fi
EMBEDDED="$(env -u APPIMAGE_EXTRACT_AND_RUN "$NEW" \
    --appimage-updateinformation 2>/dev/null || true)"
if [ "$EMBEDDED" = "$UPDATE_INFO" ]; then
    echo "✔ Update-Info eingebettet: $EMBEDDED"
else
    echo "WARNUNG: Update-Info fehlt in der AppImage (gelesen: '$EMBEDDED')."
fi
VERSION_OUT="$(env -u APPIMAGE_EXTRACT_AND_RUN "$NEW" --version 2>/dev/null || true)"
if [ "$VERSION_OUT" = "$ID $VERSION" ]; then
    echo "✔ Startet: $VERSION_OUT"
else
    echo "WARNUNG: --version gibt '$VERSION_OUT' aus (erwartet: '$ID $VERSION')."
fi
if [ ! -s "$NEW.zsync" ]; then
    echo "FEHLER: keine .zsync erzeugt – die Dateien in build/ bleiben unverändert."
    exit 1
fi
echo "✔ Delta-Update-Datei: build/$(basename "$OUT").zsync"

echo "[5/5] Räume auf..."
find "$OUT_DIR" -maxdepth 1 -type f \( -name '*.AppImage' -o -name '*.zsync' \) -delete
mv "$NEW" "$NEW.zsync" "$OUT_DIR/"
rm -rf "$BUILD_DIR" "$NEW_DIR"

echo "✔ Fertig: build/$(basename "$OUT")  ($(du -h "$OUT" | cut -f1))"
echo "   Ins GitHub-Release: $(basename "$OUT") UND $(basename "$OUT").zsync"
echo "   Zum Starten: chmod +x \"$OUT\" && \"$OUT\""
echo "   Ohne FUSE testen: APPIMAGE_EXTRACT_AND_RUN=1 \"$OUT\""
