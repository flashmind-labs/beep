use super::image;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NAME: &str = "show_products";
const MAX_PRODUCTS: usize = 6;
const MAX_TITLE: usize = 200;
const MAX_NOTE: usize = 120;

/// Shows products as cards under the reply, each checked here.
pub(crate) struct ShowProductsTool;

#[derive(Debug, Deserialize)]
struct Args {
    products: Vec<Requested>,
}

/// A product as the model gives it.
#[derive(Debug, Deserialize)]
struct Requested {
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

/// A product card; `currency` is set with `price`, and `was` only when above it.
#[derive(Debug, PartialEq, Serialize)]
struct Card {
    title: String,
    url: String,
    store: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    was: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    currency: Option<Currency>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reviews: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// An ISO 4217 code such as `EUR`: three uppercase letters.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
struct Currency(String);

impl Currency {
    fn parse(code: &str) -> Option<Self> {
        let code = code.trim().to_ascii_uppercase();
        (code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())).then_some(Self(code))
    }
}

#[derive(Debug, Serialize)]
struct Shown {
    action: &'static str,
    products: Vec<Card>,
    /// What was left out and why, for the model.
    dropped: Vec<String>,
}

#[async_trait]
impl Tool for ShowProductsTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Show products to the user as cards under your reply: title, shop, price, picture, rating. Use it whenever the user is shopping, after searching. Only use what a tool or page gave you: the real product page URL, the price you saw. Leave out a field you do not know. Links and images are checked, and anything dropped is reported back."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "products": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_PRODUCTS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string", "maxLength": MAX_TITLE },
                            "url": { "type": "string", "description": "The product's own page" },
                            "store": { "type": "string", "description": "The shop's name; defaults to the URL's domain" },
                            "price": { "type": "number", "exclusiveMinimum": 0, "description": "The current price as a plain number, like 249.99" },
                            "was": { "type": "number", "exclusiveMinimum": 0, "description": "The price before a discount" },
                            "currency": { "type": "string", "description": "ISO 4217 code of price and was, like EUR or USD" },
                            "image": { "type": "string", "description": "https URL of a product photo" },
                            "rating": { "type": "number", "minimum": 0, "maximum": 5 },
                            "reviews": { "type": "integer", "minimum": 0 },
                            "note": { "type": "string", "maxLength": MAX_NOTE, "description": "Why pick this one, in a few words" }
                        },
                        "required": ["title", "url"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["products"],
            "additionalProperties": false
        })
    }
    /// Checking images and reading pages for missing ones takes up to 30 s.
    fn timeout_secs(&self) -> Option<u64> {
        Some(60)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Showing products…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match ctx.parse_args(NAME) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        if args.products.is_empty() || args.products.len() > MAX_PRODUCTS {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Show between 1 and {MAX_PRODUCTS} products."),
            ));
        }
        let mut dropped = Vec::new();
        let mut cards = Vec::new();
        for requested in args.products {
            match card(requested, &mut dropped) {
                Ok(card) => cards.push(card),
                Err(reason) => dropped.push(reason),
            }
        }
        if cards.is_empty() {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No product could be shown. {}", dropped.join("; ")),
            ));
        }
        let checks = cards.iter().map(|card| async move {
            match card.image.as_deref() {
                Some(url) => image::check(url).await.err(),
                None => None,
            }
        });
        let failures = futures::future::join_all(checks).await;
        for (card, failure) in cards.iter_mut().zip(failures) {
            if let Some(reason) = failure {
                dropped.push(format!("image of {}: {reason}", card.title));
                card.image = None;
            }
        }
        // A card without a picture gets its page's link preview image.
        let previews = cards.iter().map(|card| async move {
            match card.image {
                Some(_) => None,
                None => image::page_image(&card.url).await,
            }
        });
        let previews = futures::future::join_all(previews).await;
        for (card, preview) in cards.iter_mut().zip(previews) {
            if preview.is_some() {
                card.image = preview;
            }
        }
        let shown = Shown {
            action: NAME,
            products: cards,
            dropped,
        };
        Ok(ToolResult::success(
            ctx.tool_call_id,
            serde_json::to_string(&shown)?,
        ))
    }
}

