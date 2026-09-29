#!/usr/bin/env bash
# ================================================================
#  Build a portable, key-free Open Antigravity with YOUR proxy baked in.
#  Linux edition - one self-contained script (generates launch.sh,
#  install.sh and README itself; nothing from linux/ is needed).
#
#  Run from the repository root:
#      AG_BUILTIN_PROXY='aguser:PASSWORD@IP:48123' \
#      AG_UPDATE_URL='https://YOUR_SITE/version.json' \
#      bash build_portable.sh
#
#  Default build: universal glibc build (full Wayland/X11 GUI + TUI fallback,
#  compatible with Steam Deck and all modern Linux distros).
#  Set AG_MUSL=1 for a static musl build (TUI-only).
# ================================================================
set -euo pipefail

# --- BAKED-IN PROXY (optional: leave empty for clean public build, or pass AG_BUILTIN_PROXY) ---
# Format: login:password@host:port (or multiple separated by semicolon: proxy1;proxy2)
export AG_BUILTIN_PROXY="${AG_BUILTIN_PROXY:-}"

# --- PORTABLE MODE BAKED INTO THE BUILD -------------------------
export AG_PORTABLE=1

# --- VERSION REPORTED BY THE BUILD (SemVer from root VERSION file) ---
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_VERSION="$(tr -d '\r\n' < "$SCRIPT_DIR/VERSION" 2>/dev/null || echo '1.1.0')"
export AG_FULL_VERSION="${AG_FULL_VERSION:-$ROOT_VERSION}"

# --- UPDATE CHECK FEED (edit me, or pass as env) ----------------
# The binary fetches this JSON and shows a banner when its "version"
# is newer than the one baked here. Empty string disables the check.
# Leave as-is until your site is up, then point at your host.
export AG_UPDATE_URL="${AG_UPDATE_URL:-}"

cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"

command -v cargo >/dev/null 2>&1 || {
    echo "cargo не найден. Установите rustup:" >&2
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    exit 1
}

# Quiet apt install when possible (root in Docker, sudo on desktop).
apt_install() {
    if command -v apt-get >/dev/null 2>&1; then
        if [ "$(id -u)" -eq 0 ]; then
            apt-get update -qq >/dev/null 2>&1 || true
            apt-get install -y -qq "$@" >/dev/null 2>&1 || true
        elif command -v sudo >/dev/null 2>&1; then
            sudo apt-get update -qq >/dev/null 2>&1 || true
            sudo apt-get install -y -qq "$@" >/dev/null 2>&1 || true
        fi
    fi
}
# musl-tools: C compiler for the static target; the rest: headers the
# GUI backend wants on minimal systems, harmless if already present.
apt_install build-essential pkg-config musl-tools libxkbcommon-dev libwayland-dev

BIN=""
KIND="динамическая (glibc хоста сборки)"
if [ "${AG_MUSL:-0}" = "1" ] \
   && command -v rustup >/dev/null 2>&1 \
   && rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1; then
    echo "Building STATIC musl build (runs on any Linux, incl. SteamOS)..."
    if cargo build --release --target x86_64-unknown-linux-musl; then
        BIN="target/x86_64-unknown-linux-musl/release/open_antigravity"
        if [ ! -f "$BIN" ]; then
            BIN="target/x86_64-unknown-linux-musl/release/ag_unlocker"
        fi
        KIND="статическая (musl) - без привязки к glibc"
        echo "" >&2
        echo "ВАЖНО: у статической musl-сборки нет GUI-окна — только терминальный режим (TUI)." >&2
        echo "Причина: Wayland/X11-библиотеки целевой машины собраны на glibc и не" >&2
        echo "загружаются в musl-бинарник. Окно можно получить только glibc-сборкой" >&2
        echo "на СТАРОМ дистрибутиве (glibc <= целевой), например в Docker на VPS:" >&2
        echo "  docker run --rm -v \"\$PWD\":/src -w /src -e AG_MUSL=0 rust:1-bookworm bash build_portable.sh" >&2
    else
        echo "musl-сборка не удалась, откатываюсь к обычной (динамической)." >&2
    fi
fi
if [ -z "$BIN" ]; then
    echo "Building normal (glibc) build..."
    cargo build --release
    BIN="target/release/open_antigravity"
    if [ ! -f "$BIN" ]; then
        BIN="target/release/ag_unlocker"
    fi
fi

OUTDIR="$PWD/dist_portable_linux"
mkdir -p "$OUTDIR"
cp -f "$BIN" "$OUTDIR/open_antigravity"
cp -f "assets/icon.png" "$OUTDIR/icon.png" 2>/dev/null || true

# Launcher: handles noexec media (copies the binary into the home dir)
# and falls back to the terminal UI when there is no graphical session.
cat > "$OUTDIR/launch.sh" <<'EOF'
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
EOF
chmod +x "$OUTDIR/launch.sh"

cat > "$OUTDIR/README.txt" <<'EOF'
Портативная сборка Open Antigravity для Linux
=============================================

Запуск:
    ./launch.sh (или клик по open_antigravity)

Особенности:
- Все настройки и кэш хранятся в папке "data" рядом с программой.
- Прокси уже встроен в сборку.
- Полный сброс: удаление папки "data".
- Диагностика (--check), ярлыки на рабочий стол (--shortcut) и в Steam (--steam)
  доступны прямо кнопками в GUI или через флаги запуска.
- Запуск с карты памяти / флешки: launch.sh автоматически обработает
  монтирование с noexec.
EOF

echo
echo "Done. Output folder:"
echo "  $OUTDIR"
echo "  сборка: $KIND"
