import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const ACTIONS = new Set(['toggle-popup', 'clear-history', 'toggle-private-mode']);
const ACTION_MODES = Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW;

export class Shortcuts {
    constructor() {
        this._bindings = new Map();
        this._calls = new Set();
        this._owner = null;
        this._signalId = global.display.connect('accelerator-activated',
            (_display, action) => this._activate(action));
    }

    configure(bindings, owner) {
        this.clear();
        const actions = new Set();
        const accelerators = new Set();
        try {
            for (const [action, shortcuts] of bindings) {
                if (!ACTIONS.has(action))
                    throw new Error(`Unsupported shortcut action: ${action}`);
                if (actions.has(action))
                    throw new Error(`Duplicate shortcut action: ${action}`);
                actions.add(action);
                for (const accelerator of shortcuts) {
                    if (!accelerator || accelerators.has(accelerator))
                        throw new Error(`Invalid or duplicate shortcut for ${action}: ${accelerator}`);
                    accelerators.add(accelerator);
                    let id;
                    try {
                        id = global.display.grab_accelerator(accelerator, Meta.KeyBindingFlags.NONE);
                    } catch (error) {
                        throw new Error(`Cannot register ${action} (${accelerator}): ${error.message}`);
                    }
                    if (id === Meta.KeyBindingAction.NONE)
                        throw new Error(`Shortcut unavailable for ${action}: ${accelerator}`);
                    this._bindings.set(id, action);
                    Main.wm.allowKeybinding(Meta.external_binding_name_for_action(id), ACTION_MODES);
                }
            }
            this._owner = owner;
        } catch (error) {
            this.clear();
            throw error;
        }
    }

    clear() {
        this._owner = null;
        for (const cancellable of this._calls)
            cancellable.cancel();
        this._calls.clear();
        for (const id of this._bindings.keys()) {
            Main.wm.allowKeybinding(Meta.external_binding_name_for_action(id), Shell.ActionMode.NONE);
            global.display.ungrab_accelerator(id);
        }
        this._bindings.clear();
    }

    destroy() {
        this.clear();
        global.display.disconnect(this._signalId);
    }

    _activate(id) {
        const action = this._bindings.get(id);
        if (!action || !this._owner || !(Main.actionMode & ACTION_MODES))
            return;
        const cancellable = new Gio.Cancellable();
        this._calls.add(cancellable);
        Gio.DBus.session.call(this._owner, '/org/waytoclip/WayToClip',
            'org.gtk.Actions', 'Activate',
            new GLib.Variant('(sava{sv})', [action, [], {}]),
            new GLib.VariantType('()'), Gio.DBusCallFlags.NO_AUTO_START,
            3000, cancellable, (connection, result) => {
                this._calls.delete(cancellable);
                try {
                    connection.call_finish(result);
                } catch (error) {
                    if (!cancellable.is_cancelled())
                        console.error(`WayToClip shortcut action ${action} failed: ${error.message}`);
                }
            });
    }
}
