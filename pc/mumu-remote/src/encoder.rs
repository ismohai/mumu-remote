use crate::capture::Frame;
use image::codecs::jpeg::JpegEncoder;
use image::ColorType;

#[derive(Debug)]
pub struct EncodeError;

pub type EncodeResult<T> = Result<T, EncodeError>;

pub struct Encoder {
    width: i32,
    height: i32,
    quality: u8,
    fps: u32,
    bitrate: u32,
}

impl Encoder {
    pub fn new(width: i32, height: i32, fps: u32, bitrate: u32) -> EncodeResult<Self> {
        if width <= 0 || height <= 0 || fps == 0 || bitrate == 0 {
            return Err(EncodeError);
        }
        Ok(Encoder {
            width,
            height,
            quality: 80,
            fps,
            bitrate,
        })
    }

    pub fn encode(&mut self, frame: &Frame) -> EncodeResult<Vec<u8>> {
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError);
        }

        let mut rgb = vec![0u8; (frame.width * frame.height * 3) as usize];
        let mut src = 0usize;
        let mut dst = 0usize;
        while src + 3 < frame.bgra.len() {
            rgb[dst] = frame.bgra[src + 2];
            rgb[dst + 1] = frame.bgra[src + 1];
            rgb[dst + 2] = frame.bgra[src];
            src += 4;
            dst += 3;
        }

        let mut encoded = Vec::new();
        let mut jpeg_encoder = JpegEncoder::new_with_quality(&mut encoded, self.quality);
        if jpeg_encoder
            .encode(
                &rgb,
                self.width as u32,
                self.height as u32,
                ColorType::Rgb8.into(),
            )
            .is_err()
        {
            return Err(EncodeError);
        }

        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::{EncodeError, Encoder};
    use crate::capture::Frame;

    #[test]
    fn new_rejects_non_positive_dimensions() {
        assert!(Encoder::new(0, 100, 60, 1_000_000).is_err());
        assert!(Encoder::new(100, 0, 60, 1_000_000).is_err());
    }

    #[test]
    fn encode_rejects_mismatched_size() {
        let mut encoder = Encoder::new(100, 100, 60, 1_000_000).unwrap();
        let frame = Frame {
            width: 50,
            height: 50,
            bgra: vec![0; 50 * 50 * 4],
        };
        let result = encoder.encode(&frame);
        assert!(matches!(result, Err(EncodeError)));
    }

    #[test]
    fn encode_produces_non_empty_jpeg() {
        let mut encoder = Encoder::new(2, 2, 60, 1_000_000).unwrap();
        let frame = Frame {
            width: 2,
            height: 2,
            bgra: vec![
                0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
            ],
        };
        let encoded = encoder.encode(&frame).expect("encode should succeed");
        assert!(!encoded.is_empty());
        assert_eq!(encoded[0], 0xFF);
        assert_eq!(encoded[1], 0xD8);
    }
}
