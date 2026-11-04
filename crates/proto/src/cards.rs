//! Cards the assistant shows in a chat: routes and charts.

use serde::{Deserialize, Serialize};

/// What a `get_route` call found, as the route card draws it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCard {
    /// One to three ways there, best first, as the model was told them.
    pub routes: Vec<RouteOption>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOption {
    /// Clock times where they happen, like "19:01".
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub transfers: u32,
    /// Whether a leg has live times rather than the timetable's.
    pub live: bool,
    pub legs: Vec<RouteLeg>,
}

/// One walk or ride of a route.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLeg {
    /// The router's mode in lowercase: "walk", "bus", "tram", "subway",
    /// "rail", "ferry" and others. Text rather than an enum so a mode the
    /// router adds later reaches older apps as an unknown ride.
    pub mode: String,
    /// The line's short name, like "G". Empty for a walk.
    pub line: String,
    /// The line's long name, like "Ginza Line". Empty when the feed has none.
    pub name: String,
    /// "#RRGGBB", empty when the feed gives none.
    pub color: String,
    /// "#RRGGBB" for text on `color`, empty when the feed gives none.
    pub text_color: String,
    pub headsign: String,
    /// Stop names. "" stands for the start or the destination themselves.
    pub from: String,
    pub to: String,
    /// The platform or track, empty when unknown.
    pub from_track: String,
    pub to_track: String,
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    /// Stops ridden, counting the last one. Zero for a walk.
    pub stops: u32,
    /// Zero when unknown.
    pub meters: u32,
    /// The leg's shape, simplified to what a small sketch needs.
    pub points: Vec<RoutePoint>,
    /// The station codes signs show, like "G10" in Tokyo. Empty when the
    /// feed has none. Cards saved before codes lack these and `between`.
    #[serde(default)]
    pub from_code: String,
    #[serde(default)]
    pub to_code: String,
    /// The stops ridden through, in order, not counting `from` and `to`.
    #[serde(default)]
    pub between: Vec<RouteStop>,
}

/// A stop a ride passes.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteStop {
    pub name: String,
    /// The station code, empty when the feed has none.
    pub code: String,
    /// The clock time there, like "19:08".
    pub time: String,
    pub at: RoutePoint,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoutePoint {
    pub latitude: f64,
    pub longitude: f64,
}

/// `RouteCard` as wire 2 apps read it from `GetRoute`. Frozen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCardV1 {
    pub routes: Vec<RouteOptionV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteOptionV1 {
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub transfers: u32,
    pub live: bool,
    pub legs: Vec<RouteLegV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteLegV1 {
    pub mode: String,
    pub line: String,
    pub name: String,
    pub color: String,
    pub text_color: String,
    pub headsign: String,
    pub from: String,
    pub to: String,
    pub from_track: String,
    pub to_track: String,
    pub departs: String,
    pub arrives: String,
    pub minutes: u32,
    pub stops: u32,
    pub meters: u32,
    pub points: Vec<RoutePoint>,
}

impl From<RouteCard> for RouteCardV1 {
    fn from(card: RouteCard) -> Self {
        Self {
            routes: card
                .routes
                .into_iter()
                .map(|route| RouteOptionV1 {
                    departs: route.departs,
                    arrives: route.arrives,
                    minutes: route.minutes,
                    transfers: route.transfers,
                    live: route.live,
                    legs: route
                        .legs
                        .into_iter()
                        .map(|leg| RouteLegV1 {
                            mode: leg.mode,
                            line: leg.line,
                            name: leg.name,
                            color: leg.color,
                            text_color: leg.text_color,
                            headsign: leg.headsign,
                            from: leg.from,
                            to: leg.to,
                            from_track: leg.from_track,
                            to_track: leg.to_track,
                            departs: leg.departs,
                            arrives: leg.arrives,
                            minutes: leg.minutes,
                            stops: leg.stops,
                            meters: leg.meters,
                            points: leg.points,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// A chart under a reply, as `show_chart` and `show_stock_chart` return it
/// in their tool result and `StockChart` answers it.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub title: String,
    pub kind: ChartKind,
    /// Whether `x` is a time in Unix milliseconds or an index into
    /// `categories`.
    pub x: ChartAxis,
    /// The labels of a category axis, in order. Empty on a time axis.
    pub categories: Vec<String>,
    /// What the values measure, such as "USD" or "°C". Empty when unknown.
    pub unit: String,
    /// One to four series, each drawn in its own color.
    pub series: Vec<ChartSeries>,
    /// Set on a stock chart, so the app can offer the other ranges.
    pub stock: Option<StockInfo>,
    /// Where the numbers come from, such as "Yahoo Finance". Empty when
    /// the model gave them.
    pub source: String,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub points: Vec<ChartPoint>,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartPoint {
    pub x: f64,
    pub y: f64,
}

/// The quote a stock chart is drawn for.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockInfo {
    pub symbol: String,
    /// The company or fund, such as "Apple Inc.". Empty when unknown.
    pub name: String,
    pub exchange: String,
    pub currency: String,
    pub range: ChartRange,
    /// The latest price.
    pub last: f64,
    /// The close before the range started, which the change is against.
    pub previous: f64,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Line,
    Area,
    Bar,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartAxis {
    Time,
    Category,
}

/// How far back a stock chart reaches.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartRange {
    #[serde(rename = "1d")]
    Day,
    #[serde(rename = "5d")]
    Week,
    #[serde(rename = "1mo")]
    Month,
    #[serde(rename = "6mo")]
    HalfYear,
    #[serde(rename = "ytd")]
    YearToDate,
    #[serde(rename = "1y")]
    Year,
    #[serde(rename = "5y")]
    FiveYears,
    Max,
}

impl ChartRange {
    /// The name it has in JSON and in Yahoo Finance's `range`, such as "1mo".
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "5d",
            Self::Month => "1mo",
            Self::HalfYear => "6mo",
            Self::YearToDate => "ytd",
            Self::Year => "1y",
            Self::FiveYears => "5y",
            Self::Max => "max",
        }
    }
}
