//! Language server lookup, LSP JSON-RPC framing, URI conversion and location parsing.

use anyhow::{Context, Result, bail};
use beep_proto::node::agent::LspLocation;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};
use url::Url;

use crate::{agents::locate, files::paths};

/// The largest JSON-RPC frame we accept from a language server.
const MAX_FRAME: usize = 16 * 1024 * 1024;
/// The largest header line we accept before rejecting a malformed stream.
const MAX_HEADER_LINE: usize = 8 * 1024;

/// A language family that shares one language server process per workspace root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    CCpp,
    Swift,
    Zig,
    Lua,
    Shell,
    Ruby,
    Java,
    Kotlin,
}

impl Language {
    /// Human-readable name for status notes.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::CCpp => "C/C++",
            Self::Swift => "Swift",
            Self::Zig => "Zig",
            Self::Lua => "Lua",
            Self::Shell => "Shell",
            Self::Ruby => "Ruby",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
        }
    }

    /// Short hint naming the primary server binary to install.
    pub(crate) fn install_hint(self) -> &'static str {
        match self {
            Self::Rust => "rust-analyzer",
            Self::TypeScript => "vtsls or typescript-language-server",
            Self::Python => "basedpyright or pyright",
            Self::Go => "gopls",
            Self::CCpp => "clangd",
            Self::Swift => "sourcekit-lsp",
            Self::Zig => "zls",
            Self::Lua => "lua-language-server",
            Self::Shell => "bash-language-server",
            Self::Ruby => "ruby-lsp",
            Self::Java => "jdtls",
            Self::Kotlin => "kotlin-language-server",
        }
    }

    /// Candidate server commands, preferred first.
    pub(crate) fn servers(self) -> &'static [ServerSpec] {
        match self {
            Self::Rust => &[ServerSpec {
                program: "rust-analyzer",
                args: &[],
                local_dirs: &[],
            }],
            Self::TypeScript => &[
                ServerSpec {
                    program: "vtsls",
                    args: &["--stdio"],
                    local_dirs: &["node_modules/.bin"],
                },
                ServerSpec {
                    program: "typescript-language-server",
                    args: &["--stdio"],
                    local_dirs: &["node_modules/.bin"],
                },
            ],
            Self::Python => &[
                ServerSpec {
                    program: "basedpyright-langserver",
                    args: &["--stdio"],
                    local_dirs: &[".venv/bin", "venv/bin", ".venv/Scripts", "venv/Scripts"],
                },
                ServerSpec {
                    program: "pyright-langserver",
                    args: &["--stdio"],
                    local_dirs: &[".venv/bin", "venv/bin", ".venv/Scripts", "venv/Scripts"],
                },
                ServerSpec {
                    program: "pylsp",
                    args: &[],
                    local_dirs: &[".venv/bin", "venv/bin", ".venv/Scripts", "venv/Scripts"],
                },
                ServerSpec {
                    program: "ty",
                    args: &["server"],
                    local_dirs: &[".venv/bin", "venv/bin", ".venv/Scripts", "venv/Scripts"],
                },
            ],
            Self::Go => &[ServerSpec {
                program: "gopls",
                args: &[],
                local_dirs: &[],
            }],
            Self::CCpp => &[ServerSpec {
                program: "clangd",
                args: &[],
                local_dirs: &[],
            }],
            Self::Swift => &[ServerSpec {
                program: "sourcekit-lsp",
                args: &[],
                local_dirs: &[],
            }],
            Self::Zig => &[ServerSpec {
                program: "zls",
                args: &[],
                local_dirs: &[],
            }],
            Self::Lua => &[ServerSpec {
                program: "lua-language-server",
                args: &[],
                local_dirs: &[],
            }],
            Self::Shell => &[ServerSpec {
                program: "bash-language-server",
                args: &["start"],
                local_dirs: &["node_modules/.bin"],
            }],
            Self::Ruby => &[
                ServerSpec {
                    program: "ruby-lsp",
                    args: &[],
                    local_dirs: &[],
                },
                ServerSpec {
                    program: "solargraph",
                    args: &["stdio"],
                    local_dirs: &[],
                },
            ],
            Self::Java => &[ServerSpec {
                program: "jdtls",
                args: &[],
                local_dirs: &[],
            }],
            Self::Kotlin => &[ServerSpec {
                program: "kotlin-language-server",
                args: &[],
                local_dirs: &[],
            }],
        }
    }
}

/// One candidate language server executable and its stdio arguments.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ServerSpec {
    pub(crate) program: &'static str,
    pub(crate) args: &'static [&'static str],
    /// Workspace-relative directories checked before `PATH`.
    pub(crate) local_dirs: &'static [&'static str],
}

/// Maps a file path's extension to its server family and LSP `languageId`.
pub(crate) fn language_for(path: &str) -> Option<(Language, &'static str)> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();

    Some(match ext.as_str() {
        "rs" => (Language::Rust, "rust"),
        "ts" | "mts" | "cts" => (Language::TypeScript, "typescript"),
        "tsx" => (Language::TypeScript, "typescriptreact"),
        "js" | "mjs" | "cjs" => (Language::TypeScript, "javascript"),
        "jsx" => (Language::TypeScript, "javascriptreact"),
        "py" | "pyi" => (Language::Python, "python"),
        "go" => (Language::Go, "go"),
        "c" | "h" => (Language::CCpp, "c"),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => (Language::CCpp, "cpp"),
        "swift" => (Language::Swift, "swift"),
        "zig" => (Language::Zig, "zig"),
        "lua" => (Language::Lua, "lua"),
        "sh" | "bash" | "zsh" => (Language::Shell, "shellscript"),
        "rb" => (Language::Ruby, "ruby"),
        "java" => (Language::Java, "java"),
        "kt" | "kts" => (Language::Kotlin, "kotlin"),
        _ => return None,
    })
}

