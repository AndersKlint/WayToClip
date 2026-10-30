import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {APP_ID, isApplicationWindow, isPopup, PopupPlacement} from './popup.js';
import {Shortcuts} from './shortcuts.js';

const BUS_NAME = 'org.gnome.Shell.Extensions.WayToClipBridge';
const OBJECT_PATH = '/org/gnome/Shell/Extensions/WayToClipBridge';
const XML = `<node>
<interface name="org.gnome.Shell.Extensions.WayToClipBridge">
  <method name="ConfigureShortcuts">
    <arg name="bindings" type="a(sas)" direction="in"/>
  </method>
  <method name="BeginPopup">
    <arg name="popup_position_mode" type="i" direction="in"/>
    <arg name="token" type="u" direction="out"/>
    <arg name="app_id" type="s" direction="out"/>
    <arg name="wm_class" type="s" direction="out"/>
    <arg name="is_terminal" type="b" direction="out"/>
    <arg name="has_target" type="b" direction="out"/>
    <arg name="available_width" type="i" direction="out"/>
    <arg name="available_height" type="i" direction="out"/>
  </method>
  <method name="SettlePopup">
    <arg name="token" type="u" direction="in"/>
    <arg name="width" type="i" direction="in"/>
    <arg name="height" type="i" direction="in"/>
    <arg name="available_height" type="i" direction="out"/>
  </method>
  <method name="FinishPopup">
    <arg name="token" type="u" direction="in"/>
    <arg name="mimetype" type="s" direction="in"/>
    <arg name="bytes" type="ay" direction="in"/>
    <arg name="should_paste" type="b" direction="in"/>
    <arg name="is_terminal" type="b" direction="in"/>
  </method>
  <method name="CancelPopup">
    <arg name="token" type="u" direction="in"/>
    <arg name="should_restore_focus" type="b" direction="in"/>
  </method>
  <method name="WatchClipboard">
    <arg name="after_serial" type="t" direction="in"/>
    <arg name="serial" type="t" direction="out"/>
    <arg name="mimetypes" type="as" direction="out"/>
    <arg name="app_id" type="s" direction="out"/>
    <arg name="wm_class" type="s" direction="out"/>
    <arg name="is_own" type="b" direction="out"/>
  </method>
  <method name="ReadClipboardContent">
    <arg name="serial" type="t" direction="in"/>
    <arg name="mimetype" type="s" direction="in"/>
    <arg name="bytes" type="ay" direction="out"/>
  </method>
  <method name="WriteClipboard">
    <arg name="mimetype" type="s" direction="in"/>
    <arg name="bytes" type="ay" direction="in"/>
  </method>
</interface>
</node>`;

const SECRET_HINT_BYTES = new Uint8Array([115, 101, 99, 114, 101, 116]);
const PRESENCE_SECRET_MIMETYPES = new Set([
    'application/x-nspasteboard-concealed-type',
]);
const WATCH_TIMEOUT_MS = 25000;
const CLIPBOARD_READ_TIMEOUT_MS = 3000;
const OWN_WRITE_TIMEOUT_MS = 2000;
const OWN_WRITE_READ_TIMEOUT_MS = 500;
const MAX_BYTES = 16 * 1024 * 1024;
const MAX_MIME_BYTES = 4096;

function isPresenceSecretMimetype(mimetype) {
    return PRESENCE_SECRET_MIMETYPES.has(String(mimetype).toLowerCase());
}

