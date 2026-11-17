#!/usr/bin/env bash
# Generate the Homebrew formula for a GitHub Release and print it to stdout.
#
#   scripts/homebrew-formula.sh v0.1.0 > "$(brew --repository adevjoe/tap)/Formula/mdr.rb"
#
# Reads each asset's SHA-256 from the release's checksums.txt, so github.com must be reachable.
set -euo pipefail

tag=${1:?usage: $0 <tag>, e.g. v0.1.0}
repo=${GITHUB_REPOSITORY:-adevjoe/mdr}
version=${tag#v}
base="https://github.com/$repo/releases/download/$tag"

checksums=$(curl -fsSL "$base/checksums.txt") || {
  echo "failed to download $base/checksums.txt; make sure release $tag exists" >&2
  exit 1
}
sha() {
  local s
  s=$(awk -v n="$1" '$2 == n { print $1 }' <<<"$checksums")
  [ -n "$s" ] || { echo "$1 is not listed in checksums.txt" >&2; exit 1; }
  echo "$s"
}

arm=mdr-aarch64-apple-darwin.tar.gz
intel=mdr-x86_64-apple-darwin.tar.gz
linux_arm=mdr-aarch64-unknown-linux-musl.tar.gz
linux_intel=mdr-x86_64-unknown-linux-musl.tar.gz

# Older releases have no Linux binaries; only emit on_linux when the release has them.
on_linux=""
if grep -q " $linux_intel\$" <<<"$checksums" && grep -q " $linux_arm\$" <<<"$checksums"; then
  on_linux="
  on_linux do
    on_arm do
      url \"$base/$linux_arm\"
      sha256 \"$(sha "$linux_arm")\"
    end
    on_intel do
      url \"$base/$linux_intel\"
      sha256 \"$(sha "$linux_intel")\"
    end
  end
"
fi

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
$on_linux
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
