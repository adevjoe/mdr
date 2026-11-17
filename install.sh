#!/bin/sh
# mdr 安装脚本：下载对应平台的预编译二进制，校验 SHA-256 后安装。
# 支持 Linux（x86_64 / aarch64，静态链接，不依赖 glibc 版本）和 macOS（arm64 / x86_64）。
#
#   curl -fsSL https://raw.githubusercontent.com/adevjoe/mdr/main/install.sh | sh
#
# 选项（也可以用环境变量）：
#   --version <tag>   安装指定版本，例如 v0.2.0；默认最新版本     MDR_VERSION
#   --dir <目录>       安装目录；默认 root 用 /usr/local/bin，      MDR_INSTALL_DIR
#                     其他用户用 ~/.local/bin
#
#   curl -fsSL https://raw.githubusercontent.com/adevjoe/mdr/main/install.sh | sh -s -- --dir /usr/local/bin
#
# 卸载：删除安装目录里的 mdr 即可。
set -eu

repo="adevjoe/mdr"
version="${MDR_VERSION:-}"
install_dir="${MDR_INSTALL_DIR:-}"

say() { printf '%s\n' "$*" >&2; }
die() { say "错误：$*"; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die "--version 需要一个参数"; version="$2"; shift 2 ;;
    --version=*) version="${1#*=}"; shift ;;
    --dir) [ $# -ge 2 ] || die "--dir 需要一个参数"; install_dir="$2"; shift 2 ;;
    --dir=*) install_dir="${1#*=}"; shift ;;
    -h | --help)
      cat <<'EOF'
用法：install.sh [--version <tag>] [--dir <目录>]
  --version <tag>  安装指定版本，例如 v0.2.0；默认最新版本（环境变量 MDR_VERSION）
  --dir <目录>      安装目录；默认 root 用 /usr/local/bin，其他用户用 ~/.local/bin（MDR_INSTALL_DIR）
EOF
      exit 0 ;;
    *) die "未知选项：$1（可用 --version、--dir）" ;;
  esac
done

# ---- 平台 ----
os=$(uname -s)
arch=$(uname -m)
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) die "暂不支持的 CPU 架构：$arch（支持 x86_64、aarch64）" ;;
esac
case "$os" in
  Linux) target="$arch-unknown-linux-musl" ;;
  Darwin)
    # 在 Rosetta 下运行的终端里 uname -m 会报 x86_64，这时仍装原生的 arm64 版本。
    if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
      arch=aarch64
    fi
    target="$arch-apple-darwin"
    ;;
  *) die "暂不支持的系统：$os（支持 Linux、macOS）" ;;
esac

# ---- 下载工具 ----
if command -v curl >/dev/null 2>&1; then
  fetch_once() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch_once() { wget -q -O "$2" "$1"; }
else
  die "需要 curl 或 wget"
fi
# 网络不稳时重试（curl 自带的 --retry 不处理 TLS 断连等错误，而且老版本 wget 没有重试选项）。
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

say "下载 $asset（${version:-最新版本}）..."
fetch "$base/$asset" "$tmp/$asset" || die "下载 $base/$asset 失败。这个版本可能还没有 $target 的二进制"
fetch "$base/checksums.txt" "$tmp/checksums.txt" || die "下载 checksums.txt 失败"

# ---- 校验 ----
expected=$(awk -v n="$asset" '$2 == n || $2 == "*" n { print $1 }' "$tmp/checksums.txt")
[ -n "$expected" ] || die "checksums.txt 里没有 $asset"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp/$asset" | awk '{ print $1 }')
else
  die "需要 sha256sum 或 shasum 来校验下载的文件"
fi
[ "$actual" = "$expected" ] || die "SHA-256 校验失败（期望 $expected，实际 $actual）"

# ---- 安装 ----
tar -xzf "$tmp/$asset" -C "$tmp" mdr
mkdir -p "$install_dir" 2>/dev/null || true
if [ -w "$install_dir" ]; then
  install -m 755 "$tmp/mdr" "$install_dir/mdr"
elif command -v sudo >/dev/null 2>&1; then
  say "$install_dir 需要管理员权限，使用 sudo 安装"
  sudo mkdir -p "$install_dir"
  sudo install -m 755 "$tmp/mdr" "$install_dir/mdr"
else
  die "没有 $install_dir 的写权限。可以用 --dir 换一个目录"
fi

say "已安装：$("$install_dir/mdr" --version)  →  $install_dir/mdr"

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    say ""
    say "注意：$install_dir 不在 PATH 里。把下面这行加到 ~/.bashrc 或 ~/.zshrc："
    say "  export PATH=\"$install_dir:\$PATH\""
    ;;
esac
