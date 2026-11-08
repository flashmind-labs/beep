//! When a project tool may act: the Project app's level, a saved "always" and the call just
//! allowed. Unlike `permissions::guard`, each tool decides what its call asks for.

use crate::{
    apps::permissions::{ApprovalPayload, PROJECT_APP, Rules, ToolSlug},
    protocol::{Approval, PermissionLevel},
};
use flashmind::types::tool::ToolResult;
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
}

impl ProjectGate {
    pub(crate) fn new(rules: Rules, approved: Option<String>) -> Self {
        Self { rules, approved }
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
                            .allows(PROJECT_APP.slug, ToolSlug::new(tool).as_ref())
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
}

#[cfg(test)]
#[path = "../tests/projects_gate.rs"]
mod tests;
