# Porting plan

## Direction and boundaries

Port the behavior of `/home/anders/git/way-to-clip` faithfully to Rust. Its source domains are clipboard, history, cursorPopup, paste, panel, settings, and common, connected by a small controller and entrypoint. Preserve those responsibilities while using native Rust ownership and GTK4 APIs.

The Rust application owns the entire GTK4 popup. The GNOME Shell bridge supplies only privileged clipboard operations, placement/focus, and paste operations. On KDE, a native Rust Wayland worker owns text clipboard access and fake-input; a nonvisual declarative KWin script owns placement/focus. Keep these narrow desktop boundaries, not a generic plugin framework or parallel Qt UI. Shell code must not import GTK; UI work belongs in the app. Bridge enable/disable must own and release its signals, sources, and exported services.

Shared identity:

- Application: `org.waytoclip.WayToClip`
- GNOME bridge bus name/interface: `org.gnome.Shell.Extensions.WayToClipBridge`
- GNOME bridge object path: `/org/gnome/Shell/Extensions/WayToClipBridge`
- KDE endpoint on the application bus name: interface `org.waytoclip.WayToClip.Kde`, fixed object path `/org/waytoclip/WayToClip/Kde`
- KWin package ID: `waytoclip-bridge`

The bridge XML in the implementation is authoritative for method signatures. GNOME operations are `BeginPopup` (session token, target app ID, terminal hint), `FinishPopup` (token, text, paste choice, terminal choice), `CancelPopup` (token, focus-restoration choice), and explicit `ReadClipboard`. KDE's `NextCommand`, `ReportResult`, and `ReportEvent` carry token-scoped placement/focus commands and results; only the current `org.kde.KWin` owner may call the endpoint. Clipboard text stays in the native worker, not the KWin command transport. Keep the app and bridge contracts synchronized.

## 1. GNOME GTK interaction proof — complete, basic interaction accepted

Baseline: GNOME Shell 50, locally installed 50.6. One in-memory editable text payload, Paste and Copy only actions, terminal-paste toggle, explicit Read clipboard, and Escape/focus-loss dismissal. Launch toggles the resident app: hidden opens; visible or reading cancels/dismisses, matching the original extension. Quit or `waytoclip --quit` shuts it down. No history capture, persistence, images, or private mode in this slice.

Agents perform build verification only. The user performs desktop acceptance, preferably with `make nested-session` staging and enabling the bridge in a disposable isolated session; normal-session installation is optional. No agent modifies installed extensions or the running Shell.

The user confirms nested GNOME “fully works. pops up at cursor and pastes fine and all”. Basic cursor placement and paste are therefore user-confirmed. This is not an exhaustive record of first-frame, monitor-edge, multi-monitor, or differing-scale tests; retain the detailed gate below for acceptance and regression checks.

### Manual go/no-go gate

Basic interaction is user-accepted for both proofs. Retain these detailed checks for further acceptance and regression coverage:

- The first visible frame is placed at the cursor without an intermediate misplaced frame or flicker.
- Placement works across monitors, monitor edges, and differing display scales.
- Repeated invocation toggles the same resident app: hidden opens; visible or reading cancels/dismisses safely, including cancellation of an outstanding clipboard read.
- Paste returns focus to the intended target and inserts the edited payload; terminal paste uses the terminal-appropriate shortcut and honors the toggle.
- Copy only updates the clipboard without injecting a paste.
- Explicit clipboard read loads text only when requested.
- Escape and external focus loss dismiss safely, without unintended paste or focus theft; stale/cancelled sessions cannot finish later.
- Recover after unrelated focus changes or closure of the original target; quit and restart the app and repeat the interaction.

Record failures and resolve feasibility blockers before expanding the feature set. A successful build alone does not pass this gate.

## 2. KDE Wayland interaction proof — complete, basic interaction accepted

Target Plasma/KWin 6.4+ Wayland with the same Rust GTK4 UI. The implemented native worker uses `ext_data_control_manager_v1` for explicit UTF-8 text read/publication and `org_kde_kwin_fake_input` v4+ for paste; blocking transfers stay off the UI thread. The declarative nonvisual KWin script captures the original target and cursor, places/activates the popup, restores focus, and reports invalidation. Rust owns cancellation and worker shutdown. No root helper, `wl-paste`, Qt UI duplication, or Qt runtime dependency in Rust is required. This slice does not implement history monitoring.

The backend selects KDE from `XDG_CURRENT_DESKTOP` with `XDG_SESSION_TYPE=wayland`. Test in a real KDE session, not a GNOME session with spoofed environment variables. Plasma/KWin 6.4+ requires a desktop entry matching the actual executable to declare `X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input` without a trailing semicolon, because KService parses `X-KDE-*` properties as comma-separated lists. The user-run `tools/install-kde.sh` installs/upgrades the package, writes that entry with the canonical binary path, and refreshes the executable match cache using `kbuildsycoca6 --noincremental`. The user enables the script in System Settings and supplies their own shortcut; installation does not start the app or enable scripts automatically. See [README.md](README.md#kde-wayland-proof-user-installation).

The `xkbcommon` 0.9 crate adds a libxkbcommon development-package requirement. GTK >= 4.20 and GLib/GIO >= 2.80 remain build constraints even on distros providing KWin 6.4+. Paste rejects keymaps without consistent `v`, Control, and Shift keycodes across all configured layouts. Before hiding the popup, Rust performs a bounded wait while focused for Control, Shift, Alt, Super, Meta, and Hyper to clear in the GTK main connection's keyboard modifier state; locks are ignored. Latched focus loss or missing keyboard state aborts before clipboard publication and cancels without focus restoration. The worker still cannot establish the target's live modifier state. Focus restoration and injection across separate connections retain a non-atomic race; no atomic target-safety guarantee is established.

After the fake-input blocker was fixed, the user confirmed popup placement at the cursor and paste into the original target worked, accepting the KDE proof. This establishes basic interaction, not exhaustive coverage. Retain the manual gate for first/subsequent placement, monitor edges, multiple monitors, differing scales, and recovery after unrelated focus, target closure, and app restart. A build is not desktop acceptance.

## 3. Behavioral port — current

Port pure history and clipboard logic first, preserving the source's filtering and ordering behavior. Then port persistence/settings, GTK popup and preferences feature parity, and finally lifecycle/distribution work. Consult source behavior as each domain is ported; the original README's feature list is context, not an approved exhaustive scope or acceptance specification.

Keep domain logic free of GTK, Wayland, and D-Bus dependencies. Introduce platform seams when a real backend needs them. Preserve cleanup ownership, explicit cancellation, and nonblocking UI behavior throughout.

## Open decisions

- Minimum supported GNOME/KDE versions beyond the GNOME 50 proof.
- Exact feature scope and acceptance details for each later milestone.
- Whether history/settings migration is wanted and what compatibility it would require. No importer or backwards-compatibility implementation is planned by default.
- Exact popup behavior when external focus cancels an editing context, including retention of unfinished edits and focus restoration.
- Packaging and sandbox constraints on clipboard access, placement, focus, and paste.

Resolve these with the user when they affect the next milestone. Earlier unanswered proposals are not accepted decisions.
