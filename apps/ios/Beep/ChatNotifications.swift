import Intents
import SwiftUI
import UserNotifications

/// Shows the server's notices as communication notifications: one thread per chat
/// or group, the sender's avatar, and Reply and Mark as read in place.
final class ChatNotifier: NSObject, NoticeListener, UNUserNotificationCenterDelegate, @unchecked Sendable {
    /// Set once by the model that owns this.
    @MainActor weak var model: AppModel?

    /// One line of a notification: who said it, nil for Beep.
    struct Line {
        var sender: String?
        var text: String
    }

    private enum Key {
        static let chat = "chat"
        static let approval = "approval"
        static let reply = "reply"
        static let read = "read"
        static let allow = "allow"
        static let deny = "deny"
    }

    override init() {
        super.init()
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        let reply = UNTextInputNotificationAction(
            identifier: Key.reply, title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Message")
        let read = UNNotificationAction(identifier: Key.read, title: "Mark as read")
        let allow = UNNotificationAction(identifier: Key.allow, title: "Allow", options: [.authenticationRequired])
        let deny = UNNotificationAction(identifier: Key.deny, title: "Deny", options: [.destructive])
        center.setNotificationCategories([
            UNNotificationCategory(identifier: Key.chat, actions: [reply, read], intentIdentifiers: []),
            UNNotificationCategory(identifier: Key.approval, actions: [allow, deny], intentIdentifiers: []),
        ])
    }

    func requestPermission() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }

    // MARK: Notices

    /// Called off the main thread by the bridge. Returning true takes the notice off the server's queue.
    func onNotice(notice: Notice) -> Bool {
        Task { @MainActor in self.handle(notice) }
        return true
    }

    @MainActor private func handle(_ notice: Notice) {
        guard let model else { return }
        if notice.changed || notice.title.isEmpty {
            model.noticeChanged(notice)
            return
        }
        if notice.group { model.noticeChanged(notice) }
        let active = UIApplication.shared.applicationState == .active
        if let approval = notice.approval {
            // On screen, the card in the chat asks instead. A group's always asks here.
            if !notice.group, active, model.onScreen == notice.sessionId { return }
            post(threadId: notice.sessionId, title: notice.title, lines: [Line(sender: nil, text: notice.body)],
                 group: notice.group, approval: approval)
            return
        }
        if active, model.onScreen == notice.sessionId { return }
        if notice.app != nil {
            post(threadId: "apps", title: notice.title, lines: [Line(sender: nil, text: notice.body)], group: false)
            return
        }
        // No chat, app or approval: a read of mail or a calendar finished.
        if notice.sessionId.isEmpty, !notice.group {
            post(threadId: "read", title: notice.title, lines: [Line(sender: nil, text: notice.body)], group: false)
            return
        }
        post(threadId: notice.sessionId, title: notice.title, lines: [Self.line(notice.body, group: notice.group)],
             group: notice.group)
    }

    /// A group notice's body is "Name: text".
    private static func line(_ body: String, group: Bool) -> Line {
        guard group, let colon = body.range(of: ": ") else { return Line(sender: nil, text: body) }
        let name = String(body[..<colon.lowerBound])
        return Line(sender: name == "Beep" ? nil : name, text: String(body[colon.upperBound...]))
    }

    /// A chat's reply the app produced itself, such as one finished in the background.
    @MainActor func show(chatId: String, title: String, lines: [Line]) {
        post(threadId: chatId, title: title, lines: lines, group: false)
    }

    @MainActor private func post(threadId: String, title: String, lines: [Line], group: Bool,
                                 approval: PendingApproval? = nil) {
        guard let last = lines.last else { return }
        let content = UNMutableNotificationContent()
        content.title = title
        // A chat with Beep shows only its latest reply; a group keeps who said what.
        let shown = group ? lines : [last]
        content.body = shown.map { line in
            shown.count > 1 ? "\(line.sender ?? "Beep"): \(line.text)" : line.text
        }.joined(separator: "\n")
        content.threadIdentifier = threadId
        content.sound = .default
        content.categoryIdentifier = approval == nil ? Key.chat : Key.approval
        var info: [String: Any] = ["thread": threadId, "group": group]
        if let approval {
            info["query"] = approval.queryId
            content.body = "\(approval.approval.action) with \(approval.approval.app)?"
        }
        content.userInfo = info

        let sender = last.sender == "You" ? nil : last.sender
        let request: UNNotificationRequest
        if let message = try? content.updating(from: intent(threadId: threadId, title: title, sender: sender,
                                                            text: content.body, group: group)) {
            request = UNNotificationRequest(identifier: UUID().uuidString, content: message, trigger: nil)
        } else {
            request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        }
        UNUserNotificationCenter.current().add(request)
    }

