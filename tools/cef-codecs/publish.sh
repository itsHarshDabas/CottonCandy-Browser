#!/usr/bin/env bash
# Uploads the finished CEF (the minimal binary distribution) to the draft release cef-codecs-<version>.
set -euo pipefail
dist=$WORK/chromium/src/cef/binary_distrib
[ "$RUNNER_OS" = Windows ] && dist=$(cygpath -u "$dist")
file=$(ls "$dist"/cef_binary_*_minimal.tar.bz2 | head -1)
version=$(basename "$file" | sed -E 's/^cef_binary_([0-9.]+)\+.*/\1/')
tag=cef-codecs-$version
sha256sum "$file" | sed "s#  .*/#  #" > "$file.sha256"
if ! gh release view "$tag" -R "$GITHUB_REPOSITORY" > /dev/null 2>&1; then
  gh release create "$tag" -R "$GITHUB_REPOSITORY" --draft --title "CEF $version with MP4 codecs" \
    --notes "CEF $version built by .github/workflows/cef-codecs.yml with proprietary_codecs=true ffmpeg_branding=Chrome (H.264, AAC). release.yml uses these files instead of the stock CEF when they are here."
fi
gh release upload "$tag" -R "$GITHUB_REPOSITORY" --clobber "$file" "$file.sha256"
echo "Uploaded $(basename "$file") to $tag"
