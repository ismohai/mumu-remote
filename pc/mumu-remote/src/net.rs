use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoPacketHeader {
    pub session_id: u32,
    pub frame_index: u32,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub timestamp_micros: u64,
}

impl VideoPacketHeader {
    pub const SIZE: usize = 4 + 4 + 2 + 2 + 8;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];
        buf[0..4].copy_from_slice(&self.session_id.to_be_bytes());
        buf[4..8].copy_from_slice(&self.frame_index.to_be_bytes());
        buf[8..10].copy_from_slice(&self.chunk_index.to_be_bytes());
        buf[10..12].copy_from_slice(&self.chunk_count.to_be_bytes());
        buf[12..20].copy_from_slice(&self.timestamp_micros.to_be_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let session_id = u32::from_be_bytes(buf[0..4].try_into().ok()?);
        let frame_index = u32::from_be_bytes(buf[4..8].try_into().ok()?);
        let chunk_index = u16::from_be_bytes(buf[8..10].try_into().ok()?);
        let chunk_count = u16::from_be_bytes(buf[10..12].try_into().ok()?);
        let timestamp_micros = u64::from_be_bytes(buf[12..20].try_into().ok()?);
        Some(VideoPacketHeader {
            session_id,
            frame_index,
            chunk_index,
            chunk_count,
            timestamp_micros,
        })
    }

    pub fn now_timestamp_micros() -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        now.as_micros() as u64
    }
}

pub struct UdpVideoSender {
    socket: UdpSocket,
}

impl UdpVideoSender {
    pub fn new(bind_addr: &str) -> io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;
        Ok(UdpVideoSender { socket })
    }

    pub fn send_frame(
        &self,
        remote: &str,
        header: &VideoPacketHeader,
        payload: &[u8],
    ) -> io::Result<()> {
        let addr: SocketAddr = remote
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        // conservative payload size per packet
        let max_payload_per_packet: usize = 1200;
        let total_chunks = if payload.is_empty() {
            1
        } else {
            ((payload.len() + max_payload_per_packet - 1) / max_payload_per_packet)
                .min(u16::MAX as usize)
        };

        for chunk_index in 0..total_chunks {
            let start = chunk_index * max_payload_per_packet;
            let end = ((chunk_index + 1) * max_payload_per_packet).min(payload.len());
            let part = if start < end {
                &payload[start..end]
            } else {
                &[][..]
            };

            let chunk_header = VideoPacketHeader {
                session_id: header.session_id,
                frame_index: header.frame_index,
                chunk_index: chunk_index as u16,
                chunk_count: total_chunks as u16,
                timestamp_micros: header.timestamp_micros,
            };

            let mut buf = Vec::with_capacity(VideoPacketHeader::SIZE + part.len());
            buf.extend_from_slice(&chunk_header.to_bytes());
            buf.extend_from_slice(part);
            let _ = self.socket.send_to(&buf, addr)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{UdpVideoSender, VideoPacketHeader};
    use std::net::UdpSocket;
    use std::time::Duration;

    #[test]
    fn header_roundtrip() {
        let header = VideoPacketHeader {
            session_id: 1,
            frame_index: 2,
            chunk_index: 3,
            chunk_count: 4,
            timestamp_micros: 5,
        };
        let bytes = header.to_bytes();
        let decoded = VideoPacketHeader::from_bytes(&bytes).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn send_frame_sends_single_packet_with_header_and_payload() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let receiver_addr = receiver.local_addr().unwrap();
        let sender = UdpVideoSender::new("127.0.0.1:0").unwrap();

        let header = VideoPacketHeader {
            session_id: 42,
            frame_index: 7,
            chunk_index: 0,
            chunk_count: 1,
            timestamp_micros: 123,
        };
        let payload: Vec<u8> = vec![1, 2, 3, 4, 5];

        sender
            .send_frame(&receiver_addr.to_string(), &header, &payload)
            .unwrap();

        let mut buf = [0u8; 1500];
        let (n, _src) = receiver.recv_from(&mut buf).expect("no packet received");
        assert!(n >= VideoPacketHeader::SIZE + payload.len());

        let decoded_header =
            VideoPacketHeader::from_bytes(&buf[..VideoPacketHeader::SIZE]).unwrap();
        assert_eq!(decoded_header.session_id, header.session_id);
        assert_eq!(decoded_header.frame_index, header.frame_index);
        assert_eq!(decoded_header.chunk_index, 0);
        assert_eq!(decoded_header.chunk_count, 1);

        let payload_start = VideoPacketHeader::SIZE;
        let payload_end = payload_start + payload.len();
        assert_eq!(&buf[payload_start..payload_end], &payload[..]);
    }
}
