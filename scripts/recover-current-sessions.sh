#!/usr/bin/env bash
set -u

# Recover the deliberately selected sessions from docs/CURRENT_SESSION_RECOVERY.md.
# Each tab starts a normal zsh and retains that shell after jcode exits or Ctrl+C.

if ! command -v kitty >/dev/null 2>&1; then
  printf 'kitty is not on PATH\n' >&2
  exit 1
fi

launch_session() {
  local title=$1
  local cwd=$2
  local session_id=$3

  kitty @ launch \
    --type=tab \
    --cwd="$cwd" \
    --tab-title="$title" \
    zsh -lc '
      jcode --no-update --resume "$1"
      rc=$?
      printf "\\n[jcode exited %s; shell retained in %s]\\n" "$rc" "$PWD"
      exec zsh
    ' zsh "$session_id"
}

launch_session \
  'Autospec / deer' \
  '/home/ari/repos/autospec' \
  'session_deer_1785959709238_1af8ca4ca2345274'

launch_session \
  'Locus / penguin' \
  '/home/ari/repos/locus' \
  'session_penguin_1785963770473_43f44b84e3b74796'

launch_session \
  'Jcode / sheep' \
  '/home/ari/repos/jcode' \
  'session_sheep_1785963902901_190ce18535a3e20b'
