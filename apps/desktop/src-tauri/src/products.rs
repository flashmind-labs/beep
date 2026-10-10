//! Product cards. Beep writes one ```product block of JSON per item, and
//! blocks in a row show as a row of cards that scrolls sideways. Clicking a
//! card opens the shop in the browser; the corner button copies the link.
//! The `show_products` tool sends the same cards with typed fields.

use serde::Deserialize;

const COPY_ICON: &str = r#"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>"#;

#[derive(Debug, Deserialize)]
pub struct Product {
    title: String,
    url: String,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    was: Option<String>,
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    rating: Option<Number>,
    #[serde(default)]
    reviews: Option<Number>,
    #[serde(default)]
    note: Option<String>,
}

/// One entry of a `show_products` result.
#[derive(Debug, Deserialize)]
struct Listing {
    title: String,
    url: String,
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    price: Option<f64>,
    #[serde(default)]
    was: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    reviews: Option<u64>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Deserialize)]
struct Shown {
    products: Vec<serde_json::Value>,
}

impl From<Listing> for Product {
    fn from(l: Listing) -> Self {
        let currency = l.currency.as_deref();
        Product {
            title: l.title,
            url: l.url,
            price: l.price.map(|p| price(p, currency)),
            was: l.was.map(|w| price(w, currency)),
            store: l.store,
            image: l.image,
            rating: l.rating.map(Number::Float),
            reviews: l.reviews.map(|r| Number::Float(r as f64)),
            note: l.note,
        }
    }
}

/// The row of cards a `show_products` result lists, or None when it has
/// none. An entry that does not parse is left out.
pub fn shown(output: &str) -> Option<String> {
    let shown: Shown = serde_json::from_str(output).ok()?;
    let cards: Vec<String> = shown
        .products
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Listing>(entry).ok())
        .map(Product::from)
        .filter(Product::valid)
        .map(|p| p.card())
        .collect();
    (!cards.is_empty()).then(|| row(&cards))
}

/// `amount` in `currency`, as in `€249`, `€249.99` or `249 CHF`.
fn price(amount: f64, currency: Option<&str>) -> String {
    let whole = currency == Some("JPY") || (amount * 100.0).round() % 100.0 == 0.0;
    let number = if whole {
        grouped(&format!("{:.0}", amount.round()))
    } else {
        let fixed = format!("{amount:.2}");
        let (int, cents) = fixed.split_once('.').unwrap_or((&fixed, "00"));
        format!("{}.{cents}", grouped(int))
    };
    let symbol = match currency {
        Some("EUR") => "€",
        Some("USD") => "$",
        Some("GBP") => "£",
        Some("JPY") => "¥",
        Some(code) => return format!("{number} {code}"),
        None => "",
    };
    format!("{symbol}{number}")
}

/// Digits with a comma every three places, `1,204` for `1204`.
fn grouped(digits: &str) -> String {
    let (sign, digits) = match digits.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", digits),
    };
    let mut out = String::from(sign);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Ratings and review counts as the model writes them, `4.6` or `"4.6"`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Number {
    Float(f64),
    Text(String),
}

impl Product {
    /// The product in a block's text, or None when it is not valid JSON
    /// with a title and a web link.
    pub fn parse(json: &str) -> Option<Self> {
        let product: Self = serde_json::from_str(json.trim()).ok()?;
        product.valid().then_some(product)
    }

    fn valid(&self) -> bool {
        !self.title.trim().is_empty() && web_link(&self.url)
    }

    pub fn card(&self) -> String {
        let store = self
            .store
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map_or_else(|| host(&self.url).to_owned(), str::to_owned);
        let image = match self.image.as_deref().filter(|url| web_link(url)) {
            Some(url) => format!(
                r#"<span class="dprod-img"><img src="{}" alt="" loading="lazy"></span>"#,
                escape(url)
            ),
            None => String::new(),
        };
        let price = match self.price.as_deref().filter(|p| !p.trim().is_empty()) {
            Some(price) => {
                let was = self
                    .was
                    .as_deref()
                    .filter(|w| !w.trim().is_empty())
                    .map(|w| format!("<s>{}</s>", escape(w)))
                    .unwrap_or_default();
                format!(
                    r#"<span class="dprod-price"><b>{}</b>{was}</span>"#,
                    escape(price)
                )
            }
            None => r#"<span class="dprod-meta">See price on the site</span>"#.to_owned(),
        };
        let mut meta = Vec::new();
        if let Some(rating) = self.rating.as_ref().map(Number::rating) {
            let reviews = self
                .reviews
                .as_ref()
                .map(|r| format!(" ({})", r.count()))
                .unwrap_or_default();
            meta.push(format!("{rating} &#9733;{reviews}"));
        }
        if let Some(note) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
            meta.push(escape(note));
        }
        let meta = if meta.is_empty() {
            String::new()
        } else {
            format!(
                r#"<span class="dprod-meta">{}</span>"#,
                meta.join(" &middot; ")
            )
        };
        let label = match self.price.as_deref() {
            Some(price) => format!("{}, {price}, opens {store}", self.title),
            None => format!("{}, opens {store}", self.title),
        };
        let url = escape(&self.url);
        format!(
            r#"<div class="dprod"><a class="dprod-card" href="{url}" aria-label="{label}">{image}<span class="dprod-info"><span class="dprod-store">{store}</span><span class="dprod-title">{title}</span>{price}{meta}</span></a><button class="dprod-copy" data-copy="{url}" aria-label="Copy link">{COPY_ICON}</button></div>"#,
            label = escape(&label),
            store = escape(&store),
            title = escape(&self.title),
        )
    }
}

