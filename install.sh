#!/bin/sh
# mdr installer: downloads the prebuilt binary for this platform, verifies its SHA-256 and installs it.
# Supports Linux (x86_64 / aarch64, statically linked, no glibc version requirement) and macOS (arm64 / x86_64).
#
#   curl -fsSL https://raw.githubusercontent.com/adevjoe/mdr/main/install.sh | sh
#
# Options (environment variables work too):
#   --version <tag>   install a specific version, e.g. v0.2.0; default: latest   MDR_VERSION
#   --dir <dir>       install directory; default: /usr/local/bin for root,     MDR_INSTALL_DIR
#                     ~/.local/bin for other users
#
#   curl -fsSL https://raw.githubusercontent.com/adevjoe/mdr/main/install.sh | sh -s -- --dir /usr/local/bin
#
# Uninstall: delete mdr from the install directory.
set -eu

repo="adevjoe/mdr"
version="${MDR_VERSION:-}"
install_dir="${MDR_INSTALL_DIR:-}"

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die "--version requires an argument"; version="$2"; shift 2 ;;
    --version=*) version="${1#*=}"; shift ;;
    --dir) [ $# -ge 2 ] || die "--dir requires an argument"; install_dir="$2"; shift 2 ;;
    --dir=*) install_dir="${1#*=}"; shift ;;
    -h | --help)
      cat <<'EOF'
Usage: install.sh [--version <tag>] [--dir <dir>]
  --version <tag>  install a specific version, e.g. v0.2.0; default: latest (env MDR_VERSION)
  --dir <dir>      install directory; default: /usr/local/bin for root, ~/.local/bin otherwise (env MDR_INSTALL_DIR)
EOF
      exit 0 ;;
    *) die "unknown option: $1 (available: --version, --dir)" ;;
  esac
done

# ---- platform ----
os=$(uname -s)
arch=$(uname -m)
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "unsupported CPU architecture: $arch (supported: x86_64, aarch64)" ;;
esac
case "$os" in
  Linux) target="$arch-unknown-linux-musl" ;;
  Darwin)
    # In a terminal running under Rosetta, uname -m reports x86_64; install the native arm64 build anyway.
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
      arch=aarch64
    fi
    target="$arch-apple-darwin"
    ;;
  *) die "unsupported OS: $os (supported: Linux, macOS)" ;;
esac

# ---- downloader ----
if command -v curl >/dev/null 2>&1; then
  fetch_once() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch_once() { wget -q -O "$2" "$1"; }
else
  die "curl or wget is required"
fi
# Retry on flaky networks (curl's --retry doesn't cover errors like dropped TLS connections, and old wget has no retry option).
fetch() {
  for _ in 1 2 3; do
    fetch_once "$1" "$2" && return 0
    sleep 2
  done
  return 1
}

if [ -n "$version" ]; then
  case "$version" in v*) ;; *) version="v$version" ;; esac
  base="https://github.com/$repo/releases/download/$version"
else
  base="https://github.com/$repo/releases/latest/download"
fi

if [ -z "$install_dir" ]; then
  if [ "$(id -u)" = 0 ]; then install_dir=/usr/local/bin; else install_dir="$HOME/.local/bin"; fi
fi

asset="mdr-$target.tar.gz"
tmp=$(mktemp -d 2>/dev/null || mktemp -d -t mdr)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $asset (${version:-latest})..."
fetch "$base/$asset" "$tmp/$asset" || die "failed to download $base/$asset. This version may not have a binary for $target"
fetch "$base/checksums.txt" "$tmp/checksums.txt" || die "failed to download checksums.txt"

# ---- verify ----
expected=$(awk -v n="$asset" '$2 == n || $2 == "*" n { print $1 }' "$tmp/checksums.txt")
[ -n "$expected" ] || die "$asset is not listed in checksums.txt"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp/$asset" | awk '{ print $1 }')
else
  die "sha256sum or shasum is required to verify the download"
fi
[ "$actual" = "$expected" ] || die "SHA-256 mismatch (expected $expected, got $actual)"

# ---- install ----
tar -xzf "$tmp/$asset" -C "$tmp" mdr
mkdir -p "$install_dir" 2>/dev/null || true
if [ -w "$install_dir" ]; then
  install -m 755 "$tmp/mdr" "$install_dir/mdr"
elif command -v sudo >/dev/null 2>&1; then
  say "$install_dir requires administrator privileges, installing with sudo"
  sudo mkdir -p "$install_dir"
  sudo install -m 755 "$tmp/mdr" "$install_dir/mdr"
else
  die "no write permission for $install_dir. Use --dir to choose another directory"
fi

say "Installed: $("$install_dir/mdr" --version)  ->  $install_dir/mdr"

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    say ""
    say "Note: $install_dir is not in your PATH. Add this line to ~/.bashrc or ~/.zshrc:"
    say "  export PATH=\"$install_dir:\$PATH\""
    ;;
esac
