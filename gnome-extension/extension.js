import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Bridge} from './bridge.js';

export default class WayToClipBridgeExtension extends Extension {
    enable() {
        this._bridge = new Bridge();
        this._bridge.enable();
    }

    disable() {
        this._bridge.disable();
        this._bridge = null;
    }
}
