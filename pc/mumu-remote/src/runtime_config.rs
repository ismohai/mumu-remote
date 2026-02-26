use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct StreamRuntimeConfig {
    pub fps: u32,
    pub resolution_mode: String,
}

impl Default for StreamRuntimeConfig {
    fn default() -> Self {
        Self {
            fps: 60,
            resolution_mode: "default".to_string(),
        }
    }
}

static CONFIG: OnceLock<Mutex<StreamRuntimeConfig>> = OnceLock::new();

#[cfg(test)]
static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn config_mutex() -> &'static Mutex<StreamRuntimeConfig> {
    CONFIG.get_or_init(|| Mutex::new(StreamRuntimeConfig::default()))
}

pub fn get_stream_config() -> StreamRuntimeConfig {
    match config_mutex().lock() {
        Ok(cfg) => cfg.clone(),
        Err(_) => StreamRuntimeConfig::default(),
    }
}

pub fn apply_setting(resolution: Option<String>, fps: Option<u32>) {
    if let Ok(mut cfg) = config_mutex().lock() {
        if let Some(fps_value) = fps {
            cfg.fps = fps_value.clamp(30, 120);
        }
        if let Some(mode) = resolution {
            cfg.resolution_mode = mode;
        }
    }
}

#[cfg(test)]
pub fn lock_for_tests() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock runtime config test guard")
}

#[cfg(test)]
mod tests {
    use super::{apply_setting, get_stream_config, lock_for_tests};

    #[test]
    fn apply_setting_updates_fps() {
        let _guard = lock_for_tests();
        apply_setting(Some("default".to_string()), Some(90));
        let cfg = get_stream_config();
        assert_eq!(cfg.fps, 90);
    }

    #[test]
    fn fps_is_clamped() {
        let _guard = lock_for_tests();
        apply_setting(None, Some(999));
        let cfg = get_stream_config();
        assert_eq!(cfg.fps, 120);
    }
}
