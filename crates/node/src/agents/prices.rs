//! What tokens cost at the providers' list prices (`price`), for sessions
//! whose logs give only token counts (see `super::spend`). A model not listed
//! stays unpriced: its tokens are counted, not costed.

use serde::{Deserialize, Serialize};

/// One model's prices, per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Price {
    pub(crate) input: f64,
    pub(crate) output: f64,
    pub(crate) cache_read: f64,
    /// A write to a five-minute cache, or the only kind the provider has.
    pub(crate) cache_write: f64,
    /// A write to Anthropic's one-hour cache: twice the input price.
    pub(crate) cache_write_long: f64,
}

/// Tokens of one request or a sum of them, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Tokens {
    /// Input read fresh, not from the cache.
    pub(crate) input: u64,
    /// Output, reasoning included.
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    /// Written to a five-minute cache, or the only kind there is.
    pub(crate) cache_write: u64,
    /// Written to Anthropic's one-hour cache.
    pub(crate) cache_write_long: u64,
}

impl Tokens {
    pub(crate) fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
            .saturating_add(self.cache_write_long)
    }

    pub(crate) fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.cache_write_long = self.cache_write_long.saturating_add(other.cache_write_long);
    }
}

impl Price {
    /// Anthropic's: cache reads as given, writes at 1.25 and 2 times input.
    const fn anthropic(input: f64, output: f64, cache_read: f64) -> Self {
        Self {
            input,
            output,
            cache_read,
            cache_write: input * 1.25,
            cache_write_long: input * 2.0,
        }
    }

    /// Any other provider's: one kind of cache write.
    const fn other(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Self {
            input,
            output,
            cache_read,
            cache_write,
            cache_write_long: cache_write,
        }
    }

    /// What `tokens` cost, in US dollars.
    pub(crate) fn cost(&self, tokens: &Tokens) -> f64 {
        let part = |count: u64, per_million: f64| count as f64 * per_million / 1_000_000.0;
        part(tokens.input, self.input)
            + part(tokens.output, self.output)
            + part(tokens.cache_read, self.cache_read)
            + part(tokens.cache_write, self.cache_write)
            + part(tokens.cache_write_long, self.cache_write_long)
    }
}

