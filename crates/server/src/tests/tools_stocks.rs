use super::*;

// Made-up numbers in the shape Yahoo answers with.
const SAMPLE: &str = r#"{"chart":{"result":[{"meta":{"currency":"USD","symbol":"ACME",
        "exchangeName":"NMS","fullExchangeName":"NasdaqGS","regularMarketPrice":110.0,
        "longName":"Acme Corp.","shortName":"Acme","chartPreviousClose":100.0,"previousClose":108.0},
        "timestamp":[1790000000,1790086400,1790172800,1790259200],
        "indicators":{"quote":[{"close":[101.0,null,105.5,110.0]}]}}],"error":null}}"#;

fn symbol(text: &str) -> Symbol {
    Symbol::parse(text).expect("a valid symbol")
}

#[test]
fn parses_meta_and_skips_missing_closes() {
    let history = parse(&symbol("acme"), ChartRange::Month, SAMPLE.as_bytes()).expect("parses");
    assert_eq!(history.info.symbol, "ACME");
    assert_eq!(history.info.name, "Acme Corp.");
    assert_eq!(history.info.exchange, "NasdaqGS");
    assert_eq!(history.info.currency, "USD");
    assert_eq!((history.info.last, history.info.previous), (110.0, 100.0));
    assert_eq!(history.points.len(), 3);
    assert_eq!(
        history.points[1],
        ChartPoint {
            x: 1_790_172_800_000.0,
            y: 105.5
        }
    );
    let change = history.change();
    assert_eq!((change.absolute, change.percent), (10.0, 10.0));
}

#[test]
fn checks_symbols() {
    for good in ["AAPL", "btc-usd", "^GSPC", "EURUSD=X", "BRK.B", "ES_F"] {
        assert!(Symbol::parse(good).is_ok(), "{good}");
    }
    assert_eq!(symbol("btc-usd").to_string(), "BTC-USD");
    for bad in ["", "AAPL MSFT", "../x", "A/B", "ABCDEFGHIJKLMNOPQRSTU", "Ä"] {
        assert!(Symbol::parse(bad).is_err(), "{bad}");
    }
}
