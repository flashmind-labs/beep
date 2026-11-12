//! Open files, folders and web links with the operating system, and the
//! folder saved files go to.

use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

pub(crate) fn open(target: impl AsRef<std::ffi::OsStr>) -> crate::error::Result<()> {
    open::that_detached(target).map_err(anyhow::Error::from)?;
    Ok(())
}

/// Where a saved file goes: the folder picked in Settings, or Downloads
/// when none is or it is gone. On an iPhone, the app's Documents folder,
/// which the Files app shows under "On My iPhone".
pub(crate) fn saves_dir<R: Runtime>(app: &AppHandle<R>) -> crate::error::Result<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        let dir = app.path().home_dir()?.join("Documents");
        std::fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
        Ok(dir)
    }
    #[cfg(not(target_os = "ios"))]
    {
        let picked = app
            .try_state::<std::sync::Arc<crate::beep::Beep>>()
            .and_then(|beep| beep.save_folder())
            .filter(|folder| folder.is_dir());

        match picked {
            Some(folder) => Ok(folder),
            None => Ok(app.path().download_dir()?),
        }
    }
}

pub(crate) fn reveal(path: &std::path::Path) -> crate::error::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(anyhow::Error::from)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        open(path.parent().unwrap_or(path))
    }
}
