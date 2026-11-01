//! When a project tool may act: the Project app's level, a saved "always",
//! and the call the user just allowed. Unlike `permissions::guard`, the
//! tools decide what each call asks for, since a plan of three tasks or an
//! issue on a public repository always asks, whatever the level.

use super::model::Visibility;
use crate::{
    permissions::{ApprovalPayload, PROJECT_APP, Rules, ToolSlug},
    protocol::{Approval, ApprovalField, PermissionLevel},
};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::sync::Arc;

/// What a call asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Asks {
    /// Runs under Allow or a saved Always, else asks first.
    ByLevel,
    /// Asks first every time.
    Always,
}

/// What the gate says about one call.
#[derive(Debug)]
pub(crate) enum Gate {
    Run,
    /// Not done, with the result the model reads.
    Refuse(String),
    /// Stops the run on this card until the user answers.
    Park(ToolResult),
}

/// The project's rules for one turn.
pub(crate) struct ProjectGate {
    rules: Rules,
    /// The call the user just allowed once.
    approved: Option<String>,
    visibility: Visibility,
}

impl ProjectGate {
    pub(crate) fn new(rules: Rules, approved: Option<String>, visibility: Visibility) -> Self {
        Self {
            rules,
            approved,
            visibility,
        }
    }

    fn level(&self) -> PermissionLevel {
        self.rules.level(PROJECT_APP.slug)
    }

    pub(crate) fn check(
        &self,
        call_id: &str,
        tool: &str,
        asks: Asks,
        card: impl FnOnce() -> Approval,
    ) -> Gate {
        let level = self.level();
        if level == (PermissionLevel::ReadOnly {}) {
            return Gate::Refuse(
                "Not done: the user set Project to read only. Tell them, and that they can \
                 change it under Apps in Beep's menu."
                    .to_owned(),
            );
        }
        let approved = self.approved.as_deref() == Some(call_id);
        let runs = approved
            || match asks {
                Asks::Always => false,
                Asks::ByLevel => {
                    level == (PermissionLevel::Allow {})
                        || self
                            .rules
                            .allows(PROJECT_APP.slug, ToolSlug::new(tool).as_str())
                }
            };
        if runs {
            return Gate::Run;
        }
        Gate::Park(ToolResult::interrupt(
            call_id,
            Arc::new(ApprovalPayload { approval: card() }),
        ))
    }

    /// Whether a worker may push its own branch and open a pull request
    /// without asking: under Allow, and only on a private repository.
    pub(crate) fn outside_allowed(&self) -> bool {
        self.level() == (PermissionLevel::Allow {}) && self.visibility == Visibility::Private
    }
}

/// `exec` in a project chat, which asks first unless Project is set to Allow.
pub(crate) struct GatedExec {
    pub(crate) exec: Arc<dyn Tool>,
    pub(crate) gate: Arc<ProjectGate>,
}

#[async_trait]
impl Tool for GatedExec {
    fn name(&self) -> &str {
        self.exec.name()
    }

    fn description(&self) -> &str {
        self.exec.description()
    }

    fn parameters(&self) -> Value {
        self.exec.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let card = || {
            let text = |key: &str| {
                ctx.args
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            let mut fields = vec![ApprovalField {
                label: "Command".into(),
                value: text("command"),
            }];
            let machine = text("in");
            if !machine.is_empty() {
                fields.push(ApprovalField {
                    label: "Machine".into(),
                    value: machine,
                });
            }
            Approval {
                app: PROJECT_APP.name.to_owned(),
                action: "Run a command".into(),
                fields,
            }
        };
        match self
            .gate
            .check(ctx.tool_call_id, self.exec.name(), Asks::ByLevel, card)
        {
            Gate::Run => self.exec.execute(ctx).await,
            Gate::Refuse(reason) => Ok(ToolResult::success(ctx.tool_call_id, reason)),
            Gate::Park(result) => Ok(result),
        }
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.exec.timeout_secs()
    }

    fn max_output_bytes(&self) -> usize {
        self.exec.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.exec.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.exec.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.exec.concurrent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AppPermissionRow;

    fn rules(level: PermissionLevel, allowed: &[&str]) -> Rules {
        Rules::from_rows(vec![AppPermissionRow {
            app: "project".into(),
            level,
            allowed: allowed.iter().map(|tool| (*tool).to_owned()).collect(),
        }])
    }

    fn card() -> Approval {
        Approval {
            app: "Project".into(),
            action: "Create 1 task".into(),
            fields: Vec::new(),
        }
    }

    fn parks(gate: &ProjectGate, call: &str, asks: Asks) -> bool {
        match gate.check(call, "project_plan", asks, card) {
            Gate::Park(result) => {
                let payload = result.payload().expect("a parked call has a payload");
                payload.as_any().downcast_ref::<ApprovalPayload>().is_some()
            }
            Gate::Run => false,
            Gate::Refuse(reason) => panic!("refused: {reason}"),
        }
    }

    #[test]
    fn gate_rules() {
        let ask = ProjectGate::new(Rules::default(), None, Visibility::Private);
        assert!(parks(&ask, "c1", Asks::ByLevel));
        let approved = ProjectGate::new(Rules::default(), Some("c1".into()), Visibility::Private);
        assert!(!parks(&approved, "c1", Asks::ByLevel));
        assert!(parks(&approved, "c2", Asks::ByLevel));
        assert!(!parks(&approved, "c1", Asks::Always));
        let always = ProjectGate::new(
            rules(PermissionLevel::Ask {}, &["PROJECT_PLAN"]),
            None,
            Visibility::Private,
        );
        assert!(!parks(&always, "c1", Asks::ByLevel));
        assert!(parks(&always, "c1", Asks::Always));
        let allow = ProjectGate::new(
            rules(PermissionLevel::Allow {}, &[]),
            None,
            Visibility::Private,
        );
        assert!(!parks(&allow, "c1", Asks::ByLevel));
        assert!(parks(&allow, "c1", Asks::Always));
        assert!(allow.outside_allowed());
        assert!(!ask.outside_allowed());
        let read_only = ProjectGate::new(
            rules(PermissionLevel::ReadOnly {}, &[]),
            Some("c1".into()),
            Visibility::Private,
        );
        for asks in [Asks::ByLevel, Asks::Always] {
            assert!(matches!(
                read_only.check("c1", "project_plan", asks, card),
                Gate::Refuse(_)
            ));
        }
    }

    #[test]
    fn public_repos_keep_workers_asking() {
        let allow = ProjectGate::new(
            rules(PermissionLevel::Allow {}, &[]),
            None,
            Visibility::Public,
        );
        assert!(!allow.outside_allowed());
    }
}
