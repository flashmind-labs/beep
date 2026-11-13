use tauri::AppHandle;

/// What plugins placed, as the page reads it.
#[cfg(desktop)]
type Shown = Vec<beep_plugins::Placement>;
#[cfg(mobile)]
type Shown = Vec<serde_json::Value>;

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
