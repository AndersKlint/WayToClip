# WayToClip

A Rust port of the keyboard-driven WayToClip clipboard manager for Wayland. The Rust application owns the full GTK4 popup; GNOME Shell or a KWin script supplies only the privileged desktop operations.

## Status

Milestones 1–5 are implemented. The application is now a resident, event-driven text and image clipboard manager with:

- pure history, clipboard, paste-target, search, shortcut, and secret-hint logic in Rust;
- an app-owned GSettings schema, async JSON history persistence, a clear-history scheduler, and bundled translations;
- text capture with exclusions, secret filtering, own-write suppression, and private mode;
- live image capture, asynchronous image previews, original image payload publication, and image persistence;
- a GTK4 popup with 10 numbered slots per page, paging, search, favorites, deletion, shortcut hints, theme-aware styling, and workarea fitting;
- GNOME Shell and KDE Wayland bridges, resident lifecycle, and CLI actions;
- an app-owned GTK4 Settings window and native global shortcut registration on both desktops.

The user reported the basic milestones 1–5 nested GNOME popup/history/image flow working (“good it works”). This is limited current-app feedback, not comprehensive desktop acceptance or KDE acceptance. Settings and global shortcuts are implemented but have no new desktop acceptance yet. Milestone 6 is user-run desktop acceptance, with Settings/global shortcuts next. Panel, autostart, and distribution/packaging remain deferred. The earlier POC acceptance is historical only; see [Historical POC acceptance](#historical-poc-acceptance).

## Build and run

Requires a Rust toolchain supporting edition 2024, **GTK4 >= 4.20**, **GLib/GIO >= 2.80**, **libxkbcommon development packages**, `pkg-config`, and **`glib-compile-schemas`**. Typical libxkbcommon package names are `libxkbcommon-dev` or `libxkbcommon-devel`.

```sh
cargo build
./target/debug/waytoclip
```

`build.rs` compiles `schemas/` into `$OUT_DIR/schemas`. At runtime the application uses a system-installed `org.waytoclip.WayToClip` schema first and falls back to that build-tree directory. A binary copied outside this checkout still needs the schema installed in the system schema path (for example `/usr/share/glib-2.0/schemas`, followed by `glib-compile-schemas`) or `GSETTINGS_SCHEMA_DIR` set to a directory containing it.

Launching with no arguments starts the resident recorder and opens the popup. Dismissing the popup does not quit the app: capture continues while it is hidden. Run the same command again to open the popup, or invoke it while the popup is visible to dismiss it. Configure app-owned global shortcuts in Settings. There is no autostart: launch the app once per session; shortcuts work only while it is resident. `--quit` releases its registrations.

Available command-line actions:

```text
waytoclip
waytoclip --settings
waytoclip --quit
waytoclip --clear-history
waytoclip --toggle-private-mode
waytoclip --reset-clear-timer
```

With no arguments, `waytoclip` launches or toggles the popup. `--clear-history` follows the `confirm-clear` setting and is ignored while private mode is active. Scheduled interval clears use a separate path and are not gated by private mode.

## Popup keys

These are the default local shortcuts; customize them in Settings → Popup shortcuts or through the `local-*` schema keys.

| Key | Action |
| --- | --- |
| `s` | Enter search |
| `f` | Toggle favorite on the selected entry |
| `d` | Delete the selected entry |
| `p` | Toggle private mode |
| `-` | Open the favorites view or go back |
| `Up` / `Down` | Move the selection |
| `Left` / `Right` or `Shift+Tab` / `Tab` | Previous / next page |
| `Enter` | Confirm the selected entry |
| `Esc` | Close the popup, or leave search |
| `1`–`9`, `0` | Quick-select slots 1–10 |
| `Alt+C` / `Alt+R` | Toggle case-sensitive / regex search |

## Settings

Open `waytoclip --settings`, or use the small gear in the popup. The gear cancels the popup transaction before opening Settings. Settings is an ordinary application window, independent of popup placement. In nested GNOME, append `--settings` to the printed temporary wrapper command.

The theme-aware, translated GTK4 window is owned by the app and adds no libadwaita or other dependencies. It ports the original settings groups: **General**, **Popup**, **Behavior**, **Limits**, **Exclusion**, **Global shortcuts**, and **Popup shortcuts**, preserving source key names. Changes apply live; reopen Settings after changing language to refresh its labels. `clear-on-boot` is accurately labeled **Clear history when WayToClip starts**: it clears on application start, not only on OS reboot.

Global shortcuts retain the original three actions: toggle popup (`toggle-popup`), private mode (`private-mode-binding`), and clear history (`clear-history`). All three shortcut lists default to empty; choose a binding such as **Alt+X** for the popup. `enable-keybindings` controls registration. Shortcut chips support multiple bindings per action, Add/remove, and Reset to defaults. Status reports registration success, conflicts, or backend unavailability. Capture first waits for the desktop backend to suspend the app's global grabs, then accepts a key; completing or cancelling capture restores the configured registrations.

The application uses schema `org.waytoclip.WayToClip` at `/org/waytoclip/WayToClip/`. It is app-owned and preserves the source key names and defaults, including shortcut lists. It is not the original extension's schema; there is no settings or history migration from the original extension or the earlier POC.

For a local debug build, inspect or change a key through the compiled schema directory:

```sh
gsettings --schemadir target/debug/build/waytoclip-*/out/schemas get org.waytoclip.WayToClip auto-paste
gsettings --schemadir target/debug/build/waytoclip-*/out/schemas set org.waytoclip.WayToClip auto-paste false
```

Use `ls -d target/debug/build/waytoclip-*/out/schemas` to see the actual path, and replace the glob if it is ambiguous. For a release build, use `target/release/build/...` instead.

`dconf` is not a guaranteed dependency, but on systems that provide it the same key can be changed directly:

```sh
dconf write /org/waytoclip/WayToClip/auto-paste false
```

The optional `dconf-editor` path is `/org/waytoclip/WayToClip/`.

## Images

Capture follows the source MIME priority: plain-text variants (`text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`, `STRING`), then GIF, PNG, JPG, JPEG, WebP, SVG, then HTML. An offer containing text and images therefore prefers text. Clipboard payloads are limited to **16 MiB** on both desktops.

Image previews decode asynchronously on a worker using native GdkPixbuf loaders; available formats depend on the installed loaders. GIF previews are static. `image-preview-size` is clamped to 32–512 logical pixels, with aspect ratio preserved. Pending, unsupported, or corrupt images use a generic image icon. Selecting an image publishes its stored MIME type and full original bytes, not the preview.

`should-cache-images=false` skips new image capture entirely, rather than merely disabling disk writes. Captured images are cached as checksum-named PNG/JPEG/GIF/WebP/SVG payload files alongside `registry.txt`; persisted records reload those files on restart, and missing files are skipped. Deletion and history clearing clean up removed image payloads. `cache-only-favorites` persists only favorite history records, but image payload files are still cached upon capture, matching the source behavior.

## Desktop bridges and protocol change

The bridge protocol changed for history monitoring and content-aware paste. **The new application does not support the old bridge protocol.** Refresh a staged nested bridge by restarting `make nested-session`, or update the bridge installed in your normal session and restart/reload GNOME Shell or KWin as appropriate. Reinstalling only the Rust binary while leaving an old bridge installed will not work.

GNOME now also requires `ConfigureShortcuts(a(sas)) → ()`: an authoritative replacement list of action names and accelerator arrays. Keep the app and bridge contract synchronized. Restart `make nested-session` to stage the new `shortcuts.js` and updated bridge before testing Settings/global shortcuts. Schema keys and shortcut defaults are unchanged.

Agents build and inspect the code; they do not install or modify desktop extensions or running desktop sessions. Bridge installation and acceptance are user-run.

Milestone 5 also includes bridge bug fixes, payload caps, and byte-reading changes despite unchanged method signatures. Refresh the staged GNOME bridge by restarting `make nested-session` before image acceptance.

GNOME source-app attribution uses focused-app context at the clipboard event, not a true clipboard-owner identity. Exclusions based on that context can misattribute copies under focus changes or races.

### GNOME

These commands are for the user to execute from the checkout on GNOME 50:

```sh
mkdir -p "$HOME/.local/share/gnome-shell/extensions/waytoclip-bridge@waytoclip"
cp -R gnome-extension/. "$HOME/.local/share/gnome-shell/extensions/waytoclip-bridge@waytoclip/"
```

On Wayland, log out and back in after installing or updating the extension, then enable it:

```sh
gnome-extensions enable waytoclip-bridge@waytoclip
```

Launch the app and assign shortcuts in Settings → Global shortcuts. The bridge uses Meta accelerator grabs in `NORMAL | OVERVIEW`, never on the lock screen. It does not require the GNOME settings daemon, including in nested Shell sessions. No autostart entry is installed.

### KDE

In a Plasma/KWin 6.4+ Wayland session, install `kpackagetool6` and `kbuildsycoca6` alongside the build prerequisites, then run these commands yourself:

```sh
cargo build
bash tools/install-kde.sh
```

The optional argument selects a built executable, for example `bash tools/install-kde.sh target/release/waytoclip`. The helper upgrades package `waytoclip-bridge`, writes the matching desktop entry for the canonical binary path, and refreshes KDE's service cache. Re-run it after KWin script or binary changes; if fake-input remains restricted, log out and back in. Enable **WayToClip Bridge** in **System Settings → Window Management → KWin Scripts**, apply, then launch the app and assign shortcuts in its Settings. Backend selection requires `KDE` in `XDG_CURRENT_DESKTOP` and `XDG_SESSION_TYPE=wayland`; test in a real KDE session.

Global shortcuts use native KGlobalAccel over session D-Bus, with stable component identity `org.waytoclip.WayToClip` and app-owned `_k_session:` action IDs registered nonpersistently. The KWin script has no visual UI. Registrations refresh when the daemon owner restarts. A forced app kill may leave runtime grabs until app relaunch or KGlobalAccel restart; use `--quit` for normal release.

KDE paste is unsupported if the configured layouts do not provide consistent `v`, Control, and Shift keycodes. Before hiding the popup, the app briefly waits for held modifiers to clear; release them if paste is blocked. Focus restoration and injection remain non-atomic across separate connections.

KDE's data-control protocol exposes no clipboard owner. Source-app attribution therefore uses a best-effort snapshot of the focused app reported by KWin, and own-write suppression correlates the next selection echo after publication within a short deadline. Either can miss or misclassify under races or unusual focus changes; this is not an atomic ownership guarantee.

## Nested GNOME development

`make nested-session` builds the debug binary, stages the latest bridge in temporary session-local data, enables it in isolated settings, and opens a nested GNOME Shell on a private D-Bus session. It requires `make`, Bash, `dbus-run-session`, `gsettings`, `gdbus`, `tail`, `glib-compile-schemas`, and GNOME Shell with `--devkit` and `--wayland`. No installation into your normal session or logout is needed.

1. Open an editor or terminal inside the nested Shell and focus an editable target there.
2. Wait for **Bridge ready**, then run the printed temporary wrapper command from a terminal or **Alt+F2** inside the nested Shell. The wrapper targets the nested Wayland socket, private bus, and isolated settings.
3. The first launch starts the recorder and opens the popup. Dismiss it, copy text in the nested session, and launch the same command again to verify capture. Run it again while the popup is visible to dismiss it.

Running the binary directly or using `cargo run` in the original terminal uses the host display and bus. Use the wrapper command, including with `--settings`. App-owned global shortcuts work through the bridge without a settings daemon; the nested session can now exercise them. The helper checks bridge readiness only; it does not verify clipboard capture, placement, paste, or shortcuts. Restart `make nested-session` after bridge edits to stage the new files. The helper terminates the app on exit and discards the temporary session data.

For placement diagnostics, `WAYTOCLIP_PLACEMENT_DEBUG=1 make nested-session` enables one Shell line per popup map with app IDs, title, captured pointer, desired position, and actual frame; it does not log clipboard contents.

## Manual acceptance checklist

Build verification does not establish desktop behavior. Basic milestones 1–5 nested GNOME popup/history/image flow was reported working (“good it works”); broader acceptance remains user-run. Settings/global shortcuts are the next gate and have no new desktop acceptance. KDE requires a separate real KDE Wayland session.

1. Restart `make nested-session` to stage the updated bridge, then run its printed wrapper with `--settings`.
2. Under **Global shortcuts**, use **Add** for the popup action and record **Alt+X** after capture reports readiness. Close Settings and focus an editor inside the nested session.
3. Press the hotkey to open the popup and select an entry to paste. Reopen and press the hotkey again to verify toggle dismissal.
4. Edit/remove bindings, Reset, and disable `enable-keybindings`; observe that old grabs are released. Try a system-reserved key and check conflict status. Cancel capture and verify configured shortcuts return.
5. Open Settings from the popup gear and verify popup cancellation completes first. Check that Settings remains an ordinary window and does not receive popup placement. Exercise live controls and reopen after a language change.

- **Image round trip:** restart `make nested-session`, launch its printed wrapper, dismiss the popup, copy a screenshot inside the nested session, reopen, verify its preview, then select it and paste into an image-capable application there.
- **Image lifecycle and policy:** favorite an image and restart the app to verify reload (including `cache-only-favorites`); delete or clear images and check payload cleanup, respecting retained entries. Verify private mode and `should-cache-images=false` suppress new image captures. Recopy the same MIME type and bytes and confirm no duplicate entry appears, including after selecting/pasting it.
- **Image variety:** where available, try transparent, very wide, large, and non-PNG images; verify aspect-preserving previews and original-image paste. Check the generic-icon fallback for unsupported/corrupt images and rejection of payloads above 16 MiB.

- **Capture while hidden:** copy text from several applications after dismissing the popup; reopen and confirm the new entries appear. Capture must continue while no popup is visible.
- **Restart and persistence:** restart the app and confirm history and favorites reload from `$XDG_CACHE_HOME/org.waytoclip.WayToClip/registry.txt` (default `~/.cache/org.waytoclip.WayToClip/registry.txt`). Be aware that `clear-on-boot` is currently implemented as clear on every application start, not only on an OS reboot.
- **Favorites and deletion:** toggle a favorite, reopen the app, and verify it survives trimming; delete an entry and verify it disappears. In the favorites view, verify the back row and removal of a favorite.
- **Private mode:** verify `--toggle-private-mode` or the popup shortcut suppresses new captures and pending reads, and that leaving private mode restores the selected item. Verify `--clear-history` is ignored while private mode is active.
- **Secrets and exclusions:** copy from a password manager with `ignore-secret-mimetypes` enabled and confirm it is not recorded; add an application's app ID or WM class to `excluded-apps` and confirm its copies are skipped. Selecting or pasting an item must not re-record the app's own write.
- **Keyboard, search, and custom bindings:** verify number slots `1`–`0`, paging, `s` search, case-sensitive and regex toggles, and invalid-regex fallback to substring matching. Change a local shortcut in Settings and confirm the popup and footer hints use it. Verify all three app-owned global actions and multiple bindings per action.
- **Cursor-side fit:** verify cursor mode chooses above or below using the measured popup size, receives that side's available height, constrains the popup, and only then presents it. This path has no known gap now, but desktop behavior remains unverified. Also test window-center mode on GNOME and KDE, on first and subsequent opens, near monitor edges, and with multiple monitors or mixed scales. Confirm the popup fits the workarea and scrolls instead of overflowing.
- **Scheduler:** enable interval clearing, verify `next-history-clear` persists, confirm an overdue clear runs at start, and verify `--reset-clear-timer` reschedules.

## Historical POC acceptance

The earlier interaction proofs accepted a removed single-payload POC editor with **Paste**, **Copy only**, **Read clipboard**, **Quit**, and a terminal-paste toggle. Those controls no longer exist, and those acceptances do not apply to the current implementation.

- **GNOME historical acceptance:** the user confirmed nested GNOME “fully works. pops up at cursor and pastes fine and all”. This covered basic cursor placement and paste for the old POC, not exhaustive first-frame, multi-monitor, edge, or mixed-scale behavior.
- **KDE historical acceptance:** after the fake-input blocker was fixed, the user confirmed popup placement at the cursor and paste into the original target. This also covered only basic interaction for the old POC.

These POC reports are separate from the limited current-app nested GNOME feedback recorded above. See [PORTING.md](PORTING.md#historical-interaction-proofs) for the preserved proof records.

## Origins and license

This ports the original WayToClip GNOME extension at `/home/anders/git/way-to-clip`. The original was inspired by [CLCL](https://nakka.com/soft/clcl/index_eng.html), with clipboard monitoring based on [Clipboard Indicator](https://github.com/Tudmotu/gnome-shell-extension-clipboard-indicator).

Licensed under the MIT license; see [LICENSE.rst](LICENSE.rst) for the preserved copyright and permission notices.
