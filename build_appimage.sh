#!/usr/bin/env bash
# ================================================================
#  Сборщик AppImage для Open Antigravity (Linux).
#
#  Создает единый самодостаточный исполняемый файл:
#    release/OpenAntigravity-x86_64.AppImage
#
#  Использование:
#    bash build_appimage.sh
# ================================================================
set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"

DISTDIR="$PWD/dist_portable_linux"
RELDIR="$PWD/release"
mkdir -p "$RELDIR"

if [ ! -f "$DISTDIR/open_antigravity" ]; then
    echo "Бинарник $DISTDIR/open_antigravity не найден. Выполняю сборку..."
    bash build_portable.sh
fi

echo "══════════════════════════════════════════════════════════"
echo "  Создание AppDir структуры"
echo "══════════════════════════════════════════════════════════"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

APPDIR="$TMPDIR/OpenAntigravity.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# 1. Бинарник и флаг портативности
cp -a "$DISTDIR/open_antigravity" "$APPDIR/usr/bin/open_antigravity"
touch "$APPDIR/usr/bin/portable.flag"

# 2. Иконки
ICON_SRC="assets/icon.png"
if [ ! -f "$ICON_SRC" ] && [ -f "$DISTDIR/icon.png" ]; then
    ICON_SRC="$DISTDIR/icon.png"
fi

if [ -f "$ICON_SRC" ]; then
    for sz in 16 32 48 64 128 256 512; do
        mkdir -p "$APPDIR/usr/share/icons/hicolor/${sz}x${sz}/apps"
        if command -v ffmpeg >/dev/null 2>&1; then
            ffmpeg -y -v quiet -i "$ICON_SRC" -vf "scale=${sz}:${sz}" -update 1 "$APPDIR/usr/share/icons/hicolor/${sz}x${sz}/apps/open_antigravity.png"
        else
            cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/${sz}x${sz}/apps/open_antigravity.png"
        fi
    done

    # В корне AppDir для совместимости со стандартом AppImage Type 2
    if [ -f "$APPDIR/usr/share/icons/hicolor/512x512/apps/open_antigravity.png" ]; then
        cp "$APPDIR/usr/share/icons/hicolor/512x512/apps/open_antigravity.png" "$APPDIR/open_antigravity.png"
        cp "$APPDIR/usr/share/icons/hicolor/512x512/apps/open_antigravity.png" "$APPDIR/.DirIcon"
    else
        cp "$ICON_SRC" "$APPDIR/open_antigravity.png"
        cp "$ICON_SRC" "$APPDIR/.DirIcon"
    fi
fi

# 3. Десктоп-файл
cat > "$APPDIR/open_antigravity.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Open Antigravity
GenericName=Antigravity IDE Unlocker
Comment=Портативная разблокировка Antigravity 2.0 (Google IDE)
Exec=open_antigravity %u
Icon=open_antigravity
Categories=Development;IDE;Utility;
Terminal=false
StartupNotify=true
EOF

# 4. AppRun скрипт запуска
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "${0}")")"
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
export AG_PORTABLE=1

# Интеграция системной иконки для панели задач и меню приложений
if [ -n "$APPIMAGE" ]; then
    for sz in 16 32 48 64 128 256; do
        SRC_ICO="${HERE}/usr/share/icons/hicolor/${sz}x${sz}/apps/open_antigravity.png"
        DST_ICO="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/${sz}x${sz}/apps/open_antigravity.png"
        if [ -f "$SRC_ICO" ] && [ ! -f "$DST_ICO" ]; then
            mkdir -p "$(dirname "$DST_ICO")"
            cp "$SRC_ICO" "$DST_ICO" 2>/dev/null || true
        fi
    done
fi

# Если запускается из каталога, где есть права на запись, храним data рядом с AppImage,
# иначе в ~/.local/share/openantigravity
if [ -n "$APPIMAGE" ]; then
    APPIMAGE_DIR="$(dirname "$(readlink -f "$APPIMAGE")")"
    if [ -w "$APPIMAGE_DIR" ]; then
        cd "$APPIMAGE_DIR"
    else
        DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/openantigravity"
        mkdir -p "$DATA_DIR"
        cd "$DATA_DIR"
    fi
else
    cd "${HERE}/usr/bin"
fi

exec "${HERE}/usr/bin/open_antigravity" "$@"
EOF
chmod +x "$APPDIR/AppRun"

echo "══════════════════════════════════════════════════════════"
echo "  Упаковка в AppImage"
echo "══════════════════════════════════════════════════════════"

APPIMAGETOOL="$TMPDIR/appimagetool"
if ! command -v appimagetool >/dev/null 2>&1; then
    echo "Загрузка appimagetool..."
    curl -fsSL -o "$APPIMAGETOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" || {
        echo "Не удалось скачать appimagetool напрямую. Попытка через запасное зеркало..."
        curl -fsSL -o "$APPIMAGETOOL" "https://github.com/probonopd/go-appimage/releases/download/continuous/appimagetool-continuous-x86_64.AppImage" || true
    }
    if [ -f "$APPIMAGETOOL" ]; then
        chmod +x "$APPIMAGETOOL"
    fi
else
    APPIMAGETOOL="appimagetool"
fi

OUTPUT_APPIMAGE="$RELDIR/OpenAntigravity-x86_64.AppImage"

if [ -x "$APPIMAGETOOL" ] || command -v appimagetool >/dev/null 2>&1; then
    # --appimage-extract-and-run allows running inside containers (Docker/podman) without FUSE
    ARCH=x86_64 "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$OUTPUT_APPIMAGE" || {
        ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT_APPIMAGE"
    }
    sha256sum "$OUTPUT_APPIMAGE" | awk '{print $1}' > "${OUTPUT_APPIMAGE}.sha256"
    echo
    echo "✓ AppImage успешно создан: $OUTPUT_APPIMAGE"
    echo "  SHA256: $(cat "${OUTPUT_APPIMAGE}.sha256")"
else
    echo "ВНИМАНИЕ: appimagetool недоступен в текущем окружении (нет FUSE/сети)." >&2
    echo "AppDir подготовлен в: $APPDIR" >&2
fi
