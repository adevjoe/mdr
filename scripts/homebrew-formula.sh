#!/usr/bin/env bash
# 根据某个 GitHub Release 生成 Homebrew formula，输出到标准输出。
#
#   scripts/homebrew-formula.sh v0.1.0 > "$(brew --repository adevjoe/tap)/Formula/mdr.rb"
#
# 从 release 的 checksums.txt 读取各个附件的 SHA-256，需要能访问 github.com。
set -euo pipefail

tag=${1:?用法: $0 <tag>，例如 v0.1.0}
repo=${GITHUB_REPOSITORY:-adevjoe/mdr}
version=${tag#v}
base="https://github.com/$repo/releases/download/$tag"

checksums=$(curl -fsSL "$base/checksums.txt") || {
  echo "下载 $base/checksums.txt 失败，请确认 release $tag 存在" >&2
  exit 1
}
sha() {
  local s
  s=$(awk -v n="$1" '$2 == n { print $1 }' <<<"$checksums")
  [ -n "$s" ] || { echo "checksums.txt 里没有 $1" >&2; exit 1; }
  echo "$s"
}

arm=mdr-aarch64-apple-darwin.tar.gz
intel=mdr-x86_64-apple-darwin.tar.gz

cat <<EOF
class Mdr < Formula
  desc "Render Markdown in the terminal with CJK-aware wrapping and tables"
  homepage "https://github.com/$repo"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/$arm"
      sha256 "$(sha "$arm")"
    end
    on_intel do
      url "$base/$intel"
      sha256 "$(sha "$intel")"
    end
  end

  def install
    bin.install "mdr"
  end

  test do
    (testpath/"t.md").write <<~MD
      # 你好

      | 名称 | 值 |
      |------|----|
      | 中文 | 1  |
    MD
    output = shell_output("#{bin}/mdr --no-color --width 40 t.md")
    assert_match "你好", output
    assert_match "│ 中文 │ 1  │", output
    assert_match version.to_s, shell_output("#{bin}/mdr --version")
  end
end
EOF
