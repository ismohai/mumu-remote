use std::io;
use std::process::Command;

pub struct AdbClient {
    serial: String,
}

impl AdbClient {
    pub fn from_env() -> Self {
        let serial =
            std::env::var("MUMU_ADB_TARGET").unwrap_or_else(|_| "127.0.0.1:7555".to_string());
        Self { serial }
    }

    pub fn ensure_connected(&self) -> io::Result<()> {
        let output = Command::new("adb")
            .arg("connect")
            .arg(&self.serial)
            .output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    pub fn tap(&self, x: i32, y: i32) -> io::Result<()> {
        self.shell_input(&["tap", &x.to_string(), &y.to_string()])
    }

    pub fn swipe(&self, x1: i32, y1: i32, x2: i32, y2: i32, duration_ms: i32) -> io::Result<()> {
        self.shell_input(&[
            "swipe",
            &x1.to_string(),
            &y1.to_string(),
            &x2.to_string(),
            &y2.to_string(),
            &duration_ms.to_string(),
        ])
    }

    pub fn keyevent(&self, key: &str) -> io::Result<()> {
        self.shell_input(&["keyevent", key])
    }

    fn shell_input(&self, args: &[&str]) -> io::Result<()> {
        let mut command = Command::new("adb");
        command
            .arg("-s")
            .arg(&self.serial)
            .arg("shell")
            .arg("input");
        for arg in args {
            command.arg(arg);
        }

        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }
}