/// The card for `requested`, or why not; bad fields are noted in `dropped`.
fn card(requested: Requested, dropped: &mut Vec<String>) -> Result<Card, String> {
    let title = clip(&requested.title, MAX_TITLE);
    if title.is_empty() {
        return Err("a product without a title".to_owned());
    }
    let url = requested.url.trim();
    let host = public_host(url).ok_or_else(|| format!("{title}: {url} is not a web page"))?;
    let store = requested
        .store
        .as_deref()
        .map(|s| clip(s, 60))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| host.strip_prefix("www.").unwrap_or(&host).to_owned());
    let price = requested.price.filter(|p| p.is_finite() && *p > 0.0);
    let currency = requested.currency.as_deref().and_then(Currency::parse);
    let (price, currency) = match (price, currency) {
        (Some(price), Some(currency)) => (Some(price), Some(currency)),
        (Some(_), None) => {
            dropped.push(format!("price of {title}: no ISO currency code"));
            (None, None)
        }
        _ => (None, None),
    };
    let was = requested
        .was
        .filter(|was| price.is_some_and(|price| was.is_finite() && *was > price));
    let image = requested
        .image
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty());
    let rating = requested.rating.filter(|r| (0.0..=5.0).contains(r));
    let note = requested
        .note
        .as_deref()
        .map(|n| clip(n, MAX_NOTE))
        .filter(|n| !n.is_empty());
    Ok(Card {
        title,
        url: url.to_owned(),
        store,
        price,
        was,
        currency,
        image,
        rating,
        reviews: requested.reviews,
        note,
    })
}

/// The host of an http(s) page on a public domain name.
pub(super) fn public_host(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || url.len() > 2048 {
        return None;
    }
    let host = parsed.domain()?;
    (host != "localhost" && !host.ends_with(".localhost") && host.contains('.'))
        .then(|| host.to_owned())
}

pub(super) fn clip(text: &str, max: usize) -> String {
    text.trim()
        .chars()
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested(json: Value) -> Requested {
        serde_json::from_value(json).expect("test product is valid")
    }

    #[test]
    fn keeps_what_holds_up() {
        let mut dropped = Vec::new();
        let card = card(
            requested(json!({
                "title": " Dreame Z30 ", "url": "https://www.shop.example/z30",
                "price": 249.0, "was": 329.0, "currency": "eur", "rating": 4.6, "reviews": 1204,
                "note": "Quietest"
            })),
            &mut dropped,
        )
        .expect("a valid product");
        assert_eq!(card.title, "Dreame Z30");
        assert_eq!(card.store, "shop.example");
        assert_eq!(card.price, Some(249.0));
        assert_eq!(card.was, Some(329.0));
        assert_eq!(card.currency, Some(Currency("EUR".to_owned())));
        assert!(dropped.is_empty(), "{dropped:?}");
    }

    #[test]
    fn drops_a_price_without_a_currency_and_a_was_below_the_price() {
        let mut dropped = Vec::new();
        let card = card(
            requested(json!({ "title": "Kettle", "url": "https://shop.example/k", "price": 30.0, "currency": "euros" })),
            &mut dropped,
        )
        .expect("a valid product");
        assert_eq!((card.price, card.currency), (None, None));
        assert_eq!(dropped.len(), 1);

        let card = card_of(
            json!({ "title": "Kettle", "url": "https://shop.example/k", "price": 30.0, "was": 25.0, "currency": "USD" }),
        );
        assert_eq!((card.price, card.was), (Some(30.0), None));
    }

    #[test]
    fn rejects_links_that_are_not_public_pages() {
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://localhost/a",
            "https://127.0.0.1/a",
            "https://intranet/a",
            "shop.example/a",
        ] {
            let result = card(
                requested(json!({ "title": "Kettle", "url": url })),
                &mut Vec::new(),
            );
            assert!(result.is_err(), "{url}");
        }
    }

    #[test]
    fn output_leaves_out_unknown_fields() {
        let shown = Shown {
            action: NAME,
            products: vec![card_of(
                json!({ "title": "Kettle", "url": "https://shop.example/k" }),
            )],
            dropped: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(&shown).expect("serializes"),
            json!({
                "action": "show_products",
                "products": [{ "title": "Kettle", "url": "https://shop.example/k", "store": "shop.example" }],
                "dropped": []
            })
        );
    }

    fn card_of(json: Value) -> Card {
        card(requested(json), &mut Vec::new()).expect("a valid product")
    }
}
