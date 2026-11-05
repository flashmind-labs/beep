//! Finding string values in one JSON line without parsing and writing it
//! again, so a rewrite leaves every other byte as it was.

use std::ops::Range;

/// One step down into a value.
#[derive(Clone, Copy)]
pub(super) enum Step {
    Key(&'static str),
    Each,
}

/// The byte ranges (quotes included) of the string values `path` leads to
/// in `line`, in order. Values that are not strings are skipped.
pub(super) fn string_spans(line: &[u8], path: &[Step]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let start = skip_ws(line, 0);
    // A line that is not JSON ends the walk early; what it found stays.
    let _ = collect(line, start, path, &mut out);
    out
}

/// The string at `path`, decoded.
pub(super) fn string_at(line: &[u8], path: &[Step]) -> Option<String> {
    let span = string_spans(line, path).into_iter().next()?;
    serde_json::from_slice(&line[span]).ok()
}

/// `line` with each span replaced by the text `change` gives for the
/// string it holds; a span `change` answers `None` for stays.
pub(super) fn replace(
    line: &[u8],
    spans: &[Range<usize>],
    change: impl Fn(&str) -> Option<String>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len() + 32);
    let mut at = 0;
    for span in spans {
        let Some(new) = serde_json::from_slice::<String>(&line[span.clone()])
            .ok()
            .and_then(|old| change(&old))
        else {
            continue;
        };
        let Ok(encoded) = serde_json::to_vec(&new) else {
            continue;
        };
        out.extend_from_slice(&line[at..span.start]);
        out.extend_from_slice(&encoded);
        at = span.end;
    }
    out.extend_from_slice(&line[at..]);
    out
}

fn collect(b: &[u8], i: usize, path: &[Step], out: &mut Vec<Range<usize>>) -> Option<()> {
    let Some((step, rest)) = path.split_first() else {
        if b.get(i) == Some(&b'"') {
            out.push(i..skip_string(b, i)?);
        }
        return Some(());
    };
    match (step, b.get(i)?) {
        (Step::Key(want), b'{') => {
            let mut i = skip_ws(b, i + 1);
            while b.get(i)? != &b'}' {
                let key_end = skip_string(b, i)?;
                let key: Option<String> = serde_json::from_slice(&b[i..key_end]).ok();
                i = skip_ws(b, key_end);
                if b.get(i)? != &b':' {
                    return None;
                }
                i = skip_ws(b, i + 1);
                if key.as_deref() == Some(want) {
                    collect(b, i, rest, out)?;
                }
                i = skip_ws(b, skip_value(b, i)?);
                if b.get(i)? == &b',' {
                    i = skip_ws(b, i + 1);
                }
            }
        }
        (Step::Each, b'[') => {
            let mut i = skip_ws(b, i + 1);
            while b.get(i)? != &b']' {
                collect(b, i, rest, out)?;
                i = skip_ws(b, skip_value(b, i)?);
                if b.get(i)? == &b',' {
                    i = skip_ws(b, i + 1);
                }
            }
        }
        _ => {}
    }
    Some(())
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// The index after the string that starts at `i`.
fn skip_string(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i)? != &b'"' {
        return None;
    }
    let mut i = i + 1;
    loop {
        match b.get(i)? {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
}

/// The index after the value that starts at `i`.
fn skip_value(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => skip_string(b, i),
        open @ (b'{' | b'[') => {
            let close = if *open == b'{' { b'}' } else { b']' };
            let mut i = skip_ws(b, i + 1);
            while b.get(i)? != &close {
                if *open == b'{' {
                    i = skip_ws(b, skip_string(b, i)?);
                    if b.get(i)? != &b':' {
                        return None;
                    }
                    i = skip_ws(b, i + 1);
                }
                i = skip_ws(b, skip_value(b, i)?);
                if b.get(i)? == &b',' {
                    i = skip_ws(b, i + 1);
                }
            }
            Some(i + 1)
        }
        _ => {
            let mut i = i;
            while b
                .get(i)
                .is_some_and(|c| !matches!(c, b',' | b'}' | b']') && !c.is_ascii_whitespace())
            {
                i += 1;
            }
            Some(i)
        }
    }
}
