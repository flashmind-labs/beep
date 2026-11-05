//! What went wrong, in words the user sees.

/// A dictation that did not produce text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DictationError {
    #[error("Pick a provider for dictation in Assistant.")]
    Off,
    #[error("Add a {0} key in Assistant first.")]
    NoKey(&'static str),
    #[error("{0} refused the key. Check it in Assistant.")]
    Refused(&'static str),
    #[error("{0} does not know that model. Pick another in Assistant.")]
    BadModel(&'static str),
    #[error("{0} is limiting requests. Try again in a moment.")]
    Busy(&'static str),
    #[error("{0} had an error ({1}). Try again.")]
    Provider(&'static str, u16),
    #[error("Could not reach {0}.")]
    Network(&'static str),
    #[error("{0} did not answer as expected.")]
    Unreadable(&'static str),
}

impl DictationError {
    /// Reads what flashmind's providers report: `STT API error (401 Unauthorized): ...` for a
    /// refused request, and `STT request failed` when nothing came back.
    pub(crate) fn from_report(provider: &'static str, report: &str) -> Self {
        const MARK: &str = "STT API error (";
        if let Some((_, rest)) = report.split_once(MARK) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            return match digits.parse::<u16>() {
                Ok(401 | 403) => Self::Refused(provider),
                Ok(404) => Self::BadModel(provider),
                Ok(429) => Self::Busy(provider),
                Ok(status) => Self::Provider(provider, status),
                Err(_) => Self::Unreadable(provider),
            };
        }
        if report.contains("STT request failed") {
            Self::Network(provider)
        } else {
            Self::Unreadable(provider)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(report: &str) -> DictationError {
        DictationError::from_report("OpenAI", report)
    }

    #[test]
    fn provider_errors_become_short_messages() {
        assert_eq!(
            read("STT API error (401 Unauthorized): {\"error\":{}}"),
            DictationError::Refused("OpenAI")
        );
        assert_eq!(
            read("STT API error (404 Not Found): no"),
            DictationError::BadModel("OpenAI")
        );
        assert_eq!(
            read("STT API error (429 Too Many Requests): slow"),
            DictationError::Busy("OpenAI")
        );
        assert_eq!(
            read("STT API error (503 Service Unavailable): down"),
            DictationError::Provider("OpenAI", 503)
        );
        assert_eq!(
            read("STT request failed: error sending request"),
            DictationError::Network("OpenAI")
        );
        assert_eq!(
            read("Failed to parse transcription response: eof"),
            DictationError::Unreadable("OpenAI")
        );
    }

    #[test]
    fn a_message_never_carries_the_providers_body() {
        let shown = read("STT API error (400 Bad Request): body-text").to_string();
        assert!(!shown.contains("body-text"));
        assert_eq!(shown, "OpenAI had an error (400). Try again.");
    }
}
