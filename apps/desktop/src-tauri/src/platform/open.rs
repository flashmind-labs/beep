//! Open files, folders and web links with the operating system.

pub(crate) fn open(target: impl AsRef<std::ffi::OsStr>) -> crate::error::Result<()> {
    open::that_detached(target).map_err(anyhow::Error::from)?;
    Ok(())
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
