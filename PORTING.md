# Porting plan

## Direction and boundaries

Port the behavior of `/home/anders/git/way-to-clip` faithfully to Rust. Its source domains are clipboard, history, cursorPopup, paste, panel, settings, and common, connected by a small controller and entrypoint. Preserve those responsibilities while using native Rust ownership and GTK4 APIs.

The Rust application owns the entire GTK4 popup. The GNOME Shell bridge supplies only privileged clipboard operations, placement/focus, and paste operations. Keep this a narrow desktop boundary, not a generic plugin framework. Shell code must not import GTK; UI work belongs in the app. Bridge enable/disable must own and release its signals, sources, and exported services.

Shared identity:

- Application: `org.waytoclip.WayToClip`
- Bridge bus name/interface: `org.gnome.Shell.Extensions.WayToClipBridge`
- Bridge object path: `/org/gnome/Shell/Extensions/WayToClipBridge`

The bridge XML in the implementation is authoritative for method signatures. The agreed operations are `BeginPopup` (session token, target app ID, terminal hint), `FinishPopup` (token, text, paste choice, terminal choice), `CancelPopup` (token, focus-restoration choice), and explicit `ReadClipboard`. Keep the app and bridge contracts synchronized.

## 1. GNOME GTK interaction proof — current

Baseline: GNOME Shell 50, locally installed 50.6. One in-memory editable text payload, Paste and Copy only actions, terminal-paste toggle, explicit Read clipboard, and Escape/focus-loss dismissal. Launch toggles the resident app: hidden opens; visible or reading cancels/dismisses, matching the original extension. Quit or `waytoclip --quit` shuts it down. No history capture, persistence, images, or private mode in this slice.

Agents perform build verification only. The user performs desktop acceptance, preferably with `make nested-session` staging and enabling the bridge in a disposable isolated session; normal-session installation is optional. No agent modifies installed extensions or the running Shell.

The user confirms that the popup opens in nested GNOME and reports that “other operations worked”; cursor placement fails, with the popup at the top-left. This does not establish that every detailed acceptance item below passed. The bridge now recognizes the Wayland app ID as well as GTK application metadata and reapplies placement once before redraw after mapping. Corrected placement, including the first visible frame, awaits a new manual retry.

### Manual go/no-go gate

Proceed only after the user verifies:

- The first visible frame is placed at the cursor without an intermediate misplaced frame or flicker.
- Placement works across monitors, monitor edges, and differing display scales.
- Repeated invocation toggles the same resident app: hidden opens; visible or reading cancels/dismisses safely, including cancellation of an outstanding clipboard read.
- Paste returns focus to the intended target and inserts the edited payload; terminal paste uses the terminal-appropriate shortcut and honors the toggle.
- Copy only updates the clipboard without injecting a paste.
- Explicit clipboard read loads text only when requested.
- Escape and external focus loss dismiss safely, without unintended paste or focus theft; stale/cancelled sessions cannot finish later.

Record failures and resolve feasibility blockers before expanding the feature set. A successful build alone does not pass this gate.

## 2. KDE Wayland interaction proof

After the GNOME gate, prove clipboard access through a KDE bridge or direct data-control path, popup placement/focus, and paste into the original target. Reuse the Rust GTK4 UI; do not create a parallel Qt UI. Validate the availability and access requirements of any private APIs or protocols before committing to that implementation. Apply the same manual interaction gate.

## 3. Behavioral port

Port pure history and clipboard logic first, preserving the source's filtering and ordering behavior. Then port persistence/settings, GTK popup and preferences feature parity, and finally lifecycle/distribution work. Consult source behavior as each domain is ported; the original README's feature list is context, not an approved exhaustive scope or acceptance specification.

Keep domain logic free of GTK, Wayland, and D-Bus dependencies. Introduce platform seams when a real backend needs them. Preserve cleanup ownership, explicit cancellation, and nonblocking UI behavior throughout.

## Open decisions

- Minimum supported GNOME/KDE versions beyond the GNOME 50 proof.
- Exact feature scope and acceptance details for each later milestone.
- Whether history/settings migration is wanted and what compatibility it would require. No importer or backwards-compatibility implementation is planned by default.
- Exact popup behavior when external focus cancels an editing context, including retention of unfinished edits and focus restoration.
- Packaging and sandbox constraints on clipboard access, placement, focus, and paste.

Resolve these with the user when they affect the next milestone. Earlier unanswered proposals are not accepted decisions.
