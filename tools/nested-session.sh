#!/usr/bin/env bash
set -euo pipefail
umask 077

source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
session_directory=$(mktemp -d "${TMPDIR:-/tmp}/waytoclip-session.XXXXXXXX")
nested_display="waytoclip-${session_directory##*.}"
trap 'rm -rf -- "$session_directory"' EXIT

export XDG_DATA_HOME="$session_directory/data"
export XDG_CONFIG_HOME="$session_directory/config"
export XDG_CACHE_HOME="$session_directory/cache"
export XDG_STATE_HOME="$session_directory/state"
export GSETTINGS_BACKEND=keyfile

extension_directory="$XDG_DATA_HOME/gnome-shell/extensions/waytoclip-bridge@waytoclip"
mkdir -p -- "$extension_directory" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
cp -R -- "$source_root/gnome-extension/." "$extension_directory/"
gsettings set org.gnome.shell enabled-extensions "['waytoclip-bridge@waytoclip']"
gsettings set org.gnome.shell disable-user-extensions false

export source_root session_directory nested_display
dbus-run-session -- bash -c '
set -euo pipefail
shell_pid=
log_pid=

cleanup() {
    local application_owner
    if application_owner=$(gdbus call --session --timeout 2 \
        --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.GetConnectionUnixProcessID \
        org.waytoclip.WayToClip 2>/dev/null); then
        if [[ $application_owner =~ uint32\ ([0-9]+) ]]; then
            kill -TERM "${BASH_REMATCH[1]}" 2>/dev/null || true
        fi
    fi
    if [[ -n $shell_pid ]]; then
        kill -TERM "$shell_pid" 2>/dev/null || true
        wait "$shell_pid" 2>/dev/null || true
    fi
    if [[ -n $log_pid ]]; then
        kill -TERM "$log_pid" 2>/dev/null || true
        wait "$log_pid" 2>/dev/null || true
    fi
}

trap cleanup EXIT
trap "exit 130" INT
trap "exit 143" TERM
gnome-shell --devkit --wayland --wayland-display "$nested_display" &
shell_pid=$!

deadline=$((SECONDS + 30))
is_bridge_ready=false
while (( SECONDS < deadline )) && kill -0 "$shell_pid" 2>/dev/null; do
    if bridge_info=$(gdbus call --session --timeout 1 \
        --dest org.gnome.Shell.Extensions.WayToClipBridge \
        --object-path /org/gnome/Shell/Extensions/WayToClipBridge \
        --method org.freedesktop.DBus.Introspectable.Introspect 2>&1) &&
        [[ $bridge_info == *org.gnome.Shell.Extensions.WayToClipBridge* && $bridge_info == *BeginPopup* ]]; then
        is_bridge_ready=true
        break
    fi
    sleep 0.25
done

if [[ $is_bridge_ready != true ]]; then
    printf "Bridge readiness failed (30-second startup limit or Shell exited). Last probe: %s\n" "${bridge_info:-not attempted}" >&2
    for method in GetExtensionInfo GetExtensionErrors; do
        printf "Extension %s:\n" "$method" >&2
        gdbus call --session --timeout 2 --dest org.gnome.Shell.Extensions \
            --object-path /org/gnome/Shell/Extensions \
            --method "org.gnome.Shell.Extensions.$method" \
            waytoclip-bridge@waytoclip >&2 || true
    done
    exit 1
fi

launcher="$session_directory/waytoclip"
application_log="$session_directory/app.stderr.log"
printf "#!/usr/bin/env bash\nsource_root=%q\napplication_log=%q\n" \
    "$source_root" "$application_log" > "$launcher"
for variable in DBUS_SESSION_BUS_ADDRESS XDG_RUNTIME_DIR XDG_DATA_HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_STATE_HOME GSETTINGS_BACKEND; do
    printf "export %s=%q\n" "$variable" "${!variable}" >> "$launcher"
done
printf "export WAYLAND_DISPLAY=%q\n" "$nested_display" >> "$launcher"
cat "$source_root/tools/nested-launch.sh" >> "$launcher"
chmod 700 "$launcher"
touch "$application_log"
tail -n +1 -f "$application_log" >&2 &
log_pid=$!
printf "Bridge ready (introspection only). App stderr: %s\n" "$application_log"
printf "Run in either a terminal or nested Alt+F2: \"%s\"\n" "$launcher"
printf "Keep the editable target focused inside the nested desktop.\n"
wait "$shell_pid"
'
