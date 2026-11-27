//! What the agents wrote into their own stores and logs: the sessions they
//! kept, their transcripts, what they spent and the machine's processes.
//! Readers only, with caches that last while a file is unchanged; nothing
//! here follows, watches or decides.

pub(crate) mod claude;
pub(crate) mod claude_dir;
pub(crate) mod found;
pub(crate) mod picture;
pub(crate) mod prices;
pub(crate) mod processes;
pub(crate) mod spend;
pub(crate) mod subagent;
pub(crate) mod tool_details;
pub(crate) mod window;

/// Records how long `stage` of a transcript read took since `started`:
/// finding the file, reading, parsing, subagents, encoding or sending.
pub(crate) fn transcript_stage(stage: &'static str, started: std::time::Instant) {
    metrics::histogram!("beep_transcript_stage_seconds", "stage" => stage)
        .record(started.elapsed().as_secs_f64());
}
