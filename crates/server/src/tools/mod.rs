mod alarm;
mod attach;
mod browser;
mod cards;
mod charts;
mod chat;
mod contacts;
pub(crate) mod file;
pub(crate) mod group_memory;
mod habits;
mod image;
mod location;
pub(crate) mod machines;
mod maps;
pub(crate) mod media;
mod memory;
mod notify_user;
mod open_app;
mod places;
mod products;
mod progress;
pub(crate) mod projects;
mod route;
pub(crate) mod skills;
pub(crate) mod stocks;
pub(crate) mod suggest;

pub(crate) use attach::AttachTool;
pub(crate) use file::SendFileTool;
pub(crate) use image::{NAME as SEND_IMAGE, SendImageTool};
pub(crate) use memory::{NAME as MEMORY, recall_tool};
pub(crate) use notify_user::{NAME as NOTIFY_USER, NotifyUserTool};
pub(crate) use route::GetRouteTool;
pub use route::Transitous;
pub(crate) use suggest::{
    NAME as SUGGEST_REPLIES, SUGGESTING as SUGGESTING_REPLIES, choice_output, replies_output,
};

use crate::{
    app::{App, Draws},
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
    draws: Draws,
) -> ToolRegistry {
    let run_id = run.id.as_str();
    let mut registry = ToolRegistry::new();
    memory::register(&mut registry, user, &run.session_id);
    registry.register(Arc::new(skills::SkillTool {
        store: app.store.clone(),
        owner: user.skill_owner(),
        writes: true,
    }));
    registry.register(Arc::new(TimeTool));
    if let Ok(chat) = run.session_id.parse() {
        registry.register(Arc::new(chat::ChatSearchTool {
            user: user.clone(),
            chat,
        }));
        registry.register(Arc::new(chat::ChatReadTool {
            user: user.clone(),
            chat,
        }));
    }
    crate::cron::register_tools(
        &mut registry,
        app.store.clone(),
        user.id(),
        &run.session_id,
        timezone,
    );
    registry.register(Arc::new(location::GetLocationTool { user: user.clone() }));
    registry.register(Arc::new(products::ShowProductsTool));
    if draws.cards {
        registry.register(Arc::new(cards::ShowCardsTool { user: user.clone() }));
    }
    registry.register(Arc::new(habits::HabitTool { user: user.clone() }));
    for kind in places::PlaceToolKind::ALL {
        registry.register(Arc::new(places::PlaceTool {
            user: user.clone(),
            kind,
            chat: run.session_id.parse().ok(),
        }));
    }
    registry.register(Arc::new(progress::TaskProgressTool));
    registry.register(Arc::new(projects::ProjectsTool {
        app: app.clone(),
        user: user.clone(),
    }));
    if draws.charts {
        registry.register(Arc::new(charts::ShowChartTool));
        registry.register(Arc::new(charts::ShowStockChartTool));
    }
    // These act on the phone, which a run started on a computer cannot reach.
    if origin == (DeviceKind::Phone {}) {
        registry.register(Arc::new(open_app::OpenAppTool));
        registry.register(Arc::new(alarm::SetAlarmTool));
        registry.register(Arc::new(browser::OpenBrowserTool));
        registry.register(Arc::new(maps::OpenMapsTool { user: user.clone() }));
        registry.register(Arc::new(contacts::ContactsTool {
            app,
            run_id: run_id.to_owned(),
        }));
    }
    registry
}
