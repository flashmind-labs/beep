//! The apps Beep can use: connecting them, and what Beep may do in each.
//!
//! Composio apps sign in on a web page the server hands out, and the bar
//! looks again until the app shows as connected. Fastmail only redirects to
//! `http://localhost`, so this Mac listens there for the one-time code and
//! passes it to the server, which keeps the tokens.
//!
//! MCP servers the user adds by URL sign in through the web page at
//! `mcp_callback_url()`, which hands the code to `beep://oauth`; `open_link`
//! passes it on to the server.

use crate::{
    bar,
    bar::app::open_url,
    beep::Beep,
    error::{DesktopError, Result},
};
use beep_proto::{
    AddAppAccount, AddMcpServer, AppAccount, AppIntegration, AppKind, AppPermissions, ConnectApp,
    ConnectLink, ConnectMcpServer, DisconnectApp, FinishAppAuth, FinishMcpAuth, GetAppPermissions,
    ListAppAccounts, ListApps, ListMcpServers, ListMcpTools, McpCallback, McpConnect, McpServer,
    McpTools, PermissionLevel, RemoveAppAccount, RemoveMcpServer, SetAppPermissions, SetMcpTool,
    StartAppAuth, mcp_callback,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::AbortHandle,
    time::timeout,
};
use url::Url;

type Service<'a> = State<'a, Arc<Beep>>;

/// How long a browser sign-in may take before the listener gives up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long one browser request may take to arrive in full.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest request line and headers read from the browser.
const REQUEST_LIMIT: usize = 16 * 1024;

/// How an app signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// On a page the server hands out.
    Composio,
    /// Through a redirect to this Mac.
    Fastmail,
}

impl From<AppKind> for Kind {
    fn from(kind: AppKind) -> Self {
        match kind {
            AppKind::Composio {} => Self::Composio,
            AppKind::Fastmail {} => Self::Fastmail,
        }
    }
}

/// An app as the Apps view lists it.
#[derive(Serialize)]
pub struct AppView {
    slug: String,
    name: String,
    description: String,
    logo_url: Option<String>,
    connected: bool,
    /// False when the server lacks what the app needs to connect.
    configured: bool,
    kind: Kind,
}

impl From<AppIntegration> for AppView {
    fn from(app: AppIntegration) -> Self {
        Self {
            slug: app.slug,
            name: app.name,
            description: app.description,
            logo_url: app.logo_url,
            connected: app.connected,
            configured: app.configured,
            kind: app.kind.into(),
        }
    }
}

/// What Beep may do in one app, as the permissions view shows it.
#[derive(Serialize)]
pub struct PermissionsView {
    /// `ask`, `allow` or `read`.
    level: &'static str,
    actions: Vec<ActionView>,
}

#[derive(Serialize)]
pub struct ActionView {
    tool: String,
    label: String,
    allowed: bool,
}

impl From<AppPermissions> for PermissionsView {
    fn from(permissions: AppPermissions) -> Self {
        Self {
            level: permissions.level.as_str(),
            actions: permissions
                .actions
                .into_iter()
                .map(|a| ActionView {
                    tool: a.tool,
                    label: a.label,
                    allowed: a.allowed,
                })
                .collect(),
        }
    }
}

/// The browser sign-in under way, so Cancel can stop its listener.
#[derive(Default)]
pub struct SignIn(Mutex<Option<AbortHandle>>);

impl SignIn {
    /// Stops the sign-in under way, if any, and keeps `task` instead.
    fn replace(&self, task: Option<AbortHandle>) {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = slot.take() {
            old.abort();
        }
        *slot = task;
    }
}

#[tauri::command]
pub async fn list_apps(beep: Service<'_>) -> Result<Vec<AppView>> {
    let apps = beep.call(ListApps {}).await?;
    Ok(apps.into_iter().map(AppView::from).collect())
}

