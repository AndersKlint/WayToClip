# WayToClip

A Rust port of the keyboard-driven WayToClip clipboard manager for Wayland. The goal is to preserve the original application's behavior with a full GTK4 popup and a thin desktop-specific bridge.

The interaction proofs target **GNOME Shell 50** (local environment: 50.6) and **Plasma/KWin 6.4+ on Wayland**. Both proofs are complete with user-accepted basic cursor placement and paste interaction. Broader version support is undecided.

## Current milestone

The current milestone is the **behavioral port** described in [PORTING.md](PORTING.md#3-behavioral-port--current). The completed interaction proofs implement one editable text payload with **Paste**, **Copy only**, **Read clipboard**, **Quit**, and a terminal-paste toggle. The payload stays in memory only. Escape or focus loss dismisses the popup. Launching toggles the resident app: hidden opens; visible or reading the clipboard cancels and dismisses. History capture, persistence, images, and private mode belong to later work.

This is not yet the complete clipboard manager. Build verification does not establish desktop compatibility; the detailed manual gate remains available for further acceptance and regression checks.

The user confirms nested GNOME “fully works. pops up at cursor and pastes fine and all”. This confirms basic placement and paste interaction, without establishing exhaustive first-frame, multi-monitor, edge, or mixed-scale acceptance.

After the fake-input blocker was fixed, the user also confirmed KDE popup placement at the cursor and paste into the original target worked, accepting the KDE proof. Detailed monitor/scale and other edge-case coverage remains outstanding.

## Quick development: nested GNOME session

Requires a Rust toolchain supporting edition 2024, **GTK4 >= 4.20**, **GLib/GIO >= 2.80**, **libxkbcommon development packages**, and `pkg-config`. Cargo uses `gtk4` 0.11 with `v4_20`, `gio` 0.22 with `v2_80`, and `xkbcommon` 0.9 (the crate version, not a system-library minimum). Typical libxkbcommon package names are `libxkbcommon-dev` or `libxkbcommon-devel`. These GTK/GLib requirements limit which distro releases can build the app, independently of their Plasma version. Run from this checkout:

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

For detailed placement acceptance, compare the first open with subsequent opens at different pointer positions. Optional `WAYTOCLIP_PLACEMENT_DEBUG=1 make nested-session` enables one Shell diagnostic per popup map with app IDs, title, captured pointer, desired position, and actual frame; it never logs editor or clipboard contents. Report these lines if placement regresses.

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

Agents have not installed or exercised the bridge. The user confirmation above covers nested GNOME; normal-session acceptance remains user-run. Updating bridge code also requires reloading the session to use the installed update.

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

## KDE Wayland proof: user installation

The same Rust GTK4 UI uses native `ext-data-control` text access and KWin fake-input on a worker connection. A nonvisual declarative KWin script handles cursor placement and target focus. There is no duplicated Qt UI, Qt runtime dependency in Rust, root helper, or `wl-paste` dependency. This is an explicit text interaction proof, not history monitoring.

In an actual **Plasma/KWin 6.4+ Wayland session**, install `kpackagetool6` and `kbuildsycoca6` alongside the build prerequisites above. From this checkout, run these commands yourself:

```sh
cargo build
bash tools/install-kde.sh
```

The optional argument selects a built executable, for example `bash tools/install-kde.sh target/release/waytoclip` after `cargo build --release`. The default is this checkout's `target/debug/waytoclip`, regardless of the invoking directory; explicit relative arguments are relative to your current directory. Paths with spaces work. Other than ASCII letters, digits, spaces, `/`, `.`, `_`, and `-`, binary-path characters are rejected with an error.

The helper installs or upgrades package ID `waytoclip-bridge`, writes `$XDG_DATA_HOME/applications/org.waytoclip.WayToClip.desktop` (default `~/.local/share/applications/`), optionally validates it with `desktop-file-validate`, and runs `kbuildsycoca6 --noincremental`. Its quoted `Exec` and unquoted `TryExec` reference the canonical absolute executable path. The entry declares `X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input` without a trailing semicolon, because KWin/KService parses `X-KDE-*` properties as comma-separated lists rather than XDG semicolon lists. A matching executable entry is required to expose private fake-input on Plasma 6.4+; the mechanism is verified on KWin 6.7.5/KF 6.30. Keep the binary at that path; rerun installation when switching binaries. Quit the resident app before upgrades or binary switches. The helper refreshes the service cache; if a running KWin still reports the interface as restricted, log out and back in as a fallback.

Enable **WayToClip Bridge** in **System Settings → Window Management → KWin Scripts**, then **Apply**. Installation does not enable/reconfigure scripts, start the app, or install autostart or global shortcuts. After enabling, run the installed binary or bind its absolute path to a custom KDE shortcut. Focus an editor before invoking that shortcut for acceptance. Backend selection uses `KDE` in `XDG_CURRENT_DESKTOP` and `XDG_SESSION_TYPE=wayland`; launch in the real KDE session, without spoofing these variables in GNOME.

Apply the [manual gate](PORTING.md#manual-gono-go-gate), including first/subsequent cursor placement, Copy only, Read clipboard, paste and terminal-toggle behavior, outside click/Escape, repeated invocation, recovery after unrelated focus, target closure, and app quit/restart. Multi-monitor and mixed-scale behavior are not runtime verified. Paste is unsupported if `v`, Control, or Shift keycodes are inconsistent across configured keyboard layouts. Before hiding the popup, KDE paste briefly waits for held modifiers to clear using GTK's keyboard state; release modifier keys if paste is blocked. Focus loss or unavailable keyboard state aborts before clipboard publication, without restoring focus. The worker still cannot establish the target's live modifier state, and focus restoration/injection across connections remain non-atomic; the guard does not guarantee atomic target safety.

For diagnostics, launch the binary from a terminal and retain stderr; that launch captures the terminal as the target. Use a custom shortcut with an editor focused for editor-paste acceptance. KWin errors may also appear in `journalctl --user -b`; where the distro supplies it, `journalctl --user -b -u plasma-kwin_wayland.service` narrows the log. If fake-input is unavailable, check the matching desktop entry, confirm the value has no trailing semicolon, and refresh the cache with `kbuildsycoca6 --noincremental`; log out and back in if KWin still rejects the interface.

## Origins and license

This ports the original WayToClip GNOME extension at `/home/anders/git/way-to-clip`. The original was inspired by [CLCL](https://nakka.com/soft/clcl/index_eng.html), with clipboard monitoring based on [Clipboard Indicator](https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator).

Licensed under the MIT license; see [LICENSE.rst](LICENSE.rst) for the preserved copyright and permission notices.
