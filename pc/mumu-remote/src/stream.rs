use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::capture::capture_window;
use crate::encoder::Encoder;
use crate::mumu::find_mumu_window;
use crate::net::{UdpVideoSender, VideoPacketHeader};
use crate::runtime_config::get_stream_config;

pub struct StreamController {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl StreamController {
    pub fn start(remote: String) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let join_handle = thread::Builder::new()
            .name("mumu-remote-stream".to_string())
            .spawn(move || {
                let sender = match UdpVideoSender::new("0.0.0.0:0") {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let mut encoder: Option<Encoder> = None;
                let mut frame_index: u32 = 0;

                while running_clone.load(Ordering::Relaxed) {
                    let runtime_cfg = get_stream_config();
                    let fps = runtime_cfg.fps.max(1);
                    let frame_interval_ms = (1000u64 / fps as u64).max(1);

                    if let Some(window) = find_mumu_window() {
                        if let Ok(frame) = capture_window(window.handle) {
                            if frame.width > 0 && frame.height > 0 {
                                if encoder.is_none() {
                                    if let Ok(enc) =
                                        Encoder::new(frame.width, frame.height, fps, 8_000_000)
                                    {
                                        encoder = Some(enc);
                                    }
                                }
                                if let Some(enc) = encoder.as_mut() {
                                    if let Ok(payload) = enc.encode(&frame) {
                                        let header = VideoPacketHeader {
                                            session_id: 1,
                                            frame_index,
                                            chunk_index: 0,
                                            chunk_count: 0,
                                            timestamp_micros:
                                                VideoPacketHeader::now_timestamp_micros(),
                                        };
                                        if sender.send_frame(&remote, &header, &payload).is_ok() {
                                            frame_index = frame_index.wrapping_add(1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(frame_interval_ms));
                }
            })?;

        Ok(StreamController {
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

impl Drop for StreamController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::StreamController;

    #[test]
    fn start_and_stop_stream_controller() {
        let mut controller = StreamController::start("127.0.0.1:5000".to_string()).unwrap();
        controller.stop();
    }
}
