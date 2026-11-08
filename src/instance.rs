use std::{
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
pub struct Instance {
    pub show: Arc<AtomicBool>,
    pub context: Arc<Mutex<Option<eframe::egui::Context>>>,
    path: PathBuf,
}
impl Instance {
    pub fn acquire(demo: bool) -> std::io::Result<Option<Self>> {
        let path = PathBuf::from(format!(
            "/run/user/{}/relay{}.sock",
            unsafe { libc::getuid() },
            if demo { "-preview" } else { "" }
        ));
        if let Ok(mut s) = UnixStream::connect(&path) {
            let _ = s.write_all(b"show");
            return Ok(None);
        }
        if path.exists() {
            use std::os::unix::fs::FileTypeExt;
            if std::fs::symlink_metadata(&path)?.file_type().is_socket() {
                std::fs::remove_file(&path)?;
            } else {
                return Err(std::io::Error::other("Instance path is not a socket"));
            }
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let show = Arc::new(AtomicBool::new(false));
        let context = Arc::new(Mutex::new(None::<eframe::egui::Context>));
        let s = show.clone();
        let c = context.clone();
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let mut b = [0; 4];
                let _ = stream.read_exact(&mut b);
                if &b == b"show" {
                    s.store(true, Ordering::SeqCst);
                    if let Some(ctx) = c.lock().unwrap().as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
        });
        Ok(Some(Self {
            show,
            context,
            path,
        }))
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
