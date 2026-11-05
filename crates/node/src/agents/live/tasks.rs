//! The commands an agent runs in the background, from its `async_task_*`
//! updates, and the tool calls that started them.

use super::{Live, MAX_LINE, MAX_TASKS, content::cut, tools::ToolCall};
use crate::agents::acp::AsyncTask;
use agent_client_protocol::schema::v1::{Meta, ToolCallId};
use beep_proto::node::agent::{Task, TaskState};

/// The task type of a turn the agent started on its own, which is not a
/// command.
const TURN: &str = "turn";

impl Live {
    /// Applies one `async_task_*` update. True when the tasks changed.
    pub(crate) fn apply_task(&mut self, update: AsyncTask) -> bool {
        let before = self.info.state.tasks.clone();
        let call = match update {
            AsyncTask::Spawned {
                async_task_id,
                name,
                task_type,
                description,
                can_stop,
                output_file_path,
                tool_call_id,
            } => {
                if task_type == TURN {
                    return false;
                }
                let task = Task {
                    id: async_task_id,
                    name: cut(&name, MAX_LINE),
                    description: cut(&description, MAX_LINE),
                    state: TaskState::Running,
                    can_stop,
                    output_file: output_file_path,
                    tool_call: tool_call_id.clone(),
                    summary: None,
                    entry: None,
                };
                let tasks = &mut self.info.state.tasks;
                match tasks.iter_mut().find(|known| known.id == task.id) {
                    Some(known) => *known = task,
                    None => {
                        if tasks.len() >= MAX_TASKS {
                            // The oldest ended one goes; with none, the oldest.
                            let gone = tasks.iter().position(|task| task.state.ended());
                            tasks.remove(gone.unwrap_or(0));
                        }
                        tasks.push(task);
                    }
                }
                tool_call_id
            }
            AsyncTask::Progress {
                async_task_id,
                description,
                output_file_path,
                tool_call_id,
            } => {
                let Some(task) = self.task_mut(&async_task_id) else {
                    return false;
                };
                if let Some(description) = description {
                    task.description = cut(&description, MAX_LINE);
                }
                task.output_file = output_file_path.or(task.output_file.take());
                task.tool_call = tool_call_id.or(task.tool_call.take());
                task.tool_call.clone()
            }
            AsyncTask::StateUpdate {
                async_task_id,
                state,
                summary,
                output_file_path,
                tool_call_id,
            } => {
                let Some(task) = self.task_mut(&async_task_id) else {
                    return false;
                };
                task.state = TaskState::from_acp(&state);
                task.summary = summary.map(|summary| cut(&summary, MAX_LINE));
                task.output_file = output_file_path.or(task.output_file.take());
                task.tool_call = tool_call_id.or(task.tool_call.take());
                task.tool_call.clone()
            }
        };
        if let Some(call) = call {
            self.follow_background(&ToolCallId::new(call));
        }
        self.info.state.tasks != before
    }

    /// The session ended, and with it the commands its agent ran.
    pub(crate) fn end_tasks(&mut self) {
        let mut calls = Vec::new();
        for task in &mut self.info.state.tasks {
            if !task.state.ended() {
                task.state = TaskState::Stopped;
                calls.extend(task.tool_call.clone());
            }
        }
        for call in calls {
            self.follow_background(&ToolCallId::new(call));
        }
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.info.state.tasks.iter_mut().find(|task| task.id == id)
    }

    /// Whether a task of tool call `call` still runs.
    fn task_runs_for(&self, call: &ToolCallId) -> bool {
        self.info
            .state
            .tasks
            .iter()
            .any(|task| task.tool_call.as_deref() == Some(&*call.0) && !task.state.ended())
    }

    /// Whether a task of tool call `call` ended.
    fn task_ended_for(&self, call: &ToolCallId) -> bool {
        self.info
            .state
            .tasks
            .iter()
            .any(|task| task.tool_call.as_deref() == Some(&*call.0) && task.state.ended())
    }

    /// The tool call `call` went to the background: its entry reads as
    /// running while its task does, and as it ended after.
    fn follow_background(&mut self, call: &ToolCallId) {
        let running = self.task_runs_for(call);
        let Some(tool) = self.tools.get_mut(call) else {
            return;
        };
        let entry = tool.entry;
        for task in &mut self.info.state.tasks {
            if task.tool_call.as_deref() == Some(&*call.0) {
                task.entry = Some(entry);
            }
        }
        if tool.backgrounded != running {
            tool.backgrounded = running;
            self.write_tool(call);
        }
    }

    /// The update of tool call `tool` marks its command as gone to the
    /// background (the adapter's `backgrounded`), unless its task ended
    /// before the mark came.
    pub(super) fn mark_backgrounded(
        &self,
        tool: &mut ToolCall,
        id: &ToolCallId,
        meta: Option<&Meta>,
    ) {
        let marked = meta
            .and_then(|meta| meta.get("jetbrains"))
            .and_then(|air| air.pointer("/air/asyncTasks/backgrounded"))
            .and_then(|marked| marked.as_bool())
            .unwrap_or(false);
        if marked {
            tool.backgrounded = !self.task_ended_for(id);
        }
    }
}
