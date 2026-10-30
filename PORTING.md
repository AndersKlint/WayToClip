# Porting plan

## Direction and boundaries

Port the behavior of `/home/anders/git/way-to-clip` faithfully to Rust. Its source domains are clipboard, history, cursorPopup, paste, panel, settings, and common, connected by a small controller and entrypoint. Preserve those responsibilities while using native Rust ownership and GTK4 APIs.

The Rust application owns the entire GTK4 popup, Settings window, and history/recorder lifecycle. The GNOME Shell bridge supplies privileged clipboard access, placement/focus, paste operations, and global accelerator grabs. On KDE, a native Rust Wayland worker owns text and image clipboard access and fake-input; a nonvisual declarative KWin script owns placement/focus, while native Rust uses KGlobalAccel over session D-Bus for shortcuts. Keep these narrow desktop boundaries, not a generic plugin framework or parallel Qt UI. Shell code must not import GTK; UI work belongs in the app. Bridge enable/disable must own and release its signals, sources, and exported services.

Pure domain behavior stays independent of GTK, Wayland, and D-Bus. Normalize platform details at the bridge boundary. Use native ownership and `Drop`, weak GTK signal captures where strong captures would form cycles, and explicit shutdown/cancellation for outstanding work. Keep blocking I/O off the UI thread and use asynchronous boundary operations.

Shared identity:

- Application: `org.waytoclip.WayToClip`
- Settings schema: `org.waytoclip.WayToClip` at `/org/waytoclip/WayToClip/`
- Cache: `$XDG_CACHE_HOME/org.waytoclip.WayToClip/registry.txt`
- GNOME bridge bus name/interface: `org.gnome.Shell.Extensions.WayToClipBridge`
- GNOME bridge object path: `/org/gnome/Shell/Extensions/WayToClipBridge`
- KDE endpoint on the application bus name: interface `org.waytoclip.WayToClip.Kde`, fixed object path `/org/waytoclip/WayToClip/Kde`
- KWin package ID: `waytoclip-bridge`

