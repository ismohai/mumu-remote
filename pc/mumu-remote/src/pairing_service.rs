use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::pairing::generate_token;

pub const DEFAULT_PAIR_PORT: u16 = 56000;
pub const DEFAULT_VIDEO_PORT: u16 = 5000;
pub const DEFAULT_CONTROL_PORT: u16 = 5001;

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub device_id: String,
    pub device_name: String,
    pub from: String,
    pub ip: String,
    pub video_port: u16,
    pub control_port: u16,
}

#[derive(Debug, Clone)]
pub struct IncomingPairRequest {
    pub request_id: String,
    pub device_id: String,
    pub device_name: String,
    pub token: Option<String>,
    pub addr: SocketAddr,
    pub video_port: u16,
    pub control_port: u16,
}

#[derive(Debug, Clone)]
pub struct PairResponseEvent {
    pub request_id: String,
    pub accepted: bool,
    pub device_id: String,
    pub device_name: String,
    pub addr: SocketAddr,
    pub video_port: u16,
    pub control_port: u16,
}

#[derive(Debug, Clone)]
pub enum PairingEvent {
    Discovered(DiscoveredDevice),
    IncomingRequest(IncomingPairRequest),
    PairResponse(PairResponseEvent),
    Error(String),
}

enum PairingCommand {
    SendPairRequest {
        target_ip: String,
    },
    ReplyIncoming {
        request_id: String,
        accepted: bool,
        addr: SocketAddr,
    },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PairingPacket {
    DiscoverProbe {
        from: String,
        device_id: String,
        device_name: String,
    },
    DiscoverResponse {
        #[serde(default)]
        from: String,
        device_id: String,
        device_name: String,
        video_port: u16,
        control_port: u16,
    },
    PairRequest {
        request_id: String,
        from: String,
        device_id: String,
        device_name: String,
        token: Option<String>,
        video_port: Option<u16>,
        control_port: Option<u16>,
    },
    PairResponse {
        request_id: String,
        accepted: bool,
        device_id: String,
        device_name: String,
        video_port: u16,
        control_port: u16,
    },
}

pub struct PairingService {
    cmd_tx: Sender<PairingCommand>,
    event_rx: Receiver<PairingEvent>,
    join_handle: Option<JoinHandle<()>>,
}

impl PairingService {
    pub fn start() -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DEFAULT_PAIR_PORT))?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(Duration::from_millis(200)))?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<PairingCommand>();
        let (event_tx, event_rx) = mpsc::channel::<PairingEvent>();

        let local_device_id = format!("pc-{}", generate_token());
        let local_device_name = "MuMu Remote PC".to_string();

        let join_handle = thread::Builder::new()
            .name("pairing-service".to_string())
            .spawn(move || {
                run_pairing_loop(socket, cmd_rx, event_tx, local_device_id, local_device_name);
            })?;

        Ok(Self {
            cmd_tx,
            event_rx,
            join_handle: Some(join_handle),
        })
    }

    pub fn poll_events(&self) -> Vec<PairingEvent> {
        let mut out = Vec::new();
        loop {
            match self.event_rx.try_recv() {
                Ok(event) => out.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    pub fn send_pair_request(&self, target_ip: String) {
        let _ = self
            .cmd_tx
            .send(PairingCommand::SendPairRequest { target_ip });
    }

    pub fn reply_incoming(&self, request: &IncomingPairRequest, accepted: bool) {
        let _ = self.cmd_tx.send(PairingCommand::ReplyIncoming {
            request_id: request.request_id.clone(),
            accepted,
            addr: request.addr,
        });
    }
}

impl Drop for PairingService {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PairingCommand::Shutdown);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_pairing_loop(
    socket: UdpSocket,
    cmd_rx: Receiver<PairingCommand>,
    event_tx: Sender<PairingEvent>,
    local_device_id: String,
    local_device_name: String,
) {
    let mut last_discover = Instant::now() - Duration::from_secs(5);
    let mut buffer = [0u8; 4096];

    loop {
        loop {
            match cmd_rx.try_recv() {
                Ok(PairingCommand::SendPairRequest { target_ip }) => {
                    let packet = PairingPacket::PairRequest {
                        request_id: generate_token(),
                        from: "pc".to_string(),
                        device_id: local_device_id.clone(),
                        device_name: local_device_name.clone(),
                        token: None,
                        video_port: Some(DEFAULT_VIDEO_PORT),
                        control_port: Some(DEFAULT_CONTROL_PORT),
                    };
                    let _ = send_packet(
                        &socket,
                        SocketAddr::new(
                            target_ip.parse().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                            DEFAULT_PAIR_PORT,
                        ),
                        &packet,
                    );
                }
                Ok(PairingCommand::ReplyIncoming {
                    request_id,
                    accepted,
                    addr,
                }) => {
                    let packet = PairingPacket::PairResponse {
                        request_id,
                        accepted,
                        device_id: local_device_id.clone(),
                        device_name: local_device_name.clone(),
                        video_port: DEFAULT_VIDEO_PORT,
                        control_port: DEFAULT_CONTROL_PORT,
                    };
                    let _ = send_packet(&socket, addr, &packet);
                }
                Ok(PairingCommand::Shutdown) => return,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if last_discover.elapsed() >= Duration::from_secs(3) {
            let probe = PairingPacket::DiscoverProbe {
                from: "pc".to_string(),
                device_id: local_device_id.clone(),
                device_name: local_device_name.clone(),
            };
            let broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DEFAULT_PAIR_PORT);
            let _ = send_packet(&socket, broadcast, &probe);
            last_discover = Instant::now();
        }

        match socket.recv_from(&mut buffer) {
            Ok((n, addr)) => {
                if let Some(packet) = decode_packet(&buffer[..n]) {
                    handle_packet(
                        packet,
                        addr,
                        &socket,
                        &event_tx,
                        &local_device_id,
                        &local_device_name,
                    );
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
            Err(err) if err.kind() == io::ErrorKind::TimedOut => {}
            Err(err) => {
                let _ = event_tx.send(PairingEvent::Error(format!("pairing socket error: {err}")));
            }
        }
    }
}

fn handle_packet(
    packet: PairingPacket,
    addr: SocketAddr,
    socket: &UdpSocket,
    event_tx: &Sender<PairingEvent>,
    local_device_id: &str,
    local_device_name: &str,
) {
    match packet {
        PairingPacket::DiscoverProbe {
            from, device_id, ..
        } => {
            if from == "pc" && device_id == local_device_id {
                return;
            }

            let response = PairingPacket::DiscoverResponse {
                from: "pc".to_string(),
                device_id: local_device_id.to_string(),
                device_name: local_device_name.to_string(),
                video_port: DEFAULT_VIDEO_PORT,
                control_port: DEFAULT_CONTROL_PORT,
            };
            let _ = send_packet(socket, addr, &response);
        }
        PairingPacket::DiscoverResponse {
            from,
            device_id,
            device_name,
            video_port,
            control_port,
        } => {
            let event = PairingEvent::Discovered(DiscoveredDevice {
                device_id,
                device_name,
                from,
                ip: addr.ip().to_string(),
                video_port,
                control_port,
            });
            let _ = event_tx.send(event);
        }
        PairingPacket::PairRequest {
            request_id,
            from,
            device_id,
            device_name,
            token,
            video_port,
            control_port,
        } => {
            if from == "pc" {
                return;
            }

            let event = PairingEvent::IncomingRequest(IncomingPairRequest {
                request_id,
                device_id,
                device_name,
                token,
                addr,
                video_port: video_port.unwrap_or(DEFAULT_VIDEO_PORT),
                control_port: control_port.unwrap_or(DEFAULT_CONTROL_PORT),
            });
            let _ = event_tx.send(event);
        }
        PairingPacket::PairResponse {
            request_id,
            accepted,
            device_id,
            device_name,
            video_port,
            control_port,
        } => {
            let event = PairingEvent::PairResponse(PairResponseEvent {
                request_id,
                accepted,
                device_id,
                device_name,
                addr,
                video_port,
                control_port,
            });
            let _ = event_tx.send(event);
        }
    }
}

fn send_packet(socket: &UdpSocket, addr: SocketAddr, packet: &PairingPacket) -> io::Result<()> {
    let payload = serde_json::to_vec(packet)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let _ = socket.send_to(&payload, addr)?;
    Ok(())
}

fn decode_packet(data: &[u8]) -> Option<PairingPacket> {
    serde_json::from_slice::<PairingPacket>(data).ok()
}

#[cfg(test)]
mod tests {
    use super::{decode_packet, PairingPacket, DEFAULT_CONTROL_PORT, DEFAULT_VIDEO_PORT};

    #[test]
    fn decode_discover_probe() {
        let bytes = serde_json::to_vec(&PairingPacket::DiscoverProbe {
            from: "pc".to_string(),
            device_id: "abc".to_string(),
            device_name: "name".to_string(),
        })
        .expect("encode discover probe");

        assert!(decode_packet(&bytes).is_some());
    }

    #[test]
    fn decode_pair_response() {
        let bytes = serde_json::to_vec(&PairingPacket::PairResponse {
            request_id: "r1".to_string(),
            accepted: true,
            device_id: "d1".to_string(),
            device_name: "device".to_string(),
            video_port: DEFAULT_VIDEO_PORT,
            control_port: DEFAULT_CONTROL_PORT,
        })
        .expect("encode pair response");

        assert!(decode_packet(&bytes).is_some());
    }

    #[test]
    fn decode_discover_response_with_from() {
        let bytes = serde_json::to_vec(&PairingPacket::DiscoverResponse {
            from: "phone".to_string(),
            device_id: "phone-id".to_string(),
            device_name: "phone".to_string(),
            video_port: DEFAULT_VIDEO_PORT,
            control_port: DEFAULT_CONTROL_PORT,
        })
        .expect("encode discover response");

        assert!(decode_packet(&bytes).is_some());
    }
}
