//! Charts under a reply, which the macOS app draws: a stock's price from
//! Yahoo Finance, or numbers the model already got from a tool or a page.

use super::{
    products::clip,
    stocks::{self, History, MAX_SYMBOLS, Symbol},
};
use crate::protocol::{Chart, ChartAxis, ChartKind, ChartPoint, ChartRange, ChartSeries};
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write;

const STOCK: &str = "show_stock_chart";
const CHART: &str = "show_chart";
const MAX_SERIES: usize = 4;
const MAX_POINTS: usize = 500;
const MAX_TITLE: usize = 120;
const MAX_NAME: usize = 60;
const MAX_UNIT: usize = 20;

/// Shows a stock's price over a range, or compares up to four.
pub(crate) struct ShowStockChartTool;

#[derive(Debug, Deserialize)]
struct StockArgs {
    symbols: Vec<String>,
    #[serde(default = "default_range")]
    range: ChartRange,
}

fn default_range() -> ChartRange {
    ChartRange::Month
}

#[derive(Debug, Serialize)]
struct Shown {
    action: &'static str,
    chart: Chart,
    /// What the model can say about it, since it does not see the chart.
    #[serde(skip_serializing_if = "String::is_empty")]
    summary: String,
    /// What was left out and why, for the model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dropped: Vec<String>,
}

#[async_trait]
impl Tool for ShowStockChartTool {
    fn name(&self) -> &str {
        STOCK
    }
    fn description(&self) -> &str {
        "Show the price chart of a stock, fund, index, currency pair or crypto under your reply, from Yahoo Finance. Give one symbol for its price, or up to four to compare them as percent change. Use Yahoo's ticker symbols, such as AAPL, BTC-USD, EURUSD=X or ^GSPC. The user can switch the range on the chart. The result gives the latest price and the change over the range for your reply."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbols": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_SYMBOLS,
                    "items": { "type": "string", "description": "A Yahoo Finance ticker, like AAPL, BTC-USD or ^GSPC" }
                },
                "range": {
                    "type": "string",
                    "enum": ["1d", "5d", "1mo", "6mo", "ytd", "1y", "5y", "max"],
                    "description": "How far back the chart reaches; defaults to 1mo"
                }
            },
            "required": ["symbols"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(45)
    }
    fn humanize(&self, _args: &Value) -> String {
        "Charting the price…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: StockArgs = match ctx.parse_args(STOCK) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        if args.symbols.is_empty() || args.symbols.len() > MAX_SYMBOLS {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("Chart between 1 and {MAX_SYMBOLS} symbols."),
            ));
        }
        let mut symbols = Vec::new();
        for text in &args.symbols {
            match Symbol::parse(text) {
                Ok(symbol) if !symbols.contains(&symbol) => symbols.push(symbol),
                Ok(_) => {}
                Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
            }
        }
        let histories = match stocks::fetch(&symbols, args.range).await {
            Ok(histories) => histories,
            Err(e) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("No chart: {e}."),
                ));
            }
        };
        let shown = Shown {
            action: STOCK,
            chart: stocks::chart(args.range, &histories),
            summary: summary(args.range, &histories),
            dropped: Vec::new(),
        };
        Ok(ToolResult::success(
            ctx.tool_call_id,
            serde_json::to_string(&shown)?,
        ))
    }
}

/// One line per symbol: its latest price and the change over `range`.
fn summary(range: ChartRange, histories: &[History]) -> String {
    let mut out = String::new();
    for history in histories {
        let info = &history.info;
        let change = history.change();
        if !out.is_empty() {
            out.push('\n');
        }
        let name = if info.name.is_empty() {
            info.symbol.clone()
        } else {
            format!("{} ({})", info.name, info.symbol)
        };
        let _ = write!(
            out,
            "{name}: last {} {}, {:+.2} ({:+.2}%) over {} from {}",
            price(info.last),
            info.currency,
            change.absolute,
            change.percent,
            range.as_str(),
            price(info.previous),
        );
        if !info.exchange.is_empty() {
            let _ = write!(out, ", on {}", info.exchange);
        }
        out.push('.');
    }
    out
}

