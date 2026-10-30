import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

export const APP_ID = 'org.waytoclip.WayToClip';

export function isPopup(window) {
    return window !== null &&
        (window.get_wm_class() === APP_ID || window.get_gtk_application_id() === APP_ID) &&
        window.get_title() === 'WayToClip';
}

export class PopupPlacement {
    enable() {
        this._windows = new Map();
        this._laters = global.compositor.get_laters();
        this._layoutIds = new Map();
        this._hasDiagnostics = GLib.getenv('WAYTOCLIP_PLACEMENT_DEBUG') === '1';
        this._point = null;
        this._createdId = global.display.connect('window-created', (_display, window) => this._track(window));
        this._mapId = global.window_manager.connect('map', (_wm, actor) => {
            const window = actor.meta_window;
            if (isPopup(window) && this._point) {
                this._place(window);
                window.activate(global.get_current_time());
            }
            if (this._point && (isPopup(window) || window.get_title() === 'WayToClip'))
                this._schedulePlacement(window);
        });
        for (const actor of global.get_window_actors())
            this._track(actor.meta_window);
    }

    disable() {
        this.cancel();
        global.display.disconnect(this._createdId);
        global.window_manager.disconnect(this._mapId);
        for (const [window, signals] of this._windows) {
            for (const signal of signals)
                window.disconnect(signal);
        }
        this._windows.clear();
        this._point = null;
    }

    begin(point) {
        this.cancel();
        this._point = point;
        for (const window of this._windows.keys())
            this._prepare(window);
    }

    cancel() {
        this._point = null;
        for (const id of this._layoutIds.values())
            this._laters.remove(id);
        this._layoutIds.clear();
    }

    _track(window) {
        if (this._windows.has(window))
            return;
        const signals = [];
        this._windows.set(window, signals);
        for (const signal of ['notify::gtk-application-id', 'notify::wm-class', 'notify::title', 'size-changed']) {
            signals.push(window.connect(signal, () => this._prepare(window)));
        }
        // MetaWindow mapped notification precedes meta_window_show() and the Shell map effect.
        signals.push(window.connect('notify::mapped', () => {
            if (window.mapped)
                this._prepare(window, true);
        }));
        signals.push(window.connect('unmanaged', () => {
            const layoutId = this._layoutIds.get(window);
            if (layoutId !== undefined) {
                this._laters.remove(layoutId);
                this._layoutIds.delete(window);
            }
            for (const signal of signals)
                window.disconnect(signal);
            this._windows.delete(window);
        }));
        this._prepare(window);
    }

    _prepare(window, shouldSkipEffect = !window.mapped) {
        if (!isPopup(window) || !this._point)
            return;
        const actor = window.get_compositor_private();
        if (actor && shouldSkipEffect)
            Main.wm.skipNextEffect(actor);
        this._place(window);
    }

    _schedulePlacement(window) {
        if (this._layoutIds.has(window))
            return;
        // Mutter's show-time placement can replace pre-map coordinates before redraw.
        const id = this._laters.add(Meta.LaterType.BEFORE_REDRAW, () => {
            this._layoutIds.delete(window);
            if (!this._point || !window.mapped)
                return GLib.SOURCE_REMOVE;
            const desired = isPopup(window) ? this._place(window) : null;
            if (this._hasDiagnostics) {
                const frame = window.get_frame_rect();
                console.log(`WayToClip placement ${JSON.stringify({
                    appId: window.get_wm_class(),
                    gtkAppId: window.get_gtk_application_id(),
                    title: window.get_title(),
                    pointer: this._point,
                    desired,
                    actual: [frame.x, frame.y, frame.width, frame.height],
                })}`);
            }
            return GLib.SOURCE_REMOVE;
        });
        this._layoutIds.set(window, id);
    }

    _place(window) {
        const [x, y] = this._point;
        const monitor = Main.layoutManager.monitors.find(output =>
            x >= output.x && x < output.x + output.width && y >= output.y && y < output.y + output.height);
        if (!monitor)
            return null;
        const area = global.workspace_manager.get_active_workspace().get_work_area_for_monitor(monitor.index);
        if (window.get_monitor() !== monitor.index)
            window.move_to_monitor(monitor.index);
        const frame = window.get_frame_rect();
        const targetX = Math.max(area.x, Math.min(x, area.x + area.width - frame.width));
        const targetY = Math.max(area.y, Math.min(y, area.y + area.height - frame.height));
        if (frame.x !== targetX || frame.y !== targetY)
            window.move_frame(false, targetX, targetY);
        return [targetX, targetY];
    }
}
