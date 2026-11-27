//! The commands an agent runs in the background, from its `async_task_*`
//! updates, and the tool calls that started them.

use super::{
    AutonomousTurn, Live,
    content::cut,
    limits::{MAX_LINE, MAX_TASKS},
    state::ToolCall,
};
use crate::agents::acp::AsyncTask;
use agent_client_protocol::v1::schema::{Meta, ToolCallId};
use beep_proto::node::agent::{Status, Task, TaskState};
use std::time::Instant;

/// The task type of a turn the agent started on its own, which is not a
/// command.
const TURN: &str = "turn";

impl Live {
    /// Applies one `async_task_*` update. True when the tasks changed.
    pub(in crate::agents) fn apply_task(&mut self, update: AsyncTask) -> bool {
        let before = self.info.state.tasks.clone();
        let subagents = self.info.state.subagents;
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
                    let shown = self.autonomous_turn.as_ref().is_some_and(|turn| turn.shown);
                    self.autonomous_turn = Some(AutonomousTurn {
                        task: async_task_id,
                        shown,
                    });
                    return self.show_autonomous_turn();
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
                if self
                    .autonomous_turn
                    .as_ref()
                    .is_some_and(|turn| turn.task == async_task_id)
                {
                    let state = state.parse().unwrap_or(TaskState::Unknown);

                    if !state.ended() {
                        return false;
                    }

                    // Claude Code works on its own once a background
                    // subagent reports; that work ended.
                    let shown = self.autonomous_turn.take().is_some_and(|turn| turn.shown);
                    if shown && self.info.state.status == Status::Working {
                        self.info.state.status = Status::Idle;
                        self.info.state.summary.clear();
                    }

                    let subagents = self.info.state.subagents;
                    self.end_background_subagents();
                    return shown || self.info.state.subagents != subagents;
                }

                let Some(task) = self.task_mut(&async_task_id) else {
                    return false;
                };
                task.state = state.parse().unwrap_or(TaskState::Unknown);
                task.summary = summary.map(|summary| cut(&summary, MAX_LINE));
                task.output_file = output_file_path.or(task.output_file.take());
                task.tool_call = tool_call_id.or(task.tool_call.take());
                task.tool_call.clone()
            }
        };

        if let Some(call) = call {
            self.follow_background(&ToolCallId::new(call));
        }

        self.count_subagents();
        self.info.state.tasks != before || self.info.state.subagents != subagents
    }

    /// An idle session shows Working while a turn the agent started on its
    /// own runs, such as Claude Code's after a background task reported.
    /// True when the status changed.
    pub(crate) fn show_autonomous_turn(&mut self) -> bool {
        let Some(turn) = &mut self.autonomous_turn else {
            return false;
        };

        if self.info.state.status != Status::Idle {
            return false;
        }

        turn.shown = true;
        self.active_at = Some(Instant::now());
        self.info.state.status = Status::Working;
        self.info.state.summary = "Working on its own".into();
        true
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

    /// Whether the session works, its subagents run, or a command it left
    /// in the background still runs.
    pub(crate) fn works(&self) -> bool {
        let state = &self.info.state;
        state.runs() || state.tasks.iter().any(|task| !task.state.ended())
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.info.state.tasks.iter_mut().find(|task| task.id == id)
    }

    /// Whether a task of tool call `call` has ended (`ended`) or still runs.
    fn task_for(&self, call: &ToolCallId, ended: bool) -> bool {
        self.info
            .state
            .tasks
            .iter()
            .any(|task| task.tool_call.as_deref() == Some(&*call.0) && task.state.ended() == ended)
    }

    /// The tool call `call` went to the background: its entry reads as
    /// running while its task does, and as it ended after.
    fn follow_background(&mut self, call: &ToolCallId) {
        let running = self.task_for(call, false);
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
            // A background `Agent` call counts as a running subagent only
            // while its task runs.
            self.count_subagents();
        }
    }

    /// The update of tool call `tool` marks its command as gone to the
    /// background (the adapter's `backgrounded`), unless its task ended
    /// before the mark came. A Claude Code `Agent` call whose result says
    /// it launched the subagent in the background (`isAsync`) reads as
    /// running too, until `end_background_subagents`: Claude Code reports
    /// no task for a subagent.
    pub(super) fn mark_backgrounded(
        &self,
        tool: &mut ToolCall,
        id: &ToolCallId,
        meta: Option<&Meta>,
    ) {
        let flag = |key: &str, pointer: &str| {
            meta.and_then(|meta| meta.get(key))
                .and_then(|value| value.pointer(pointer))
                .and_then(|flag| flag.as_bool())
                .unwrap_or(false)
        };

        if flag("jetbrains", "/air/asyncTasks/backgrounded") {
            tool.backgrounded = !self.task_for(id, true);
        }

        if tool.subagent.as_ref().is_some_and(|sub| !sub.codex)
            && flag("claudeCode", "/toolResponse/isAsync")
        {
            tool.backgrounded = true;
        }
    }

    /// The background subagents Claude Code started have ended: a turn
    /// ended on its own, which Claude Code holds until they do, or the
    /// work it did on its own after their reports ended. Those whose task
    /// still runs keep running.
    pub(crate) fn end_background_subagents(&mut self) {
        let ended: Vec<ToolCallId> = self
            .tools
            .iter()
            .filter(|(id, tool)| {
                tool.backgrounded
                    && tool.subagent.as_ref().is_some_and(|sub| !sub.codex)
                    && !self.task_for(id, false)
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in &ended {
            if let Some(tool) = self.tools.get_mut(id) {
                tool.backgrounded = false;
            }
            self.write_tool(id);
        }

        self.count_subagents();
    }
}
