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
AG_MUSL="${AG_MUSL:-0}"

while [ $# -gt 0 ]; do
    case "$1" in
        --package|-p) DO_PACKAGE=1; shift ;;
        --check|-c)   DO_CHECK=1; shift ;;
        --musl)       AG_MUSL=1; shift ;;
        *)            shift ;;
    esac
done

# --- Версия ---
ROOT_VERSION="$(tr -d '\r\n' < "$SCRIPT_DIR/VERSION" 2>/dev/null || echo '1.1.3')"
export AG_FULL_VERSION="${AG_FULL_VERSION:-$ROOT_VERSION}"
export AG_PORTABLE=1
export AG_UPDATE_URL="${AG_UPDATE_URL:-https://keelbismark.github.io/OpenAntigravity/version.json}"

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
export AG_MUSL
bash "$SCRIPT_DIR/build_portable.sh"

if [ "$DO_PACKAGE" -eq 1 ]; then
    echo "================================================================"
    echo "  Упаковка релизных архивов (.tar.gz и AppImage)"
    echo "================================================================"
    bash "$SCRIPT_DIR/package_release.sh" --skip-build --version "$ROOT_VERSION"
    if [ "$AG_MUSL" -eq 0 ] && [ -f "$SCRIPT_DIR/build_appimage.sh" ]; then
        bash "$SCRIPT_DIR/build_appimage.sh"
    fi
fi

echo "[✓] Сборка завершена успешно!"
