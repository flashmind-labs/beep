import Intents
import SwiftUI
import UserNotifications

/// Shows the server's notices: one thread per chat or group, Reply and Mark as read in
/// place, and in a group the sender's avatar as a communication notification.
final class ChatNotifier: NSObject, NoticeListener, UNUserNotificationCenterDelegate, @unchecked Sendable {
    /// Set once by the model that owns this.
    @MainActor weak var model: AppModel?

    private enum Key {
        static let chat = NoticeAlert.Key.chat
        static let approval = NoticeAlert.Key.approval
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
        guard let alert = NoticeAlert(notice) else {
            model.noticeChanged(notice)
            return
        }
        if notice.group { model.noticeChanged(notice) }
        // Background work asks only through notices; its chat keeps a card for the question.
        model.askedInBackground(notice)
        let active = UIApplication.shared.applicationState == .active
        // On screen, the chat shows it instead; an approval there gets its card. A group's
        // approval always asks here.
        if active, model.onScreen == notice.sessionId, notice.approval == nil || !notice.group { return }
        post(alert)
    }

    /// A chat's reply the app produced itself, such as one finished in the background.
    @MainActor func show(chatId: String, title: String, lines: [NoticeAlert.Line]) {
        post(NoticeAlert(threadId: chatId, title: title, lines: lines, group: false))
    }

    @MainActor private func post(_ alert: NoticeAlert) {
        let content = UNMutableNotificationContent()
        alert.fill(content)
        let request: UNNotificationRequest
        // Only a group posts as a message from a person, with their avatar; Beep's own
        // notifications keep the app icon rather than a drawn "b".
        if alert.group, let message = try? content.updating(from: intent(threadId: alert.threadId, title: alert.title,
                                                                         sender: alert.sender, text: content.body,
                                                                         group: true)) {
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
        intent.setImage(avatar(initials, fill: group ? UIColor(Mint.tint(for: threadId)) : UIColor(Mint.brand), light: !group),
                        forParameterNamed: \.speakableGroupName)
        let interaction = INInteraction(intent: intent, response: nil)
        interaction.direction = .incoming
        interaction.donate(completion: nil)
        return intent
    }

    @MainActor private func person(_ name: String, id: String, beep: Bool) -> INPerson {
        let image = beep ? avatar("b", fill: UIColor(Mint.brand), light: true)
            : avatar(initial(name), fill: UIColor(Mint.tint(for: name)), light: false)
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

    /// In the foreground, a chat that is not on screen still shows a banner. A push for the
    /// chat on screen, sent while the connection was down, shows nothing.
    /// Both answers run on the main actor: iOS takes their completion from the thread they
    /// finish on, and UIKit aborts when that is not the main thread.
    @MainActor func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification)
        async -> UNNotificationPresentationOptions
    {
        let content = notification.request.content
        let thread = content.userInfo["thread"] as? String
        if thread != nil, thread == model?.onScreen, content.categoryIdentifier != Key.approval { return [] }
        return [.banner, .list, .sound]
    }

    @MainActor func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
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
            clear(thread)
            await model?.answerFromNotice(sessionId: thread, queryId: query, group: group, allow: response.actionIdentifier == Key.allow)
        case UNNotificationDefaultActionIdentifier:
            guard thread != "apps" else { return }
            if thread == "read" { model?.destination = .latestRead; return }
            model?.destination = group ? .group(thread) : .chat(thread)
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
        let deadline = Date.now.addingTimeInterval(20)
        while !model.connected {
            try? await Task.sleep(for: .milliseconds(300))
            if Date.now >= deadline || UIApplication.shared.backgroundTimeRemaining < 5 { return }
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
