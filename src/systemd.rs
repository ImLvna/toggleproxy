use anyhow::{anyhow, Result};

#[cfg(not(target_os = "linux"))]
pub fn systemd_restart() -> Result<()> {
    Err(anyhow!("Systemd is not supported on this platform"))
}

#[cfg(target_os = "linux")]
use systemctl::reload_or_restart;
#[cfg(target_os = "linux")]
pub fn systemd_restart() -> Result<()> {
    match reload_or_restart("toggleproxy") {
        Ok(_) => Ok(()),
        Err(err) => {
            error!("Failed to restart toggleproxy");
            trace!("{}", err);
            Err(anyhow!("Failed to restart toggleproxy"))
        }
    }
}
