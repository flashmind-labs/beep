//! The link to Hermes's dashboard socket. One task owns the socket; a
//! `Gateway` talks to it over channels, so a call can wait for its reply
//! while events and requests from Hermes keep arriving for `next`.

use beep_proto::SecretText;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error as WsError, Message},
};
use url::Url;

const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub(super) enum LinkError {
    #[error("Hermes refused the token")]
    Refused,
    #[error("{0}")]
    Unreachable(String),
    #[error("the link to Hermes closed")]
    Closed,
    #[error("Hermes answered with an error ({code}): {message}")]
    Rpc { code: i64, message: String },
}

/// What Hermes sends that is not the reply to a call.
#[derive(Debug, PartialEq)]
pub(super) enum Incoming {
    /// The `params` of an `event` message: `type`, `payload` and `seq`.
    Event(Value),
    /// A request Hermes waits on: answer it with `Gateway::answer`.
    Ask { id: String, method: String, params: Value },
}

enum Command {
    Call { method: String, params: Value, reply: oneshot::Sender<Result<Value, LinkError>> },
    Answer { id: String, result: Value },
}

pub(super) struct Gateway {
    commands: mpsc::UnboundedSender<Command>,
    incoming: mpsc::UnboundedReceiver<Incoming>,
}

impl Gateway {
    /// Connects, waits for `gateway.ready` and tells Hermes that this client
    /// answers its requests.
    pub(super) async fn connect(host: &str, port: u16, token: &SecretText) -> Result<Gateway, LinkError> {
        let unreachable = || LinkError::Unreachable(format!("nothing answers on {host}:{port}"));
        let url = socket_url(host, port, token).ok_or_else(unreachable)?;

        let (mut socket, _) = connect_async(url.as_str()).await.map_err(|err| match err {
            WsError::Http(response) if matches!(response.status().as_u16(), 401 | 403) => LinkError::Refused,
            _ => unreachable(),
        })?;

        let ready = async {
            while let Some(frame) = socket.next().await {
                let Ok(Message::Text(text)) = frame else { continue };
                let frame: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if frame["method"] == "event" && frame["params"]["type"] == "gateway.ready" {
                    return true;
                }
            }
            false
        };
        match tokio::time::timeout(READY_TIMEOUT, ready).await {
            Ok(true) => {}
            Ok(false) => return Err(LinkError::Refused),
            Err(_) => return Err(unreachable()),
        }

        let (commands, command_rx) = mpsc::unbounded_channel();
        let (incoming_tx, incoming) = mpsc::unbounded_channel();
        tokio::spawn(run(socket, command_rx, incoming_tx));

        let mut gateway = Gateway { commands, incoming };
        gateway.call("client.capabilities", json!({"server_requests": true})).await?;

        Ok(gateway)
    }

    /// Calls `method` and returns its `result`, whatever else arrives meanwhile.
    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        let (reply, answered) = oneshot::channel();
        let command = Command::Call { method: method.to_owned(), params, reply };
        self.commands.send(command).map_err(|_| LinkError::Closed)?;

        answered.await.map_err(|_| LinkError::Closed)?
    }

    /// The next event or request from Hermes; `None` once the link closed.
    pub(super) async fn next(&mut self) -> Option<Incoming> {
        self.incoming.recv().await
    }

    /// Answers the request `id` with `result`.
    pub(super) async fn answer(&mut self, id: &str, result: Value) -> Result<(), LinkError> {
        self.commands.send(Command::Answer { id: id.to_owned(), result }).map_err(|_| LinkError::Closed)
    }
}

/// The socket's address, with the token as a query value. `None` for a host
/// that makes no address.
pub(super) fn socket_url(host: &str, port: u16, token: &SecretText) -> Option<Url> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
    let mut url = Url::parse(&format!("ws://{host}:{port}/api/ws")).ok()?;
    url.query_pairs_mut().append_pair("token", token.expose());

    Some(url)
}

/// Owns the socket until it closes or the `Gateway` is dropped.
async fn run(
    mut socket: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    incoming: mpsc::UnboundedSender<Incoming>,
) {
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value, LinkError>>> = HashMap::new();
    let mut next_id = 0u64;

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                let frame = match command {
                    Command::Call { method, params, reply } => {
                        next_id += 1;
                        pending.insert(next_id, reply);
                        json!({"jsonrpc": "2.0", "id": next_id, "method": method, "params": params})
                    }
                    Command::Answer { id, result } => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                };
                if socket.send(Message::text(frame.to_string())).await.is_err() {
                    break;
                }
            }
            frame = socket.next() => {
                let Some(Ok(frame)) = frame else { break };
                match frame {
                    Message::Text(text) => {
                        let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
                        if !route(frame, &mut pending, &incoming) {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    // Dropping `pending` and `incoming` fails the waiting calls and ends `next`.
}

/// Sends a frame where it belongs. `false` once nobody reads `incoming`.
fn route(mut frame: Value, pending: &mut HashMap<u64, oneshot::Sender<Result<Value, LinkError>>>, incoming: &mpsc::UnboundedSender<Incoming>) -> bool {
    let method = frame["method"].as_str().map(str::to_owned);

    match (method, &frame["id"]) {
        (Some(method), _) if method == "event" => incoming.send(Incoming::Event(frame["params"].take())).is_ok(),
        (Some(method), Value::String(id)) => {
            let id = id.clone();
            incoming.send(Incoming::Ask { id, method, params: frame["params"].take() }).is_ok()
        }
        (None, Value::Number(id)) => {
            if let Some(reply) = id.as_u64().and_then(|id| pending.remove(&id)) {
                let outcome = match frame.get("error") {
                    Some(error) => Err(LinkError::Rpc {
                        code: error["code"].as_i64().unwrap_or_default(),
                        message: error["message"].as_str().unwrap_or_default().to_owned(),
                    }),
                    None => Ok(frame["result"].take()),
                };
                let _ = reply.send(outcome);
            }
            true
        }
        _ => true,
    }
}
