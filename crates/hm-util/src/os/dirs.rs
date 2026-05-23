use std::path::PathBuf;
use tokio;
use io;


/// Platform home directory (`~/` on Unix, `C:\Users\<user>` on Windows).
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Platform config directory (`~/.config` on Posix, `%APPDATA%` on Windows).
///
/// Note this doesn't respect XDG.
pub async fn user_config_dir() -> io::Result<Option<PathBuf>> {
    #[cfg(unix)]
    {
        home_dir().and_then(async move |d: PathBuf| {
            let d: PathBuf = d.join(".config");
            if tokio::fs::try_exists(d).await? {
                Some(d)
            } else {
                None
            }
        })
    }

    #[cfg(windows)]
    {
        Ok(dirs::config_dir())
    }
}

/// Platform-equivalent of /etc/.
pub async fn sys_config_dir() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/etc")
    }

    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or("C:\\ProgramData")
    }
}
