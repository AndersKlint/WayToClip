import QtQuick
import org.kde.kwin 3.0 as KWin

Item {
    id: bridge
    visible: false

    readonly property string instance: Date.now().toString(36) + "-" + Math.random().toString(36).slice(2)
    readonly property string applicationId: "org.waytoclip.WayToClip"
    property bool isStopping: false
    property var commandCall: null
    property var outgoingCalls: []
    property var sessionToken: null
    property var targetWindow: null
    property var popupWindow: null
    property var popupPid: null
    property var cursorPosition: null
    property var sessionDesktop: null
    property string sessionActivity: ""
    property bool isInvalidated: false
    property bool hasActivatedPopup: false
    property var restoreCommand: null

    Component {
        id: dbusCallComponent
        KWin.DBusCall {
            service: "org.waytoclip.WayToClip"
            path: "/org/waytoclip/WayToClip/Kde"
            dbusInterface: "org.waytoclip.WayToClip.Kde"
        }
    }

    Timer {
        id: reconnectTimer
        interval: 1000
        onTriggered: bridge.nextCommand()
    }

    Timer {
        id: commandWatchdog
        interval: 20000
        onTriggered: {
            if (bridge.commandCall) {
                bridge.commandCall.destroy()
                bridge.commandCall = null
            }
            bridge.invalidate("KDE command connection timed out")
            bridge.nextCommand()
        }
    }

    Timer {
        id: restoreDeadline
        interval: 1500
        onTriggered: bridge.finishRestore(false, "Original target did not regain focus after popup dismissal")
    }

    Timer {
        id: placementCorrection
        interval: 0
        onTriggered: bridge.placePopup()
    }

    Connections {
        target: KWin.Workspace
        function onWindowAdded(window) {
            bridge.considerPopup(window)
        }
        function onWindowRemoved(window) {
            if (window === bridge.popupWindow) {
                bridge.popupWindow = null
                bridge.hasActivatedPopup = false
                bridge.checkRestore()
            }
            if (window === bridge.targetWindow)
                bridge.invalidate("Original target window was removed")
        }
        function onWindowActivated(window) {
            if (bridge.sessionToken === null)
                return
            if (window && window !== bridge.targetWindow && window !== bridge.popupWindow) {
                if (bridge.matchesPopup(window))
                    bridge.considerPopup(window)
                else
                    bridge.invalidate("Focus moved to an unrelated window")
            }
            bridge.checkRestore()
        }
        function onCurrentDesktopChanged() {
            bridge.checkContext()
        }
        function onCurrentActivityChanged() {
            bridge.checkContext()
        }
        function onScreensChanged() {
            bridge.placePopup()
        }
    }

    Connections {
        target: bridge.popupWindow
        function onFrameGeometryChanged() {
            bridge.placePopup()
        }
        function onHiddenChanged() {
            if (bridge.popupWindow && !bridge.popupWindow.hidden)
                bridge.placePopup()
            bridge.checkRestore()
        }
    }

    function send(method, record) {
        var call = dbusCallComponent.createObject(bridge, {
            method: method,
            arguments: [instance, JSON.stringify(record)]
        })
        outgoingCalls.push(call)
        function release() {
            var index = outgoingCalls.indexOf(call)
            if (index !== -1)
                outgoingCalls.splice(index, 1)
            call.destroy()
        }
        call.finished.connect(release)
        call.failed.connect(release)
        call.call()
    }

    function nextCommand() {
        if (isStopping || commandCall)
            return
        var call = dbusCallComponent.createObject(bridge, {
            method: "NextCommand",
            arguments: [instance]
        })
        commandCall = call
        call.finished.connect(function(reply) {
            if (commandCall !== call)
                return
            commandWatchdog.stop()
            commandCall = null
            call.destroy()
            try {
                execute(JSON.parse(reply[0]))
            } catch (error) {
                invalidate("Invalid KDE command: " + error)
            }
            nextCommand()
        })
        call.failed.connect(function() {
            if (commandCall !== call)
                return
            commandWatchdog.stop()
            commandCall = null
            call.destroy()
            invalidate("KDE command connection failed")
            reconnectTimer.restart()
        })
        commandWatchdog.restart()
        call.call()
    }

    function result(command, ok, error, extra) {
        var record = {request: command.request, token: command.token, ok: ok}
        if (error)
            record.error = error
        if (extra) {
            record.app_id = extra.app_id
            record.is_terminal = extra.is_terminal
        }
        send("ReportResult", record)
    }

    function isUint32(value) {
        return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 4294967295
    }

    function execute(command) {
        if (command.operation === "idle")
            return
        if (!isUint32(command.request) || !isUint32(command.token))
            throw new Error("Missing request or token")
        try {
            if (command.operation === "begin") {
                begin(command)
                return
            }
            if (command.token !== sessionToken) {
                result(command, false, "Stale popup token")
                return
            }
            if (command.operation === "restore") {
                restore(command)
            } else if (command.operation === "cancel") {
                cancel(command)
            } else if (command.operation === "complete") {
                finishRestore(false, "Popup session completed")
                clearSession()
                result(command, true)
            } else {
                result(command, false, "Unknown KDE operation")
            }
        } catch (error) {
            result(command, false, String(error))
            invalidate("KWin operation failed: " + error)
        }
    }

    function begin(command) {
        if (!isUint32(command.pid) || command.pid === 0) {
            result(command, false, "Missing popup process ID")
            return
        }
        if (sessionToken !== null) {
            finishRestore(false, "Popup session was superseded by a new begin")
            clearSession()
        }
        if (typeof KWin.Workspace.screenAt !== "function" || typeof KWin.Workspace.clientArea !== "function" || !KWin.Workspace.windows || !KWin.Workspace.cursorPos || !KWin.Workspace.currentDesktop) {
            result(command, false, "Required KWin 6.4 workspace APIs are unavailable")
            return
        }
        var target = KWin.Workspace.activeWindow
        if (!target || target.deleted || target.pid === command.pid || !target.wantsInput) {
            result(command, false, "No eligible original target window")
            return
        }
        sessionToken = command.token
        popupPid = command.pid
        targetWindow = target
        cursorPosition = Qt.point(KWin.Workspace.cursorPos.x, KWin.Workspace.cursorPos.y)
        sessionDesktop = KWin.Workspace.currentDesktop
        sessionActivity = KWin.Workspace.currentActivity
        isInvalidated = false
        var appId = target.desktopFileName || target.resourceClass
        var terminalId = String(appId).toLowerCase().replace(/\.desktop$/, "")
        var isTerminal = ["org.kde.konsole", "konsole", "org.gnome.terminal", "org.gnome.ptyxis", "org.wezfurlong.wezterm", "alacritty", "kitty", "foot", "com.mitchellh.ghostty"].indexOf(terminalId) !== -1
        var windows = KWin.Workspace.windows
        for (var index = 0; index < windows.length; ++index)
            considerPopup(windows[index])
        if (sessionToken !== command.token) {
            result(command, false, "Popup session was invalidated during begin")
            return
        }
        result(command, true, "", {app_id: String(appId), is_terminal: isTerminal})
    }

    function matchesPopup(window) {
        return sessionToken !== null && window && window.pid === popupPid && window.captionNormal === "WayToClip" && (window.resourceClass === applicationId || window.desktopFileName === applicationId)
    }

    function considerPopup(window) {
        if (isInvalidated || !matchesPopup(window))
            return
        if (popupWindow !== window) {
            popupWindow = window
            hasActivatedPopup = false
        }
        placePopup()
        if (sessionToken !== null)
            placementCorrection.restart()
    }

    function checkContext() {
        if (sessionToken !== null && (KWin.Workspace.currentDesktop !== sessionDesktop || KWin.Workspace.currentActivity !== sessionActivity))
            invalidate("Desktop or activity changed during popup session")
    }

    function placePopup() {
        try {
            placePopupFrame()
        } catch (error) {
            invalidate("KWin popup placement failed: " + error)
        }
    }

    function placePopupFrame() {
        if (sessionToken === null || isInvalidated || !popupWindow || restoreCommand)
            return
        var output = KWin.Workspace.screenAt(cursorPosition)
        if (!output) {
            invalidate("Cursor output is no longer available")
            return
        }
        var area = KWin.Workspace.clientArea(KWin.Workspace.PlacementArea, output, sessionDesktop)
        if (!area || area.width <= 0 || area.height <= 0) {
            invalidate("KWin placement work area is unavailable")
            return
        }
        var geometry = popupWindow.frameGeometry
        if (!geometry || !Number.isFinite(geometry.width) || !Number.isFinite(geometry.height)) {
            invalidate("KWin popup frame geometry is unavailable")
            return
        }
        var x = Math.max(area.x, Math.min(cursorPosition.x, area.x + area.width - geometry.width))
        var y = Math.max(area.y, Math.min(cursorPosition.y, area.y + area.height - geometry.height))
        if (geometry.x !== x || geometry.y !== y)
            popupWindow.frameGeometry = Qt.rect(x, y, geometry.width, geometry.height)
        if (sessionToken === null || !popupWindow)
            return
        if (!hasActivatedPopup && !popupWindow.hidden && !popupWindow.minimized) {
            popupWindow.desktops = [sessionDesktop]
            if (sessionToken === null || !popupWindow)
                return
            popupWindow.activities = sessionActivity ? [sessionActivity] : []
            if (sessionToken === null || !popupWindow)
                return
            hasActivatedPopup = true
            KWin.Workspace.activeWindow = popupWindow
        }
    }

    function canRestore() {
        var active = KWin.Workspace.activeWindow
        return !isInvalidated && targetWindow && !targetWindow.deleted && (!active || active === targetWindow || active === popupWindow) && KWin.Workspace.currentDesktop === sessionDesktop && KWin.Workspace.currentActivity === sessionActivity
    }

    function restore(command) {
        if (restoreCommand) {
            result(command, false, "Target restoration is already pending")
            return
        }
        if (!canRestore()) {
            result(command, false, "Original target cannot safely be restored")
            return
        }
        restoreCommand = command
        placementCorrection.stop()
        restoreDeadline.restart()
        checkRestore()
    }

    function checkRestore() {
        if (!restoreCommand)
            return
        if (!canRestore()) {
            finishRestore(false, "Target restoration was invalidated")
            return
        }
        if (popupWindow && !popupWindow.hidden)
            return
        if (KWin.Workspace.activeWindow !== targetWindow)
            KWin.Workspace.activeWindow = targetWindow
        if (KWin.Workspace.activeWindow === targetWindow)
            finishRestore(true)
    }

    function finishRestore(ok, error) {
        if (!restoreCommand)
            return
        var command = restoreCommand
        restoreCommand = null
        restoreDeadline.stop()
        if (command.operation === "cancel")
            clearSession()
        result(command, ok, error)
    }

    function cancel(command) {
        if (typeof command.should_restore_focus !== "boolean") {
            result(command, false, "Missing focus restoration choice")
            return
        }
        if (restoreCommand)
            finishRestore(false, "Target restoration was cancelled")
        if (command.should_restore_focus && canRestore()) {
            restore(command)
        } else {
            clearSession()
            result(command, true)
        }
    }

    function invalidate(error) {
        if (sessionToken === null || isInvalidated)
            return
        var token = sessionToken
        isInvalidated = true
        placementCorrection.stop()
        send("ReportEvent", {token: token, event: "invalidated", error: error})
        finishRestore(false, error)
        clearSession()
    }

    function clearSession() {
        restoreDeadline.stop()
        placementCorrection.stop()
        restoreCommand = null
        sessionToken = null
        targetWindow = null
        popupWindow = null
        popupPid = null
        cursorPosition = null
        sessionDesktop = null
        sessionActivity = ""
        hasActivatedPopup = false
        isInvalidated = false
    }

    Component.onCompleted: nextCommand()
    Component.onDestruction: {
        isStopping = true
        reconnectTimer.stop()
        commandWatchdog.stop()
        invalidate("KWin bridge script unloaded")
        clearSession()
        if (commandCall)
            commandCall.destroy()
        for (var index = 0; index < outgoingCalls.length; ++index)
            outgoingCalls[index].destroy()
    }
}
