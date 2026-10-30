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
LC_ALL=C
export LC_ALL
[[ $binary != *[!a-zA-Z0-9_./\ -]* ]] || fail 'Binary path may contain only ASCII letters, digits, spaces, /, ., _, and -; use a build path without other characters.'

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
printf '%s\n' \
    '[Desktop Entry]' \
    'Type=Application' \
    'Name=WayToClip' \
    'Comment=Edit and paste clipboard text' \
    "Exec=\"$binary\"" \
    "TryExec=$binary" \
    'Terminal=false' \
    'Categories=Utility;' \
    'X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input;' > "$desktop_file"
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate "$desktop_file"
fi
kbuildsycoca6
printf 'Installed %s\nEnable WayToClip Bridge in System Settings → Window Management → KWin Scripts, then Apply.\nLaunch %s after enabling it.\n' "$desktop_file" "$binary"
