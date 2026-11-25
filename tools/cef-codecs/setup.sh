#!/usr/bin/env bash
# Frees disk space on the runner and sets WORK (the build folder), STATE_DIR (where the saved work
# folder goes) and PYTHON for the later steps.
set -euo pipefail
if [ "$RUNNER_OS" = Linux ]; then
  # A Chromium build needs about 100 GB; the runner image's preinstalled SDKs take much of that.
  sudo rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /usr/local/.ghcup /opt/hostedtoolcache/CodeQL \
    /usr/local/share/boost /usr/local/share/powershell /usr/share/swift /opt/microsoft || true
  sudo docker image prune --all --force > /dev/null 2>&1 || true
  sudo mkdir -p /mnt/state && sudo chown "$(id -u):$(id -g)" /mnt/state
  mkdir -p "$HOME/w"
  {
    echo "WORK=$HOME/w"
    echo "STATE_DIR=/mnt/state"
    echo "PYTHON=python3"
  } >> "$GITHUB_ENV"
  df -h / /mnt
else
  powershell -NoProfile -Command "Get-PSDrive -PSProvider FileSystem | Format-Table Name, @{n='FreeGB';e={[int](\$_.Free/1GB)}}, @{n='UsedGB';e={[int](\$_.Used/1GB)}}"
  # The same for Windows: SDKs and tool caches a Chromium build doesn't use. (Visual Studio stays.)
  for d in /c/Android /c/hostedtoolcache/windows/Java_* /c/hostedtoolcache/windows/go /c/hostedtoolcache/windows/Ruby \
    /c/hostedtoolcache/windows/PyPy /c/hostedtoolcache/windows/CodeQL /c/hostedtoolcache/windows/node /c/ghcup \
    /c/Strawberry /c/Miniconda /c/selenium /c/tools/php /c/rtools* "/c/Program Files/MongoDB" "/c/Program Files/PostgreSQL" \
    "/c/Program Files/Mozilla Firefox" "/c/Program Files (x86)/Android" /c/msys64 /c/vcpkg "/c/Program Files/Azure Cosmos DB Emulator" \
    "/c/Program Files/R" /c/julia* "/c/Program Files/Haskell" /c/cabal /c/SeleniumWebDrivers; do
    [ -e "$d" ] && { du -sh "$d" 2> /dev/null | tail -1; rm -rf "$d" 2> /dev/null || true; }
  done
  git config --global core.longpaths true
  git config --global core.autocrlf false
  command -v zstd > /dev/null || choco install zstandard -y --no-progress
  # Build on whichever drive has more room; save the work folder on the other one if there is one.
  free() { powershell -NoProfile -Command "[int]((Get-PSDrive $1 -ErrorAction SilentlyContinue).Free/1GB)" 2> /dev/null || echo 0; }
  c=$(free C) d=0
  [ -d /d ] && d=$(free D)
  echo "::notice::Free after cleanup: C: ${c} GB, D: ${d} GB"
  if [ "${d:-0}" -gt "${c:-0}" ]; then work=D:/w state=C:/state; else work=C:/w state=$([ -d /d ] && echo D:/state || echo C:/state); fi
  mkdir -p "$(cygpath -u "$work")" "$(cygpath -u "$state")"
  {
    echo "WORK=$work"
    echo "STATE_DIR=$state"
    echo "PYTHON=python"
  } >> "$GITHUB_ENV"
  powershell -NoProfile -Command "Get-PSDrive -PSProvider FileSystem | Format-Table Name, @{n='FreeGB';e={[int](\$_.Free/1GB)}}, @{n='UsedGB';e={[int](\$_.Used/1GB)}}"
fi