/// Opens the app's sign-in page. A Composio app returns at once and shows
/// as connected once the page is done; Fastmail returns when the browser
/// comes back, connected. `add` signs in another Fastmail account instead
/// of reconnecting the first.
#[tauri::command]
pub async fn connect_app(
    beep: Service<'_>,
    sign_in: State<'_, SignIn>,
    slug: String,
    kind: Kind,
    add: Option<bool>,
) -> Result<()> {
    if kind == Kind::Composio {
        let ConnectLink { url } = beep.call(ConnectApp { slug }).await?;
        return open_url(url);
    }
    let task = tokio::spawn(sign_in_here(
        beep.inner().clone(),
        slug,
        add.unwrap_or(false),
    ));
    sign_in.replace(Some(task.abort_handle()));
    match task.await {
        Ok(done) => done,
        Err(e) if e.is_cancelled() => Err(DesktopError::BadRequest("cancelled".into())),
        Err(e) => Err(anyhow::Error::from(e).into()),
    }
}

/// Stops waiting for a browser sign-in.
#[tauri::command]
pub fn cancel_app_sign_in(sign_in: State<'_, SignIn>) {
    sign_in.replace(None);
}

#[tauri::command]
pub async fn disconnect_app(beep: Service<'_>, slug: String) -> Result<()> {
    beep.call(DisconnectApp { slug }).await
}

/// The accounts of an app that connects several, such as Fastmail, the
/// agent's default first.
#[tauri::command]
pub async fn list_app_accounts(beep: Service<'_>, slug: String) -> Result<Vec<AppAccount>> {
    beep.call(ListAppAccounts { slug }).await
}

/// Disconnects one account of `slug`, by `AppAccount::id`.
#[tauri::command]
pub async fn remove_app_account(beep: Service<'_>, slug: String, id: String) -> Result<()> {
    beep.call(RemoveAppAccount { slug, id }).await
}

#[tauri::command]
pub async fn app_permissions(beep: Service<'_>, slug: String) -> Result<PermissionsView> {
    Ok(beep.call(GetAppPermissions { slug }).await?.into())
}

/// `level` is `ask`, `allow` or `read`; `allowed` names the actions that
/// skip asking under `ask`.
#[tauri::command]
pub async fn set_app_permissions(
    beep: Service<'_>,
    slug: String,
    level: String,
    allowed: Vec<String>,
) -> Result<()> {
    let level = PermissionLevel::parse(&level)
        .ok_or_else(|| DesktopError::BadRequest(format!("no permission level {level}")))?;
    beep.call(SetAppPermissions {
        slug,
        level,
        allowed,
    })
    .await
}

#[tauri::command]
pub async fn list_mcp_servers(beep: Service<'_>) -> Result<Vec<McpServer>> {
    beep.call(ListMcpServers {}).await
}

/// Adds a server by its URL. One that needs a sign-in opens it in the
/// browser and comes back not connected yet; the link back finishes it.
#[tauri::command]
pub async fn add_mcp_server(beep: Service<'_>, name: String, url: String) -> Result<McpServer> {
    open_sign_in(beep.call(AddMcpServer { name, url }).await?)
}

/// Connects server `id` again, signing in again when it has to.
#[tauri::command]
pub async fn connect_mcp_server(beep: Service<'_>, id: String) -> Result<McpServer> {
    open_sign_in(beep.call(ConnectMcpServer { id }).await?)
}

/// Every tool server `id` lists, and which of them Beep uses.
#[tauri::command]
pub async fn list_mcp_tools(beep: Service<'_>, id: String) -> Result<McpTools> {
    beep.call(ListMcpTools { id }).await
}

/// Turns one tool of server `id` on or off for Beep.
#[tauri::command]
pub async fn set_mcp_tool(
    beep: Service<'_>,
    id: String,
    tool: String,
    enabled: bool,
) -> Result<McpTools> {
    beep.call(SetMcpTool { id, tool, enabled }).await
}

#[tauri::command]
pub async fn remove_mcp_server(beep: Service<'_>, id: String) -> Result<()> {
    beep.call(RemoveMcpServer { id }).await
}

fn open_sign_in(connect: McpConnect) -> Result<McpServer> {
    if let Some(url) = connect.auth_url {
        open_url(url)?;
    }
    Ok(connect.server)
}

/// Takes a `beep://` link macOS opened Beep with. An MCP sign-in goes to
/// the server, then Apps opens and the bar hears `mcp-signed-in` with
/// `null`, or why it failed.
pub fn open_link<R: Runtime>(app: &AppHandle<R>, url: &str) {
    if !url.starts_with("beep://oauth") {
        return;
    }
    let Some(McpCallback {
        code,
        state,
        issuer,
    }) = mcp_callback(url)
    else {
        // Cancelled on the page: nothing to finish.
        bar::show(app, Some("apps"));
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let beep = app.state::<Arc<Beep>>().inner().clone();
        let finished = beep
            .call(FinishMcpAuth {
                code,
                state,
                issuer,
            })
            .await;
        let failed = finished.err().map(|e| e.to_string());
        bar::show(&app, Some("apps"));
        let _ = app.emit_to("bar", "mcp-signed-in", failed);
    });
}

