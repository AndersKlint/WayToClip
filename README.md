# WayToClip

A Rust port of the keyboard-driven WayToClip clipboard manager for Wayland. The goal is to preserve the original application's behavior with a full GTK4 popup and a thin desktop-specific bridge.

The first feasibility slice targets **GNOME Shell 50** (local environment: 50.6). KDE Wayland is next, after the GNOME interaction gate. Broader version support is undecided.

## Current milestone

The first slice implements one editable text payload with **Paste**, **Copy only**, **Read clipboard**, **Quit**, and a terminal-paste toggle. The payload stays in memory only. Escape or focus loss dismisses the popup. Launching toggles the resident app: hidden opens; visible or reading the clipboard cancels and dismisses. History capture, persistence, images, and private mode belong to later work.

This is an interaction proof, not the complete clipboard manager. Build verification does not establish desktop compatibility: user-run acceptance is required. See [PORTING.md](PORTING.md) for the gate and subsequent milestones.

The user confirms the popup opens in nested GNOME and says “other operations worked”, but placement was at the top-left instead of the cursor. The placement correction awaits manual acceptance; the detailed gate is still open.

## Quick development: nested GNOME session

Requires a Rust toolchain supporting edition 2024, **GTK4 >= 4.20** and **GLib/GIO >= 2.80** development packages, and `pkg-config`. Cargo uses `gtk4` 0.11 with `v4_20` and `gio` 0.22 with `v2_80`. Run from this checkout:

```sh
make nested-session
```

This is the preferred development workflow on GNOME Shell 50. It requires `make`, Bash, `dbus-run-session`, `gsettings`, `gdbus`, `tail`, and GNOME Shell with `--devkit` and `--wayland`. It builds the debug executable, stages the latest bridge in temporary session-local data, enables it in isolated settings, and opens a nested Shell on a private D-Bus session. No installation into your normal session or logout is needed.

1. Open an editor or terminal **inside the nested Shell**, and focus an editable target there.
2. Wait for **Bridge ready** in the original terminal. Run the exact temporary wrapper command printed by the helper, for example `"/tmp/waytoclip-session.XXXXXXXX/waytoclip"`, from either a terminal or **Alt+F2** inside the nested Shell. Keep the quotes if the path contains spaces. The wrapper launches this checkout's debug binary using captured session settings, runtime directory, private bus, and the explicitly named nested Wayland socket, with GTK restricted to Wayland. It refuses to launch if that socket, bus, or bridge owner is unavailable.
3. Invoke the same command again to toggle the resident popup. Test Paste, Copy only, Read clipboard, dismissal, and terminal paste against the [manual gate](PORTING.md#manual-gono-go-gate).

Running the binary directly or using `cargo run` in the original terminal still uses the host display and bus. The generated wrapper deliberately targets the nested session regardless of the invoking terminal's environment. Keep the editable target focused **inside the nested desktop**; the wrapper does not create or focus a target. Launch target applications inside the nested Shell. Shell-only devkit sessions do not provide the settings daemon needed for custom GNOME keyboard shortcuts, so use the wrapper command here. No popup is opened automatically at startup.

The helper allows about 30 seconds for the bridge name and object to answer introspection. If readiness fails or Shell exits during startup, it prints the last probe error plus GNOME's extension info/errors, then exits unsuccessfully. This checks bridge availability only; it does not call `BeginPopup`, read the clipboard, or verify focus/placement.

Shell output remains visible in the original terminal. The wrapper appends app stderr to the printed session-local `app.stderr.log`, mirrored to that terminal, and records the configured nested Wayland display on each invocation. If no popup appears, report the readiness/probe result, extension info/errors if shown, the launch line and following app errors, and relevant Shell errors. No clipboard payloads or clipboard method replies are collected by these diagnostics. Copy relevant diagnostics before closing the session: the log is temporary.

Close the nested Shell to end the foreground command. The helper terminates the app owning `org.waytoclip.WayToClip` on the private bus and discards the temporary data, configuration, cache, state, wrapper, and app log. The host runtime directory and display connection are retained for nesting. Restart `make nested-session` after bridge edits to stage the new files. Desktop behavior remains user-verified.

For the placement retry, move the pointer well inside the nested screen before launching, and compare the first open with subsequent opens at different pointer positions. Optional `WAYTOCLIP_PLACEMENT_DEBUG=1 make nested-session` enables one Shell diagnostic per popup map with app IDs, title, captured pointer, desired position, and actual frame; it never logs editor or clipboard contents. Report these lines if placement still fails.

## Optional: install the GNOME bridge in your normal session

The Rust application uses ID `org.waytoclip.WayToClip` and remains resident after dismissal. This slice does not install an auto-start entry. The following workflow is for testing in your normal session instead of the isolated nested session.

### Install the bridge yourself

These commands are for the user to execute from the checkout on GNOME 50:

```sh
mkdir -p "$HOME/.local/share/gnome-shell/extensions/waytoclip-bridge@waytoclip"
cp -R gnome-extension/. "$HOME/.local/share/gnome-shell/extensions/waytoclip-bridge@waytoclip/"
```

On Wayland, **log out and back in** after installing a new extension so GNOME Shell discovers it. Then enable it:

```sh
gnome-extensions enable waytoclip-bridge@waytoclip
```

Installation and runtime interaction have not been performed by the agents. First-frame placement and focus behavior remain unverified on the desktop. Updating bridge code also requires reloading the session to use the installed update.

### Set a shortcut

In GNOME Settings, add a custom keyboard shortcut with this absolute command for the current checkout:

```text
/home/anders/git/WayToClip/target/debug/waytoclip
```

After `cargo build --release`, the corresponding command is `/home/anders/git/WayToClip/target/release/waytoclip`. Adjust the path if you move the checkout. Choose the shortcut yourself; none is installed by default.

The original WayToClip extension can conflict if it uses the same shortcut, and its clipboard monitoring can re-record payloads copied by this app. Account for it when manually testing.

### Normal-session manual run workflow

1. Install the bridge yourself, log out/in, and enable it as above. Build and bind the shortcut.
2. Focus a target editor, then invoke the shortcut. Type a payload and choose **Paste**; verify insertion into that editor.
3. Invoke again and try **Copy only**; verify the clipboard changes without a paste. **Read clipboard** explicitly loads clipboard text into the editor.
4. Focus a terminal before invoking and check **Terminal paste (Ctrl+Shift+V)**, including changing the toggle.
5. Try Escape and an outside click that moves focus. Invoke repeatedly: hidden opens, while visible or reading cancels/dismisses. Check placement and focus against the [manual gate](PORTING.md#manual-gono-go-gate).

Dismissal leaves the app resident. Choose **Quit**, run `cargo run -- --quit`, or execute `/home/anders/git/WayToClip/target/debug/waytoclip --quit` to shut it down.

## Origins and license

This ports the original WayToClip GNOME extension at `/home/anders/git/way-to-clip`. The original was inspired by [CLCL](https://nakka.com/soft/clcl/index_eng.html), with clipboard monitoring based on [Clipboard Indicator](https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator).

Licensed under the MIT license; see [LICENSE.rst](LICENSE.rst) for the preserved copyright and permission notices.
