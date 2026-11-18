//! Per-config-directory process lock, plus an authenticated loopback message that
//! reveals the UI or forwards a subscription link for confirmation.
use crate::error::{Error, Result};
use crate::subscription_links::{self, MAX_LINK_BYTES};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Address {
    port: u16,
    token: String,
}

pub struct Instance {
    // The OS releases this lock even after an abnormal exit. Never unlink it:
    // doing so would let another process lock a different inode.
    _lock: File,
    listener: TcpListener,
    token: String,
}

fn lock_file(path: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

impl Instance {
    /// None means an existing process accepted this launch.
    pub fn acquire(
        dir: &Path,
        background: bool,
        handoff: bool,
        link: Option<&str>,
    ) -> Result<Option<Self>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("instance.lock");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match lock_file(&path)? {
                Some(file) => {
                    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
                    let token = uuid::Uuid::new_v4().simple().to_string();
                    let address = Address {
                        port: listener.local_addr()?.port(),
                        token: token.clone(),
                    };
                    // Windows byte-range locks prevent reading discovery data in the lock file.
                    crate::util::write_atomic(
                        &dir.join("instance.json"),
                        &serde_json::to_vec(&address)?,
                    )?;
                    return Ok(Some(Self {
                        _lock: file,
                        listener,
                        token,
                    }));
                }
                None => {
                    // Elevation handoffs wait for the old owner to finish core cleanup.
                    if !handoff {
                        let address = std::fs::read(dir.join("instance.json"))
                            .ok()
                            .and_then(|b| serde_json::from_slice::<Address>(&b).ok());
                        if let Some(address) = address {
                            if let Ok(mut stream) = TcpStream::connect_timeout(
                                &std::net::SocketAddr::from(([127, 0, 0, 1], address.port)),
                                Duration::from_millis(500),
                            ) {
                                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                                let request = match link {
                                    Some(link) => format!("import:{link}"),
                                    None if background => "background".into(),
                                    None => "open".into(),
                                };
                                if write!(stream, "{}:{request}", address.token).is_ok() {
                                    if link.is_none() {
                                        return Ok(None);
                                    }
                                    // The startup lock can change owners during Windows elevation.
                                    // Wait for the running app to acknowledge receipt before exiting.
                                    stream.shutdown(Shutdown::Write)?;
                                    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                                    let mut ack = [0];
                                    if stream.read_exact(&mut ack).is_ok() && ack == [1] {
                                        return Ok(None);
                                    }
                                }
                            }
                        }
                    }
                    if Instant::now() >= deadline {
                        return Err(Error::msg(
                            "The existing app is not responding. Close it and try again.",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    pub fn listen(&self, app: tauri::AppHandle) -> Result<()> {
        let listener = self.listener.try_clone()?;
        let expected = format!("{}:", self.token);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut bytes = Vec::new();
                if (&mut stream)
                    .take((MAX_LINK_BYTES + 128) as u64)
                    .read_to_end(&mut bytes)
                    .is_err()
                {
                    continue;
                }
                let Ok(message) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                match message.strip_prefix(&expected) {
                    Some("open") => crate::desktop::show(&app),
                    Some(request) => {
                        if let Some(link) = request
                            .strip_prefix("import:")
                            .filter(|link| subscription_links::is_link(link))
                        {
                            subscription_links::enqueue(&app, link.to_owned());
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                            let _ = stream.write_all(&[1]);
                            crate::desktop::show(&app);
                        }
                    }
                    None => {}
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_launch_does_not_acquire_a_second_lock() {
        let dir = std::env::temp_dir().join(format!("mihomo-instance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("instance.lock");
        let first = lock_file(&path).unwrap().unwrap();
        assert!(lock_file(&path).unwrap().is_none());
        drop(first);
        assert!(lock_file(&path).unwrap().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
