use super::parse_id;
use crate::{AsyncMutex, BeepClient, BeepError, CALL_TIMEOUT, timed};
use beep_mesh::machine::terminals::{self, Heard};
use beep_proto::{
    AttachTerminal, CloseTerminal, GetTerminal, ListAllTerminals, MAX_TERMINAL_INPUT, OpenTerminal,
    Terminal, TerminalEvent, TerminalInput, TerminalOutput,
    client::{Replies, Uplink},
    node::{
        LinkRx, LinkTx,
        terminal::{Reply as TerminalReply, Request as TerminalRequest, Size},
    },
};
use std::sync::Arc;

#[derive(uniffi::Record)]
pub struct CommandStart {
    pub project: String,
    pub machine_id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn all_terminals(&self) -> Result<Vec<Terminal>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.all_terminals().await);
        }
        self.call(ListAllTerminals {}).await
    }

    pub async fn terminal_output(&self, id: String) -> Result<TerminalOutput, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.terminal_output(parse_id(&id)?).await;
        }
        self.call(GetTerminal { id: parse_id(&id)? }).await
    }

    pub async fn start_command(&self, start: CommandStart) -> Result<Terminal, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.start_command(start).await;
        }
        self.call(OpenTerminal {
            project: start.project,
            machine_id: start.machine_id,
            name: start.name,
            command: start.command,
            cwd: start.cwd,
            cols: 80,
            rows: 24,
        })
        .await
    }

    pub async fn attach_terminal(
        &self,
        id: String,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<TerminalConnection>, BeepError> {
        let (cols, rows) = (clamp_cells(cols), clamp_cells(rows));
        if let Some(mesh) = self.mesh().await? {
            return mesh
                .attach_terminal(parse_id(&id)?, Size { cols, rows })
                .await;
        }
        let request = AttachTerminal {
            id: parse_id(&id)?,
            from: 0,
            cols: cols.into(),
            rows: rows.into(),
        };
        let client = self.client()?;
        let (input, output) =
            timed(CALL_TIMEOUT, "attach terminal", client.duplex(&request)).await?;
        Ok(Arc::new(TerminalConnection {
            input: AsyncMutex::new(TerminalInputStream::Server(input)),
            output: AsyncMutex::new(TerminalOutputStream::Server(output)),
        }))
    }

    pub async fn close_terminal(&self, id: String) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.close_terminal(parse_id(&id)?).await;
        }
        self.call(CloseTerminal { id: parse_id(&id)? }).await
    }
}

/// The screen owns both halves and drops them when it leaves. No background task owns a terminal.
#[derive(uniffi::Object)]
pub struct TerminalConnection {
    pub(crate) input: AsyncMutex<TerminalInputStream>,
    pub(crate) output: AsyncMutex<TerminalOutputStream>,
}

pub(crate) enum TerminalInputStream {
    Server(Uplink<TerminalInput>),
    Mesh(LinkTx),
    ViewOnly,
}

pub(crate) enum TerminalOutputStream {
    Server(Replies<TerminalEvent>),
    Mesh(LinkRx),
    ViewOnly(bool),
}

#[derive(uniffi::Record)]
pub struct MobileTerminalFrame {
    pub data: Vec<u8>,
    pub view_only: bool,
    pub ended: bool,
}

#[uniffi::export(async_runtime = "tokio")]
impl TerminalConnection {
    /// Next PTY output, or whether the terminal is read-only or ended.
    pub async fn next(&self) -> Result<Option<MobileTerminalFrame>, BeepError> {
        let mut output = self.output.lock().await;
        let event = match &mut *output {
            TerminalOutputStream::Server(output) => output.next().await?,
            TerminalOutputStream::Mesh(output) => loop {
                let Some(reply) = output.recv::<TerminalReply>().await? else {
                    break None;
                };
                match terminals::heard(reply) {
                    Heard::Show(event) => break Some(event),
                    Heard::End(state) => {
                        break Some(TerminalEvent::Ended { state });
                    }
                    Heard::Ignore => {}
                }
            },
            TerminalOutputStream::ViewOnly(pending) => {
                std::mem::take(pending).then_some(TerminalEvent::ViewOnly {})
            }
        };
        Ok(event.map(|event| match event {
            TerminalEvent::Output { data, .. } => MobileTerminalFrame {
                data: data.into_vec(),
                view_only: false,
                ended: false,
            },
            TerminalEvent::ViewOnly {} => MobileTerminalFrame {
                data: Vec::new(),
                view_only: true,
                ended: false,
            },
            TerminalEvent::Ended { .. } => MobileTerminalFrame {
                data: Vec::new(),
                view_only: false,
                ended: true,
            },
        }))
    }

    /// Tells the terminal the screen's new size in character cells.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), BeepError> {
        let (cols, rows) = (clamp_cells(cols), clamp_cells(rows));
        let mut input = self.input.lock().await;
        match &mut *input {
            TerminalInputStream::Server(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal resize",
                    input.send(&TerminalInput::Resize {
                        cols: cols.into(),
                        rows: rows.into(),
                    }),
                )
                .await
            }
            TerminalInputStream::Mesh(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal resize",
                    input.send(&TerminalRequest::Resize {
                        size: Size { cols, rows },
                    }),
                )
                .await
            }
            TerminalInputStream::ViewOnly => Ok(()),
        }
    }

    pub async fn keys(&self, text: String) -> Result<(), BeepError> {
        if text.len() > MAX_TERMINAL_INPUT {
            return Err(BeepError::BadRequest {
                reason: "Terminal input is too long".into(),
            });
        }
        let mut input = self.input.lock().await;
        match &mut *input {
            TerminalInputStream::Server(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal input",
                    input.send(&TerminalInput::Keys {
                        data: text.into_bytes().into(),
                    }),
                )
                .await
            }
            TerminalInputStream::Mesh(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal input",
                    input.send(&TerminalRequest::Input {
                        data: text.into_bytes().into(),
                    }),
                )
                .await
            }
            TerminalInputStream::ViewOnly => Err(crate::bad_request("This terminal is view only.")),
        }
    }
}

fn clamp_cells(cells: u16) -> u16 {
    cells.clamp(1, 500)
}
