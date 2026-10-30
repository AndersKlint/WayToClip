set -euo pipefail
exec 2>> "$application_log"
# Inherited activation tokens and Wayland FDs can refer to the host compositor.
unset DBUS_STARTER_ADDRESS DBUS_STARTER_BUS_TYPE WAYLAND_SOCKET XDG_ACTIVATION_TOKEN DESKTOP_STARTUP_ID
if [[ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]; then
    printf 'WayToClip launch refused: nested Wayland socket is missing: %s/%s\n' "$XDG_RUNTIME_DIR" "$WAYLAND_DISPLAY" >&2
    exit 1
fi
if ! bridge_owner=$(gdbus call --address "$DBUS_SESSION_BUS_ADDRESS" --timeout 2 \
    --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.NameHasOwner org.gnome.Shell.Extensions.WayToClipBridge 2>&1); then
    printf 'WayToClip launch refused: private session bus unavailable: %s\n' "$bridge_owner" >&2
    exit 1
fi
if [[ $bridge_owner != "(true,)" ]]; then
    printf 'WayToClip launch refused: bridge has no owner on the private session bus.\n' >&2
    exit 1
fi
printf 'WayToClip launch: WAYLAND_DISPLAY=%s\n' "$WAYLAND_DISPLAY" >&2
export GDK_BACKEND=wayland
exec "$source_root/target/debug/waytoclip" "$@"