/// A price with the decimals it needs: two, or more below one.
fn price(value: f64) -> String {
    if value.abs() < 1.0 {
        format!("{value:.4}")
    } else {
        format!("{value:.2}")
    }
}

/// Draws numbers the model got from a tool or a page.
pub(crate) struct ShowChartTool;

#[derive(Debug, Deserialize)]
struct ChartArgs {
    title: String,
    #[serde(default = "default_kind")]
    kind: ChartKind,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    categories: Option<Vec<String>>,
    series: Vec<RequestedSeries>,
}

fn default_kind() -> ChartKind {
    ChartKind::Line
}

/// A series as the model gives it: `values` against categories, or `points`
/// against time.
#[derive(Debug, Deserialize)]
struct RequestedSeries {
    name: String,
    #[serde(default)]
    values: Option<Vec<Option<f64>>>,
    #[serde(default)]
    points: Option<Vec<TimePoint>>,
}

#[derive(Debug, Deserialize)]
struct TimePoint {
    t: String,
    v: Option<f64>,
}

#[async_trait]
impl Tool for ShowChartTool {
    fn name(&self) -> &str {
        CHART
    }
    fn description(&self) -> &str {
        "Show a line, area or bar chart under your reply, of numbers a tool or page gave you in this chat, never ones you made up or estimated. For stock prices use show_stock_chart instead. Either give `categories` (labels such as months or names) and each series' `values` in the same order, or give each series `points` of a date or time and a value. Up to 4 series of up to 500 points. Invalid points are dropped and reported back."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "maxLength": MAX_TITLE },
                "kind": { "type": "string", "enum": ["line", "area", "bar"], "description": "Defaults to line" },
                "unit": { "type": "string", "maxLength": MAX_UNIT, "description": "What the values measure, like USD, °C, % or km" },
                "source": { "type": "string", "maxLength": MAX_NAME, "description": "Where the numbers come from, like the site's name" },
                "categories": {
                    "type": "array",
                    "maxItems": MAX_POINTS,
                    "items": { "type": "string" },
                    "description": "Labels of the x axis, in order. Leave out to use points over time."
                },
                "series": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_SERIES,
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "maxLength": MAX_NAME },
                            "values": {
                                "type": "array",
                                "maxItems": MAX_POINTS,
                                "items": { "type": ["number", "null"] },
                                "description": "One value per category, null where unknown"
                            },
                            "points": {
                                "type": "array",
                                "maxItems": MAX_POINTS,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "t": { "type": "string", "description": "A date like 2026-10-01 or an ISO 8601 time like 2026-10-01T14:30:00Z" },
                                        "v": { "type": "number" }
                                    },
                                    "required": ["t", "v"],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["name"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["title", "series"],
            "additionalProperties": false
        })
    }
    fn humanize(&self, _args: &Value) -> String {
        "Drawing a chart…".into()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: ChartArgs = match ctx.parse_args(CHART) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        let mut dropped = Vec::new();
        let chart = match build(args, &mut dropped) {
            Ok(chart) => chart,
            Err(reason) => return Ok(ToolResult::failure(ctx.tool_call_id, reason)),
        };
        let shown = Shown {
            action: CHART,
            chart,
            summary: String::new(),
            dropped,
        };
        Ok(ToolResult::success(
            ctx.tool_call_id,
            serde_json::to_string(&shown)?,
        ))
    }
}