/// Extra per-user toolchain folders that bare service `PATH`s omit.
const TOOLCHAIN_BINS: &[&str] = &[".cargo/bin", "go/bin", ".rustup/toolchains"];

/// Finds the first installed language server for `language`, preferring workspace-local
/// binaries, then `search_path`, then common user install folders under `home`.
pub(crate) fn find_server(
    language: Language,
    root: &Path,
    search_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Option<(PathBuf, &'static [&'static str])> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };

    for spec in language.servers() {
        for dir in spec.local_dirs {
            for ext in exts {
                let candidate = root.join(dir).join(format!("{}{ext}", spec.program));
                if candidate.is_file() {
                    return Some((candidate, spec.args));
                }
            }
        }

        if let Ok(found) = which::which_in(spec.program, search_path, root) {
            return Some((found, spec.args));
        }

        if let Some(home) = home {
            if let Some(found) = locate::fallback(spec.program, home) {
                return Some((found, spec.args));
            }

            for rel in TOOLCHAIN_BINS {
                for ext in exts {
                    let candidate = home.join(rel).join(format!("{}{ext}", spec.program));
                    if candidate.is_file() {
                        return Some((candidate, spec.args));
                    }
                }
            }
        }
    }

    None
}

/// Encodes a JSON-RPC message with an LSP `Content-Length` header.
pub(crate) fn encode_frame(message: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(message).unwrap_or_default();
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(&body);
    frame
}

/// Reads the next JSON-RPC frame from an LSP stdout stream; `Ok(None)` on clean EOF.
pub(crate) async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    let mut saw_header = false;
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .await
            .context("reading LSP header")?;

        if read == 0 {
            if saw_header {
                bail!("unexpected EOF in LSP headers");
            }
            return Ok(None);
        }

        if line.len() > MAX_HEADER_LINE {
            bail!("LSP header line exceeded limit");
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);

        if trimmed.is_empty() {
            if saw_header {
                break;
            }
            continue;
        }

        saw_header = true;

        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let parsed: usize = value
                .trim()
                .parse()
                .context("invalid LSP Content-Length header")?;
            content_length = Some(parsed);
        }
    }

    let length = content_length.context("missing LSP Content-Length header")?;

    if length > MAX_FRAME {
        bail!("LSP frame of {length} bytes exceeds limit");
    }

    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .context("reading LSP body")?;
    let value: Value = serde_json::from_slice(&body).context("parsing LSP JSON body")?;

    Ok(Some(value))
}

/// Converts an absolute filesystem path into a `file://` URI string.
pub(crate) fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.into())
        .map_err(|()| anyhow::anyhow!("cannot convert {} to a file URI", path.display()))
}

/// Replies to common server-to-client LSP requests so servers never block waiting on us.
pub(crate) fn server_request_reply(
    method: &str,
    params: Option<&Value>,
    root_uri: &str,
    root_name: &str,
) -> Value {
    match method {
        "workspace/configuration" => {
            let count = params
                .and_then(|value| value.get("items"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; count])
        }
        "workspace/workspaceFolders" => json!([{ "uri": root_uri, "name": root_name }]),
        _ => Value::Null,
    }
}

/// Parses a `textDocument/definition` response (`null`, `Location`, `Location[]`, or
/// `LocationLink[]`) into deduplicated workspace-normalized [`LspLocation`] values.
pub(crate) fn parse_locations(result: &Value, root: &Path) -> Vec<LspLocation> {
    let items: Vec<&Value> = match result {
        Value::Null => return Vec::new(),
        Value::Array(array) => array.iter().collect(),
        Value::Object(_) => vec![result],
        _ => return Vec::new(),
    };

    let mut out = Vec::with_capacity(items.len());
    let mut seen: HashSet<(String, bool, u32, u32)> = HashSet::new();

    for item in items {
        let Some(location) = parse_one_location(item, root) else {
            continue;
        };
        let key = (
            location.path.clone(),
            location.inside_root,
            location.line,
            location.column,
        );

        if seen.insert(key) {
            out.push(location);
        }
    }

    out
}

fn parse_one_location(item: &Value, root: &Path) -> Option<LspLocation> {
    let (uri, range) = if let Some(target_uri) = item.get("targetUri").and_then(Value::as_str) {
        let range = item
            .get("targetSelectionRange")
            .or_else(|| item.get("targetRange"))?;
        (target_uri, range)
    } else {
        let uri = item.get("uri").and_then(Value::as_str)?;
        let range = item.get("range")?;
        (uri, range)
    };

    let start = range.get("start")?;
    let end = range.get("end").unwrap_or(start);
    let line = u32::try_from(start.get("line")?.as_u64()?).ok()?;
    let column = u32::try_from(start.get("character")?.as_u64()?).ok()?;
    let end_line = end
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(line);
    let end_column = end
        .get("character")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(column);

    let url = Url::parse(uri).ok()?;
    if url.scheme() != "file" {
        return None;
    }

    let raw_path = url.to_file_path().ok()?;
    let resolved = paths::canonicalize(&raw_path).unwrap_or(raw_path);

    let (path, inside_root) = match resolved.strip_prefix(root) {
        Ok(rel) => (rel_to_slash(rel)?, true),
        Err(_) => (resolved.to_string_lossy().into_owned(), false),
    };

    Some(LspLocation {
        path,
        inside_root,
        line,
        column,
        end_line,
        end_column,
    })
}

fn rel_to_slash(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();

    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }

    (!parts.is_empty()).then(|| parts.join("/"))
}
