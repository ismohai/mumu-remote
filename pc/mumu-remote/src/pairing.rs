use std::fs;
use std::io;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::pairing_service::DEFAULT_PAIR_PORT;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingInfo {
    pub ip: String,
    pub port: u16,
    #[serde(default = "default_control_port")]
    pub control_port: u16,
    #[serde(default = "default_pair_port")]
    pub pair_port: u16,
    pub token: String,
}

fn default_control_port() -> u16 {
    5001
}

fn default_pair_port() -> u16 {
    DEFAULT_PAIR_PORT
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairedDevice {
    pub name: String,
    pub ip: String,
    pub port: u16,
    #[serde(default = "default_control_port")]
    pub control_port: u16,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PairingStore {
    pub devices: Vec<PairedDevice>,
}

pub fn generate_token() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect::<String>()
}

pub fn make_pairing_info(ip: &str, port: u16) -> PairingInfo {
    make_pairing_info_with_control(ip, port, default_control_port())
}

pub fn make_pairing_info_with_control(ip: &str, port: u16, control_port: u16) -> PairingInfo {
    PairingInfo {
        ip: ip.to_string(),
        port,
        control_port,
        pair_port: default_pair_port(),
        token: generate_token(),
    }
}

pub fn encode_pairing_info(info: &PairingInfo) -> String {
    serde_json::to_string(info).unwrap_or_default()
}

pub fn decode_pairing_info(s: &str) -> Option<PairingInfo> {
    serde_json::from_str::<PairingInfo>(s).ok()
}

pub fn make_device_from_pairing(name: &str, info: &PairingInfo) -> PairedDevice {
    PairedDevice {
        name: name.to_string(),
        ip: info.ip.clone(),
        port: info.port,
        control_port: info.control_port,
        device_id: info.token.clone(),
    }
}

pub fn upsert_device(store: &mut PairingStore, device: PairedDevice) {
    if let Some(existing) = store
        .devices
        .iter_mut()
        .find(|it| it.device_id == device.device_id)
    {
        *existing = device;
    } else {
        store.devices.push(device);
    }
}

pub fn default_store_path() -> PathBuf {
    match std::env::current_dir() {
        Ok(dir) => dir.join("pairings.json"),
        Err(_) => PathBuf::from("pairings.json"),
    }
}

pub fn load_store(path: &Path) -> PairingStore {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<PairingStore>(&content).unwrap_or_default(),
        Err(_) => PairingStore::default(),
    }
}

pub fn save_store(path: &Path, store: &PairingStore) -> io::Result<()> {
    let content = serde_json::to_string_pretty(store)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, content)
}

pub fn detect_local_ip() -> String {
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        decode_pairing_info, default_store_path, encode_pairing_info, generate_token, load_store,
        make_device_from_pairing, make_pairing_info, save_store, upsert_device, PairingStore,
    };

    #[test]
    fn token_has_expected_length_and_is_hex() {
        let token = generate_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn pairing_info_roundtrip_json() {
        let info = make_pairing_info("192.168.0.10", 5000);
        let json = encode_pairing_info(&info);
        let decoded = decode_pairing_info(&json).expect("decode");
        assert_eq!(info.ip, decoded.ip);
        assert_eq!(info.port, decoded.port);
        assert_eq!(info.control_port, decoded.control_port);
        assert_eq!(info.pair_port, decoded.pair_port);
        assert_eq!(info.token, decoded.token);
    }

    #[test]
    fn store_roundtrip() {
        let path = std::env::temp_dir().join(format!("mumu-remote-{}.json", generate_token()));
        let info = make_pairing_info("192.168.1.10", 5000);
        let mut store = PairingStore::default();
        upsert_device(&mut store, make_device_from_pairing("My PC", &info));

        save_store(&path, &store).expect("save store");
        let loaded = load_store(&path);
        assert_eq!(loaded, store);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn default_store_path_is_file_path() {
        let path = default_store_path();
        assert!(path.file_name().is_some());
    }
}