/// The chart for `args`, or why there is none. Points that do not hold up
/// are left out and noted in `dropped`.
fn build(args: ChartArgs, dropped: &mut Vec<String>) -> Result<Chart, String> {
    if args.series.is_empty() || args.series.len() > MAX_SERIES {
        return Err(format!("Give between 1 and {MAX_SERIES} series."));
    }
    let title = clip(&args.title, MAX_TITLE);
    let categories: Vec<String> = args
        .categories
        .unwrap_or_default()
        .iter()
        .map(|label| clip(label, MAX_NAME))
        .collect();
    if categories.len() > MAX_POINTS {
        return Err(format!("Give at most {MAX_POINTS} categories."));
    }
    let x = if categories.is_empty() {
        ChartAxis::Time
    } else {
        ChartAxis::Category
    };
    let mut series = Vec::new();
    for (i, requested) in args.series.into_iter().enumerate() {
        let name = match clip(&requested.name, MAX_NAME) {
            name if name.is_empty() => format!("Series {}", i + 1),
            name => name,
        };
        let points = match x {
            ChartAxis::Category => category_points(&name, requested, categories.len(), dropped)?,
            ChartAxis::Time => time_points(&name, requested, dropped)?,
        };
        if points.is_empty() {
            dropped.push(format!("{name}: no valid points"));
        } else {
            series.push(ChartSeries { name, points });
        }
    }
    if series.is_empty() {
        return Err(format!("Nothing to chart. {}", dropped.join("; ")));
    }
    Ok(Chart {
        title,
        kind: args.kind,
        x,
        categories,
        unit: args
            .unit
            .as_deref()
            .map(|unit| clip(unit, MAX_UNIT))
            .unwrap_or_default(),
        series,
        stock: None,
        source: args
            .source
            .as_deref()
            .map(|source| clip(source, MAX_NAME))
            .unwrap_or_default(),
    })
}

/// `values` against the category at each index; a series that does not
/// match the categories fails the whole chart, so the model fixes it.
fn category_points(
    name: &str,
    requested: RequestedSeries,
    categories: usize,
    dropped: &mut Vec<String>,
) -> Result<Vec<ChartPoint>, String> {
    let Some(values) = requested.values else {
        return Err(format!(
            "{name}: give `values`, one per category, since categories are set."
        ));
    };
    if values.len() != categories {
        return Err(format!(
            "{name}: {} values for {categories} categories; give one per category, null where unknown.",
            values.len()
        ));
    }
    let mut skipped = 0;
    let points = values
        .iter()
        .enumerate()
        .filter_map(|(i, value)| match value.filter(|v| v.is_finite()) {
            Some(y) => Some(ChartPoint { x: i as f64, y }),
            None => {
                skipped += 1;
                None
            }
        })
        .collect();
    if skipped > 0 {
        dropped.push(format!("{name}: {skipped} missing values"));
    }
    Ok(points)
}

/// `points` against time, sorted; points with an unreadable time or value
/// are left out.
fn time_points(
    name: &str,
    requested: RequestedSeries,
    dropped: &mut Vec<String>,
) -> Result<Vec<ChartPoint>, String> {
    let Some(requested) = requested.points else {
        return Err(format!(
            "{name}: give `points` of a time and a value, or set `categories` and give `values`."
        ));
    };
    if requested.len() > MAX_POINTS {
        return Err(format!("{name}: at most {MAX_POINTS} points."));
    }
    let mut bad_times = Vec::new();
    let mut bad_values = 0;
    let mut points = Vec::new();
    for point in requested {
        match (time_ms(&point.t), point.v.filter(|v| v.is_finite())) {
            (Some(x), Some(y)) => points.push(ChartPoint { x, y }),
            (None, _) => bad_times.push(point.t),
            (Some(_), None) => bad_values += 1,
        }
    }
    if !bad_times.is_empty() {
        let shown: Vec<String> = bad_times.iter().take(3).map(|t| format!("{t:?}")).collect();
        dropped.push(format!(
            "{name}: {} points with an unreadable time, such as {}",
            bad_times.len(),
            shown.join(", ")
        ));
    }
    if bad_values > 0 {
        dropped.push(format!("{name}: {bad_values} points without a number"));
    }
    points.sort_by(|a, b| a.x.total_cmp(&b.x));
    Ok(points)
}

