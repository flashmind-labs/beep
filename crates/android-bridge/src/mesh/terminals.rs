use super::Connected;
use crate::{
    BeepError,
    workspace::{CommandStart, TerminalConnection, TerminalInputStream, TerminalOutputStream},
};
use beep_mesh::{
    Key,
    machine::{
        Tasks,
        terminals::{self, Kept},
    },
};
use beep_proto::{
    Terminal, TerminalOutput, TerminalState,
    node::{
        mesh_state::{StateValue, TerminalRecord},
        process::{Reply as TaskReply, Request as TaskRequest, TaskId},
        terminal::Size,
    },
};
use futures::{StreamExt, stream};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

struct Row {
    id: Uuid,
    record: TerminalRecord,
}
impl Row {
    fn kept(&self) -> Kept<'_> {
        Kept {
            task: self.record.task,
            pty: self.record.pty,
            command: &self.record.command,
        }
    }
}

impl Connected {
    fn terminal_rows(&self) -> Vec<Row> {
        self.mesh
            .scan("terminal/")
            .into_iter()
            .filter_map(|(key, value)| {
                let Key::Terminal(id) = key else { return None };
                Some(Row {
                    id: id.parse().ok()?,
                    record: TerminalRecord::from_bytes(&value).ok()?,
                })
            })
            .collect()
    }
    fn terminal_row(&self, id: Uuid) -> Result<Row, BeepError> {
        self.terminal_rows()
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(BeepError::NotFound)
    }
    fn terminal_view(&self, row: &Row, state: TerminalState) -> Terminal {
        let record = &row.record;
        Terminal {
            id: row.id,
            project: record.project.clone(),
            machine_id: record.machine.to_string(),
            machine_name: self
                .mesh
                .devices()
                .into_iter()
                .find(|device| device.device.id == record.machine)
                .map(|device| device.device.name)
                .unwrap_or_else(|| "Removed machine".into()),
            name: record.name.clone(),
            command: record.command.clone(),
            cwd: record.cwd.clone(),
            state,
            started_at: UNIX_EPOCH + Duration::from_millis(record.started_at),
        }
    }
    pub async fn all_terminals(&self) -> Vec<Terminal> {
        stream::iter(self.terminal_rows())
            .map(|row| async move {
                self.terminal_output(row.id)
                    .await
                    .map(|output| output.terminal)
                    .unwrap_or_else(|_| self.terminal_view(&row, TerminalState::Offline {}))
            })
            .buffer_unordered(4)
            .collect()
            .await
    }
    pub async fn terminal_output(&self, id: Uuid) -> Result<TerminalOutput, BeepError> {
        let row = self.terminal_row(id)?;
        let mut output = TerminalOutput {
            terminal: self.terminal_view(&row, TerminalState::Offline {}),
            lines: vec![],
            earlier: false,
        };
        let Ok(client) = self.client(&row.record.machine.to_string()).await else {
            return Ok(output);
        };
        if row.record.pty {
            let tasks = terminals::list_ptys(&client).await?;
            output.terminal.state = terminals::state_of(&row.kept(), &tasks);
        } else {
            match Tasks(&client)
                .ask(&TaskRequest::Status {
                    id: TaskId(row.record.task),
                })
                .await?
            {
                TaskReply::Status {
                    task,
                    lines,
                    earlier,
                } if task.command == row.record.command => {
                    output.terminal.state = terminals::state(task.state);
                    output.lines = lines;
                    output.earlier = earlier;
                }
                _ => output.terminal.state = TerminalState::Gone {},
            }
        }
        Ok(output)
    }
    pub async fn start_command(&self, start: CommandStart) -> Result<Terminal, BeepError> {
        let command = terminals::field("command", &start.command).map_err(BeepError::from)?;
        let cwd = terminals::field("folder", &start.cwd).map_err(BeepError::from)?;
        let name = terminals::field("name", &start.name).map_err(BeepError::from)?;
        let project = terminals::optional("project", &start.project).map_err(BeepError::from)?;
        let client = self.client(&start.machine_id).await?;
        let machine = start
            .machine_id
            .parse()
            .map_err(|_| crate::bad_request("Invalid machine identity"))?;
        let task = terminals::open(
            &client,
            Some(command.into()),
            Some(cwd.into()),
            Size::DEFAULT,
        )
        .await?;
        let row = Row {
            id: Uuid::new_v4(),
            record: TerminalRecord {
                project: project.into(),
                machine,
                task: task.id.0,
                pty: true,
                name: name.into(),
                command: task.command,
                cwd: cwd.into(),
                started_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |duration| {
                        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
                    }),
            },
        };
        let saved = self
            .mesh
            .set(
                Key::Terminal(row.id.to_string()),
                Some(row.record.to_bytes().map_err(anyhow::Error::from)?),
            )
            .await;
        if let Err(error) = saved {
            let _ = terminals::stop(&client, &row.kept()).await;
            return Err(error.into());
        }
        Ok(self.terminal_view(&row, TerminalState::Running {}))
    }
    pub async fn attach_terminal(&self, id: Uuid) -> Result<Arc<TerminalConnection>, BeepError> {
        let row = self.terminal_row(id)?;
        if !row.record.pty {
            return Ok(Arc::new(TerminalConnection {
                input: crate::AsyncMutex::new(TerminalInputStream::ViewOnly),
                output: crate::AsyncMutex::new(TerminalOutputStream::ViewOnly(true)),
            }));
        }
        let client = self.client(&row.record.machine.to_string()).await?;
        let tasks = terminals::list_ptys(&client).await?;
        if !tasks
            .iter()
            .any(|task| task.id.0 == row.record.task && task.command == row.record.command)
        {
            return Err(BeepError::NotFound);
        }
        let (tx, rx) =
            terminals::attach(&client, TaskId(row.record.task), 0, Size::DEFAULT).await?;
        Ok(Arc::new(TerminalConnection {
            input: crate::AsyncMutex::new(TerminalInputStream::Mesh(tx)),
            output: crate::AsyncMutex::new(TerminalOutputStream::Mesh(rx)),
        }))
    }
    pub async fn close_terminal(&self, id: Uuid) -> Result<(), BeepError> {
        let row = self.terminal_row(id)?;
        let client = self.client(&row.record.machine.to_string()).await?;
        terminals::stop(&client, &row.kept()).await?;
        self.mesh.set(Key::Terminal(id.to_string()), None).await?;
        Ok(())
    }
}
