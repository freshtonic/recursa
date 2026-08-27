//! Platform browser process launching and bounded cleanup.

use std::io;
use std::process::{Child, Command};

/// Opens the system browser only after the loopback listener is ready.
pub(super) fn open(url: &str) -> io::Result<Session> {
    #[cfg(target_os = "macos")]
    let child = Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    let child = Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let child = Command::new("xdg-open").arg(url).spawn()?;
    Ok(Session { child })
}

/// Short-lived platform opener process retained until server shutdown.
pub(super) struct Session {
    /// Child handle used to reap or terminate an opener that did not exit.
    child: Child,
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