/// Each model by its name as `normalize` writes it, in US dollars per
/// million tokens: the providers' API prices and OpenRouter's catalog as of
/// November 2026.
const PRICES: &[(&str, Price)] = &[
    ("claude-fable-5-1", Price::anthropic(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", Price::anthropic(10.0, 50.0, 0.25)),
    ("claude-fable-5", Price::anthropic(10.0, 50.0, 1.0)),
    ("claude-mythos-5", Price::anthropic(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", Price::anthropic(4.0, 20.0, 0.2)),
    ("claude-opus-5", Price::anthropic(5.0, 25.0, 0.5)),
    ("claude-opus-4-8", Price::anthropic(5.0, 25.0, 0.5)),
    ("claude-opus-4-7", Price::anthropic(5.0, 25.0, 0.5)),
    ("claude-opus-4-6", Price::anthropic(5.0, 25.0, 0.5)),
    ("claude-opus-4-5", Price::anthropic(5.0, 25.0, 0.5)),
    ("claude-opus-4-1", Price::anthropic(15.0, 75.0, 1.5)),
    ("claude-opus-4", Price::anthropic(15.0, 75.0, 1.5)),
    ("claude-sonnet-5-5", Price::anthropic(2.0, 10.0, 0.2)),
    ("claude-sonnet-5", Price::anthropic(2.0, 10.0, 0.2)),
    ("claude-sonnet-4-6", Price::anthropic(3.0, 15.0, 0.3)),
    ("claude-sonnet-4-5", Price::anthropic(3.0, 15.0, 0.3)),
    ("claude-sonnet-4", Price::anthropic(3.0, 15.0, 0.3)),
    ("claude-haiku-5-5", Price::anthropic(0.1, 0.5, 0.01)),
    ("claude-haiku-4-5", Price::anthropic(1.0, 5.0, 0.1)),
    ("gpt-6-1-sol", Price::other(2.0, 10.0, 0.1, 2.5)),
    ("gpt-6-1-sol-pro", Price::other(2.0, 10.0, 0.1, 2.5)),
    ("gpt-6-1-terra", Price::other(1.0, 5.0, 0.05, 1.25)),
    ("gpt-6-1-terra-pro", Price::other(1.0, 5.0, 0.05, 1.25)),
    ("gpt-6-astra", Price::other(10.0, 50.0, 1.0, 12.5)),
    ("gpt-6-astra-pro", Price::other(10.0, 50.0, 1.0, 12.5)),
    ("gpt-6-sol", Price::other(2.0, 10.0, 0.2, 2.5)),
    ("gpt-6-sol-pro", Price::other(2.0, 10.0, 0.2, 2.5)),
    ("gpt-6-luna", Price::other(0.1, 0.5, 0.01, 0.125)),
    ("gpt-6-luna-pro", Price::other(0.1, 0.5, 0.01, 0.125)),
    ("gpt-5-6-sol", Price::other(4.0, 20.0, 0.4, 5.0)),
    ("gpt-5-6-terra", Price::other(2.0, 12.0, 0.2, 2.5)),
    ("gpt-5-6-luna", Price::other(0.2, 1.2, 0.02, 0.25)),
    ("gpt-5-5", Price::other(5.0, 30.0, 0.5, 5.0)),
    ("gpt-5-4", Price::other(2.5, 15.0, 0.25, 2.5)),
    ("gpt-5-4-mini", Price::other(0.75, 4.5, 0.075, 0.75)),
    ("gpt-5-3-codex", Price::other(1.75, 14.0, 0.175, 1.75)),
    ("gpt-5-2", Price::other(1.75, 14.0, 0.175, 1.75)),
    ("gpt-5-2-codex", Price::other(1.75, 14.0, 0.175, 1.75)),
    ("gpt-5-1", Price::other(1.25, 10.0, 0.125, 1.25)),
    ("gpt-5-1-codex", Price::other(1.25, 10.0, 0.125, 1.25)),
    ("gpt-5-1-codex-max", Price::other(1.25, 10.0, 0.125, 1.25)),
    ("gpt-5-1-codex-mini", Price::other(0.25, 2.0, 0.025, 0.25)),
    ("gpt-5", Price::other(1.25, 10.0, 0.125, 1.25)),
    ("gpt-5-codex", Price::other(1.25, 10.0, 0.125, 1.25)),
    ("gpt-5-mini", Price::other(0.25, 2.0, 0.025, 0.25)),
];

/// The price of `model` as an agent names it, such as `claude-opus-5-5`,
/// `claude-opus-4-1-20250805`, `openai/gpt-6.1-sol` or
/// `openrouter:anthropic/claude-sonnet-5.5`.
pub(crate) fn price(model: &str) -> Option<Price> {
    let name = normalize(model);
    PRICES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, price)| *price)
}

/// `model` without its provider or maker, a date or a `[1m]` suffix, in
/// lower case with dots as dashes.
fn normalize(model: &str) -> String {
    let model = model.rsplit(':').next().unwrap_or(model);
    let model = model.rsplit('/').next().unwrap_or(model);
    let model = model.split('[').next().unwrap_or(model);
    let mut name = model.trim().to_ascii_lowercase().replace('.', "-");
    if let Some((rest, date)) = name.rsplit_once('-')
        && date.len() == 8
        && date.bytes().all(|b| b.is_ascii_digit())
    {
        name.truncate(rest.len());
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_are_found_by_the_names_agents_give() {
        assert_eq!(price("claude-opus-5-5").map(|p| p.input), Some(4.0));
        assert_eq!(
            price("claude-opus-4-1-20250805").map(|p| p.input),
            Some(15.0)
        );
        assert_eq!(price("claude-sonnet-5-5[1m]").map(|p| p.input), Some(2.0));
        assert_eq!(
            price("openrouter:anthropic/claude-sonnet-5.5").map(|p| p.input),
            Some(2.0)
        );
        assert_eq!(price("gpt-6.1-sol").map(|p| p.output), Some(10.0));
        assert_eq!(price("openai/GPT-6-Luna").map(|p| p.input), Some(0.1));
        assert_eq!(price("claude-opus-9"), None);
        assert_eq!(price("<synthetic>"), None);
    }
}
