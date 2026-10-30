#!/usr/bin/env bash
set -euo pipefail

fail() {
    printf 'install-kde: %s\n' "$*" >&2
    exit 1
}

[[ $# -le 1 ]] || fail 'Usage: bash tools/install-kde.sh [built-binary]'
for command in realpath kpackagetool6 kbuildsycoca6; do
    command -v "$command" >/dev/null || fail "Required command not found: $command"
done

script_directory=$(dirname -- "$(realpath -e -- "${BASH_SOURCE[0]}")")
source_directory=$(realpath -e -- "$script_directory/../kwin-script")
binary=$(realpath -e -- "${1:-$script_directory/../target/debug/waytoclip}") || fail 'Build the binary first or supply an existing binary path.'
[[ -f $binary && -x $binary ]] || fail "Binary is not an executable file: $binary"

# A restricted alphabet avoids the distinct Exec and TryExec escaping grammars.
lc_all_was_set=${LC_ALL+x}
lc_all_value=${LC_ALL-}
LC_ALL=C
[[ $binary != *[!a-zA-Z0-9_./\ -]* ]] || fail 'Binary path may contain only ASCII letters, digits, spaces, /, ., _, and -; use a build path without other characters.'
if [[ $lc_all_was_set ]]; then
    export LC_ALL=$lc_all_value
else
    unset LC_ALL
fi

data_directory=${XDG_DATA_HOME:-${HOME:?HOME is unset}/.local/share}
[[ $data_directory = /* ]] || fail 'XDG_DATA_HOME must be an absolute path.'
desktop_directory=$data_directory/applications
desktop_file=$desktop_directory/org.waytoclip.WayToClip.desktop

if kpackagetool6 --type=KWin/Script --show waytoclip-bridge >/dev/null 2>&1; then
    kpackagetool6 --type=KWin/Script --upgrade "$source_directory"
else
    kpackagetool6 --type=KWin/Script --install "$source_directory"
fi

mkdir -p -- "$desktop_directory"
desktop_tmp=$(mktemp -- "$desktop_directory/.org.waytoclip.WayToClip.XXXXXX.desktop")
trap 'rm -f -- "$desktop_tmp"' EXIT
# KService parses X-KDE-* properties as comma-separated lists, unlike XDG semicolon lists.
printf '%s\n' \
    '[Desktop Entry]' \
    'Type=Application' \
    'Name=WayToClip' \
    'Comment=Edit and paste clipboard text' \
    "Exec=\"$binary\"" \
    "TryExec=$binary" \
    'Terminal=false' \
    'Categories=Utility;' \
    'X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input' > "$desktop_tmp"
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate "$desktop_tmp"
fi
chmod 0644 -- "$desktop_tmp"
mv -- "$desktop_tmp" "$desktop_file"
trap - EXIT
kbuildsycoca6 --noincremental
printf 'Installed %s\nEnable WayToClip Bridge in System Settings → Window Management → KWin Scripts, then Apply.\nLaunch %s after enabling it.\nIf fake-input is still unavailable, log out and back in to restart KWin.\n' "$desktop_file" "$binary"
