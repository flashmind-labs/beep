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

    let bare = Product::parse(
        r#"{"title":"Kettle","url":"https://shop.example/k","image":"https://shop.example/k.jpg"}"#,
    )
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
