use tauri::AppHandle;

/// What plugins placed, as the page reads it.
#[cfg(desktop)]
type Shown = Vec<beep_plugins::Placement>;
#[cfg(mobile)]
type Shown = Vec<serde_json::Value>;

/// The topics plugins follow, as the page reads them.
#[cfg(desktop)]
type Wanted = Vec<beep_plugins::Topic>;
#[cfg(mobile)]
type Wanted = Vec<serde_json::Value>;

/// An update the page sends, checked by the host.
#[cfg(desktop)]
type Update = beep_plugins::Publication;
#[cfg(mobile)]
type Update = serde_json::Value;

/// What the page reports about the slots on it, as the host reads it.
#[cfg(desktop)]
type Instances = beep_plugins::InstanceBatch;
#[cfg(mobile)]
type Instances = serde_json::Value;

/// What the user did to a control a plugin drew.
#[cfg(desktop)]
type Event = beep_plugins::UiEvent;
#[cfg(mobile)]
type Event = serde_json::Value;

/// Every placement of every running plugin. Empty on a phone and when no host runs.
#[tauri::command]
pub(crate) fn plugins_placements(app: AppHandle) -> Shown {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        app.try_state::<beep_plugins::Host>()
            .map(|host| host.placements())
            .unwrap_or_default()
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Shown::new()
    }
}

/// The topics running plugins follow now. Empty on a phone and when no host runs.
#[tauri::command]
pub(crate) fn plugins_wanted(app: AppHandle) -> Wanted {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        app.try_state::<beep_plugins::Host>()
            .map(|host| host.wanted())
            .unwrap_or_default()
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Wanted::new()
    }
}

/// Hands an update the page heard to the plugins that follow its topic. The host refuses
/// topics no plugin may follow and updates over its size limit. A phone does nothing.
#[tauri::command]
pub(crate) fn plugins_publish(app: AppHandle, update: Update) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        match app.try_state::<beep_plugins::Host>() {
            Some(host) => host.publish(&update).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, update);
        Ok(())
    }
}

/// Tells the plugins which slots are on the page and with what context: what changed
/// since the last batch, or everything after a `reset`. The host refuses batches over its
/// limits. A phone does nothing.
#[tauri::command]
pub(crate) fn plugins_instances(app: AppHandle, batch: Instances) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        match app.try_state::<beep_plugins::Host>() {
            Some(host) => host.set_instances(batch).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, batch);
        Ok(())
    }
}

/// Hands a press, switch or text change to the plugin that drew the control. The host
/// ignores an event for a control that is no longer shown and refuses text over 4 KB. A
/// phone does nothing.
#[tauri::command]
pub(crate) fn plugins_event(app: AppHandle, event: Event) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        match app.try_state::<beep_plugins::Host>() {
            Some(host) => host.event(event).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, event);
        Ok(())
    }
}
