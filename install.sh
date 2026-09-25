#!/bin/sh
# tgsum installer, the `npm install -g` of this app: installs what building it
# needs (WebKitGTK and a C toolchain on Linux, Rust when it is missing), then
# `cargo install tgsum`, and opens the app so it shows up in the launcher.
#
#   curl -fsSL https://raw.githubusercontent.com/Roflochinsky/tgsum/main/install.sh | sh
#
# TGSUM_GIT=1 builds the latest commit from GitHub instead of the crates.io
# release; TGSUM_BRANCH=<name> builds a branch.
set -eu

REPO=https://github.com/Roflochinsky/tgsum
MIN_RUST_MINOR=85 # Rust 1.85

say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die() {
  printf '\033[1;31mОшибка:\033[0m %s\n' "$*" >&2
  exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif have sudo; then
    sudo "$@"
  else
    die "нужны права администратора (sudo) для: $*"
  fi
}

# Which package family the system is, from os-release (a Debian box can
# have pacman installed and the other way round); what is installed decides
# only when os-release does not tell.
distro() {
  ids=
  if [ -r /etc/os-release ]; then
    # shellcheck source=/dev/null
    ids=$(. /etc/os-release && echo "${ID:-} ${ID_LIKE:-}")
  fi
  case " $ids " in
  *" arch "*) echo arch ;;
  *" debian "* | *" ubuntu "*) echo debian ;;
  *" fedora "* | *" rhel "*) echo fedora ;;
  *" suse "* | *" opensuse "*) echo suse ;;
  *)
    if have apt-get; then
      echo debian
    elif have dnf; then
      echo fedora
    elif have zypper; then
      echo suse
    elif have pacman; then
      echo arch
    fi
    ;;
  esac
}

# The WebKitGTK the app draws its window with, pkg-config and a C compiler.
install_system_deps() {
  case "$(uname -s)" in
  Linux)
    family=$(distro)
    if [ "$family" = arch ]; then
      say "Ставлю WebKitGTK и инструменты сборки (pacman)…"
      as_root pacman -S --needed --noconfirm base-devel webkit2gtk-4.1 ||
        die "pacman не справился; обнови систему (sudo pacman -Syu) и запусти установку ещё раз"
    elif [ "$family" = debian ]; then
      say "Ставлю WebKitGTK и инструменты сборки (apt)…"
      as_root apt-get update
      as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y \
        build-essential pkg-config curl libwebkit2gtk-4.1-dev
    elif [ "$family" = fedora ]; then
      say "Ставлю WebKitGTK и инструменты сборки (dnf)…"
      as_root dnf install -y gcc pkgconf-pkg-config curl webkit2gtk4.1-devel
    elif [ "$family" = suse ]; then
      say "Ставлю WebKitGTK и инструменты сборки (zypper)…"
      as_root zypper --non-interactive install gcc pkg-config curl webkit2gtk3-devel
    else
      say "Не знаю пакетный менеджер этой системы. Поставь WebKitGTK 4.1 (dev-пакет) и компилятор C:"
      say "https://v2.tauri.app/start/prerequisites/#linux"
    fi
    ;;
  Darwin)
    if ! xcode-select -p >/dev/null 2>&1; then
      xcode-select --install || true
      die "поставь Command Line Tools for Xcode (окно установки уже открыто) и запусти установку ещё раз"
    fi
    ;;
  *)
    die "этот установщик для Linux и macOS; для Windows скачай установщик со страницы $REPO/releases"
    ;;
  esac
}

rust_ok() {
  have cargo && have rustc || return 1
  version=$(rustc --version 2>/dev/null | cut -d' ' -f2)
  major=${version%%.*}
  rest=${version#*.}
  minor=${rest%%.*}
  case "$major$minor" in '' | *[!0-9]*) return 1 ;; esac
  [ "$major" -gt 1 ] || [ "$minor" -ge "$MIN_RUST_MINOR" ]
}

ensure_rust() {
  # A rustup install that is not on this shell's PATH yet.
  if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
  fi
  if rust_ok; then return; fi
  if have rustup; then
    say "Обновляю Rust (нужен 1.$MIN_RUST_MINOR или новее)…"
    rustup update stable
  else
    say "Ставлю Rust (rustup)…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
      RUSTUP_INIT_SKIP_PATH_CHECK=yes sh -s -- -y --profile minimal
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
  fi
  rust_ok || die "нужен Rust 1.$MIN_RUST_MINOR или новее (rustup default stable)"
}

install_tgsum() {
  if [ -n "${TGSUM_BRANCH:-}" ]; then
    set -- --git "$REPO" --branch "$TGSUM_BRANCH" tgsum
  elif [ -n "${TGSUM_GIT:-}" ] || ! cargo search tgsum --limit 1 2>/dev/null | grep -q '^tgsum = '; then
    set -- --git "$REPO" tgsum
  else
    set -- tgsum
  fi
  say "Собираю tgsum: cargo install --locked $* (несколько минут)…"
  cargo install --locked "$@"
}

# The first start adds tgsum to the launcher (see src-tauri/src/launcher.rs).
open_app() {
  bin="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/tgsum"
  [ -x "$bin" ] || die "не нашёл установленный $bin"
  if [ "$(uname -s)" = Linux ] && [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
    say "Готово! Открываю tgsum; теперь он есть в меню приложений (в Omarchy: Super + Space)."
    setsid -f "$bin" >/dev/null 2>&1 </dev/null ||
      (nohup "$bin" >/dev/null 2>&1 </dev/null &)
  else
    say "Готово! Запуск: $bin"
  fi
  if ! have tgsum; then
    say "Команда tgsum появится в новом окне терминала."
  fi
}

install_system_deps
ensure_rust
install_tgsum
open_app
