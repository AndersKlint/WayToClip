import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {APP_ID, isPopup, PopupPlacement} from './popup.js';

const BUS_NAME = 'org.gnome.Shell.Extensions.WayToClipBridge';
const OBJECT_PATH = '/org/gnome/Shell/Extensions/WayToClipBridge';
const XML = `<node>
<interface name="org.gnome.Shell.Extensions.WayToClipBridge">
  <method name="BeginPopup">
    <arg name="token" type="u" direction="out"/>
    <arg name="target_app_id" type="s" direction="out"/>
    <arg name="is_terminal" type="b" direction="out"/>
  </method>
  <method name="FinishPopup">
    <arg name="token" type="u" direction="in"/>
    <arg name="text" type="s" direction="in"/>
    <arg name="should_paste" type="b" direction="in"/>
    <arg name="is_terminal" type="b" direction="in"/>
  </method>
  <method name="CancelPopup">
    <arg name="token" type="u" direction="in"/>
    <arg name="should_restore_focus" type="b" direction="in"/>
  </method>
  <method name="ReadClipboard">
    <arg name="text" type="s" direction="out"/>
  </method>
</interface>
</node>`;

export class Bridge {
    enable() {
        this._owner = null;
        this._serial = 0;
        this._session = null;
        this._pending = new Set();
        this._sources = new Set();
        this._clipboard = St.Clipboard.get_default();
        this._keyboard = Clutter.get_default_backend().get_default_seat()
            .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        this._placement = new PopupPlacement();
        this._placement.enable();
        this._focusId = global.display.connect('notify::focus-window', () => {
            const focus = global.display.focus_window;
            if (this._session && focus && focus !== this._session.target && !isPopup(focus))
                this._reset('FocusChanged', 'Focus moved to another window');
        });
        this._exported = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._exported.export(Gio.DBus.session, OBJECT_PATH);
        this._watchId = Gio.bus_watch_name(Gio.BusType.SESSION, APP_ID, Gio.BusNameWatcherFlags.NONE,
            (_connection, _name, owner) => {
                this._reset('Disconnected', 'Application owner changed');
                this._owner = owner;
            }, () => {
                this._owner = null;
                this._reset('Disconnected', 'Application disconnected');
            });
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.NONE,
            null, () => this._reset('Unavailable', 'Bridge bus name lost'));
    }

    disable() {
        this._reset('Disabled', 'Bridge disabled');
        Gio.bus_unwatch_name(this._watchId);
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
        global.display.disconnect(this._focusId);
        this._placement.disable();
        // Virtual input devices retain a Wayland resource until explicitly disposed.
        this._keyboard.run_dispose();
        this._keyboard = null;
        this._clipboard = null;
        this._owner = null;
    }

    BeginPopupAsync(_parameters, invocation) {
        if (!this._authorize(invocation))
            return;
        const previous = this._session;
        const focus = global.display.focus_window;
        const target = isPopup(focus) ? previous?.target : focus;
        this._reset('Superseded', 'A new popup session started');
        if (!target || isPopup(target)) {
            this._error(invocation, 'NoTarget', 'No focused target window');
            return;
        }
        if (this._serial === 0xffffffff) {
            this._error(invocation, 'TokenExhausted', 'Re-enable the bridge to start a new token sequence');
            return;
        }
        const token = ++this._serial;
        const isTerminal = isPopup(focus) ? previous.isTerminal :
            Main.inputMethod.content_purpose === Clutter.InputContentPurpose.TERMINAL;
        const targetId = Shell.WindowTracker.get_default().get_window_app(target)?.get_id() ?? '';
        this._session = {token, target, isTerminal};
        this._targetId = target.connect('unmanaged', () => this._reset('TargetClosed', 'Target window closed'));
        this._placement.begin(global.get_pointer().slice(0, 2));
        invocation.return_value(new GLib.Variant('(usb)', [token, targetId, isTerminal]));
    }

    FinishPopupAsync([token, text, shouldPaste, isTerminal], invocation) {
        if (!this._validate(token, invocation))
            return;
        if (this._pending.size > 0) {
            this._error(invocation, 'Busy', 'Another request is still pending');
            return;
        }
        const session = this._session;
        this._placement.cancel();
        this._pending.add(invocation);
        this._clipboard.set_text(St.ClipboardType.CLIPBOARD, text);
        session.target.activate(global.get_current_time());
        const deadline = GLib.get_monotonic_time() + 1500000;
        const earliest = GLib.get_monotonic_time() + 100000;
        const source = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 20, () => {
            const focus = global.display.focus_window;
            const modifiers = global.get_pointer()[2];
            const held = Clutter.ModifierType.SHIFT_MASK | Clutter.ModifierType.CONTROL_MASK |
                Clutter.ModifierType.MOD1_MASK | Clutter.ModifierType.SUPER_MASK |
                Clutter.ModifierType.META_MASK | Clutter.ModifierType.HYPER_MASK | Clutter.ModifierType.MOD5_MASK;
            if (focus === session.target && GLib.get_monotonic_time() >= earliest && (!shouldPaste || !(modifiers & held))) {
                this._sources.delete(source);
                if (shouldPaste)
                    this._paste(isTerminal);
                this._pending.delete(invocation);
                invocation.return_value(new GLib.Variant('()', []));
                this._reset('Completed', 'Popup completed');
                return GLib.SOURCE_REMOVE;
            }
            if (GLib.get_monotonic_time() >= deadline) {
                this._sources.delete(source);
                this._reset('FocusTimeout', 'Target focus or modifier release was not confirmed before the deadline');
                return GLib.SOURCE_REMOVE;
            }
            return GLib.SOURCE_CONTINUE;
        });
        this._sources.add(source);
    }

    CancelPopupAsync([token, shouldRestoreFocus], invocation) {
        if (!this._validate(token, invocation))
            return;
        const target = this._session.target;
        const focus = global.display.focus_window;
        this._reset('Cancelled', 'Popup cancelled');
        if (shouldRestoreFocus && (!focus || isPopup(focus) || focus === target))
            target.activate(global.get_current_time());
        invocation.return_value(new GLib.Variant('()', []));
    }

    ReadClipboardAsync(_parameters, invocation) {
        if (!this._authorize(invocation))
            return;
        this._pending.add(invocation);
        const source = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 3000, () => {
            this._sources.delete(source);
            if (this._pending.delete(invocation))
                this._error(invocation, 'ClipboardTimeout', 'Clipboard owner did not return text before the deadline');
            return GLib.SOURCE_REMOVE;
        });
        this._sources.add(source);
        this._clipboard.get_text(St.ClipboardType.CLIPBOARD, (_clipboard, text) => {
            if (!this._pending.delete(invocation))
                return;
            GLib.Source.remove(source);
            this._sources.delete(source);
            invocation.return_value(new GLib.Variant('(s)', [text ?? '']));
        });
    }

    _authorize(invocation) {
        if (this._owner && invocation.get_sender() === this._owner)
            return true;
        this._error(invocation, 'AccessDenied', 'Caller must own org.waytoclip.WayToClip');
        return false;
    }

    _validate(token, invocation) {
        if (!this._authorize(invocation))
            return false;
        if (this._session?.token === token)
            return true;
        this._error(invocation, 'StaleToken', 'Popup session is no longer active');
        return false;
    }

    _reset(code, message) {
        for (const source of this._sources)
            GLib.Source.remove(source);
        this._sources.clear();
        for (const invocation of this._pending)
            this._error(invocation, code, message);
        this._pending.clear();
        if (this._session)
            this._session.target.disconnect(this._targetId);
        this._session = null;
        this._placement.cancel();
    }

    _error(invocation, code, message) {
        invocation.return_dbus_error(`${BUS_NAME}.Error.${code}`, message);
    }

    _paste(isTerminal) {
        const keys = [Clutter.KEY_Control_L];
        if (isTerminal)
            keys.push(Clutter.KEY_Shift_L);
        keys.push(Clutter.KEY_v);
        for (const key of keys)
            this._keyboard.notify_keyval(Clutter.get_current_event_time() * 1000, key, Clutter.KeyState.PRESSED);
        for (const key of keys.reverse())
            this._keyboard.notify_keyval(Clutter.get_current_event_time() * 1000, key, Clutter.KeyState.RELEASED);
    }
}
