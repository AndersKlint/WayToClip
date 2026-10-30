import QtQuick
import org.kde.kwin 3.0 as KWin

Item {
    id: bridge
    visible: false

    readonly property string instance: Date.now().toString(36) + "-" + Math.random().toString(36).slice(2)
    readonly property string applicationId: "org.waytoclip.WayToClip"
    readonly property int margin: 10
    readonly property int gapBelow: 20
    readonly property int gapAbove: 10

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

    property int positionMode: 0
    property var placementArea: null
    property real anchorRelX: 0
    property real anchorRelY: 0
    property real belowTopY: 0
    property real aboveBottomY: 0
    property real centerX: 0
    property real centerY: 0
    property bool isCentered: false
    property bool isDirectionSettled: false
    property string placementDirection: "below"
    property bool hasPlacement: false
    property bool hasLockedX: false
    property real lockedXRel: 0

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
        onTriggered: {
            if (bridge.popupWindow && !bridge.popupWindow.hidden)
                bridge.placePopup()
        }
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
            if (window && window !== bridge.popupWindow)
                bridge.reportActiveApp(window)
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
            if (bridge.popupWindow && !bridge.popupWindow.hidden) {
                bridge.placePopup()
                placementCorrection.restart()
            }
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
            record.wm_class = extra.wm_class
            record.is_terminal = extra.is_terminal
            record.has_target = extra.has_target
            record.available_width = extra.available_width
            record.available_height = extra.available_height
        }
        send("ReportResult", record)
    }

    function isUint32(value) {
        return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 4294967295
    }

    function isPositiveSize(value) {
        return typeof value === "number" && Number.isInteger(value) && value > 0 && value <= 2147483647
    }

    function normalizeId(value) {
        return String(value || "").trim().toLowerCase().replace(/\.desktop$/, "")
    }

    function reportActiveApp(window) {
        if (!window || window.deleted || window === popupWindow)
            return
        var appId = normalizeId(window.desktopFileName || window.resourceClass || "")
        var wmClass = normalizeId(window.resourceClass || "")
        if (appId === "" && wmClass === "")
            return
        send("ReportEvent", {
            event: "active_app",
            app_id: appId,
            wm_class: wmClass
        })
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
            if (command.operation === "settle") {
                settle(command)
            } else if (command.operation === "restore") {
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

    function eligibleTarget(pid) {
        var target = KWin.Workspace.activeWindow
        if (!target || target.deleted || !target.wantsInput)
            return null
        var ownApplicationId = normalizeId(applicationId)
        var isOwnApplication = target.pid === pid
            || normalizeId(target.resourceClass) === ownApplicationId
            || normalizeId(target.desktopFileName) === ownApplicationId
        if (isOwnApplication)
            return null
        return target
    }

    function capturePlacement(target, mode) {
        var cursor = KWin.Workspace.cursorPos
        if (!cursor)
            return false
        var hasTarget = target !== null && target !== undefined
        var output = null
        if (mode === 1 && hasTarget) {
            var frame = target.frameGeometry
            if (frame && Number.isFinite(frame.x) && Number.isFinite(frame.y)
                    && Number.isFinite(frame.width) && Number.isFinite(frame.height))
                output = KWin.Workspace.screenAt(Qt.point(frame.x + frame.width / 2, frame.y + frame.height / 2))
        }
        if (!output)
            output = KWin.Workspace.screenAt(cursor)
        if (!output)
            return false
        var area = KWin.Workspace.clientArea(KWin.Workspace.PlacementArea, output, sessionDesktop)
        if (!area || area.width <= 0 || area.height <= 0)
            return false
        var anchorX = cursor.x
        var anchorY = cursor.y
        if (mode === 1 && hasTarget) {
            var geometry = target.frameGeometry
            if (geometry && Number.isFinite(geometry.x) && Number.isFinite(geometry.y)
                    && Number.isFinite(geometry.width) && Number.isFinite(geometry.height)) {
                anchorX = geometry.x + geometry.width / 2
                anchorY = geometry.y + geometry.height / 2
            }
        }
        placementArea = area
        anchorRelX = anchorX - area.x
        anchorRelY = anchorY - area.y
        belowTopY = anchorRelY + gapBelow
        aboveBottomY = anchorRelY - gapAbove
        centerX = anchorRelX
        centerY = anchorRelY
        isCentered = mode === 1 && hasTarget
        isDirectionSettled = isCentered
        placementDirection = "below"
        hasPlacement = true
        hasLockedX = false
        lockedXRel = anchorRelX
        return true
    }

    function begin(command) {
        if (!isUint32(command.pid) || command.pid === 0) {
            result(command, false, "Missing popup process ID")
            return
        }
        if (!isUint32(command.popup_position_mode) || command.popup_position_mode > 1) {
            result(command, false, "Invalid popup position mode")
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
        var target = eligibleTarget(command.pid)
        sessionToken = command.token
        popupPid = command.pid
        targetWindow = target
        positionMode = command.popup_position_mode
        cursorPosition = Qt.point(KWin.Workspace.cursorPos.x, KWin.Workspace.cursorPos.y)
        sessionDesktop = KWin.Workspace.currentDesktop
        sessionActivity = KWin.Workspace.currentActivity
        isInvalidated = false
        if (!capturePlacement(target, positionMode)) {
            result(command, false, "KWin popup placement work area is unavailable")
            clearSession()
            return
        }
        if (target)
            reportActiveApp(target)
        var windows = KWin.Workspace.windows
        for (var index = 0; index < windows.length; ++index)
            considerPopup(windows[index])
        if (sessionToken !== command.token) {
            result(command, false, "Popup session was invalidated during begin")
            return
        }
        var availableWidth = Math.max(0, Math.round(placementArea.width) - 2 * margin)
        var availableHeight = Math.max(0, Math.round(isCentered
            ? placementArea.height - 2 * margin
            : Math.max(spaceAbove(placementArea), spaceBelow(placementArea))))
        result(command, true, "", {
            app_id: target ? normalizeId(target.desktopFileName || target.resourceClass || "") : "",
            wm_class: target ? normalizeId(target.resourceClass || "") : "",
            is_terminal: false,
            has_target: target !== null,
            available_width: availableWidth,
            available_height: availableHeight
        })
    }

    function settle(command) {
        if (!hasPlacement || !placementArea) {
            result(command, false, "Popup placement is unavailable")
            return
        }
        if (!isPositiveSize(command.width) || !isPositiveSize(command.height)) {
            result(command, false, "Invalid popup size")
            return
        }
        var width = command.width
        var height = command.height
        var availableHeight
        if (isCentered) {
            availableHeight = placementArea.height - 2 * margin
        } else {
            var below = spaceBelow(placementArea)
            var above = spaceAbove(placementArea)
            if (height <= below) {
                placementDirection = "below"
                availableHeight = below
            } else if (height <= above) {
                placementDirection = "above"
                availableHeight = above
            } else if (above > below) {
                placementDirection = "above"
                availableHeight = above
            } else {
                placementDirection = "below"
                availableHeight = below
            }
            isDirectionSettled = true
            lockedXRel = clamp(anchorRelX, margin, placementArea.width - width - margin)
            hasLockedX = true
        }
        result(command, true, "", {
            available_height: Math.max(0, Math.round(availableHeight))
        })
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
        if (sessionToken !== null && !window.hidden)
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

    function clamp(value, minimum, maximum) {
        if (maximum < minimum)
            return minimum
        return Math.max(minimum, Math.min(value, maximum))
    }

    function spaceBelow(area) {
        return area.height - anchorRelY - gapBelow - margin
    }

    function spaceAbove(area) {
        return anchorRelY - gapAbove - margin
    }

    function decidePopupSide(height, area) {
        var below = spaceBelow(area)
        var above = spaceAbove(area)
        if (height <= below)
            return "below"
        if (height <= above)
            return "above"
        return above > below ? "above" : "below"
    }

    function provisionalPosition(width, height, area) {
        if (isCentered)
            return [
                clamp(centerX - width / 2, margin, area.width - width - margin),
                clamp(centerY - height / 2, margin, area.height - height - margin)
            ]
        var direction = decidePopupSide(height, area)
        var x = clamp(anchorRelX, margin, area.width - width - margin)
        var y = direction === "above" ? aboveBottomY - height : belowTopY
        return [x, clamp(y, margin, area.height - height - margin)]
    }

    function placePopupFrame() {
        if (sessionToken === null || isInvalidated || !popupWindow || restoreCommand || !hasPlacement)
            return
        var area = placementArea
        if (!area) {
            invalidate("KWin placement work area is no longer available")
            return
        }
        var geometry = popupWindow.frameGeometry
        if (!geometry || !Number.isFinite(geometry.width) || !Number.isFinite(geometry.height) || geometry.width <= 0 || geometry.height <= 0) {
            invalidate("KWin popup frame geometry is unavailable")
            return
        }
        var width = geometry.width
        var height = geometry.height
        var x
        var y
        if (!isCentered && !isDirectionSettled) {
            var provisional = provisionalPosition(width, height, area)
            x = provisional[0]
            y = provisional[1]
        } else if (isCentered) {
            x = clamp(centerX - width / 2, margin, area.width - width - margin)
            y = clamp(centerY - height / 2, margin, area.height - height - margin)
        } else {
            if (!hasLockedX) {
                lockedXRel = clamp(anchorRelX, margin, area.width - width - margin)
                hasLockedX = true
            }
            x = clamp(lockedXRel, margin, area.width - width - margin)
            if (placementDirection === "above")
                y = aboveBottomY - height
            else
                y = belowTopY
            y = clamp(y, margin, area.height - height - margin)
        }
        x = area.x + Math.round(x)
        y = area.y + Math.round(y)
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
        if (isInvalidated || KWin.Workspace.currentDesktop !== sessionDesktop || KWin.Workspace.currentActivity !== sessionActivity)
            return false
        if (!targetWindow)
            return true
        return !targetWindow.deleted && (!active || active === targetWindow || active === popupWindow)
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
        if (!targetWindow) {
            finishRestore(true)
            return
        }
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
        positionMode = 0
        placementArea = null
        anchorRelX = 0
        anchorRelY = 0
        belowTopY = 0
        aboveBottomY = 0
        centerX = 0
        centerY = 0
        isCentered = false
        isDirectionSettled = false
        placementDirection = "below"
        hasPlacement = false
        hasLockedX = false
        lockedXRel = 0
    }

    Component.onCompleted: {
        reportActiveApp(KWin.Workspace.activeWindow)
        nextCommand()
    }
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
