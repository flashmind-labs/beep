import UserNotifications

/// What a notice shows as a notification: one thread per chat or group, Reply and Mark as
/// read on a chat, Allow and Deny on an approval. The app posts it for notices it gets over
/// its connection, and the notification service extension fills a push with it.
struct NoticeAlert {
    /// One line of a notification: who said it, nil for Beep.
    struct Line {
        var sender: String?
        var text: String
    }

    enum Key {
        static let chat = "chat"
        static let approval = "approval"
    }

    var threadId: String
    var title: String
    var lines: [Line]
    var group: Bool
    var approval: PendingApproval?

    init(threadId: String, title: String, lines: [Line], group: Bool, approval: PendingApproval? = nil) {
        self.threadId = threadId
        self.title = title
        self.lines = lines
        self.group = group
        self.approval = approval
    }

    /// Nil for a notice that only says a chat changed, which shows nothing.
    init?(_ notice: Notice) {
        if notice.changed || notice.title.isEmpty { return nil }
        let threadId: String
        var line = Line(sender: nil, text: notice.body)
        if notice.approval != nil {
            threadId = notice.sessionId
        } else if notice.app != nil {
            threadId = "apps"
        } else if notice.sessionId.isEmpty, !notice.group {
            // No chat, app or approval: a read of mail or a calendar finished.
            threadId = "read"
        } else {
            threadId = notice.sessionId
            line = Self.line(notice.body, group: notice.group)
        }
        self.init(threadId: threadId, title: notice.title, lines: [line], group: notice.group,
                  approval: notice.approval)
    }

    /// A group notice's body is "Name: text".
    private static func line(_ body: String, group: Bool) -> Line {
        guard group, let colon = body.range(of: ": ") else { return Line(sender: nil, text: body) }
        let name = String(body[..<colon.lowerBound])
        return Line(sender: name == "Beep" ? nil : name, text: String(body[colon.upperBound...]))
    }

    /// Who the last line is from, nil for Beep and for the user's own line.
    var sender: String? {
        let sender = lines.last?.sender
        return sender == "You" ? nil : sender
    }

    /// Fills `content`'s text, thread, actions and the ids a tap needs.
    func fill(_ content: UNMutableNotificationContent) {
        content.title = title
        // A chat with Beep shows only its latest reply; a group keeps who said what.
        let shown = group ? lines : Array(lines.suffix(1))
        content.body = shown.map { line in
            shown.count > 1 ? "\(line.sender ?? "Beep"): \(line.text)" : line.text
        }.joined(separator: "\n")
        content.threadIdentifier = threadId
        content.sound = .default
        content.categoryIdentifier = approval == nil ? Key.chat : Key.approval
        var info: [AnyHashable: Any] = ["thread": threadId, "group": group]
        if let approval {
            info["query"] = approval.queryId
            content.body = "\(approval.approval.action) with \(approval.approval.app)?"
        }
        content.userInfo = info
    }
}
