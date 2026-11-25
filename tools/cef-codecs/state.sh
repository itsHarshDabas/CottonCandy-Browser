#!/usr/bin/env bash
# save: packs the work folder into $STATE_DIR/state.tar.zst for the next stage. restore: unpacks it.
set -euo pipefail
work=$WORK state=$STATE_DIR
if [ "$RUNNER_OS" = Windows ]; then
  work=$(cygpath -u "$WORK") state=$(cygpath -u "$STATE_DIR")
fi
case "$1" in
  save)
    du -sh "$work" 2> /dev/null || true
    tar --force-local -C "$work" -cf - . | zstd -T0 -3 -q -o "$state/state.tar.zst"
    ls -la "$state"
    ;;
  restore)
    mkdir -p "$work"
    zstd -dc -q "$state/state.tar.zst" | tar --force-local -C "$work" -xf -
    rm -f "$state/state.tar.zst"
    ;;
esac
