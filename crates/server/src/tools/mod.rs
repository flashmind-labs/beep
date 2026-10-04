mod alarm;
mod browser;
mod contacts;
mod habits;
mod location;
mod maps;
mod memory;
mod notifications;
mod open_app;
mod progress;
mod screen;
mod suggest;

use crate::{app::App, protocol::Run, user::User};
use flashmind::{tools::time::TimeTool, types::ToolRegistry};
use std::sync::Arc;

pub fn registry(app: Arc<App>, run: &Run, user: &Arc<User>) -> ToolRegistry {
    let run_id = run.id.as_str();
    let mut registry = ToolRegistry::new();
    memory::register(&mut registry, user, &run.session_id);
    registry.register(Arc::new(TimeTool));
    crate::cron::register_tools(&mut registry, app.store.clone(), user.id(), None);
    registry.register(Arc::new(open_app::OpenAppTool));
    registry.register(Arc::new(alarm::SetAlarmTool));
    registry.register(Arc::new(location::GetLocationTool { user: user.clone() }));
    registry.register(Arc::new(browser::OpenBrowserTool));
    registry.register(Arc::new(maps::OpenMapsTool { user: user.clone() }));
    registry.register(Arc::new(notifications::GetNotificationsTool {
        user: user.clone(),
    }));
    for variant in ["habit_create", "habit_log", "habit_status", "habit_delete"] {
        registry.register(Arc::new(habits::HabitTool {
            user: user.clone(),
            variant,
        }));
    }
    registry.register(Arc::new(progress::TaskProgressTool));
    registry.register(Arc::new(suggest::SuggestRepliesTool));
    registry.register(Arc::new(browser::BrowserTool {
        app: app.clone(),
        run_id: run_id.to_owned(),
    }));
    registry.register(Arc::new(screen::ReadScreenTool {
        app: app.clone(),
        run_id: run_id.to_owned(),
    }));
    registry.register(Arc::new(contacts::ContactsTool {
        app,
        run_id: run_id.to_owned(),
    }));
    registry
}
