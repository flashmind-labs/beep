import Foundation
import Security

/// The account key: 32 random bytes made by the Rust side, kept in the Keychain.
enum Identity {
    private static let service = "com.flashmind.beep"
    private static let account = "identity"

    /// The stored key, or a new one saved on first use.
    static func loadOrCreate() -> Data {
        if let key = load() { return key }
        let key = newIdentity()
        save(key)
        return key
    }

    static func load() -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess else { return nil }
        return item as? Data
    }

    static func save(_ key: Data) {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(base as CFDictionary)
        var add = base
        add[kSecValueData as String] = key
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(add as CFDictionary, nil)
    }

    /// The key file the Android app also reads and writes: the server's node ID and the key as hex.
    static func export(_ key: Data) -> String {
        let hex = key.map { String(format: "%02x", $0) }.joined()
        return "{\"node_id\":\"\(BeepConfig.nodeId)\",\"identity\":\"\(hex)\"}"
    }

    /// Reads a key file's key, or nil if it holds no 32 byte key.
    static func parse(_ text: String) -> Data? {
        guard let json = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any],
            let hex = json["identity"] as? String, hex.count == 64
        else { return nil }
        var bytes = Data()
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2)
            guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        return bytes
    }

    static func name(_ key: Data) -> String {
        (try? identityName(identity: key)) ?? "you"
    }
}
