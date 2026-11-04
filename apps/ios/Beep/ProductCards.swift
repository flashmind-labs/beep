import SwiftUI

/// One ```product block from a reply. Title and url are required, the rest is optional.
struct Product: Decodable, Hashable {
    let title: String
    let url: URL
    var price: String?
    var was: String?
    var store: String?
    var image: URL?
    var rating: Double?
    var reviews: Int?
    var note: String?

    private enum CodingKeys: String, CodingKey {
        case title, url, price, was, store, image, rating, reviews, note
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        title = try c.decode(String.self, forKey: .title)
        guard let url = URL(string: try c.decode(String.self, forKey: .url)), url.host() != nil,
              ["http", "https"].contains(url.scheme?.lowercased()) else {
            throw DecodingError.dataCorruptedError(forKey: .url, in: c, debugDescription: "not a link")
        }
        self.url = url
        price = try? c.decodeIfPresent(String.self, forKey: .price)
        was = try? c.decodeIfPresent(String.self, forKey: .was)
        store = try? c.decodeIfPresent(String.self, forKey: .store)
        image = (try? c.decodeIfPresent(String.self, forKey: .image)).flatMap { $0.flatMap(URL.init(string:)) }
        rating = (try? c.decodeIfPresent(Double.self, forKey: .rating))
            ?? (try? c.decodeIfPresent(String.self, forKey: .rating)).flatMap { $0.flatMap(Double.init) }
        reviews = (try? c.decodeIfPresent(Int.self, forKey: .reviews))
            ?? (try? c.decodeIfPresent(String.self, forKey: .reviews))
                .flatMap { $0.flatMap { Int($0.filter(\.isNumber)) } }
        note = try? c.decodeIfPresent(String.self, forKey: .note)
    }

    /// Nil when the block is not valid product JSON, so it shows as code instead.
    static func parse(_ json: String) -> Product? {
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(Product.self, from: data)
    }

    var storeName: String {
        if let store, !store.isEmpty { return store }
        let host = url.host() ?? ""
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }

    var summary: String? {
        var parts: [String] = []
        if let rating {
            var text = String(format: "%.1f ★", rating)
            if let reviews { text += " (\(reviews.formatted()))" }
            parts.append(text)
        }
        if let note, !note.isEmpty { parts.append(note) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}

extension Product {
    /// The cards in a `show_products` result. Entries that do not make a card are skipped.
    static func shown(toolOutput: String) -> [Product] {
        guard let data = toolOutput.data(using: .utf8),
              let result = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let entries = result["products"] as? [Any] else { return [] }
        return entries.compactMap { ($0 as? [String: Any]).flatMap(Product.init(shown:)) }
    }

    /// One entry of a `show_products` result, with its prices formatted for this phone.
    init?(shown entry: [String: Any]) {
        guard let title = entry["title"] as? String, !title.isEmpty,
              let link = entry["url"] as? String, let url = URL(string: link), url.host() != nil,
              ["http", "https"].contains(url.scheme?.lowercased()) else { return nil }
        self.title = title
        self.url = url
        let currency = entry["currency"] as? String
        let amount = entry["price"] as? Double
        price = amount.map { Product.money($0, currency: currency) }
        was = (entry["was"] as? Double).flatMap { was in
            amount.flatMap { was > $0 ? Product.money(was, currency: currency) : nil }
        }
        store = entry["store"] as? String
        image = (entry["image"] as? String).flatMap(URL.init(string:))
            .flatMap { $0.scheme?.lowercased() == "https" ? $0 : nil }
        rating = entry["rating"] as? Double
        reviews = entry["reviews"] as? Int
        note = entry["note"] as? String
    }

    /// "€249" or "€249.99", in the phone's locale.
    private static func money(_ amount: Double, currency: String?) -> String {
        let digits = amount.rounded() == amount ? 0 : 2
        guard let currency, !currency.isEmpty else {
            return amount.formatted(.number.precision(.fractionLength(digits)))
        }
        return amount.formatted(.currency(code: currency).precision(.fractionLength(digits)))
    }
}

/// Consecutive product blocks: a row of cards that scrolls sideways, or one card at full width.
struct ProductRow: View {
    let products: [Product]

    var body: some View {
        SwiftUI.Group {
            if products.count == 1, let product = products.first {
                ProductCard(product: product)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(spacing: 10) {
                        ForEach(products, id: \.self) { product in
                            ProductCard(product: product).frame(width: 208)
                        }
                    }
                    .scrollTargetLayout()
                    .padding(.horizontal, 18)
                    .padding(.bottom, 6)
                }
                .scrollTargetBehavior(.viewAligned)
                .contentMargins(.horizontal, 0)
                .padding(.horizontal, -18)
                .blocksChatSwipe()
            }
        }
    }
}

struct ProductCard: View {
    @Environment(\.openURL) private var openURL
    let product: Product

    var body: some View {
        Button { openURL(product.url) } label: { content }
            .buttonStyle(.plain)
            .accessibilityLabel("\(product.title), \(product.price ?? "no price"), opens \(product.storeName)")
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 0) {
            picture.frame(height: 116).frame(maxWidth: .infinity).clipped()
            VStack(alignment: .leading, spacing: 5) {
                Text(product.storeName).font(.body(11)).foregroundStyle(Mint.muted).lineLimit(1)
                Text(product.title).font(.body(13.5, .medium)).foregroundStyle(Mint.ink)
                    .lineLimit(2, reservesSpace: true)
                    .multilineTextAlignment(.leading)
                price
                if let summary = product.summary {
                    Text(summary).font(.body(12)).foregroundStyle(Mint.muted).lineLimit(1)
                }
            }
            .padding(.horizontal, 12).padding(.top, 10).padding(.bottom, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Mint.surface)
        .clipShape(RoundedRectangle(cornerRadius: Mint.radius))
        .overlay(RoundedRectangle(cornerRadius: Mint.radius).stroke(Mint.ledge, lineWidth: 1.5))
    }

    @ViewBuilder private var picture: some View {
        if let image = product.image {
            AsyncImage(url: image) { phase in
                if let picture = phase.image {
                    picture.resizable().scaledToFill()
                } else if phase.error != nil {
                    placeholderIcon
                } else {
                    Mint.code
                }
            }
        } else {
            placeholderIcon
        }
    }

    private var placeholderIcon: some View {
        Mint.code.overlay(Image(systemName: "bag").font(.system(size: 28)).foregroundStyle(Mint.dim))
    }

    @ViewBuilder private var price: some View {
        if let price = product.price, !price.isEmpty {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(price).font(.body(14, .semibold)).foregroundStyle(Mint.ink)
                if let was = product.was, !was.isEmpty {
                    Text(was).font(.body(12)).foregroundStyle(Mint.muted).strikethrough()
                }
            }
        } else {
            Text("See price on the site").font(.body(12.5)).foregroundStyle(Mint.muted)
        }
    }
}