export class Bridge {
    enable() {
        this._owner = null;
        this._shortcuts = new Shortcuts();
        this._tokenSerial = 0;
        this._offer = {serial: 0, mimetypes: [], appId: '', wmClass: '', isOwn: false};
        this._initialDelivered = false;
        this._session = null;
        this._pending = new Set();
        this._sources = new Set();
        this._watches = new Set();
        this._reads = new Set();
        this._ownWrite = null;
        this._ownWriteSource = 0;
        this._pendingClassifications = new Set();
        this._isClassifyingOwnWrite = false;
        this._ownerEventGeneration = 0;
        this._clipboard = St.Clipboard.get_default();
        this._keyboard = Clutter.get_default_backend().get_default_seat()
            .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
        this._selection = Shell.Global.get().get_display().get_selection();
        this._offer = this._captureOffer(false, 0);
        this._selectionOwnerId = this._selection.connect('owner-changed',
            (_selection, selectionType) => {
                if (selectionType === Meta.SelectionType.SELECTION_CLIPBOARD)
                    this._onClipboardOwnerChanged();
            });
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
                this._shortcuts.clear();
                this._reset('Disconnected', 'Application owner changed');
                this._terminateRequests('Disconnected', 'Application owner changed');
                this._initialDelivered = false;
                this._owner = owner;
            }, () => {
                this._shortcuts.clear();
                this._owner = null;
                this._reset('Disconnected', 'Application disconnected');
                this._terminateRequests('Disconnected', 'Application disconnected');
                this._initialDelivered = false;
            });
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.NONE,
            null, () => {
                this._shortcuts.clear();
                this._reset('Unavailable', 'Bridge bus name lost');
                this._terminateRequests('Unavailable', 'Bridge bus name lost');
            });
    }

    disable() {
        this._shortcuts.destroy();
        this._reset('Disabled', 'Bridge disabled');
        this._terminateRequests('Disabled', 'Bridge disabled');
        Gio.bus_unwatch_name(this._watchId);
        Gio.bus_unown_name(this._nameId);
        this._exported.unexport();
        global.display.disconnect(this._focusId);
        if (this._selection && this._selectionOwnerId)
            this._selection.disconnect(this._selectionOwnerId);
        this._selectionOwnerId = 0;
        this._selection = null;
        this._clearOwnWriteTimeout();
        this._placement.disable();
        // Virtual input devices retain a Wayland resource until explicitly disposed.
        this._keyboard.run_dispose();
        this._keyboard = null;
        this._clipboard = null;
        this._owner = null;
    }

    ConfigureShortcutsAsync([bindings], invocation) {
        if (!this._authorize(invocation))
            return;
        try {
            this._shortcuts.configure(bindings, this._owner);
            invocation.return_value(new GLib.Variant('()', []));
        } catch (error) {
            this._error(invocation, 'ShortcutConfigurationFailed', error.message);
        }
    }

    BeginPopupAsync([popupPositionMode], invocation) {
        if (!this._authorize(invocation))
            return;
        if (popupPositionMode !== 0 && popupPositionMode !== 1) {
            this._error(invocation, 'InvalidMode', 'Unsupported popup position mode');
            return;
        }
        const previous = this._session;
        const focus = global.display.focus_window;
        const candidate = isPopup(focus) ? previous?.target : focus;
        this._reset('Superseded', 'A new popup session started');
        const hasTarget = !!candidate && !isApplicationWindow(candidate);
        if (this._tokenSerial === 0xffffffff) {
            this._error(invocation, 'TokenExhausted', 'Re-enable the bridge to start a new token sequence');
            return;
        }
        const token = ++this._tokenSerial;
        const isTerminal = isPopup(focus) ? !!previous?.isTerminal :
            Main.inputMethod.content_purpose === Clutter.InputContentPurpose.TERMINAL;
        const identity = this._windowIdentity(hasTarget ? candidate : null);
        const isAnchoredInCenter = popupPositionMode === 1 && hasTarget;
        let point;
        if (isAnchoredInCenter) {
            const rect = candidate.get_frame_rect();
            point = [rect.x + rect.width / 2, rect.y + rect.height / 2];
        } else {
            point = global.get_pointer().slice(0, 2);
        }
        this._session = {token, target: hasTarget ? candidate : null, isTerminal};
        this._targetId = 0;
        if (hasTarget) {
            this._targetId = candidate.connect('unmanaged',
                () => this._reset('TargetClosed', 'Target window closed'));
        }
        this._placement.begin(point, isAnchoredInCenter);
        const available = this._placement.availableSize();
        invocation.return_value(new GLib.Variant('(ussbbii)', [
            token,
            identity.appId,
            identity.wmClass,
            isTerminal,
            hasTarget,
            available.width,
            available.height,
        ]));
    }

    SettlePopupAsync([token, width, height], invocation) {
        if (!this._validate(token, invocation))
            return;
        width = Number(width);
        height = Number(height);
        if (!Number.isInteger(width) || width <= 0 ||
            !Number.isInteger(height) || height <= 0) {
            this._reset('InvalidSize', 'Popup size must be positive');
            this._error(invocation, 'InvalidSize', 'Popup width and height must be positive integers');
            return;
        }
        const availableHeight = this._placement.settle(width, height);
        if (availableHeight === null) {
            this._reset('InvalidPlacement', 'Popup placement is not active');
            this._error(invocation, 'InvalidPlacement', 'Popup placement is not active');
            return;
        }
        invocation.return_value(new GLib.Variant('(i)', [availableHeight]));
    }

    FinishPopupAsync([token, mimetype, bytes, shouldPaste, isTerminal], invocation) {
        if (!this._validate(token, invocation))
            return;
        if (this._pending.size > 0) {
            this._error(invocation, 'Busy', 'Another request is still pending');
            return;
        }
        const session = this._session;
        if (shouldPaste && !session.target) {
            this._error(invocation, 'NoTarget',
                'No focused target window is available for paste; use copy only');
            return;
        }
        this._placement.cancel();
        this._pending.add(invocation);
        try {
            this._setClipboardContent(mimetype, bytes);
        } catch (error) {
            this._pending.delete(invocation);
            this._error(invocation, 'ClipboardWriteFailed', error.message);
            return;
        }
        if (!session.target) {
            this._pending.delete(invocation);
            invocation.return_value(new GLib.Variant('()', []));
            this._reset('Completed', 'Popup completed');
            return;
        }
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
        if (shouldRestoreFocus && target && (!focus || isPopup(focus) || focus === target))
            target.activate(global.get_current_time());
        invocation.return_value(new GLib.Variant('()', []));
    }

    WatchClipboardAsync([afterSerial], invocation) {
        if (!this._authorize(invocation))
            return;
        afterSerial = Number(afterSerial);
        if (!this._isClassifyingOwnWrite &&
            (this._shouldReturnInitial(afterSerial) || this._offer.serial > afterSerial)) {
            if (this._offer.serial === 0)
                this._initialDelivered = true;
            invocation.return_value(this._offerVariant(this._offer));
            return;
        }
        const waiter = {invocation, afterSerial, source: 0};
        this._watches.add(waiter);
        const source = GLib.timeout_add(GLib.PRIORITY_DEFAULT, WATCH_TIMEOUT_MS, () => {
            waiter.source = 0;
            if (this._watches.delete(waiter))
                this._error(invocation, 'WatchTimeout',
                    'WatchTimeout: no clipboard owner change before the deadline');
            return GLib.SOURCE_REMOVE;
        });
        waiter.source = source;
    }

    ReadClipboardContentAsync([serial, mimetype], invocation) {
        if (!this._authorize(invocation))
            return;
        serial = Number(serial);
        if (!this._isSerialCurrent(serial)) {
            this._error(invocation, 'StaleSerial', 'Clipboard owner changed before the read');
            return;
        }
        const offered = this._offeredMimetype(mimetype);
        if (!offered) {
            this._error(invocation, 'UnsupportedMime', 'Clipboard does not offer the requested MIME type');
            return;
        }
        const request = {invocation, serial, mimetype, source: 0};
        this._reads.add(request);
        const source = GLib.timeout_add(GLib.PRIORITY_DEFAULT, CLIPBOARD_READ_TIMEOUT_MS, () => {
            request.source = 0;
            if (this._reads.delete(request))
                this._error(invocation, 'ClipboardTimeout', 'Clipboard owner did not return content before the deadline');
            return GLib.SOURCE_REMOVE;
        });
        request.source = source;
        try {
            this._clipboard.get_content(St.ClipboardType.CLIPBOARD, offered, (_clipboard, bytes) => {
                if (request.source) {
                    GLib.Source.remove(request.source);
                    request.source = 0;
                }
                if (!this._reads.delete(request))
                    return;
                if (!this._isSerialCurrent(serial)) {
                    this._error(invocation, 'StaleSerial', 'Clipboard owner changed during the read');
                    return;
                }
                if (!this._selectionOffersMimetype(mimetype)) {
                    this._error(invocation, 'StaleSerial', 'Clipboard no longer offers the requested MIME type');
                    return;
                }
                let data;
                try {
                    data = this._asBytes(bytes);
                } catch (error) {
                    this._error(invocation, 'ClipboardTooLarge', error.message);
                    return;
                }
                if (isPresenceSecretMimetype(mimetype) && data.length === 0)
                    data = SECRET_HINT_BYTES.slice();
                invocation.return_value(new GLib.Variant('(ay)', [data]));
            });
        } catch (_error) {
            if (request.source) {
                GLib.Source.remove(request.source);
                request.source = 0;
            }
            if (this._reads.delete(request))
                this._error(invocation, 'ClipboardReadFailed', 'Clipboard read could not be started');
        }
    }

    WriteClipboardAsync([mimetype, bytes], invocation) {
        if (!this._authorize(invocation))
            return;
        try {
            this._setClipboardContent(mimetype, bytes);
            invocation.return_value(new GLib.Variant('()', []));
        } catch (error) {
            this._error(invocation, 'ClipboardWriteFailed', error.message);
        }
    }

    _onClipboardOwnerChanged() {
        const generation = ++this._ownerEventGeneration;
        this._cancelPendingClassifications();
        const offer = this._captureOffer(false, this._offer.serial + 1);
        this._offer = offer;
        if (!this._ownWrite) {
            this._isClassifyingOwnWrite = false;
            this._deliverWatches();
            return;
        }
        this._isClassifyingOwnWrite = true;
        // Owner changes carry no owner identity, so confirm the published payload first.
        Promise.resolve().then(() => {
            if (generation !== this._ownerEventGeneration || !this._clipboard)
                return false;
            return this._matchesOwnWrite(offer.mimetypes);
        }).then(isOwn => {
            if (generation !== this._ownerEventGeneration)
                return;
            offer.isOwn = isOwn;
            this._isClassifyingOwnWrite = false;
            this._deliverWatches();
        }, () => {
            if (generation !== this._ownerEventGeneration)
                return;
            this._isClassifyingOwnWrite = false;
            this._deliverWatches();
        });
    }

    _matchesOwnWrite(mimetypes) {
        return new Promise(resolve => {
            let own = this._ownWrite;
            if (!own) {
                resolve(false);
                return;
            }
            const normalized = own.mimetype.toLowerCase();
            const offered = Array.from(mimetypes).find(mimetype =>
                String(mimetype).toLowerCase() === normalized);
            if (!offered) {
                resolve(false);
                return;
            }
            const classification = {source: 0, isDone: false, resolve, bytes: own.bytes};
            own = null;
            const finish = value => {
                if (classification.isDone)
                    return;
                classification.isDone = true;
                classification.bytes = null;
                if (classification.source) {
                    GLib.Source.remove(classification.source);
                    classification.source = 0;
                }
                this._pendingClassifications.delete(classification);
                resolve(value);
            };
            this._pendingClassifications.add(classification);
            classification.source = GLib.timeout_add(GLib.PRIORITY_DEFAULT,
                OWN_WRITE_READ_TIMEOUT_MS, () => {
                    classification.source = 0;
                    finish(false);
                    return GLib.SOURCE_REMOVE;
                });
            try {
                this._clipboard.get_content(St.ClipboardType.CLIPBOARD, offered,
                    (_clipboard, value) => {
                        if (classification.isDone)
                            return;
                        try {
                            const bytes = this._boundedBytes(value);
                            finish(bytes.equal(classification.bytes));
                        } catch (_error) {
                            finish(false);
                        }
                    });
            } catch (_error) {
                finish(false);
            }
        });
    }

    _cancelPendingClassifications() {
        for (const classification of [...this._pendingClassifications]) {
            classification.isDone = true;
            classification.bytes = null;
            if (classification.source) {
                GLib.Source.remove(classification.source);
                classification.source = 0;
            }
            classification.resolve(false);
        }
        this._pendingClassifications.clear();
    }

    _deliverWatches() {
        if (this._isClassifyingOwnWrite)
            return;
        for (const waiter of [...this._watches]) {
            if (!this._shouldReturnInitial(waiter.afterSerial) &&
                this._offer.serial <= waiter.afterSerial)
                continue;
            if (waiter.source) {
                GLib.Source.remove(waiter.source);
                waiter.source = 0;
            }
            this._watches.delete(waiter);
            if (this._offer.serial === 0)
                this._initialDelivered = true;
            waiter.invocation.return_value(this._offerVariant(this._offer));
        }
    }

    _shouldReturnInitial(afterSerial) {
        return afterSerial === 0 && this._offer.serial === 0 && !this._initialDelivered;
    }

    _isSerialCurrent(serial) {
        return serial === this._offer.serial;
    }

    _captureOffer(isOwn, serial) {
        const focus = global.display.focus_window;
        const ownerWindow = isPopup(focus) ? this._session?.target ?? null : focus;
        const identity = this._windowIdentity(ownerWindow);
        const mimetypes = this._selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD) ?? [];
        return {
            serial,
            mimetypes: Array.from(mimetypes),
            appId: identity.appId,
            wmClass: identity.wmClass,
            isOwn,
        };
    }

    _windowIdentity(window) {
        if (!window)
            return {appId: '', wmClass: ''};
        const wmClass = window.get_wm_class() ?? window.get_gtk_application_id() ?? '';
        const app = Shell.WindowTracker.get_default().get_window_app(window);
        const appId = app?.get_id() ?? wmClass;
        return {appId, wmClass};
    }

    _selectionOffersMimetype(mimetype) {
        return this._offeredMimetype(mimetype) !== undefined;
    }

    _offeredMimetype(mimetype) {
        const mimetypes = this._selection.get_mimetypes(Meta.SelectionType.SELECTION_CLIPBOARD) ?? [];
        const normalized = String(mimetype).toLowerCase();
        return Array.from(mimetypes).find(candidate =>
            String(candidate).toLowerCase() === normalized);
    }

    _offerVariant(offer) {
        return new GLib.Variant('(tasssb)', [
            offer.serial,
            offer.mimetypes,
            offer.appId,
            offer.wmClass,
            offer.isOwn,
        ]);
    }

    _asBytes(value) {
        if (value && typeof value.get_size === 'function') {
            if (value.get_size() > MAX_BYTES)
                throw new Error('Clipboard payload exceeds 16 MiB');
            return value.get_data();
        }
        if ((value?.length ?? 0) > MAX_BYTES)
            throw new Error('Clipboard payload exceeds 16 MiB');
        if (value instanceof Uint8Array)
            return value;
        return new Uint8Array(value ?? []);
    }

    _boundedBytes(value) {
        if (value instanceof GLib.Bytes) {
            if (value.get_size() > MAX_BYTES)
                throw new Error('Clipboard payload exceeds 16 MiB');
            return value;
        }
        return new GLib.Bytes(this._asBytes(value));
    }

    _setClipboardContent(mimetype, bytes) {
        if (!mimetype || mimetype.length > MAX_MIME_BYTES || mimetype.includes('\0'))
            throw new Error('Invalid clipboard MIME type');
        const data = this._boundedBytes(bytes);
        this._markOwnWrite(mimetype, data);
        try {
            this._clipboard.set_content(St.ClipboardType.CLIPBOARD, mimetype, data);
        } catch (error) {
            this._clearOwnWriteTimeout();
            this._ownWrite = null;
            throw error;
        }
    }

    _markOwnWrite(mimetype, bytes) {
        this._ownWrite = {mimetype, bytes};
        this._clearOwnWriteTimeout();
        this._ownWriteSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, OWN_WRITE_TIMEOUT_MS, () => {
            this._ownWriteSource = 0;
            this._ownWrite = null;
            return GLib.SOURCE_REMOVE;
        });
    }

    _clearOwnWriteTimeout() {
        if (!this._ownWriteSource)
            return;
        GLib.Source.remove(this._ownWriteSource);
        this._ownWriteSource = 0;
    }

    _terminateRequests(code, message) {
        this._ownerEventGeneration++;
        this._cancelPendingClassifications();
        this._isClassifyingOwnWrite = false;
        this._clearOwnWriteTimeout();
        this._ownWrite = null;
        for (const waiter of this._watches) {
            if (waiter.source)
                GLib.Source.remove(waiter.source);
            this._error(waiter.invocation, code, message);
        }
        this._watches.clear();
        for (const request of this._reads) {
            if (request.source)
                GLib.Source.remove(request.source);
            this._error(request.invocation, code, message);
        }
        this._reads.clear();
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
        if (this._session && this._targetId)
            this._session.target.disconnect(this._targetId);
        this._targetId = 0;
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
