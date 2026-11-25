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
  git config --global core.longpaths true
  git config --global core.autocrlf false
  command -v zstd > /dev/null || choco install zstandard -y --no-progress
  state=C:/state
  [ -d /d ] && state=D:/state
  mkdir -p /c/w "$(cygpath -u "$state")"
  {
    echo "WORK=C:/w"
    echo "STATE_DIR=$state"
    echo "PYTHON=python"
  } >> "$GITHUB_ENV"
  powershell -NoProfile -Command "Get-PSDrive -PSProvider FileSystem | Format-Table Name, @{n='FreeGB';e={[int](\$_.Free/1GB)}}, @{n='UsedGB';e={[int](\$_.Used/1GB)}}"
fi