The bridge implementations are authoritative for signatures. GNOME exposes `BeginPopup` (popup-position mode; returns token, app ID, WM class, terminal hint, target presence, and initial workarea width/height), `SettlePopup` (token and measured popup width/height; returns the chosen side's available height), `FinishPopup` (token, MIME type, bytes, paste choice, terminal choice), `CancelPopup` (token, focus-restoration choice), `WatchClipboard` (serial cursor; returns serial, MIME types, source identity, and own-write marker), `ReadClipboardContent` (serial, MIME type), and `WriteClipboard` (MIME type, bytes). KDE's `NextCommand`, `ReportResult`, and `ReportEvent` carry token-scoped `begin`, `settle`, `restore`, `cancel`, and `complete` commands and results; `settle` also receives the measured width/height and returns the chosen side's available height. Only the current `org.kde.KWin` owner may call the endpoint. Clipboard content stays in the native worker, not the KWin command transport.

The new bridge protocol is not backward compatible with the earlier POC. The app requires the updated GNOME extension or KWin script; an old installed bridge must be replaced, and a staged nested bridge requires restarting `make nested-session`. No compatibility shim or fallback protocol is planned. Keep the app and bridge contracts synchronized.

GNOME also exposes `ConfigureShortcuts(a(sas)) → ()`, an authoritative replacement list of action names and accelerator arrays. The app and bridge must agree on this contract. Restart `make nested-session` to stage the new `shortcuts.js` and updated bridge before shortcut acceptance. Schema keys and shortcut defaults are unchanged.

Milestone 5 includes bridge bug fixes, 16 MiB payload caps on both desktops, and byte-reading changes even where method signatures are unchanged. Restart `make nested-session` to stage the updated GNOME bridge before image acceptance.

## Implemented milestones 1–5

### 1. Pure core

The source domains are represented by Rust modules rather than redesigning the product. Clipboard entries normalize plain-text MIME types, compare by normalized MIME type plus bytes, and hash content for cache filenames. History ordering, duplicate handling, trimming that preserves favorites, selection, deletion, favorite toggling, clearing, and persistable filtering are pure operations. Search, shortcut parsing/matching, terminal-paste target selection, popup positioning modes, and secret-hint interpretation are also testable without GTK, Wayland, or D-Bus.

### 2. Settings and persistence

The application owns a new `org.waytoclip.WayToClip` GSettings schema. It preserves the source key names and defaults, including history, cache, clearing, popup, terminal, exclusion, search, favorite, popup-local shortcut, and language keys. There is no migration from the original extension or the POC.

History persists as a JSON array in `$XDG_CACHE_HOME/org.waytoclip.WayToClip/registry.txt`. Writes are asynchronous and coalesced: at most one write is in flight, the latest pending history wins, and an explicit quit flushes outstanding writes. Writes use a temporary file followed by an overwrite move. Reads ignore invalid records and trim non-favorites to `history-size`. Oversized cache files are moved aside as `registry.txt~`. `cache-only-favorites` persists only favorite history records; image payload files are still cached upon capture, matching the source behavior. No importer or migration is provided.

The clear-history scheduler persists `next-history-clear`, supports `clear-history-on-interval` and `clear-history-interval`, arms or immediately clears when overdue at startup, reacts to interval and enable changes, emits countdown ticks, and exposes a reset action. Bundled translations cover the popup and native dialogs, with a stored language override.

### 3. Continuous clipboard

The app runs a resident, event-driven recorder on GNOME and KDE Wayland. It watches clipboard ownership changes rather than polling, reads supported MIME types asynchronously, deduplicates by normalized MIME type and bytes, and keeps the newest entries. Capture continues while the popup is hidden. Own writes are suppressed at the bridge and by an in-app inhibit count. Private mode suppresses capture, invalidates pending reads, and restores the selected clipboard item when disabled. Optional secret MIME hints are filtered, and exact app ID or WM class matches in `excluded-apps` are skipped.

GNOME reports focused-app context at the clipboard event and an own-write marker, not a true clipboard-owner identity; the source also uses focus context. KDE's data-control protocol exposes no clipboard owner, so source-app attribution uses a best-effort snapshot of the focused app reported by KWin, and own-write suppression treats the next selection echo after publication within a short deadline as the app's own write. Attribution and correlation can miss or misclassify under races or unusual focus changes; neither desktop provides an atomic source-identity guarantee.

### 4. GTK popup

The GTK popup displays up to 10 entries per page with numbered slots and configurable quick selection. It supports paging (optionally capped by `limit-popup-pages`/`popup-pages`), substring search with optional case sensitivity, and ECMAScript regular expressions with fallback to substring matching when a pattern is invalid. Favorites have a dedicated view and toggle; entries can be deleted; footer hints reflect the configured local shortcuts. Styling follows theme colors, and layout measures against the bridge-provided workarea so the popup fits and scrolls rather than overflowing. Popup placement supports cursor and focused-window-center modes.

The controller prepares the popup offscreen, measures its natural width and height, calls `SettlePopup` for the chosen cursor side's available height, constrains the popup, and only then presents it. That stable cursor-side fit path has no known implementation gap now, though desktop behavior remains unverified.

#### Lifecycle and CLI wiring

Launch with no arguments boots history, connects the desktop backend, starts the resident recorder, and opens the popup. Re-invoking the same command toggles the popup; hiding it leaves capture running. Backend failures are reported and retried. The controller owns bridge, recorder, scheduler, popup, and auxiliary tasks and cancels them on quit/shutdown.

CLI actions are `--settings`, `--quit`, `--clear-history`, `--toggle-private-mode`, and `--reset-clear-timer`. Manual clearing obeys `confirm-clear`, is ignored in private mode, and may be requested before boot completes; scheduled interval clears call the clear path directly and are not gated by private mode. The old POC editor actions (**Paste**, **Copy only**, **Read clipboard**, **Quit** controls) are gone.

`clear-on-boot` is currently implemented as clearing the cache folder on every application start, not only after an OS reboot, despite the schema summary. This discrepancy must be resolved before describing it as boot-only behavior.

Panel, autostart, and distribution/packaging remain deferred. The GNOME/KDE bridge installers and desktop interaction remain user-run; agents verify builds only.

### 5. Live images

Live capture follows the source MIME priority: `text/plain;charset=utf-8`, `UTF8_STRING`, `text/plain`, `STRING`, GIF, PNG, JPG, JPEG, WebP, SVG, then HTML. Text therefore wins over an image in the same offer. `should-cache-images=false` skips new image capture, not just disk persistence. Both desktop backends cap clipboard payloads at 16 MiB. Image publication uses the stored MIME type and full original bytes, independently of preview decoding.

Previews decode asynchronously on a worker using native GdkPixbuf loaders, so supported formats depend on installed loaders. GIF previews are static. `image-preview-size` is clamped to 32–512 logical pixels and previews preserve aspect ratio. Pending, unsupported, or corrupt payloads render a generic image icon.

Captured image payloads are written asynchronously to checksum-named files in the cache directory, with PNG/JPEG/GIF/WebP/SVG extensions. Registry image records refer to those files and reload the original bytes on restart; missing files are skipped. Deletion and clearing clean up removed image payloads, respecting entries retained by history policy. Favorites-only persistence filters registry records but does not prevent capture-time image payload caching.

## Implemented Settings and global shortcuts

The app-owned, theme-aware GTK4 Settings window ports the original groups **General**, **Popup**, **Behavior**, **Limits**, **Exclusion**, **Global shortcuts**, and **Popup shortcuts**, preserving source key names and using bundled translations. It adds no libadwaita or other dependencies. Open it with `--settings` (append this to the printed wrapper in nested GNOME), or the popup's small gear. The controller cancels the popup transaction before presenting Settings as an ordinary application window, independent of popup placement. Changes apply live; reopen Settings after changing language to refresh labels. The `clear-on-boot` control accurately says **Clear history when WayToClip starts**.

The original three global actions are popup toggle (`toggle-popup`), private-mode toggle (`private-mode-binding`), and clear history (`clear-history`). Their default lists remain empty; the user chooses bindings such as Alt+X. `enable-keybindings` controls registration. The editor supports multiple shortcut chips, Add/remove, Reset, and registration/conflict status. Capture waits for backend confirmation that app global grabs are suspended before recording; completion or cancellation restores configured registrations.

GNOME uses Meta grabs restricted to `NORMAL | OVERVIEW`, excluding the lock screen. No settings daemon is required, so app-owned shortcuts can now be tested in nested GNOME. KDE uses native KGlobalAccel session D-Bus actions with stable component identity `org.waytoclip.WayToClip` and app-owned `_k_session:` IDs, registered nonpersistently; there is no KWin visual UI. Daemon owner changes trigger registration refresh. Forced app termination may leave KDE runtime grabs until app relaunch or daemon restart. Normal `--quit` releases registrations.

There is no autostart: the user must launch once per session, and shortcuts operate only while the app is resident.

## Milestone 6: user-run desktop acceptance

The user reported the basic milestones 1–5 nested GNOME popup/history/image flow working (“good it works”). This establishes limited current-app feedback, not comprehensive acceptance of persistence, image lifecycle/policy, geometry, or other desktop behavior. Current KDE acceptance requires a separate real KDE Wayland session. Settings/global shortcuts have no new desktop acceptance yet.

The next approved manual step is to restart `make nested-session`, run its printed wrapper with `--settings`, and use **Global shortcuts → Add** for the popup action to record **Alt+X** after capture is ready. Close Settings, focus an editor inside the nested session, open the popup with the hotkey, and select an entry to paste. Reopen and press the hotkey again to verify toggle dismissal. Edit/unassign bindings, Reset, and disable `enable-keybindings` to observe release; try a system-reserved key and check conflict status. Cancel capture and check restoration. Verify the gear cancels the popup before presenting an ordinary Settings window without popup placement, live settings changes, and labels after reopening on language change. KDE needs separate user-run checks in a real session. The broader image and desktop checklist remains in [README.md](README.md#manual-acceptance-checklist). Agents verify builds only; desktop acceptance is user-run.

## Historical interaction proofs

These records describe the removed one-payload POC, not the current implementation. Its **Paste**, **Copy only**, **Read clipboard**, **Quit**, and terminal-toggle controls no longer exist. They are separate from the limited current-app nested GNOME feedback recorded above.

### 1. GNOME GTK interaction proof — historical POC accepted

Baseline: GNOME Shell 50, locally installed 50.6. The POC used one in-memory editable text payload, Paste and Copy only, a terminal-paste toggle, explicit Read clipboard, Escape/focus-loss dismissal, and launch-to-toggle behavior. It did not include history capture, persistence, images, or private mode.

The user confirmed nested GNOME “fully works. pops up at cursor and pastes fine and all”. That establishes basic POC cursor placement and paste only, not exhaustive first-frame, monitor-edge, multi-monitor, or differing-scale coverage for the current app.

### 2. KDE Wayland interaction proof — historical POC accepted

The POC used the same Rust GTK4 UI with `ext_data_control_manager_v1` text access and `org_kde_kwin_fake_input`, plus a nonvisual KWin script for placement/focus. This did not implement history monitoring.

After the fake-input blocker was fixed, the user confirmed popup placement at the cursor and paste into the original target worked. That accepted basic POC interaction only. It does not verify the current recorders, popup features, or geometry across monitors and scales.

The old POC manual gate checked first-frame placement, multi-monitor and mixed-scale placement, repeated invocation, cancellation of outstanding clipboard reads, focus restoration, terminal paste, copy-only behavior, and recovery after focus changes or target closure. Those checks remain useful historical regression context but are not current acceptance.

## Deferred and open decisions

- Milestone 6: user-run desktop acceptance, next Settings/global shortcuts in refreshed nested GNOME and separate current-app KDE acceptance.
- Panel, autostart, and distribution/packaging.
- Minimum supported GNOME/KDE versions beyond the GNOME 50 and KDE 6.4+ targets.
- Packaging and sandbox constraints on clipboard access, placement, focus, and paste.
- Whether `clear-on-boot` should clear only on OS reboot or retain the current clear-on-app-start behavior.

No history/settings migration is planned. Earlier unanswered proposals are not accepted decisions.
