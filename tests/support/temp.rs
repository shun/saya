use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[allow(dead_code)]
pub fn unique_temp_path(scope: &str, name: &str) -> PathBuf {
    static NEXT_UNIQUE_PATH_ID: AtomicU64 = AtomicU64::new(1);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let id = NEXT_UNIQUE_PATH_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("saya-{scope}-{name}-{nanos}-{id}"))
}
