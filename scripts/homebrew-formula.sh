#!/usr/bin/env bash
# 根据某个 GitHub Release 生成 Homebrew formula，输出到标准输出。
#
#   scripts/homebrew-formula.sh v0.1.0 > "$(brew --repository adevjoe/tap)/Formula/mdr.rb"
#
# 仓库是私有的，release 附件只能通过 GitHub API 带令牌下载，
# 所以 url 指向 API 附件地址，安装时需要设置 HOMEBREW_GITHUB_API_TOKEN。
# 需要已登录的 gh（或设置 GH_TOKEN）。
set -euo pipefail

tag=${1:?用法: $0 <tag>，例如 v0.1.0}
repo=${GITHUB_REPOSITORY:-adevjoe/mdr}
version=${tag#v}

assets=$(gh api "repos/$repo/releases/tags/$tag" --jq '.assets[] | "\(.name) \(.id)"')
asset_id() {
  local id
  id=$(awk -v n="$1" '$1 == n { print $2 }' <<<"$assets")
  [ -n "$id" ] || { echo "release $tag 里没有 $1" >&2; exit 1; }
  echo "$id"
}

checksums=$(gh api -H "Accept: application/octet-stream" "repos/$repo/releases/assets/$(asset_id checksums.txt)")
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
      url "https://api.github.com/repos/$repo/releases/assets/$(asset_id "$arm")",
          headers: [
            "Authorization: Bearer #{ENV.fetch("HOMEBREW_GITHUB_API_TOKEN", "")}",
            "Accept: application/octet-stream",
          ]
      sha256 "$(sha "$arm")"
    end
    on_intel do
      url "https://api.github.com/repos/$repo/releases/assets/$(asset_id "$intel")",
          headers: [
            "Authorization: Bearer #{ENV.fetch("HOMEBREW_GITHUB_API_TOKEN", "")}",
            "Accept: application/octet-stream",
          ]
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
