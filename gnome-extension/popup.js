import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

export const APP_ID = 'org.waytoclip.WayToClip';

const MARGIN = 10;
const GAP_BELOW = 20;
const GAP_ABOVE = 10;

export function isApplicationWindow(window) {
    return window !== null &&
        (window.get_wm_class() === APP_ID || window.get_gtk_application_id() === APP_ID);
}

export function isPopup(window) {
    return isApplicationWindow(window) && window.get_title() === 'WayToClip';
}

export class PopupPlacement {
    enable() {
        this._windows = new Map();
        this._laters = global.compositor.get_laters();
        this._layoutIds = new Map();
        this._hasDiagnostics = GLib.getenv('WAYTOCLIP_PLACEMENT_DEBUG') === '1';
        this._point = null;
        this._monitor = null;
        this._area = null;
        this._anchor = null;
        this._isAnchoredInCenter = false;
        this._createdId = global.display.connect('window-created', (_display, window) => this._track(window));
        this._mapId = global.window_manager.connect('map', (_wm, actor) => {
            const window = actor.meta_window;
            if (isPopup(window) && this._point) {
                this._place(window, true);
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
    }

    begin(point, isAnchoredInCenter = false) {
        this.cancel();
        this._point = point;
        this._isAnchoredInCenter = !!isAnchoredInCenter;
        this._monitor = this._monitorForPoint(point);
        this._area = this._workAreaForMonitor(this._monitor);
        for (const window of this._windows.keys())
            this._prepare(window);
    }

    availableSize() {
        if (!this._area || !this._point)
            return {width: 0, height: 0};
        if (this._isAnchoredInCenter) {
            return {
                width: Math.max(0, this._area.width - MARGIN * 2),
                height: Math.max(0, this._area.height - MARGIN * 2),
            };
        }
        const relY = this._point[1] - this._area.y;
        const below = this._area.height - relY - GAP_BELOW - MARGIN;
        const above = relY - GAP_ABOVE - MARGIN;
        return {
            width: Math.max(0, this._area.width - MARGIN * 2),
            height: Math.max(0, Math.max(above, below)),
        };
    }

    settle(width, height) {
        if (!this._area || !this._point || width <= 0 || height <= 0)
            return null;
        const area = this._area;
        if (this._isAnchoredInCenter) {
            this._anchor = null;
            return Math.max(0, Math.floor(area.height - MARGIN * 2));
        }
        const relX = this._point[0] - area.x;
        const relY = this._point[1] - area.y;
        const side = this.decidePopupSide(height, this._point[1], area);
        this._anchor = {
            x: area.x + Math.max(MARGIN,
                Math.min(relX, area.width - width - MARGIN)),
            side,
            belowTopY: relY + GAP_BELOW,
            aboveBottomY: relY - GAP_ABOVE,
        };
        const available = side === 'above' ?
            this._anchor.aboveBottomY - MARGIN :
            area.height - this._anchor.belowTopY - MARGIN;
        return Math.max(0, Math.floor(available));
    }

    cancel() {
        this._point = null;
        this._monitor = null;
        this._area = null;
        this._anchor = null;
        this._isAnchoredInCenter = false;
        for (const id of this._layoutIds.values())
            this._laters.remove(id);
        this._layoutIds.clear();
    }

    decidePopupSide(natH, y, area) {
        const relY = y - area.y;
        const spaceBelow = area.height - relY - GAP_BELOW - MARGIN;
        const spaceAbove = relY - GAP_ABOVE - MARGIN;
        if (natH <= spaceBelow)
            return 'below';
        if (natH <= spaceAbove)
            return 'above';
        return spaceAbove > spaceBelow ? 'above' : 'below';
    }

    _monitorForPoint(point) {
        const [x, y] = point;
        const monitor = Main.layoutManager.monitors.find(output =>
            x >= output.x && x < output.x + output.width && y >= output.y && y < output.y + output.height);
        return monitor ?? Main.layoutManager.primaryMonitor ?? null;
    }

    _workAreaForMonitor(monitor) {
        if (!monitor)
            return null;
        return global.workspace_manager.get_active_workspace().get_work_area_for_monitor(monitor.index);
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
        this._place(window, window.mapped);
    }

    _schedulePlacement(window) {
        if (this._layoutIds.has(window))
            return;
        // Mutter's show-time placement can replace pre-map coordinates before redraw.
        const id = this._laters.add(Meta.LaterType.BEFORE_REDRAW, () => {
            this._layoutIds.delete(window);
            if (!this._point || !window.mapped)
                return GLib.SOURCE_REMOVE;
            const desired = isPopup(window) ? this._place(window, true) : null;
            if (this._hasDiagnostics) {
                const frame = window.get_frame_rect();
                console.log(`WayToClip placement ${JSON.stringify({
                    appId: window.get_wm_class(),
                    gtkAppId: window.get_gtk_application_id(),
                    title: window.get_title(),
                    pointer: this._point,
                    mode: this._isAnchoredInCenter ? 'center' : this._anchor?.side ?? 'unsettled',
                    available: this.availableSize(),
                    desired,
                    actual: [frame.x, frame.y, frame.width, frame.height],
                })}`);
            }
            return GLib.SOURCE_REMOVE;
        });
        this._layoutIds.set(window, id);
    }

    _place(window, shouldSettle = false) {
        if (!this._point || !this._area || !this._monitor)
            return null;
        const area = this._area;
        const frame = window.get_frame_rect();
        if (window.get_monitor() !== this._monitor.index)
            window.move_to_monitor(this._monitor.index);
        const relX = this._point[0] - area.x;
        const relY = this._point[1] - area.y;
        let targetX;
        let targetY;

        if (this._isAnchoredInCenter) {
            targetX = area.x + Math.max(MARGIN,
                Math.min(relX - frame.width / 2, area.width - frame.width - MARGIN));
            targetY = area.y + Math.max(MARGIN,
                Math.min(relY - frame.height / 2, area.height - frame.height - MARGIN));
        } else {
            if (!this._anchor && shouldSettle) {
                const side = this.decidePopupSide(frame.height, this._point[1], area);
                this._anchor = {
                    x: area.x + Math.max(MARGIN,
                        Math.min(relX, area.width - frame.width - MARGIN)),
                    side,
                    belowTopY: relY + GAP_BELOW,
                    aboveBottomY: relY - GAP_ABOVE,
                };
            }
            const anchor = this._anchor;
            if (anchor) {
                targetX = Math.max(area.x + MARGIN,
                    Math.min(anchor.x, area.x + area.width - frame.width - MARGIN));
                targetY = anchor.side === 'above' ?
                    area.y + anchor.aboveBottomY - frame.height :
                    area.y + anchor.belowTopY;
            } else {
                const side = this.decidePopupSide(frame.height, this._point[1], area);
                targetX = area.x + Math.max(MARGIN,
                    Math.min(relX, area.width - frame.width - MARGIN));
                targetY = side === 'above' ?
                    area.y + relY - GAP_ABOVE - frame.height :
                    area.y + relY + GAP_BELOW;
            }
            targetY = Math.max(area.y, Math.min(targetY, area.y + area.height - frame.height));
        }

        if (frame.x !== targetX || frame.y !== targetY)
            window.move_frame(false, targetX, targetY);
        return [targetX, targetY];
    }
}
