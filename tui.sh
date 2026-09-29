#!/bin/sh
# Open Antigravity — терминальный режим одной командой (Linux x86-64):
#
#   curl -fsSL https://raw.githubusercontent.com/keelbismark/OpenAntigravity/main/tui.sh | sh
#
# Берёт последний релиз с GitHub, кладёт программу в
# ~/.local/share/openantigravity/ и запускает её в этом терминале. Повторный запуск
# той же командой скачивает заново, только если вышла новая версия. Без root.
set -eu

REPO="keelbismark/OpenAntigravity"
DIR="${XDG_DATA_HOME:-$HOME/.local/share}/openantigravity"
BIN="$DIR/open_antigravity"

case "$(uname -m)" in
    x86_64 | amd64) ;;
    *)
        echo "Open Antigravity собран только для x86-64, а здесь $(uname -m)." >&2
        exit 1
        ;;
esac

# The newest tag from where /releases/latest redirects to: no API call, so no
# rate limit on a server sharing its address with a hundred others.
TAG=""
if URL=$(curl -fsSLo /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest"); then
    TAG="${URL##*/}"
fi
VER="${TAG#v}"

case "$VER" in
    "" | *[!0-9._]*)
        if [ -x "$BIN" ]; then
            echo "GitHub не ответил — запускаю уже скачанную версию." >&2
        else
            echo "Не удалось узнать последнюю версию на github.com/$REPO." >&2
            exit 1
        fi
        ;;
    *)
        if [ ! -x "$BIN" ] || [ "$(cat "$BIN.version" 2>/dev/null)" != "$VER" ]; then
            echo "Скачиваю Open Antigravity $VER…" >&2
            mkdir -p "$DIR"
            TMP=$(mktemp -d)
            trap 'rm -rf "$TMP"' EXIT
            curl -fsSL "https://github.com/$REPO/releases/download/$TAG/OpenAntigravity_linux_v${VER}.tar.gz" |
                tar xz -C "$TMP"
            SRC_BIN=$(find "$TMP" -type f -name open_antigravity | head -n 1)
            if [ -z "$SRC_BIN" ]; then
                SRC_BIN=$(find "$TMP" -type f -name ag_unlocker | head -n 1)
            fi
            mv -f "$SRC_BIN" "$BIN"
            chmod +x "$BIN"
            echo "$VER" >"$BIN.version"
            rm -rf "$TMP"
            trap - EXIT
        fi
        ;;
esac

# `curl … | sh` hands this script to sh on stdin; the program's keys come from
# the terminal itself.
exec "$BIN" --tui </dev/tty
