#!/usr/bin/env bash
# ================================================================
#  Open Antigravity - Единый скрипт сборки и упаковки (Linux)
# ================================================================
#  Использование:
#    bash build.sh             - сборка переносимой версии в dist_portable_linux/
#    bash build.sh --package   - сборка и упаковка .tar.gz и AppImage в release/
#    bash build.sh --musl      - статическая сборка musl (только TUI)
#    bash build.sh --check     - запуск тестов и проверки кода
# ================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DO_PACKAGE=0
DO_CHECK=0
DO_RELEASE=0
NEW_REL_VER=""
OAG_MUSL="${OAG_MUSL:-0}"

while [ $# -gt 0 ]; do
    case "$1" in
        --release|-r) DO_RELEASE=1; NEW_REL_VER="$2"; shift 2 ;;
        --package|-p) DO_PACKAGE=1; shift ;;
        --check|-c)   DO_CHECK=1; shift ;;
        --musl)       OAG_MUSL=1; shift ;;
        *)            shift ;;
    esac
done

if [ "$DO_RELEASE" -eq 1 ]; then
    if [ -z "$NEW_REL_VER" ]; then
        echo "[!] Ошибка: укажите версию для релиза, например: bash build.sh --release 1.1.4" >&2
        exit 1
    fi
    if [ ! -d "$SCRIPT_DIR/.git" ]; then
        echo "[!] Ошибка: --release доступен только при сборке из git-репозитория." >&2
        exit 1
    fi
    if ! git diff-index --quiet HEAD --; then
        echo "[!] Ошибка: в репозитории есть незакоммиченные изменения. Сначала закоммитьте их или очистите working tree." >&2
        exit 1
    fi
    echo "================================================================"
    echo "  Автоматический релиз Open Antigravity v${NEW_REL_VER}"
    echo "================================================================"
    printf "%s\n" "$NEW_REL_VER" > "$SCRIPT_DIR/VERSION"
    if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
        sed -i -E 's/^version = "[0-9]+\.[0-9]+\.[0-9]+"/version = "'"$NEW_REL_VER"'"/' "$SCRIPT_DIR/Cargo.toml"
    fi
    if [ -f "$SCRIPT_DIR/docs/version.json" ]; then
        cat > "$SCRIPT_DIR/docs/version.json" <<EOF
{
  "version": "$NEW_REL_VER",
  "notes": "Open Antigravity v$NEW_REL_VER",
  "page": "https://github.com/keelbismark/OpenAntigravity/releases/tag/v$NEW_REL_VER",
  "url_windows": "https://github.com/keelbismark/OpenAntigravity/releases/download/v$NEW_REL_VER/OpenAntigravity_windows_v${NEW_REL_VER}_portable.zip",
  "url_windows_setup": "https://github.com/keelbismark/OpenAntigravity/releases/download/v$NEW_REL_VER/OpenAntigravity_windows_v${NEW_REL_VER}_setup.exe",
  "url_windows_portable": "https://github.com/keelbismark/OpenAntigravity/releases/download/v$NEW_REL_VER/OpenAntigravity_windows_v${NEW_REL_VER}_portable.zip",
  "url_linux": "https://github.com/keelbismark/OpenAntigravity/releases/download/v$NEW_REL_VER/OpenAntigravity_linux_v$NEW_REL_VER.tar.gz",
  "url_appimage": "https://github.com/keelbismark/OpenAntigravity/releases/download/v$NEW_REL_VER/OpenAntigravity-x86_64.AppImage"
}
EOF
    fi
    if [ -f "$SCRIPT_DIR/docs/index.html" ]; then
        sed -i -E "s/v[0-9]+\.[0-9]+\.[0-9]+/v${NEW_REL_VER}/g" "$SCRIPT_DIR/docs/index.html"
    fi
    echo "[*] Проверка кода и тесты перед релизом..."
    cargo check || exit 1
    cargo test --bin open_antigravity || exit 1
    echo "[*] Фиксация изменений в git и создание тега..."
    git add "$SCRIPT_DIR/VERSION" "$SCRIPT_DIR/Cargo.toml" "$SCRIPT_DIR/Cargo.lock" "$SCRIPT_DIR/docs/version.json" "$SCRIPT_DIR/docs/index.html"
    git commit -m "chore(release): bump version to v${NEW_REL_VER}" || exit 1
    git tag "v${NEW_REL_VER}" || exit 1
    echo "[*] Отправка в GitHub (запуск CI/CD релиза)..."
    git push origin main || { echo "[!] Ошибка отправки ветки в origin. Убедитесь, что у вас есть права на запись в репозиторий." >&2; exit 1; }
    git push origin "v${NEW_REL_VER}" || { echo "[!] Ошибка отправки тега в origin. Убедитесь, что у вас есть права на запись в репозиторий." >&2; exit 1; }
    echo
    echo "[*] Релиз v${NEW_REL_VER} успешно запущен в GitHub Actions!"
    exit 0
fi

# --- Определение версии из файла VERSION (SSOT) ---
if [ ! -f "$SCRIPT_DIR/VERSION" ]; then
    echo "[!] Ошибка: файл VERSION не найден в корне проекта." >&2
    exit 1
fi
ROOT_VERSION="$(tr -d '\r\n' < "$SCRIPT_DIR/VERSION")"
export OAG_FULL_VERSION="${OAG_FULL_VERSION:-$ROOT_VERSION}"
export OAG_PORTABLE=1
export OAG_UPDATE_URL="${OAG_UPDATE_URL:-https://keelbismark.github.io/OpenAntigravity/version.json}"

if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    sed -i -E 's/^version = "[0-9]+\.[0-9]+\.[0-9]+"/version = "'"$ROOT_VERSION"'"/' "$SCRIPT_DIR/Cargo.toml" 2>/dev/null || true
fi

if [ "$DO_CHECK" -eq 1 ]; then
    echo "================================================================"
    echo "  Проверка и тесты: Open Antigravity v${ROOT_VERSION}"
    echo "================================================================"
    cargo check
    cargo test --bin open_antigravity
    echo "[✓] Все тесты успешно пройдены!"
    exit 0
fi

echo "================================================================"
echo "  Сборка Open Antigravity v${ROOT_VERSION} (Linux)"
echo "================================================================"

# Запуск базового сборщика
export OAG_MUSL
bash "$SCRIPT_DIR/build_portable.sh"

if [ "$DO_PACKAGE" -eq 1 ]; then
    echo "================================================================"
    echo "  Упаковка релизных архивов (.tar.gz и AppImage)"
    echo "================================================================"
    bash "$SCRIPT_DIR/package_release.sh" --skip-build --version "$ROOT_VERSION"
    if [ "$OAG_MUSL" -eq 0 ] && [ -f "$SCRIPT_DIR/build_appimage.sh" ]; then
        bash "$SCRIPT_DIR/build_appimage.sh"
    fi
fi

echo "[✓] Сборка завершена успешно!"
