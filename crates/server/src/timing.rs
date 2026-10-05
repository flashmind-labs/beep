//! Where the time of a turn goes, logged once per run to find slow first tokens.

use flashmind::types::TokenUsage;
use std::{
    fmt,
    time::{Duration, Instant},
};

/// Timestamps of one run, measured from when the server started on it.
pub struct TurnTimer {
    started: Instant,
    /// Building the time, location and apps block.
    context: Duration,
    /// Everything before the first model call: loading the conversation,
    /// tools, apps and context, and waiting for image descriptions.
    prepared: Option<Duration>,
    /// When the model call in flight was sent, until its first output.
    call_sent: Option<Instant>,
    /// Time from sending each model call to its first output.
    waits: Vec<Duration>,
    first_reasoning: Option<Duration>,
    first_text: Option<Duration>,
    /// When the model and tools were done, before memories were learned.
    streamed: Option<Duration>,
    prompt_tokens: u32,
    cache_read_tokens: u32,
    completion_tokens: u32,
}

/// What the model sent first on a call.
pub enum Output {
    Reasoning,
    Text,
    Tool,
}

impl TurnTimer {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
            context: Duration::ZERO,
            prepared: None,
            call_sent: None,
            waits: Vec::new(),
            first_reasoning: None,
            first_text: None,
            streamed: None,
            prompt_tokens: 0,
            cache_read_tokens: 0,
            completion_tokens: 0,
        }
    }

    pub fn context_built(&mut self, took: Duration) {
        self.context = took;
    }

    /// The first model call is about to be sent.
    pub fn prepared(&mut self) {
        self.prepared = Some(self.started.elapsed());
        self.call_sent = Some(Instant::now());
    }

    /// Every running tool has returned, so the agent sends the next model call.
    pub fn tools_done(&mut self) {
        self.call_sent = Some(Instant::now());
    }

    pub fn output(&mut self, output: Output) {
        if let Some(sent) = self.call_sent.take() {
            self.waits.push(sent.elapsed());
        }
        let elapsed = self.started.elapsed();
        match output {
            Output::Reasoning => {
                self.first_reasoning.get_or_insert(elapsed);
            }
            Output::Text => {
                self.first_text.get_or_insert(elapsed);
            }
            Output::Tool => {}
        }
    }

    pub fn usage(&mut self, usage: &TokenUsage) {
        // The last call's prompt is the largest; output adds up across calls.
        self.prompt_tokens = usage.prompt_tokens;
        self.cache_read_tokens = usage.cache_read_tokens;
        self.completion_tokens += usage.completion_tokens;
    }

    pub fn streamed(&mut self) {
        self.streamed = Some(self.started.elapsed());
    }
}

struct Millis(Option<Duration>);

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(duration) => write!(f, "{}", duration.as_millis()),
            None => f.write_str("-"),
        }
    }
}

impl fmt::Display for TurnTimer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let waits: Vec<String> = self
            .waits
            .iter()
            .map(|wait| wait.as_millis().to_string())
            .collect();
        write!(
            f,
            "context_ms={} prepare_ms={} first_reasoning_ms={} first_text_ms={} \
             call_waits_ms=[{}] streamed_ms={} total_ms={} \
             prompt_tokens={} cached_tokens={} completion_tokens={}",
            self.context.as_millis(),
            Millis(self.prepared),
            Millis(self.first_reasoning),
            Millis(self.first_text),
            waits.join(","),
            Millis(self.streamed),
            self.started.elapsed().as_millis(),
            self.prompt_tokens,
            self.cache_read_tokens,
            self.completion_tokens,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_one_wait_per_model_call() {
        let mut timer = TurnTimer::start();
        timer.prepared();
        timer.output(Output::Reasoning);
        timer.output(Output::Tool);
        timer.tools_done();
        timer.output(Output::Text);
        timer.output(Output::Text);

        assert_eq!(timer.waits.len(), 2);
        assert!(timer.first_reasoning.is_some());
        assert!(timer.first_text >= timer.first_reasoning);
    }

    #[test]
    fn adds_up_output_tokens_across_calls() {
        let mut timer = TurnTimer::start();
        let call = |prompt, completion| TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        timer.usage(&call(1000, 50));
        timer.usage(&call(1200, 30));

        assert_eq!(timer.prompt_tokens, 1200);
        assert_eq!(timer.completion_tokens, 80);
        let line = timer.to_string();
        assert!(line.contains("first_text_ms=-"), "{line}");
    }
}
