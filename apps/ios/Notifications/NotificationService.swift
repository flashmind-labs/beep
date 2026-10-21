import UserNotifications

/// Opens a notice the server sealed to this iPhone's key and shows its text in place of the
/// push's placeholder. Apple carries only the sealed notice; the key never leaves the phone.
final class NotificationService: UNNotificationServiceExtension {
    private var deliver: ((UNNotificationContent) -> Void)?
    private var placeholder: UNNotificationContent?

    override func didReceive(_ request: UNNotificationRequest,
                             withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void) {
        deliver = contentHandler
        placeholder = request.content
        guard let content = request.content.mutableCopy() as? UNMutableNotificationContent,
              let sealed = content.userInfo["notice"] as? String,
              let identity = Identity.load(),
              let notice = try? openNotice(identity: identity, sealed: sealed),
              let alert = NoticeAlert(notice)
        else {
            contentHandler(request.content)
            return
        }
        alert.fill(content)
        contentHandler(content)
    }

    /// Time ran out before the notice opened: show the placeholder.
    override func serviceExtensionTimeWillExpire() {
        if let deliver, let placeholder { deliver(placeholder) }
    }
}
