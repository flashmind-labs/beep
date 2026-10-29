//! `<node> files ...`: files in the folders a node shares, such as
//! `build-box files read /srv/project/README.md`.

use crate::{
    nodes::Online,
    protocol::node::files::{FileEntry, FileKind, MAX_CHUNK, Reply, Request},
};
use std::fmt::Write;

/// Bytes `read` returns unless `--limit` says otherwise.
const DEFAULT_READ: u32 = 64 * 1024;
/// The most bytes `read` returns, to keep a reply readable.
const MAX_READ: u32 = 1024 * 1024;
/// Entries `ls` shows.
const LISTED: u32 = 200;

/// The commands, for `help`.
pub(super) fn usage(node: &str) -> String {
    format!(
        "{node} files roots\n\
         {node} files ls [PATH]\n\
         {node} files stat PATH\n\
         {node} files read PATH [--offset BYTES] [--limit BYTES]\n\
         {node} files write PATH [--append] \"TEXT\"\n"
    )
}

/// Runs one `files` command; the text is the result, the error a failure.
pub(super) async fn run(online: &Online, node: &str, words: &[&str]) -> Result<String, String> {
    let request = request(words).map_err(|e| format!("{e}\nUsage:\n{}", usage(node)))?;
    let reply = online.files(&request).await.map_err(|e| format!("{e:#}"))?;
    show(reply)
}

/// The request a command sends.
fn request(words: &[&str]) -> Result<Request, String> {
    let Some((command, rest)) = words.split_first() else {
        return Err("Which files command?".into());
    };
    let mut offset = 0;
    let mut limit = DEFAULT_READ;
    let mut append = false;
    let mut plain = Vec::new();
    let mut rest = rest.iter();
    while let Some(word) = rest.next() {
        let mut number = |flag: &str| -> Result<u64, String> {
            let value = rest.next().ok_or(format!("{flag} needs a number."))?;
            value
                .parse()
                .map_err(|_| format!("{flag} takes a number, not {value:?}."))
        };
        match *word {
            "--offset" if *command == "read" => offset = number("--offset")?,
            "--limit" if *command == "read" => {
                let asked = number("--limit")?;
                limit = u32::try_from(asked).unwrap_or(MAX_READ).clamp(1, MAX_READ);
            }
            "--append" if *command == "write" => append = true,
            other => plain.push(other),
        }
    }
    let path = |plain: &[&str]| match plain {
        [path] => Ok((*path).to_owned()),
        _ => Err(format!("{command} takes one path.")),
    };
    Ok(match *command {
        "roots" if plain.is_empty() => Request::Roots {},
        "ls" | "list" => Request::List {
            path: match &plain[..] {
                [] => String::new(),
                _ => path(&plain)?,
            },
            limit: LISTED,
        },
        "stat" => Request::Stat {
            path: path(&plain)?,
        },
        "read" => Request::Read {
            path: path(&plain)?,
            offset,
            limit,
        },
        "write" => {
            let Some((path, text)) = plain.split_first() else {
                return Err("write needs a path and the text.".into());
            };
            let data = text.join(" ").into_bytes();
            if data.len() > MAX_CHUNK as usize {
                return Err(format!("At most {MAX_CHUNK} bytes go in one write."));
            }
            Request::Write {
                path: (*path).to_owned(),
                data,
                append,
            }
        }
        other => return Err(format!("files has no command {other}.")),
    })
}

fn show(reply: Reply) -> Result<String, String> {
    Ok(match reply {
        Reply::Roots { roots } if roots.is_empty() => "It shares no folders.".into(),
        Reply::Roots { roots } => format!("Shared folders:\n{}", roots.join("\n")),
        Reply::Data { path, data, size } => {
            if data.contains(&0) {
                return Ok(format!(
                    "{path} looks binary ({size} bytes); it is not shown as text."
                ));
            }
            let text = String::from_utf8_lossy(&data);
            if data.len() as u64 == size {
                text.into_owned()
            } else {
                format!("({} of {size} bytes of {path})\n{text}", data.len())
            }
        }
        Reply::Written { path, size } => format!("Wrote {path}; it is {size} bytes now."),
        Reply::Entries {
            path,
            entries,
            more,
        } => {
            let mut out = format!("{path}:\n");
            for entry in &entries {
                out.push_str(&line(entry));
            }
            if entries.is_empty() {
                out.push_str("(empty)\n");
            }
            if more > 0 {
                let _ = writeln!(out, "... and {more} more");
            }
            out
        }
        Reply::Stat { path, entry } => format!("{path}\n{}", line(&entry)),
        Reply::Error { message } => return Err(message),
        other => format!("The node answered {other:?}"),
    })
}

fn line(entry: &FileEntry) -> String {
    let mut out = format!("{} {}", entry.kind.as_str(), entry.name);
    if entry.kind == FileKind::File {
        let _ = write!(out, " ({} bytes)", entry.size);
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_become_requests() {
        assert_eq!(
            request(&["read", "/srv/project/a.txt", "--limit", "10"]),
            Ok(Request::Read {
                path: "/srv/project/a.txt".into(),
                offset: 0,
                limit: 10
            })
        );
        assert_eq!(
            request(&["write", "--append", "notes.txt", "one", "two"]),
            Ok(Request::Write {
                path: "notes.txt".into(),
                data: b"one two".to_vec(),
                append: true
            })
        );
        assert_eq!(
            request(&["ls"]),
            Ok(Request::List {
                path: String::new(),
                limit: LISTED
            })
        );
        assert!(request(&["read"]).is_err());
        assert!(request(&["delete", "a"]).is_err());
    }

    #[test]
    fn replies_read_as_text() {
        let listing = show(Reply::Entries {
            path: "/srv/project".into(),
            entries: vec![
                FileEntry {
                    name: "src".into(),
                    kind: FileKind::Dir,
                    ..FileEntry::default()
                },
                FileEntry {
                    name: "README.md".into(),
                    kind: FileKind::File,
                    size: 12,
                    modified: None,
                },
            ],
            more: 3,
        });
        assert_eq!(
            listing,
            Ok("/srv/project:\ndir src\nfile README.md (12 bytes)\n... and 3 more\n".into())
        );
        let part = show(Reply::Data {
            path: "/srv/project/a.txt".into(),
            data: b"hi".to_vec(),
            size: 10,
        });
        assert_eq!(part, Ok("(2 of 10 bytes of /srv/project/a.txt)\nhi".into()));
        let refused = show(Reply::Error {
            message: "/etc/passwd is outside the shared folders".into(),
        });
        assert!(refused.is_err());
    }
}