    /// The message as a donated INSendMessageIntent, so iOS shows the sender's avatar
    /// and the chat's name, and groups it with the rest of the conversation.
    @MainActor private func intent(threadId: String, title: String, sender: String?, text: String,
                                   group: Bool) -> INSendMessageIntent {
        let from = person(sender ?? "Beep", id: sender.map { "member-\($0)" } ?? "beep", beep: sender == nil)
        let me = INPerson(personHandle: INPersonHandle(value: "me", type: .unknown), nameComponents: nil,
                          displayName: "You", image: nil, contactIdentifier: nil, customIdentifier: "me", isMe: true)
        let intent = INSendMessageIntent(
            recipients: [me, from], outgoingMessageType: .outgoingMessageText, content: text,
            speakableGroupName: INSpeakableString(spokenPhrase: title), conversationIdentifier: threadId,
            serviceName: nil, sender: from, attachments: nil)
        let initials = group ? groupInitials(title) : "b"
        intent.setImage(avatar(initials, fill: group ? UIColor(groupTint(threadId)) : UIColor(Mint.brand), light: !group),
                        forParameterNamed: \.speakableGroupName)
        let interaction = INInteraction(intent: intent, response: nil)
        interaction.direction = .incoming
        interaction.donate(completion: nil)
        return intent
    }

    @MainActor private func person(_ name: String, id: String, beep: Bool) -> INPerson {
        let image = beep ? avatar("b", fill: UIColor(Mint.brand), light: true)
            : avatar(initial(name), fill: UIColor(Mint.tints[abs(name.unicodeScalars.reduce(0) { $0 + Int($1.value) }) % 3]), light: false)
        return INPerson(personHandle: INPersonHandle(value: id, type: .unknown), nameComponents: nil,
                        displayName: name, image: image, contactIdentifier: nil, customIdentifier: id)
    }

    /// The round avatar the app draws, as a picture for the notification.
    @MainActor private func avatar(_ text: String, fill: UIColor, light: Bool) -> INImage? {
        let size = CGSize(width: 120, height: 120)
        let image = UIGraphicsImageRenderer(size: size).image { _ in
            fill.setFill()
            UIBezierPath(ovalIn: CGRect(origin: .zero, size: size)).fill()
            let font = UIFont.systemFont(ofSize: text.count > 1 ? 42 : 52, weight: .bold)
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font, .foregroundColor: light ? UIColor.white : UIColor(Mint.ink),
            ]
            let bounds = (text as NSString).size(withAttributes: attributes)
            (text as NSString).draw(at: CGPoint(x: (size.width - bounds.width) / 2, y: (size.height - bounds.height) / 2),
                                    withAttributes: attributes)
        }
        return image.pngData().map { INImage(imageData: $0) }
    }

    @MainActor private func initial(_ name: String) -> String { String(name.prefix(1)).uppercased() }

    // MARK: Delegate

    /// In the foreground, a chat that is not on screen still shows a banner.
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification)
        async -> UNNotificationPresentationOptions
    {
        [.banner, .list, .sound]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        guard let thread = info["thread"] as? String else { return }
        let group = info["group"] as? Bool ?? false
        switch response.actionIdentifier {
        case Key.read:
            clear(thread)
        case Key.reply:
            guard let text = (response as? UNTextInputNotificationResponse)?.userText
                .trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else { return }
            clear(thread)
            await reply(thread: thread, group: group, text: text)
        case Key.allow, Key.deny:
            guard let query = info["query"] as? String else { return }
            await MainActor.run { clear(thread) }
            await model?.answerFromNotice(sessionId: thread, queryId: query, group: group, allow: response.actionIdentifier == Key.allow)
        case UNNotificationDefaultActionIdentifier:
            await MainActor.run {
                guard thread != "apps" else { return }
                if thread == "read" { model?.destination = .latestRead; return }
                model?.destination = group ? .group(thread) : .chat(thread)
            }
        default:
            break
        }
    }

    /// Sends the reply with time asked of iOS, since the app may be in the background.
    @MainActor private func reply(thread: String, group: Bool, text: String) async {
        guard let model else { return }
        let task = UIApplication.shared.beginBackgroundTask(withName: "reply")
        defer { UIApplication.shared.endBackgroundTask(task) }
        if !model.connected { model.connect() }
        while !model.connected {
            try? await Task.sleep(for: .milliseconds(300))
            if UIApplication.shared.backgroundTimeRemaining < 5 { return }
        }
        if group {
            try? await model.sendToGroup(thread, requestId: UUID().uuidString.lowercased(), text: text)
        } else {
            await model.reply(toChat: thread, text: text)
        }
    }

    /// Mark as read: takes the chat's notifications off the lock screen.
    func clear(_ thread: String) {
        let center = UNUserNotificationCenter.current()
        center.getDeliveredNotifications { delivered in
            let ids = delivered.filter { $0.request.content.threadIdentifier == thread }.map(\.request.identifier)
            center.removeDeliveredNotifications(withIdentifiers: ids)
        }
    }
}
