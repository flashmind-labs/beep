mod browser;
mod contacts;
mod habits;
mod location;
mod maps;
mod memory;
mod notifications;
mod open_app;
mod progress;
mod suggest;

use crate::app::App;
use flashmind::types::ToolRegistry;
use std::sync::Arc;

pub fn registry(app: Arc<App>, run_id: &str, user: &str) -> ToolRegistry {
    let store = app.store.clone();
    let mut registry = ToolRegistry::new();
    for name in [
        "memory_store",
        "memory_recall",
        "memory_forget",
        "memory_update",
        "get_time",
    ] {
        registry.register(Arc::new(memory::MemoryTool {
            store: store.clone(),
            memories: app.memories.clone(),
            user: user.into(),
            name,
        }));
    }
    crate::cron::register_tools(&mut registry, store.clone(), user, None);
    registry.register(Arc::new(open_app::OpenAppTool));
    registry.register(Arc::new(location::GetLocationTool {
        store: store.clone(),
        user: user.into(),
    }));
    registry.register(Arc::new(browser::OpenBrowserTool));
    registry.register(Arc::new(maps::OpenMapsTool));
    registry.register(Arc::new(notifications::GetNotificationsTool {
        store: store.clone(),
        user: user.into(),
    }));
    for variant in ["habit_create", "habit_log", "habit_status", "habit_delete"] {
        registry.register(Arc::new(habits::HabitTool {
            store: store.clone(),
            user: user.into(),
            variant,
        }));
    }
    registry.register(Arc::new(progress::TaskProgressTool));
    registry.register(Arc::new(suggest::SuggestRepliesTool));
    registry.register(Arc::new(browser::BrowserTool {
        app: app.clone(),
        run_id: run_id.to_owned(),
    }));
    registry.register(Arc::new(contacts::ContactsTool {
        app,
        run_id: run_id.to_owned(),
    }));
    registry
}
