#!/bin/sh
# Compares two builds of the emulator on scripted screens, with no window.
# docs/development/frontend-guide.md says when to use it and how to read
# what it prints.
#
# Usage: compare.sh BEFORE AFTER OUT_DIR [NAMES_REGEX]
#   BEFORE, AFTER: two smilebasic binaries, kept in the same folder
#     (a binary run from target/debug behaves differently in the bench).
#   OUT_DIR: where the shots and each run's output go.
#   NAMES_REGEX: keeps the scenes whose name matches it.
#
# Each line printed is a scene, the two builds' hashes, and "same" or
# "DIFFER". Even between two runs of one build, about one scene in a
# hundred differs in a full run and matches when run again by itself, so
# run a differing scene again before believing it.
if [ $# -lt 3 ]; then
  sed -n '2,15p' "$0" >&2
  exit 2
fi
before=$1; after=$2; out=$3; only=$4
here=$(dirname "$0")
mkdir -p "$out"
# Two copies of one build compare equal whatever was changed. A build of
# the old commit made with uncommitted changes in the tree is the new build.
# Naming one binary twice is allowed: that measures how much two runs of
# one build differ by themselves.
if [ "$before" != "$after" ] && cmp -s "$before" "$after"; then
  echo "BEFORE and AFTER are the same binary: build the old commit from a clean checkout" >&2
  exit 2
fi
run() { # name binary spec flag
  # Each run gets its own data folder, so nothing one run saves reaches
  # the next.
  data=$out/data
  rm -rf "$data"
  mkdir -p "$data/smilebasic-emulator/projects"
  cp -r "$here/PERF" "$data/smilebasic-emulator/projects/PERF"
  # A file dialog shows when a file was changed.
  touch -d 2026-01-01T00:00:00Z "$data"/smilebasic-emulator/projects/PERF/*
  XDG_DATA_HOME=$data SDL_VIDEO_DRIVER=offscreen SDL_AUDIO_DRIVER=dummy \
    SB_UI_BENCH="$3,shot=$out/$1,novsync,frames=200" timeout 60 "$2" $4 > "$out/$1.txt" 2>&1
  # A scene that quits leaves no shot.
  [ -f "$out/$1-main.pam" ] || { echo "quit$(grep -c "could not" "$out/$1.txt")"; return; }
  m=$(python3 "$here/shothash.py" "$out/$1-main.pam")$(grep "^drawn by" "$out/$1.txt" | md5sum | cut -c1-3)
  [ -f "$out/$1-editor.pam" ] && m=$m$(md5sum < "$out/$1-editor.pam" | cut -c1-6)
  echo "$m"
}
i=0
grep -v '^#' "$here/scenes.txt" | while IFS='|' read -r name spec flag; do
  i=$((i+1))
  [ -n "$only" ] && ! echo "$name" | grep -Eq "$only" && continue
  a=$(run "q$i-before" "$before" "$spec" "$flag"); b=$(run "q$i-after" "$after" "$spec" "$flag")
  [ "$a" = "$b" ] && same=same || same=DIFFER
  printf "%-34s %s %s %s\n" "$name" "$a" "$b" "$same"
  # Keeps what a differing pair printed, as a later run reuses the names.
  if [ "$same" = DIFFER ]; then
    mkdir -p "$out/differ"
    cp "$out/q$i-before.txt" "$out/q$i-after.txt" "$out"/q$i-*-main.pam "$out/differ/" 2>/dev/null
    grep -H "^kept\|^stalls\|standby\|could not" "$out/q$i-before.txt" "$out/q$i-after.txt" | sed 's|.*/||' | cut -c1-110
  fi
done
