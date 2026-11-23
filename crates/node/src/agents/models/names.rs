//! Readable names for model ids that come without one, such as flsh's
//! `openrouter:deepseek/deepseek-v4.1-flash-1017`, which reads
//! `DeepSeek V4.1 Flash 1017` from `openrouter`.

/// Words with a fixed spelling, in lower case and as written.
const WORDS: &[(&str, &str)] = &[
    ("claude", "Claude"),
    ("cli", "CLI"),
    ("codestral", "Codestral"),
    ("deepseek", "DeepSeek"),
    ("fim", "FIM"),
    ("glm", "GLM"),
    ("gpt", "GPT"),
    ("grok", "Grok"),
    ("kimi", "Kimi"),
    ("llama", "Llama"),
    ("minimax", "MiniMax"),
    ("mistral", "Mistral"),
    ("ocr", "OCR"),
    ("openai", "OpenAI"),
    ("oss", "OSS"),
    ("qwen", "Qwen"),
    ("tts", "TTS"),
    ("vl", "VL"),
    ("zai", "Z.ai"),
];

/// `id` split into the provider flsh names before its first `:` and a
/// readable name of the rest. An id without a provider gives `None`.
pub(super) fn flsh_name(id: &str) -> (Option<String>, String) {
    match id.split_once(':') {
        Some((provider, model)) if !provider.is_empty() && !model.is_empty() => {
            (Some(provider.to_owned()), display_name(model))
        }
        _ => (None, display_name(id)),
    }
}

/// A readable name of the model id `model`: the maker before a `/` left
/// out, words split on `-`, `_` and spaces and title cased, known brands
/// spelled their way, version parts joined (`v4-1` reads `V4.1`), sizes
/// such as `8b` upper cased, dates and builds kept, and a variant after a
/// `:` (OpenRouter's `:batch`) put in parentheses.
pub(super) fn display_name(model: &str) -> String {
    let (model, variant) = match model.rsplit_once(':') {
        Some((model, variant)) if !model.is_empty() && !variant.is_empty() => {
            (model, Some(variant))
        }
        _ => (model, None),
    };
    let model = model.rsplit('/').next().unwrap_or(model);
    let parts: Vec<&str> = model
        .split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .collect();

    let mut words: Vec<String> = Vec::with_capacity(parts.len());
    let mut joins = Joins::Nothing;

    for part in parts {
        let short_number = part.len() <= 2 && part.bytes().all(|b| b.is_ascii_digit());

        if short_number
            && joins != Joins::Nothing
            && let Some(last) = words.last_mut()
        {
            last.push(match joins {
                Joins::Date => '-',
                _ => '.',
            });
            last.push_str(part);
            continue;
        }

        joins = Joins::after(part);
        words.push(word(part));
    }

    if words.is_empty() {
        return model.to_owned();
    }

    let mut name = words.join(" ");

    if let Some(variant) = variant {
        name.push_str(&format!(" ({})", word(variant)));
    }

    name
}

/// What a short number after a part joins it as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Joins {
    Nothing,
    /// `4-1` reads `4.1`.
    Version,
    /// `2024-08-06` and `02-15` stay dates.
    Date,
}

impl Joins {
    /// What a short number after `part` is: the minor version of a part
    /// that ends in one or two digits, such as `4`, `v4` or `qwen3`, but
    /// not of `4.1` or a size such as `8b`; the rest of a date after a year
    /// or a number with a leading zero.
    fn after(part: &str) -> Self {
        let digits = part.bytes().rev().take_while(u8::is_ascii_digit).count();

        if digits == part.len() && (digits >= 4 || part.starts_with('0')) {
            return Self::Date;
        }

        if (1..=2).contains(&digits) && !part[..part.len() - digits].ends_with('.') {
            return Self::Version;
        }

        Self::Nothing
    }
}

/// One word as written in a name.
fn word(part: &str) -> String {
    let lower = part.to_ascii_lowercase();

    if let Some((_, known)) = WORDS.iter().find(|(word, _)| *word == lower) {
        return (*known).to_owned();
    }

    if let Some((brand, known)) = WORDS.iter().find(|(word, _)| lower.starts_with(word))
        && lower[brand.len()..].starts_with(|c: char| c.is_ascii_digit())
    {
        return format!("{known}{}", &part[brand.len()..]);
    }

    if is_size(&lower) {
        return lower.to_ascii_uppercase();
    }

    if let Some(letter) = letter_version(&lower) {
        return match letter {
            'o' => lower,
            _ => lower.to_ascii_uppercase(),
        };
    }

    let mut chars = part.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A parameter count such as `8b`, `1.5b`, `a22b` or `400m`.
fn is_size(word: &str) -> bool {
    let body = word.strip_prefix('a').unwrap_or(word);
    let Some(number) = body.strip_suffix(['b', 'm', 'k']) else {
        return false;
    };
    number.starts_with(|c: char| c.is_ascii_digit())
        && number.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// The letter of a letter and a version, such as `v4`, `r1`, `k2` or
/// `v3.2`. OpenAI's `o3` keeps its lower case `o`.
fn letter_version(word: &str) -> Option<char> {
    let mut chars = word.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str();
    (rest.starts_with(|c: char| c.is_ascii_digit())
        && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
    .then_some(letter)
}

#[cfg(test)]
#[path = "../../tests/agents_models_names.rs"]
mod tests;
