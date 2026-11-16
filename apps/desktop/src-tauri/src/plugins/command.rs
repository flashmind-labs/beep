use tauri::ipc::Response;
use tauri::{AppHandle, WebviewWindow};

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

/// Each plugin's status, as the page reads it.
#[cfg(desktop)]
type Statuses = Vec<beep_plugins::PluginStatus>;
#[cfg(mobile)]
type Statuses = Vec<serde_json::Value>;

/// The style tokens plugins set, as the page reads them.
#[cfg(desktop)]
type Styled = beep_plugins::ResolvedStyle;
#[cfg(mobile)]
type Styled = serde_json::Value;

/// The name of an image placements use, a 64 digit hex hash.
#[cfg(desktop)]
type Hash = beep_plugins::AssetHash;
#[cfg(mobile)]
type Hash = String;

/// What the user did to a control a plugin drew.
#[cfg(desktop)]
type Event = beep_plugins::UiEvent;
#[cfg(mobile)]
type Event = serde_json::Value;

/// The `tokens` the enabled style plugins set, each with the value of the one turned on
/// last, and the `conflicts`: each `token` more than one sets, with the `plugins` in the
/// order they apply. Nothing on a phone and when no host runs.
#[tauri::command]
pub(crate) fn plugins_style(app: AppHandle) -> Styled {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        app.try_state::<beep_plugins::Host>()
            .map(|host| host.style())
            .unwrap_or_default()
    }
    #[cfg(mobile)]
    {
        let _ = app;
        serde_json::json!({ "tokens": {}, "conflicts": [] })
    }
}

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

/// The bytes of an image a shown placement names, by hash. The page asks once per hash and
/// keeps the answer. Fails when no placement uses the image now. A phone has none.
#[tauri::command]
pub(crate) fn plugins_asset(app: AppHandle, hash: Hash) -> Result<Response, String> {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        let asset = app
            .try_state::<beep_plugins::Host>()
            .and_then(|host| host.asset(&hash));
        match asset {
            Some(asset) => Ok(Response::new(asset.bytes().to_vec())),
            None => Err(format!("no plugin image {hash}")),
        }
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(format!("no plugin image {hash}"))
    }
}

/// Says whether the calling window is visible. While no window is, plugins draw nothing:
/// their timers and updates wait. A phone does nothing.
#[tauri::command]
pub(crate) fn plugins_visible(app: AppHandle, window: WebviewWindow, visible: bool) {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        if let Some(host) = app.try_state::<beep_plugins::Host>() {
            host.set_visible(window.label(), visible);
        }
    }
    #[cfg(mobile)]
    {
        let _ = (app, window, visible);
    }
}

/// Each plugin's id, name, version, description, the permissions it asks for and the ones
/// the user allowed, its source (`local`, `git` or `dev`), whether it is turned on, its state
/// (`waiting`, `running`, `paused`, `disabled`, `needs-approval` with the `missing` permissions,
/// or `stopped` with a `reason`), CPU time over the last minute and pending timers. Empty on
/// a phone.
#[tauri::command]
pub(crate) fn plugins_status(app: AppHandle) -> Statuses {
    #[cfg(desktop)]
    {
        use tauri::Manager;

        app.try_state::<beep_plugins::Host>()
            .map(|host| host.status())
            .unwrap_or_default()
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Statuses::new()
    }
}
