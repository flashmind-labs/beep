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
        card.contains("4.6 \u{2605} (1,204) \u{b7} Quietest"),
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
    assert!(!bare.contains('\u{2605}'), "{bare}");
}

#[test]
fn host_drops_scheme_www_port_and_path() {
    assert_eq!(host("https://www.shop.example:8443/a?b#c"), "shop.example");
    assert_eq!(host("http://shop.example"), "shop.example");
}
