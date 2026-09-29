#!/usr/bin/env bash
# ================================================================
#  Упаковщик релиза Open Antigravity для Linux.
#
#  Собирает бинарник (или использует уже собранный), формирует
#  дистрибутивный архив, считает SHA256 и (опционально) генерирует
#  version.json для фида обновлений.
#
#  Использование:
#    bash package_release.sh                          # сборка + упаковка
#    bash package_release.sh --skip-build             # упаковка из dist_portable_linux
#    bash package_release.sh --version 2.17.0.4       # задать версию вручную
#    bash package_release.sh --site https://example.com  # базовый URL для ссылок
#
#  Результат в release/:
#    OpenAntigravity_linux_v2.17.0.3.tar.gz
#    OpenAntigravity_linux_v2.17.0.3.tar.gz.sha256
#    version.json                                     (если задан --site)
# ================================================================
set -euo pipefail

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"

# ---------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------
SKIP_BUILD=0
VERSION=""
SITE_URL=""
NOTES=""

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build)   SKIP_BUILD=1; shift ;;
        --version)      VERSION="$2"; shift 2 ;;
        --site)         SITE_URL="$2"; shift 2 ;;
        --notes)        NOTES="$2"; shift 2 ;;
        *)
            echo "Неизвестный аргумент: $1" >&2
            echo "Использование: bash package_release.sh [--skip-build] [--version X.Y.Z.N] [--site URL] [--notes TEXT]" >&2
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------
# Derive version from build env or binary
# ---------------------------------------------------------------
if [ -z "$VERSION" ]; then
    VERSION="${AG_FULL_VERSION:-}"
fi

DISTDIR="$PWD/dist_portable_linux"

if [ "$SKIP_BUILD" -eq 0 ]; then
    echo "══════════════════════════════════════════════════════════"
    echo "  Этап 1: Сборка портативного дистрибутива"
    echo "══════════════════════════════════════════════════════════"
    if [ -n "$VERSION" ]; then
        export AG_FULL_VERSION="$VERSION"
    fi
    bash build_portable.sh
    echo
fi

# Get version from the built binary if not set explicitly.
if [ -z "$VERSION" ]; then
    for bin_candidate in "$DISTDIR/open_antigravity" "$DISTDIR/ag_unlocker"; do
        if [ -x "$bin_candidate" ]; then
            VERSION=$("$bin_candidate" --help 2>/dev/null | head -1 | grep -oP 'v\K[0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)?' || echo "")
            if [ -n "$VERSION" ]; then break; fi
        fi
    done
fi
if [ -z "$VERSION" ]; then
    VERSION="$(tr -d '\r\n' < "$REPO_ROOT/VERSION" 2>/dev/null || echo '1.1.0')"
    echo "Версия определена из файла VERSION: $VERSION" >&2
fi

echo "══════════════════════════════════════════════════════════"
echo "  Этап 2: Упаковка релиза v${VERSION}"
echo "══════════════════════════════════════════════════════════"

# ---------------------------------------------------------------
# Validate dist contents
# ---------------------------------------------------------------
REQUIRED_FILES=(launch.sh README.txt)
for f in "${REQUIRED_FILES[@]}"; do
    if [ ! -e "$DISTDIR/$f" ]; then
        echo "ОШИБКА: $DISTDIR/$f не найден. Сначала выполните сборку." >&2
        exit 1
    fi
done
if [ ! -e "$DISTDIR/open_antigravity" ] && [ ! -e "$DISTDIR/ag_unlocker" ]; then
    echo "ОШИБКА: Бинарник программы не найден в $DISTDIR. Сначала выполните сборку." >&2
    exit 1
fi

# ---------------------------------------------------------------
# Create release directory
# ---------------------------------------------------------------
RELDIR="$PWD/release"
mkdir -p "$RELDIR"

ARCHIVE_NAME="OpenAntigravity_linux_v${VERSION}.tar.gz"
ARCHIVE_PATH="$RELDIR/$ARCHIVE_NAME"

# ---------------------------------------------------------------
# Pack the archive
# ---------------------------------------------------------------
# The archive contains a single top-level directory so the user
# gets a folder when extracting, not a pile of loose files.
STAGING_DIR=$(mktemp -d)
FOLDER_NAME="OpenAntigravity_linux_v${VERSION}"
STAGING="$STAGING_DIR/$FOLDER_NAME"
mkdir -p "$STAGING"

# Copy dist contents (exclude data/ — that's user state)
for f in "$DISTDIR"/*; do
    base=$(basename "$f")
    if [ "$base" = "data" ]; then
        continue
    fi
    cp -a "$f" "$STAGING/"
done

echo "Архивирую: $ARCHIVE_PATH"
tar -czf "$ARCHIVE_PATH" -C "$STAGING_DIR" "$FOLDER_NAME"
rm -rf "$STAGING_DIR"

# ---------------------------------------------------------------
# SHA256 checksum
# ---------------------------------------------------------------
SHA_PATH="${ARCHIVE_PATH}.sha256"
sha256sum "$ARCHIVE_PATH" | awk '{print $1}' > "$SHA_PATH"
SHA=$(cat "$SHA_PATH")
echo "SHA256:    $SHA"

# ---------------------------------------------------------------
# version.json (if --site given)
# ---------------------------------------------------------------
if [ -n "$SITE_URL" ]; then
    # Strip trailing slash
    SITE_URL="${SITE_URL%/}"
    NOTES_ESCAPED=$(echo "$NOTES" | sed 's/"/\\"/g')

    ARCHIVE_WIN="OpenAntigravity_windows_v${VERSION}.zip"
    ARCHIVE_LIN="$ARCHIVE_NAME"

    cat > "$RELDIR/version.json" <<ENDJSON
{
  "version": "${VERSION}",
  "notes": "${NOTES_ESCAPED}",
  "page": "${SITE_URL}/",
  "url_windows": "${SITE_URL}/${ARCHIVE_WIN}",
  "url_linux": "${SITE_URL}/${ARCHIVE_LIN}"
}
ENDJSON
    echo "version.json сгенерирован: $RELDIR/version.json"
fi

# ---------------------------------------------------------------
# Summary
# ---------------------------------------------------------------
ARCHIVE_SIZE=$(du -h "$ARCHIVE_PATH" | awk '{print $1}')
echo
echo "══════════════════════════════════════════════════════════"
echo "  Релиз v${VERSION} готов!"
echo "══════════════════════════════════════════════════════════"
echo
echo "  Архив:    $ARCHIVE_PATH ($ARCHIVE_SIZE)"
echo "  SHA256:   $SHA"
if [ -n "$SITE_URL" ]; then
echo "  Фид:     $RELDIR/version.json"
fi
echo
echo "  Следующие шаги:"
echo "    1. Загрузите $ARCHIVE_NAME на ваш сайт"
if [ -n "$SITE_URL" ]; then
echo "    2. Загрузите version.json рядом (${SITE_URL}/version.json)"
echo "    3. Баннер «Новая версия» появится у пользователей в течение 8 часов"
echo "       (или сразу по кнопке «Проверить обновления» / клавише 'u' в TUI)"
else
echo "    2. Для автообновления повторите с --site https://your-site.com"
fi
echo
