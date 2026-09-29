#!/usr/bin/env bash
# Open Antigravity - Linux launcher. Runs as the normal user.
if [ -z "${BASH_VERSION:-}" ]; then exec bash "$0" "$@"; fi
set -u
DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
BIN="$DIR/open_antigravity"
chmod +x "$BIN" 2>/dev/null || true

if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
    echo "Запускайте без sudo: права root не нужны и только мешают." >&2
    exit 1
fi
if [ ! -f "$BIN" ]; then
    echo "Не найден $BIN - распакуйте папку целиком и запускайте из неё." >&2
    exit 1
fi

# Носитель без права исполнения (NTFS/FAT, noexec) - копия в домашнюю папку.
if ! "$BIN" --version >/dev/null 2>&1; then
    HOME_BIN="${XDG_DATA_HOME:-$HOME/.local/share}/openantigravity/open_antigravity"
    if mkdir -p "$(dirname "$HOME_BIN")" \
        && cp -f "$BIN" "$HOME_BIN.new" \
        && chmod +x "$HOME_BIN.new" \
        && mv -f "$HOME_BIN.new" "$HOME_BIN" \
        && "$HOME_BIN" --version >/dev/null 2>&1; then
        BIN="$HOME_BIN"
    else
        echo "Программа не запускается ни отсюда, ни из домашней папки. Нужен 64-битный Linux (x86-64)." >&2
        exit 1
    fi
fi

# Нет ни X11, ни Wayland - терминальный режим (или понятная ошибка).
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    if [ -t 0 ] && [ -t 1 ]; then
        exec "$BIN" --tui "$@"
    fi
    echo "Нет графической сессии. Запустите из терминала: $BIN --tui" >&2
    exit 1
fi
exec "$BIN" "$@"