/// Unix milliseconds of a date or an ISO 8601 time. One without an offset
/// is read as UTC.
fn time_ms(text: &str) -> Option<f64> {
    let text = text.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp_millis() as f64);
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(time) = NaiveDateTime::parse_from_str(text, format) {
            return Some(time.and_utc().timestamp_millis() as f64);
        }
    }
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d"))
        .ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::StockInfo;

    fn args(json: Value) -> ChartArgs {
        serde_json::from_value(json).expect("test arguments are valid")
    }

    #[test]
    fn category_chart_keeps_order_and_reports_gaps() {
        let mut dropped = Vec::new();
        let chart = build(
            args(json!({
                "title": " Rainfall ", "kind": "bar", "unit": "mm",
                "categories": ["Jan", "Feb", "Mar"],
                "series": [{ "name": "Lyon", "values": [50.5, null, 61.0] }]
            })),
            &mut dropped,
        )
        .expect("a valid chart");
        assert_eq!(chart.title, "Rainfall");
        assert_eq!((chart.kind, chart.x), (ChartKind::Bar, ChartAxis::Category));
        assert_eq!(chart.categories, ["Jan", "Feb", "Mar"]);
        assert_eq!(
            chart.series[0].points,
            [
                ChartPoint { x: 0.0, y: 50.5 },
                ChartPoint { x: 2.0, y: 61.0 }
            ]
        );
        assert_eq!(dropped, ["Lyon: 1 missing values"]);
    }

    #[test]
    fn values_must_match_the_categories() {
        let result = build(
            args(json!({
                "title": "T", "categories": ["a", "b"],
                "series": [{ "name": "s", "values": [1.0] }]
            })),
            &mut Vec::new(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn time_points_parse_sort_and_drop_bad_ones() {
        let mut dropped = Vec::new();
        let chart = build(
            args(json!({
                "title": "Users",
                "series": [{ "name": "", "points": [
                    { "t": "2026-10-02", "v": 2.0 },
                    { "t": "2026-10-01T12:00:00Z", "v": 1.0 },
                    { "t": "yesterday", "v": 3.0 },
                    { "t": "2026-10", "v": 4.0 }
                ] }]
            })),
            &mut dropped,
        )
        .expect("a valid chart");
        assert_eq!(chart.x, ChartAxis::Time);
        assert_eq!(chart.kind, ChartKind::Line);
        assert_eq!(chart.series[0].name, "Series 1");
        let xs: Vec<f64> = chart.series[0].points.iter().map(|p| p.x).collect();
        assert_eq!(
            xs,
            [
                1_790_812_800_000.0,
                1_790_856_000_000.0,
                1_790_899_200_000.0
            ]
        );
        assert_eq!(dropped.len(), 1, "{dropped:?}");
    }

    #[test]
    fn too_many_series_or_nothing_valid_fails() {
        let series: Vec<Value> = (0..5)
            .map(
                |i| json!({ "name": format!("s{i}"), "points": [{ "t": "2026-01-01", "v": 1.0 }] }),
            )
            .collect();
        assert!(
            build(
                args(json!({ "title": "T", "series": series })),
                &mut Vec::new()
            )
            .is_err()
        );
        assert!(
            build(
                args(json!({ "title": "T", "series": [{ "name": "s", "points": [{ "t": "soon", "v": 1.0 }] }] })),
                &mut Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn output_is_the_proto_chart() {
        let chart = build(
            args(json!({
                "title": "T", "categories": ["a"], "series": [{ "name": "s", "values": [1.0] }]
            })),
            &mut Vec::new(),
        )
        .expect("a valid chart");
        let shown = Shown {
            action: CHART,
            chart,
            summary: String::new(),
            dropped: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(&shown).expect("serializes"),
            json!({
                "action": "show_chart",
                "chart": {
                    "title": "T", "kind": "line", "x": "category", "categories": ["a"],
                    "unit": "", "series": [{ "name": "s", "points": [{ "x": 0.0, "y": 1.0 }] }],
                    "stock": null, "source": ""
                }
            })
        );
    }

    #[test]
    fn summary_gives_price_and_change() {
        let history = History {
            info: StockInfo {
                symbol: "ACME".to_owned(),
                name: "Acme Corp.".to_owned(),
                exchange: "NasdaqGS".to_owned(),
                currency: "USD".to_owned(),
                range: ChartRange::Month,
                last: 110.0,
                previous: 100.0,
            },
            points: Vec::new(),
        };
        assert_eq!(
            summary(ChartRange::Month, &[history]),
            "Acme Corp. (ACME): last 110.00 USD, +10.00 (+10.00%) over 1mo from 100.00, on NasdaqGS."
        );
    }
}
