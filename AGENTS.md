# Working on WayToClip

Faithfully port `/home/anders/git/way-to-clip` to Rust, preserving its domains and behavior rather than redesigning the product. Read `PORTING.md` before choosing the next milestone or changing process/backend boundaries.

- Preserve clipboard, history, cursorPopup, paste, panel, settings, and common responsibilities. Use small leaf modules and a small controller/entrypoint for wiring.
- Keep pure domain functions independent of GTK, Wayland, and D-Bus. Normalize platform details at the boundary.
- Rust owns the full GTK4 popup. Keep the GNOME bridge narrow: privileged clipboard access, window placement/focus, and paste operations. Add dependencies sparingly and trait seams only for actual backends.
- Use native ownership and `Drop`, weak GTK signal captures where strong captures would form cycles, and explicit shutdown/cancellation for outstanding work. Keep blocking I/O off the UI thread and use asynchronous boundary operations.
- Return `Result` at fallible boundaries. Avoid broad panic suppression and redundant defensive guards around guaranteed APIs.
- Use descriptive names. Boolean names start with `is`, `has`, `should`, or `can`, except idiomatic trait APIs.
- Code comments are single-line technical explanations of non-obvious WHY only. No WHAT comments, docstrings, or summaries.
- Run `cargo fmt --check` and `cargo build` for Rust changes, fixing formatting with `cargo fmt` as needed. Do not add, modify, or run unit tests unless explicitly requested.
- Agents verify builds only. Desktop interaction acceptance belongs to the user. Do not install extensions or modify the running Shell or installed extensions on the user's behalf.
