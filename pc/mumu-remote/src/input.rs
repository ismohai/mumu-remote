use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;

use crate::adb::AdbClient;
use crate::mumu::{find_mumu_window, window_client_size};
use crate::runtime_config::apply_setting;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum InputEvent {
    Touch {
        phase: String,
        x: f32,
        y: f32,
    },
    Key {
        key: String,
    },
    Setting {
        resolution: Option<String>,
        fps: Option<u32>,
    },
}

pub struct InputController {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl InputController {
    pub fn start(listen_addr: String) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let join_handle = thread::Builder::new()
            .name("mumu-remote-input".to_string())
            .spawn(move || {
                let adb = AdbClient::from_env();
                let _ = adb.ensure_connected();

                let socket = match UdpSocket::bind(&listen_addr) {
                    Ok(socket) => socket,
                    Err(_) => return,
                };
                let _ = socket.set_read_timeout(Some(Duration::from_millis(100)));

                let mut buffer = [0u8; 2048];
                let mut last_touch: Option<(i32, i32)> = None;

                while running_clone.load(Ordering::Relaxed) {
                    match socket.recv_from(&mut buffer) {
                        Ok((n, _)) => {
                            if let Some(event) = parse_event(&buffer[..n]) {
                                handle_event(event, &adb, &mut last_touch);
                            }
                        }
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                        Err(err) if err.kind() == io::ErrorKind::TimedOut => {}
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self {
            running,
            join_handle: Some(join_handle),
        })
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for InputController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn parse_event(buf: &[u8]) -> Option<InputEvent> {
    serde_json::from_slice::<InputEvent>(buf).ok()
}

fn handle_event(event: InputEvent, adb: &AdbClient, last_touch: &mut Option<(i32, i32)>) {
    match event {
        InputEvent::Touch { phase, x, y } => {
            let (w, h) = current_mumu_size();
            let px = norm_to_pixel(x, w);
            let py = norm_to_pixel(y, h);

            match phase.as_str() {
                "down" => {
                    *last_touch = Some((px, py));
                }
                "move" => {
                    if let Some((lx, ly)) = *last_touch {
                        let _ = adb.swipe(lx, ly, px, py, 8);
                    }
                    *last_touch = Some((px, py));
                }
                "up" => {
                    let _ = adb.tap(px, py);
                    *last_touch = None;
                }
                _ => {
                    let _ = adb.tap(px, py);
                }
            }
        }
        InputEvent::Key { key } => {
            let key_code = map_key(&key);
            let _ = adb.keyevent(&key_code);
        }
        InputEvent::Setting { resolution, fps } => {
            apply_setting(resolution, fps);
        }
    }
}

fn current_mumu_size() -> (i32, i32) {
    if let Some(window) = find_mumu_window() {
        if let Some((w, h)) = window_client_size(window.handle) {
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    (1280, 720)
}

fn norm_to_pixel(value: f32, max: i32) -> i32 {
    if max <= 1 {
        return 0;
    }
    let clamped = value.clamp(0.0, 1.0);
    let raw = (clamped * (max - 1) as f32).round() as i32;
    raw.clamp(0, max - 1)
}

fn map_key(key: &str) -> String {
    match key {
        "back" => "4".to_string(),
        "home" => "3".to_string(),
        "recent" => "187".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_event, map_key, norm_to_pixel, parse_event, InputEvent};
    use crate::adb::AdbClient;
    use crate::runtime_config::{get_stream_config, lock_for_tests};

    #[test]
    fn parse_touch_event() {
        let data = br#"{"type":"touch","phase":"down","x":0.5,"y":0.25}"#;
        assert!(parse_event(data).is_some());
    }

    #[test]
    fn parse_key_event() {
        let data = br#"{"type":"key","key":"back"}"#;
        assert!(parse_event(data).is_some());
    }

    #[test]
    fn normalize_to_pixel_bounds() {
        assert_eq!(norm_to_pixel(0.0, 100), 0);
        assert_eq!(norm_to_pixel(1.0, 100), 99);
        assert_eq!(norm_to_pixel(2.0, 100), 99);
        assert_eq!(norm_to_pixel(-1.0, 100), 0);
    }

    #[test]
    fn map_key_back_home_recent() {
        assert_eq!(map_key("back"), "4");
        assert_eq!(map_key("home"), "3");
        assert_eq!(map_key("recent"), "187");
        assert_eq!(map_key("66"), "66");
    }

    #[test]
    fn setting_event_updates_runtime_config() {
        let _guard = lock_for_tests();
        let event = InputEvent::Setting {
            resolution: Some("2k".to_string()),
            fps: Some(90),
        };
        let adb = AdbClient::from_env();
        let mut last_touch = None;

        handle_event(event, &adb, &mut last_touch);

        let cfg = get_stream_config();
        assert_eq!(cfg.fps, 90);
        assert_eq!(cfg.resolution_mode, "2k");
    }
}
