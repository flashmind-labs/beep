mod alarm;
mod browser;
mod contacts;
mod file;
pub(crate) mod group_memory;
mod habits;
mod image;
mod location;
mod maps;
mod memory;
mod notify_user;
mod open_app;
mod places;
mod products;
mod progress;
pub(crate) mod skills;
mod suggest;

pub(crate) use file::SendFileTool;
pub(crate) use notify_user::NotifyUserTool;
pub(crate) use suggest::{
    NAME as SUGGEST_REPLIES, SUGGESTING as SUGGESTING_REPLIES, choice_output, replies_output,
};

use crate::{
    app::App,
    protocol::{DeviceAnswer, DeviceKind, Run},
    user::User,
};
use flashmind::{
    tools::time::TimeTool,
    types::{ToolRegistry, tool::ToolResult},
};
use std::sync::Arc;

/// The tool result for what the phone answered a query.
fn device_result(call_id: &str, answer: anyhow::Result<DeviceAnswer>) -> ToolResult {
    match answer {
        Ok(DeviceAnswer::Ok { output }) => ToolResult::success(call_id, output),
        Ok(DeviceAnswer::Err { message }) => ToolResult::failure(call_id, message),
        Err(e) => ToolResult::failure(call_id, format!("Could not reach the user's device: {e}")),
    }
}

pub fn registry(
    app: Arc<App>,
    run: &Run,
    user: &Arc<User>,
    timezone: Option<&str>,
    origin: DeviceKind,
) -> ToolRegistry {
    let run_id = run.id.as_str();
    let mut registry = ToolRegistry::new();
    memory::register(&mut registry, user, &run.session_id);
    registry.register(Arc::new(skills::SkillViewTool {
        store: app.store.clone(),
        owner: user.skill_owner(),
    }));
    registry.register(Arc::new(TimeTool));
    crate::cron::register_tools(
        &mut registry,
        app.store.clone(),
        user.id(),
        &run.session_id,
        timezone,
    );
    registry.register(Arc::new(location::GetLocationTool { user: user.clone() }));
    registry.register(Arc::new(image::SendImageTool));
    registry.register(Arc::new(products::ShowProductsTool));
    for kind in habits::HabitToolKind::ALL {
        registry.register(Arc::new(habits::HabitTool {
            user: user.clone(),
            kind,
        }));
    }
    for kind in places::PlaceToolKind::ALL {
        registry.register(Arc::new(places::PlaceTool {
            user: user.clone(),
            kind,
        }));
    }
    registry.register(Arc::new(progress::TaskProgressTool));
    // These act on the phone, which a run started on a computer cannot reach.
    if origin == (DeviceKind::Phone {}) {
        registry.register(Arc::new(open_app::OpenAppTool));
        registry.register(Arc::new(alarm::SetAlarmTool));
        registry.register(Arc::new(browser::OpenBrowserTool));
        registry.register(Arc::new(maps::OpenMapsTool { user: user.clone() }));
        registry.register(Arc::new(browser::BrowserTool {
            app: app.clone(),
            run_id: run_id.to_owned(),
        }));
        registry.register(Arc::new(contacts::ContactsTool {
            app,
            run_id: run_id.to_owned(),
        }));
    }
    registry
}