/// What the browser brought back to the redirect.
#[derive(Debug, PartialEq, Eq)]
enum Redirect {
    Code {
        code: String,
        state: String,
        issuer: Option<String>,
    },
    /// The user cancelled, or the provider refused, with its reason.
    Refused(String),
    /// Some other request, such as the favicon.
    Other,
}

/// Listens on a loopback port, starts the sign-in with it and passes the
/// code that comes back to the server. `add` signs in another account.
async fn sign_in_here(beep: Arc<Beep>, slug: String, add: bool) -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(anyhow::Error::from)?;
    let port = listener.local_addr().map_err(anyhow::Error::from)?.port();
    let redirect_uri = format!("http://localhost:{port}/callback");
    let ConnectLink { url } = if add {
        beep.call(AddAppAccount {
            slug: slug.clone(),
            redirect_uri,
        })
        .await?
    } else {
        beep.call(StartAppAuth {
            slug: slug.clone(),
            redirect_uri,
        })
        .await?
    };
    open_url(url)?;
    timeout(SIGN_IN_TIMEOUT, async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(anyhow::Error::from)?;
            let redirect = match timeout(REQUEST_TIMEOUT, read_target(&mut stream)).await {
                Ok(Some(target)) => redirect(&target),
                _ => continue,
            };
            match redirect {
                Redirect::Other => reply(&mut stream, 404, "Not found").await,
                Redirect::Refused(why) => {
                    reply(
                        &mut stream,
                        400,
                        "Sign-in was cancelled. You can close this tab and try again in Beep.",
                    )
                    .await;
                    return Err(DesktopError::BadRequest(why));
                }
                Redirect::Code {
                    code,
                    state,
                    issuer,
                } => {
                    let finished = beep
                        .call(FinishAppAuth {
                            slug,
                            code,
                            state,
                            issuer,
                        })
                        .await;
                    let (status, text) = match finished {
                        Ok(()) => (200, "Connected. You can go back to Beep."),
                        Err(_) => (
                            500,
                            "Beep could not finish connecting. Go back to Beep and try again.",
                        ),
                    };
                    reply(&mut stream, status, text).await;
                    return finished;
                }
            }
        }
    })
    .await
    .map_err(|_| DesktopError::BadRequest("the sign-in took too long".into()))?
}

/// The path and query of the request on `stream`.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut read = Vec::new();
    let mut buffer = [0; 2048];
    while !read.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buffer).await.ok()?;
        if n == 0 || read.len() + n > REQUEST_LIMIT {
            return None;
        }
        read.extend_from_slice(&buffer[..n]);
    }
    let line = String::from_utf8_lossy(&read).lines().next()?.to_owned();
    let mut parts = line.split(' ');
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(str::to_owned)
}

fn redirect(target: &str) -> Redirect {
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
        return Redirect::Other;
    };
    if url.path() != "/callback" {
        return Redirect::Other;
    }
    let get = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    match (get("code"), get("state")) {
        (Some(code), Some(state)) => Redirect::Code {
            code,
            state,
            issuer: get("iss"),
        },
        _ => Redirect::Refused(
            get("error_description")
                .or_else(|| get("error"))
                .unwrap_or_else(|| "sign-in was cancelled".into()),
        ),
    }
}

async fn reply(stream: &mut TcpStream, status: u16, text: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Beep</title>\
         <body style=\"font:16px -apple-system,sans-serif;padding:48px\">{text}</body>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_reads_its_code_or_why_it_failed() {
        assert_eq!(
            redirect("/callback?code=a%2Bb&state=s1&iss=https%3A%2F%2Fid.example"),
            Redirect::Code {
                code: "a+b".into(),
                state: "s1".into(),
                issuer: Some("https://id.example".into()),
            }
        );
        assert_eq!(
            redirect("/callback?error=access_denied"),
            Redirect::Refused("access_denied".into())
        );
        assert_eq!(redirect("/favicon.ico"), Redirect::Other);
    }
}
