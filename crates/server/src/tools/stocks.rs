//! Price history from Yahoo Finance's chart endpoint, which needs no key.
//! `show_stock_chart` and the `StockChart` request draw from it.

use crate::protocol::{
    Chart, ChartAxis, ChartKind, ChartPoint, ChartRange, ChartSeries, StockInfo,
};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::{
    fmt::{self, Display, Formatter},
    sync::LazyLock,
    time::Duration,
};
use thiserror::Error;

const BASE: &str = "https://query1.finance.yahoo.com/v8/finance/chart/";
/// Yahoo answers 429 to a full browser User-Agent without its cookies, but
/// takes this one.
const USER_AGENT: &str = "Mozilla/5.0";
pub(crate) const SOURCE: &str = "Yahoo Finance";
/// Symbols one chart compares at most.
pub(crate) const MAX_SYMBOLS: usize = 4;
const MAX_SYMBOL: usize = 20;

static CLIENT: LazyLock<Result<Client, String>> = LazyLock::new(|| {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
});

#[derive(Debug, Error)]
pub(crate) enum StockError {
    #[error("{0:?} is not a ticker symbol: use 1-20 letters, digits or . - ^ = _")]
    BadSymbol(String),
    #[error("Yahoo Finance has no symbol {0}")]
    NotFound(Symbol),
    #[error("Yahoo Finance has no prices for {0} over this range")]
    NoPrices(Symbol),
    #[error("Yahoo Finance answered {0}")]
    Status(StatusCode),
    #[error("could not reach Yahoo Finance: {0}")]
    Http(#[from] reqwest::Error),
    #[error("could not set up the HTTP client: {0}")]
    Client(String),
    #[error("could not read Yahoo Finance's answer: {0}")]
    Parse(#[from] serde_json::Error),
}

/// A ticker as Yahoo names it, such as `AAPL`, `BTC-USD` or `^GSPC`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Symbol(String);

impl Symbol {
    pub(crate) fn parse(text: &str) -> Result<Self, StockError> {
        let text = text.trim();
        let valid = (1..=MAX_SYMBOL).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-^=_".contains(&b));
        if !valid {
            return Err(StockError::BadSymbol(text.chars().take(40).collect()));
        }
        Ok(Self(text.to_ascii_uppercase()))
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One symbol's prices over a range.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct History {
    pub(crate) info: StockInfo,
    /// Unix milliseconds against the close.
    pub(crate) points: Vec<ChartPoint>,
}

impl History {
    /// The change from `previous` to `last`, absolute and in percent.
    pub(crate) fn change(&self) -> Change {
        let absolute = self.info.last - self.info.previous;
        let percent = if self.info.previous == 0.0 {
            0.0
        } else {
            absolute / self.info.previous * 100.0
        };
        Change { absolute, percent }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Change {
    pub(crate) absolute: f64,
    pub(crate) percent: f64,
}

/// The bar size Yahoo gives for `range`: fine enough to see the day, coarse
/// enough to stay a few hundred points.
fn interval(range: ChartRange) -> &'static str {
    match range {
        ChartRange::Day => "5m",
        ChartRange::Week => "30m",
        ChartRange::Month | ChartRange::HalfYear | ChartRange::YearToDate | ChartRange::Year => {
            "1d"
        }
        ChartRange::FiveYears => "1wk",
        ChartRange::Max => "1mo",
    }
}

/// The history of each of `symbols` over `range`, in order. Fails as a whole
/// when any one fails, naming it.
pub(crate) async fn fetch(
    symbols: &[Symbol],
    range: ChartRange,
) -> Result<Vec<History>, StockError> {
    let client = CLIENT.as_ref().map_err(|e| StockError::Client(e.clone()))?;
    let each = symbols
        .iter()
        .map(|symbol| fetch_one(client, symbol, range));
    futures::future::join_all(each).await.into_iter().collect()
}

async fn fetch_one(
    client: &Client,
    symbol: &Symbol,
    range: ChartRange,
) -> Result<History, StockError> {
    let url = Url::parse_with_params(
        &format!("{BASE}{symbol}"),
        [
            ("range", range.as_str()),
            ("interval", interval(range)),
            ("includePrePost", "false"),
        ],
    )
    .map_err(|_| StockError::BadSymbol(symbol.0.clone()))?;
    let response = client.get(url).send().await?;
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(StockError::NotFound(symbol.clone()));
    }
    if !status.is_success() {
        return Err(StockError::Status(status));
    }
    let body = response.bytes().await?;
    parse(symbol, range, &body)
}

#[derive(Deserialize)]
struct Body {
    chart: Envelope,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<Vec<Found>>,
}

#[derive(Deserialize)]
struct Found {
    meta: Meta,
    #[serde(default)]
    timestamp: Vec<i64>,
    indicators: Indicators,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    exchange_name: Option<String>,
    #[serde(default)]
    full_exchange_name: Option<String>,
    #[serde(default)]
    long_name: Option<String>,
    #[serde(default)]
    short_name: Option<String>,
    #[serde(default)]
    regular_market_price: Option<f64>,
    #[serde(default)]
    chart_previous_close: Option<f64>,
    #[serde(default)]
    previous_close: Option<f64>,
}

#[derive(Deserialize)]
struct Indicators {
    #[serde(default)]
    quote: Vec<Quote>,
}

#[derive(Deserialize)]
struct Quote {
    #[serde(default)]
    close: Vec<Option<f64>>,
}

/// Reads one chart answer for `symbol`.
fn parse(symbol: &Symbol, range: ChartRange, body: &[u8]) -> Result<History, StockError> {
    let body: Body = serde_json::from_slice(body)?;
    let found = body
        .chart
        .result
        .and_then(|result| result.into_iter().next())
        .ok_or_else(|| StockError::NotFound(symbol.clone()))?;
    let closes = found
        .indicators
        .quote
        .into_iter()
        .next()
        .map(|quote| quote.close)
        .unwrap_or_default();
    let points: Vec<ChartPoint> = found
        .timestamp
        .iter()
        .zip(closes)
        .filter_map(|(&t, close)| {
            let y = close.filter(|y| y.is_finite())?;
            Some(ChartPoint {
                x: (t * 1000) as f64,
                y,
            })
        })
        .collect();
    let (Some(first), Some(latest)) = (points.first(), points.last()) else {
        return Err(StockError::NoPrices(symbol.clone()));
    };
    let meta = found.meta;
    let usable = |value: Option<f64>| value.filter(|v| v.is_finite() && *v > 0.0);
    let last = usable(meta.regular_market_price).unwrap_or(latest.y);
    let previous = usable(meta.chart_previous_close)
        .or(usable(meta.previous_close))
        .unwrap_or(first.y);
    let text = |value: Option<String>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let info = StockInfo {
        symbol: text(meta.symbol).unwrap_or_else(|| symbol.0.clone()),
        name: text(meta.long_name)
            .or(text(meta.short_name))
            .unwrap_or_default(),
        exchange: text(meta.full_exchange_name)
            .or(text(meta.exchange_name))
            .unwrap_or_default(),
        currency: text(meta.currency).unwrap_or_default(),
        range,
        last,
        previous,
    };
    Ok(History { info, points })
}

/// The chart of `histories` over `range`: one symbol's price as an area, or
/// several as lines of their percent change from their first point.
pub(crate) fn chart(range: ChartRange, histories: &[History]) -> Chart {
    let stock = histories.first().map(|first| first.info.clone());
    match histories {
        [one] => Chart {
            title: title(&one.info),
            kind: ChartKind::Area,
            x: ChartAxis::Time,
            categories: Vec::new(),
            unit: one.info.currency.clone(),
            series: vec![ChartSeries {
                name: one.info.symbol.clone(),
                points: one.points.clone(),
            }],
            stock,
            source: SOURCE.to_owned(),
        },
        many => Chart {
            title: format!(
                "{} ({})",
                many.iter()
                    .map(|history| history.info.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(" vs "),
                range.as_str()
            ),
            kind: ChartKind::Line,
            x: ChartAxis::Time,
            categories: Vec::new(),
            unit: "%".to_owned(),
            series: many
                .iter()
                .map(|history| ChartSeries {
                    name: history.info.symbol.clone(),
                    points: percent_change(&history.points),
                })
                .collect(),
            stock,
            source: SOURCE.to_owned(),
        },
    }
}

fn title(info: &StockInfo) -> String {
    if info.name.is_empty() {
        info.symbol.clone()
    } else {
        format!("{} ({})", info.name, info.symbol)
    }
}

/// Each point as its percent change from the first one.
fn percent_change(points: &[ChartPoint]) -> Vec<ChartPoint> {
    let Some(base) = points.first().map(|p| p.y).filter(|y| *y != 0.0) else {
        return Vec::new();
    };
    points
        .iter()
        .map(|p| ChartPoint {
            x: p.x,
            y: (p.y / base - 1.0) * 100.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::slice;

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
    fn an_unknown_symbol_or_no_prices_is_an_error() {
        let missing = r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found"}}}"#;
        assert!(matches!(
            parse(&symbol("NOPE"), ChartRange::Day, missing.as_bytes()),
            Err(StockError::NotFound(_))
        ));
        let empty = r#"{"chart":{"result":[{"meta":{"symbol":"NOPE"},"timestamp":[1],"indicators":{"quote":[{"close":[null]}]}}]}}"#;
        assert!(matches!(
            parse(&symbol("NOPE"), ChartRange::Day, empty.as_bytes()),
            Err(StockError::NoPrices(_))
        ));
    }

    #[test]
    fn one_symbol_is_an_area_of_prices_several_are_lines_of_percent() {
        let acme = parse(&symbol("ACME"), ChartRange::Month, SAMPLE.as_bytes()).expect("parses");
        let one = chart(ChartRange::Month, slice::from_ref(&acme));
        assert_eq!(one.kind, ChartKind::Area);
        assert_eq!(one.unit, "USD");
        assert_eq!(one.title, "Acme Corp. (ACME)");
        assert_eq!(one.series[0].points, acme.points);

        let mut other = acme.clone();
        other.info.symbol = "OTHR".to_owned();
        let both = chart(ChartRange::Month, &[acme, other]);
        assert_eq!(both.kind, ChartKind::Line);
        assert_eq!(both.unit, "%");
        assert_eq!(both.title, "ACME vs OTHR (1mo)");
        assert_eq!(both.stock.map(|s| s.symbol).as_deref(), Some("ACME"));
        let ys: Vec<f64> = both.series[1].points.iter().map(|p| p.y).collect();
        assert_eq!(ys[0], 0.0);
        assert!((ys[2] - (110.0 / 101.0 - 1.0) * 100.0).abs() < 1e-9);
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

    #[test]
    fn ranges_name_themselves_as_in_json() {
        for range in [
            ChartRange::Day,
            ChartRange::Week,
            ChartRange::Month,
            ChartRange::HalfYear,
            ChartRange::YearToDate,
            ChartRange::Year,
            ChartRange::FiveYears,
            ChartRange::Max,
        ] {
            assert_eq!(
                serde_json::to_value(range).expect("serializes"),
                range.as_str()
            );
        }
    }
}
