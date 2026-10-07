mod alarm;
mod browser;
mod contacts;
mod habits;
mod image;
mod location;
mod maps;
mod memory;
mod open_app;
mod progress;
mod suggest;

pub(crate) use suggest::{
    NAME as SUGGEST_REPLIES, SUGGESTING as SUGGESTING_REPLIES, replies_output,
};

use crate::{
    app::App,
    protocol::{DeviceKind, Run},
    user::User,
};
use flashmind::{tools::time::TimeTool, types::ToolRegistry};
use std::sync::Arc;

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
    registry.register(Arc::new(TimeTool));
    crate::cron::register_tools(
        &mut registry,
        app.store.clone(),
        user.id(),
        &run.session_id,
        timezone,
        None,
    );
    registry.register(Arc::new(location::GetLocationTool { user: user.clone() }));
    registry.register(Arc::new(image::SendImageTool));
    for variant in ["habit_create", "habit_log", "habit_status", "habit_delete"] {
        registry.register(Arc::new(habits::HabitTool {
            user: user.clone(),
            variant,
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