impl Number {
    fn rating(&self) -> String {
        match self {
            Number::Float(n) => format!("{n:.1}"),
            Number::Text(s) => escape(s.trim()),
        }
    }

    /// A review count with thousands separators, as in `1,204`.
    fn count(&self) -> String {
        match self {
            Number::Float(n) if n.fract() == 0.0 && *n >= 0.0 => grouped(&format!("{n:.0}")),
            Number::Float(n) => n.to_string(),
            Number::Text(s) => escape(s.trim()),
        }
    }
}

/// A card while its block is still streaming in.
pub fn pending_card() -> &'static str {
    r#"<div class="dprod pending"><span class="dprod-card"><span class="dprod-img"></span></span></div>"#
}

pub fn row(cards: &[String]) -> String {
    format!(r#"<div class="dprods">{}</div>"#, cards.concat())
}

fn web_link(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// The shop's name from its link, `shop.example` for
/// `https://www.shop.example/item?id=1`.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .rsplit('@')
        .next()
        .unwrap_or(rest);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_title_and_a_web_link() {
        assert!(Product::parse(r#"{"title":"Kettle","url":"https://shop.example/k"}"#).is_some());
        assert!(Product::parse(r#"{"title":"Kettle"}"#).is_none());
        assert!(Product::parse(r#"{"title":"Kettle","url":"javascript:alert(1)"}"#).is_none());
        assert!(Product::parse(r#"{"title":"Kettle","url":"https://shop.example/k""#).is_none());
    }

    #[test]
    fn card_shows_what_it_has() {
        let product = Product::parse(
            r#"{"title":"Dreame <b>Z30</b>","url":"https://www.shop.example/item?id=1","price":"€249","was":"€329","rating":4.6,"reviews":1204,"note":"Quietest"}"#,
        )
        .expect("valid product");
        let card = product.card();
        assert!(card.contains("Dreame &lt;b&gt;Z30&lt;/b&gt;"), "{card}");
        assert!(card.contains(">shop.example<"), "{card}");
        assert!(card.contains("<b>€249</b><s>€329</s>"), "{card}");
        assert!(
            card.contains("4.6 &#9733; (1,204) &middot; Quietest"),
            "{card}"
        );
        assert!(!card.contains("<img"), "{card}");

        let bare = Product::parse(r#"{"title":"Kettle","url":"https://shop.example/k","image":"https://shop.example/k.jpg"}"#)
            .expect("valid product")
            .card();
        assert!(bare.contains("See price on the site"), "{bare}");
        assert!(
            bare.contains(r#"<img src="https://shop.example/k.jpg""#),
            "{bare}"
        );
        assert!(!bare.contains("&#9733;"), "{bare}");
    }

    #[test]
    fn price_formats_by_currency() {
        assert_eq!(price(249.0, Some("EUR")), "€249");
        assert_eq!(price(249.99, Some("EUR")), "€249.99");
        assert_eq!(price(249.5, Some("USD")), "$249.50");
        assert_eq!(price(1234567.0, Some("GBP")), "£1,234,567");
        assert_eq!(price(12345.6, Some("JPY")), "¥12,346");
        assert_eq!(price(1249.0, Some("CHF")), "1,249 CHF");
        assert_eq!(price(19.999, Some("USD")), "$20");
        assert_eq!(price(5.0, None), "5");
    }

    #[test]
    fn shown_builds_a_row_from_the_tool_result() {
        let output = r#"{"action":"show_products","products":[
            {"title":"Dreame <Z30>","url":"https://shop.example/item","store":"shop.example","price":249,"was":329,"currency":"EUR","image":"https://shop.example/img.jpg","rating":4.6,"reviews":1204,"note":"Quietest \"one\""},
            {"title":"Kettle","url":"https://other.example/k","store":"other.example"},
            {"title":"Bad","url":"javascript:alert(1)","store":"x"},
            {"title":"","url":"https://shop.example/e","store":"x"},
            {"url":"https://shop.example/no-title"},
            "not an object"
        ],"dropped":["x"]}"#;
        let row = shown(output).expect("two valid entries");
        assert!(row.starts_with(r#"<div class="dprods">"#), "{row}");
        assert_eq!(row.matches(r#"<div class="dprod">"#).count(), 2, "{row}");
        assert!(row.contains("Dreame &lt;Z30&gt;"), "{row}");
        assert!(row.contains("<b>€249</b><s>€329</s>"), "{row}");
        assert!(
            row.contains("4.6 &#9733; (1,204) &middot; Quietest &quot;one&quot;"),
            "{row}"
        );
        assert!(
            row.contains(r#"<img src="https://shop.example/img.jpg""#),
            "{row}"
        );
        assert!(row.contains("See price on the site"), "{row}");
        assert!(!row.contains("javascript"), "{row}");

        assert!(shown(r#"{"products":[]}"#).is_none());
        assert!(shown(r#"{"products":[{"title":"Bad","url":"ftp://x"}]}"#).is_none());
        assert!(shown("failed").is_none());
    }

    #[test]
    fn host_drops_scheme_www_port_and_path() {
        assert_eq!(host("https://www.shop.example:8443/a?b#c"), "shop.example");
        assert_eq!(host("http://shop.example"), "shop.example");
    }
}
