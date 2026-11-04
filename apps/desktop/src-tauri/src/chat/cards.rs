//! Recommendation cards. The `show_cards` tool lists places, films and
//! shows, events or any other kind, and they show as a row of cards under the
//! reply. Every card has the same base; a place, media or event adds its own
//! part. Clicking a card opens its page, or a place in Maps.

use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct Shown {
    cards: Vec<serde_json::Value>,
}

/// One card of a `show_cards` result, as the server checked it.
#[derive(Debug, Deserialize)]
struct Card {
    kind: String,
    title: String,
    url: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    facts: Vec<String>,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    reviews: Option<u64>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    place: Option<Place>,
    #[serde(default)]
    media: Option<Media>,
    #[serde(default)]
    event: Option<Event>,
}

#[derive(Debug, Deserialize)]
struct Place {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    distance_m: Option<u32>,
    #[serde(default)]
    hours: Option<String>,
    #[serde(default)]
    open_now: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct Media {
    #[serde(default)]
    year: Option<u16>,
    #[serde(default)]
    creator: Option<String>,
    #[serde(default)]
    watch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Event {
    /// RFC 3339 with the event's own offset, as the server wrote it.
    start: String,
    #[serde(default)]
    venue: Option<String>,
}

/// The row of cards a `show_cards` result lists, or None when it has none.
/// An entry that does not parse is left out.
pub fn shown(output: &str) -> Option<String> {
    let shown: Shown = serde_json::from_str(output).ok()?;
    let cards: Vec<String> = shown
        .cards
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Card>(entry).ok())
        .filter(|c| !c.title.trim().is_empty() && web_link(&c.url))
        .map(|c| c.html())
        .collect();
    (!cards.is_empty()).then(|| format!(r#"<div class="drecs">{}</div>"#, cards.concat()))
}

impl Card {
    fn html(&self) -> String {
        let when = self.event.as_ref().and_then(|e| When::parse(&e.start));
        let picture = match (&self.media, self.kind.as_str()) {
            (Some(media), "media") => self.poster(media),
            _ => self.picture(when.as_ref()),
        };
        let subtitle = self
            .subtitle
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.fallback_subtitle())
            .map(|s| format!(r#"<span class="drec-sub">{}</span>"#, escape(&s)))
            .unwrap_or_default();
        let mut facts: Vec<String> = self
            .facts
            .iter()
            .filter(|f| !f.trim().is_empty())
            .map(|f| escape(f))
            .collect();
        if let Some(m) = self.place.as_ref().and_then(|p| p.distance_m) {
            facts.push(distance(m));
        }
        let facts = if facts.is_empty() {
            String::new()
        } else {
            let chips: String = facts.iter().map(|f| format!("<i>{f}</i>")).collect();
            format!(r#"<span class="drec-facts">{chips}</span>"#)
        };
        let status = self.status(when.as_ref());
        let mut meta = Vec::new();
        if let Some(rating) = self.rating {
            let reviews = self
                .reviews
                .map(|r| format!(" ({})", grouped(r)))
                .unwrap_or_default();
            meta.push(format!("{rating:.1} &#9733;{reviews}"));
        }
        if let Some(note) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
            meta.push(escape(note));
        }
        let meta = if meta.is_empty() {
            String::new()
        } else {
            format!(
                r#"<span class="drec-meta">{}</span>"#,
                meta.join(" &middot; ")
            )
        };
        format!(
            r#"<a class="drec" href="{href}" aria-label="{label}">{picture}<span class="drec-info"><span class="drec-title">{title}</span>{subtitle}{facts}{status}{meta}</span></a>"#,
            href = escape(&self.link()),
            label = escape(&self.title),
            title = escape(&self.title),
        )
    }

    /// Where a click goes: a place opens in Maps, anything else its page.
    fn link(&self) -> String {
        let Some(place) = self.place.as_ref().filter(|_| self.kind == "place") else {
            return self.url.clone();
        };
        let query = match place.address.as_deref().filter(|a| !a.trim().is_empty()) {
            Some(address) => format!("{}, {address}", self.title),
            None => self.title.clone(),
        };
        let mut params = vec![("q", query)];
        if let (Some(lat), Some(lng)) = (place.latitude, place.longitude) {
            params.push(("ll", format!("{lat},{lng}")));
        }
        Url::parse_with_params("https://maps.apple.com/", params)
            .map_or_else(|_| self.url.clone(), String::from)
    }

    fn picture(&self, when: Option<&When>) -> String {
        let image = self
            .image
            .as_deref()
            .filter(|url| web_link(url))
            .map(|url| format!(r#"<img src="{}" alt="" loading="lazy">"#, escape(url)))
            .unwrap_or_default();
        let badge = when
            .map(|w| {
                format!(
                    r#"<span class="drec-day"><small>{}</small><b>{}</b></span>"#,
                    w.weekday.to_ascii_uppercase(),
                    w.day
                )
            })
            .unwrap_or_default();
        format!(r#"<span class="drec-img">{image}{badge}</span>"#)
    }

    fn poster(&self, media: &Media) -> String {
        let image = self
            .image
            .as_deref()
            .filter(|url| web_link(url))
            .map(|url| format!(r#"<img src="{}" alt="" loading="lazy">"#, escape(url)))
            .unwrap_or_default();
        let watch = media
            .watch
            .as_deref()
            .filter(|w| !w.trim().is_empty())
            .map(|w| format!("<small>Watch on</small><b>{}</b>", escape(w)))
            .unwrap_or_default();
        format!(
            r#"<span class="drec-poster"><span class="drec-art">{image}</span><span class="drec-watch">{watch}</span></span>"#
        )
    }

    /// A line under the title when the model gave none.
    fn fallback_subtitle(&self) -> Option<String> {
        if let Some(media) = &self.media {
            let parts: Vec<String> = media
                .year
                .map(|y| y.to_string())
                .into_iter()
                .chain(media.creator.clone())
                .collect();
            return (!parts.is_empty()).then(|| parts.join(" · "));
        }
        if let Some(event) = &self.event {
            return event.venue.clone();
        }
        self.place.as_ref().and_then(|p| p.address.clone())
    }

    /// Today's hours for a place, or the date for an event.
    fn status(&self, when: Option<&When>) -> String {
        if let Some(w) = when {
            return format!(
                r#"<span class="drec-status">{} {} {}, {}</span>"#,
                w.weekday, w.day, w.month, w.time
            );
        }
        let Some(place) = &self.place else {
            return String::new();
        };
        let text = match (place.hours.as_deref(), place.open_now) {
            (Some(hours), _) if !hours.trim().is_empty() => escape(hours),
            (_, Some(true)) => "Open now".to_owned(),
            (_, Some(false)) => "Closed now".to_owned(),
            _ => return String::new(),
        };
        let open = if place.open_now == Some(true) {
            " open"
        } else {
            ""
        };
        format!(r#"<span class="drec-status place{open}">{text}</span>"#)
    }
}

/// An event's start as the card writes it, in the event's own time.
struct When {
    weekday: &'static str,
    day: u32,
    month: &'static str,
    time: String,
}

impl When {
    /// Reads `2026-10-18T21:00:00+02:00`.
    fn parse(start: &str) -> Option<Self> {
        let (date, rest) = start.split_once('T')?;
        let mut parts = date.split('-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        let time = rest
            .get(..5)
            .filter(|t| t.as_bytes().get(2) == Some(&b':'))?;
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let month_name = MONTHS.get(month.checked_sub(1)? as usize)?;
        if !(1..=31).contains(&day) {
            return None;
        }
        Some(Self {
            weekday: DAYS[weekday(year, month, day)],
            day,
            month: month_name,
            time: time.to_owned(),
        })
    }
}

/// The day of the week, 0 for Sunday (Sakamoto's method).
fn weekday(year: i32, month: u32, day: u32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let d = y + y / 4 - y / 100 + y / 400 + T[(month - 1) as usize] + day as i32;
    d.rem_euclid(7) as usize
}

/// `650 m` or `1.2 km`.
fn distance(meters: u32) -> String {
    if meters < 1000 {
        format!("{} m", (meters + 5) / 10 * 10)
    } else if meters < 10_000 {
        format!("{:.1} km", f64::from(meters) / 1000.0)
    } else {
        format!("{} km", (meters + 500) / 1000)
    }
}

/// A count with a comma every three places, `1,204` for `1204`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn web_link(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
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
    fn a_place_opens_in_maps_with_its_hours_and_distance() {
        let output = r#"{"action":"show_cards","cards":[
            {"kind":"place","title":"La <Pubilla>","url":"https://lapubilla.example/","site":"lapubilla.example",
             "subtitle":"Catalan","facts":["€€"],"rating":4.6,"reviews":2310,"note":"Closest",
             "place":{"address":"Plaça de la Llibertat 23","latitude":41.4,"longitude":2.15,"distance_m":648,"hours":"Open until 23:00","open_now":true}},
            {"kind":"place","title":"Bad","url":"javascript:alert(1)","site":"x"},
            "not an object"
        ],"dropped":[]}"#;
        let row = shown(output).expect("one valid card");
        assert_eq!(row.matches(r#"<a class="drec""#).count(), 1, "{row}");
        assert!(row.contains("La &lt;Pubilla&gt;"), "{row}");
        assert!(
            row.contains("https://maps.apple.com/?q=La+%3CPubilla%3E"),
            "{row}"
        );
        assert!(row.contains("&amp;ll=41.4%2C2.15"), "{row}");
        assert!(row.contains("<i>€€</i><i>650 m</i>"), "{row}");
        assert!(
            row.contains(r#"drec-status place open">Open until 23:00"#),
            "{row}"
        );
        assert!(
            row.contains("4.6 &#9733; (2,310) &middot; Closest"),
            "{row}"
        );
        assert!(!row.contains("javascript"), "{row}");
    }

    #[test]
    fn media_and_events_add_their_parts() {
        let output = r#"{"cards":[
            {"kind":"media","title":"Aftersun","url":"https://films.example/aftersun","site":"films.example",
             "media":{"year":2022,"creator":"Charlotte Wells","watch":"Filmin"}},
            {"kind":"event","title":"Trio","url":"https://jazz.example/trio","site":"jazz.example",
             "event":{"start":"2026-10-17T21:00:00+02:00","venue":"Jamboree"}},
            {"kind":"podcast","title":"99% Invisible","url":"https://pod.example/","site":"pod.example"}
        ]}"#;
        let row = shown(output).expect("three cards");
        assert!(
            row.contains("<small>Watch on</small><b>Filmin</b>"),
            "{row}"
        );
        assert!(row.contains("2022 · Charlotte Wells"), "{row}");
        assert!(row.contains("<small>SAT</small><b>17</b>"), "{row}");
        assert!(row.contains("Sat 17 Oct, 21:00"), "{row}");
        assert!(row.contains(r#"href="https://pod.example/""#), "{row}");
        assert!(shown(r#"{"cards":[]}"#).is_none());
        assert!(shown("failed").is_none());
    }

    #[test]
    fn distances_round_like_a_person_would() {
        assert_eq!(distance(648), "650 m");
        assert_eq!(distance(1240), "1.2 km");
        assert_eq!(distance(12_600), "13 km");
    }
}
