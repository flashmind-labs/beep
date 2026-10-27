//! Lists the OpenRouter models a Beep run could use: ones that call tools,
//! answer in text and cost at most the given USD per million output tokens,
//! newest first. Copy an id into `model_choices` as `openrouter:<id>`.

use anyhow::{Context, Result, ensure};
use chrono::DateTime;
use serde::Deserialize;

const USAGE: &str = "usage: beep-models [MAX_OUTPUT_PRICE] [FILTER]; MAX_OUTPUT_PRICE in USD per million output tokens, FILTER matches part of the id";
/// The price limit when none is given, in USD per million output tokens.
const MAX_OUTPUT_PRICE: f64 = 4.0;
const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

#[derive(Deserialize)]
struct Listing {
    data: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    created: i64,
    context_length: Option<u64>,
    pricing: Pricing,
    architecture: Architecture,
    #[serde(default)]
    supported_parameters: Vec<String>,
}

#[derive(Deserialize)]
struct Pricing {
    prompt: String,
    completion: String,
}

#[derive(Deserialize)]
struct Architecture {
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() <= 2, USAGE);
    let max: f64 = match args.first() {
        Some(max) => max.parse().context(USAGE)?,
        None => MAX_OUTPUT_PRICE,
    };
    let filter = args.get(1).map(|f| f.to_lowercase());

    let listing: Listing = reqwest::get(MODELS_URL)
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut models: Vec<_> = listing
        .data
        .into_iter()
        .filter(|m| m.supported_parameters.iter().any(|p| p == "tools"))
        .filter(|m| m.architecture.output_modalities.iter().any(|o| o == "text"))
        // Batch variants are not for chat, and `~` ids are aliases of listed models.
        .filter(|m| !m.id.ends_with(":batch") && !m.id.starts_with('~'))
        .filter(|m| filter.as_ref().is_none_or(|f| m.id.contains(f.as_str())))
        .filter_map(|m| {
            let input = per_million(&m.pricing.prompt)?;
            let output = per_million(&m.pricing.completion)?;
            (output > 0.0 && output <= max).then_some((m, input, output))
        })
        .collect();
    models.sort_by_key(|(m, ..)| -m.created);

    println!(
        "{:<48} {:>6} {:>6} {:>8}  {:<10} notes",
        "id", "in", "out", "context", "released"
    );
    for (m, input, output) in &models {
        let released = DateTime::from_timestamp(m.created, 0)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let mut notes = Vec::new();
        if m.architecture.input_modalities.iter().any(|i| i == "image") {
            notes.push("images");
        }
        if m.supported_parameters.iter().any(|p| p == "reasoning") {
            notes.push("reasoning");
        }
        println!(
            "{:<48} {:>6.2} {:>6.2} {:>7}k  {:<10} {}",
            m.id,
            input,
            output,
            m.context_length.unwrap_or(0) / 1000,
            released,
            notes.join(", ")
        );
    }
    println!("{} models; prices in USD per million tokens", models.len());
    Ok(())
}

/// A per-token price string as USD per million tokens.
fn per_million(price: &str) -> Option<f64> {
    price.parse::<f64>().ok().map(|p| p * 1_000_000.0)
}
