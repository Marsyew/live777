// use bytes::Bytes;
// use tracing::error;
// use webrtc::{
//     api::media_engine::*,
//     rtp::{
//         codecs::*,
//         packet::Packet,
//         packetizer::{Depacketizer, Payloader},
//     },
// };

// /// https://github.com/webrtc-rs/webrtc/blob/dcfefd7b48dc2bb9ecf50ea66c304f62719a6c4a/webrtc/src/track/mod.rs#L10C12-L10C49
// /// https://github.com/binbat/live777/issues/1200
// /// WebRTC Build-in RTP must less 1200
// const RTP_OUTBOUND_MTU: usize = 1200;

// pub(crate) trait RePayload {
//     fn payload(&mut self, packet: Packet) -> Vec<Packet>;
// }

// pub(crate) struct Forward {}

// impl Forward {
//     pub fn new() -> Forward {
//         Forward {}
//     }
// }

// impl RePayload for Forward {
//     fn payload(&mut self, packet: Packet) -> Vec<Packet> {
//         vec![packet]
//     }
// }

// pub(crate) struct RePayloadBase {
//     buffer: Vec<Bytes>,
//     sequence_number: u16,
//     src_sequence_number: u16,
// }

// impl RePayloadBase {
//     pub fn new() -> RePayloadBase {
//         RePayloadBase {
//             buffer: Vec::new(),
//             sequence_number: 0,
//             src_sequence_number: 0,
//         }
//     }

//     fn verify_sequence_number(&mut self, packet: &Packet) {
//         if self.src_sequence_number.wrapping_add(1) != packet.header.sequence_number
//             && self.src_sequence_number != 0
//         {
//             error!(
//                 "Should received sequence: {}. But received sequence: {}",
//                 self.src_sequence_number + 1,
//                 packet.header.sequence_number
//             );
//         }
//         self.src_sequence_number = packet.header.sequence_number;
//     }

//     fn clear_buffer(&mut self) {
//         self.buffer.clear();
//     }
// }

// pub(crate) struct RePayloadCodec {
//     base: RePayloadBase,
//     encoder: Box<dyn Payloader + Send>,
//     decoder: Box<dyn Depacketizer + Send>,
// }

// impl RePayloadCodec {
//     pub fn new(mime_type: String) -> RePayloadCodec {
//         RePayloadCodec {
//             base: RePayloadBase::new(),
//             decoder: match mime_type.as_str() {
//                 MIME_TYPE_VP8 => Box::default() as Box<vp8::Vp8Packet>,
//                 MIME_TYPE_VP9 => Box::default() as Box<vp9::Vp9Packet>,
//                 MIME_TYPE_H264 => Box::default() as Box<h264::H264Packet>,
//                 MIME_TYPE_OPUS => Box::default() as Box<opus::OpusPacket>,
//                 _ => Box::default() as Box<vp8::Vp8Packet>,
//             },
//             encoder: match mime_type.as_str() {
//                 MIME_TYPE_VP8 => Box::default() as Box<vp8::Vp8Payloader>,
//                 MIME_TYPE_VP9 => Box::default() as Box<vp9::Vp9Payloader>,
//                 MIME_TYPE_H264 => Box::default() as Box<h264::H264Payloader>,
//                 MIME_TYPE_OPUS => Box::default() as Box<opus::OpusPayloader>,
//                 _ => Box::default() as Box<vp8::Vp8Payloader>,
//             },
//         }
//     }
// }

// impl RePayload for RePayloadCodec {
//     fn payload(&mut self, packet: Packet) -> Vec<Packet> {
//         self.base.verify_sequence_number(&packet);

//         match self.decoder.depacketize(&packet.payload) {
//             Ok(data) => self.base.buffer.push(data),
//             Err(e) => error!("{}", e),
//         };

//         if packet.header.marker {
//             let packets = match self
//                 .encoder
//                 .payload(RTP_OUTBOUND_MTU, &Bytes::from(self.base.buffer.concat()))
//             {
//                 Ok(payloads) => {
//                     let length = payloads.len();
//                     payloads
//                         .into_iter()
//                         .enumerate()
//                         .map(|(i, payload)| {
//                             let mut header = packet.clone().header;
//                             header.sequence_number = self.base.sequence_number;
//                             header.marker = i == length - 1;
//                             self.base.sequence_number = self.base.sequence_number.wrapping_add(1);
//                             Packet { header, payload }
//                         })
//                         .collect::<Vec<Packet>>()
//                 }
//                 Err(e) => {
//                     error!("{}", e);
//                     vec![]
//                 }
//             };
//             self.base.clear_buffer();
//             packets
//         } else {
//             vec![]
//         }
//     }
// }
use bytes::{Buf, Bytes};
use tracing::{error, debug};
use webrtc::{
    api::media_engine::*,
    rtp::{
        codecs::{
            av1::Av1Payloader, h264::H264Packet, h264::H264Payloader, opus::OpusPacket,
            opus::OpusPayloader, vp8::Vp8Packet, vp8::Vp8Payloader, vp9::Vp9Packet, vp9::Vp9Payloader,
        },
        packet::Packet,
        packetizer::{Depacketizer, Payloader},
        Error as RtpError,
    },
};

/// WebRTC Build-in RTP must be less than 1200 bytes
/// Reference: https://github.com/webrtc-rs/webrtc/blob/dcfefd7b48dc2bb9ecf50ea66c304f62719a6c4a/webrtc/src/track/mod.rs#L10C12-L10C49
/// Issue: https://github.com/binbat/live777/issues/1200
const RTP_OUTBOUND_MTU: usize = 1200;

pub struct Av1Depacketizer;

impl Depacketizer for Av1Depacketizer {
    fn depacketize(&mut self, payload: &Bytes) -> Result<Bytes, RtpError> {
        if payload.is_empty() {
            return Err(RtpError::ErrShortPacket);
        }

        let mut reader = payload.clone();
        let first_byte = reader.get_u8();

        // 解析 AV1 RTP 头（参考 https://aomediacodec.github.io/av1-rtp-spec/#41-payload-header）
        let _z = (first_byte >> 7) & 0x1; // 起始分片标志
        let _y = (first_byte >> 6) & 0x1; // 结束分片标志
        let w = (first_byte >> 4) & 0x3; // 分片计数（0 表示未知）
        let _n = (first_byte >> 3) & 0x1; // 新编码帧标志

        // 如果 W=0 且有额外字节，跳过第二个字节（OBU 数量）
        if w == 0 && reader.has_remaining() {
            let _second_byte = reader.get_u8(); // 丢弃，假设单 OBU
        }

        // 提取 AV1 OBU 数据
        let obu_data = if reader.has_remaining() {
            reader.copy_to_bytes(reader.remaining())
        } else {
            Bytes::new()
        };
        debug!("AV1 OBU data: {:?}", obu_data);

        Ok(obu_data)
    }

    fn is_partition_head(&self, _payload: &Bytes) -> bool {
        // 假设第一个包是分区的头部（可以根据 Z 位改进）
        true
    }

    fn is_partition_tail(&self, _prev: bool, _payload: &Bytes) -> bool {
        // 假设最后一个包是分区的尾部（可以根据 Y 位改进）
        true
    }
}

pub(crate) trait RePayload {
    fn payload(&mut self, packet: Packet) -> Vec<Packet>;
}

pub(crate) struct Forward {}

impl Forward {
    pub fn new() -> Forward {
        Forward {}
    }
}

impl RePayload for Forward {
    fn payload(&mut self, packet: Packet) -> Vec<Packet> {
        vec![packet]
    }
}

pub(crate) struct RePayloadBase {
    buffer: Vec<Bytes>,
    sequence_number: u16,
    src_sequence_number: u16,
}

impl RePayloadBase {
    pub fn new() -> RePayloadBase {
        RePayloadBase {
            buffer: Vec::new(),
            sequence_number: 0,
            src_sequence_number: 0,
        }
    }

    fn verify_sequence_number(&mut self, packet: &Packet) {
        if self.src_sequence_number.wrapping_add(1) != packet.header.sequence_number
            && self.src_sequence_number != 0
        {
            error!(
                "Should received sequence: {}. But received sequence: {}",
                self.src_sequence_number + 1,
                packet.header.sequence_number
            );
        }
        self.src_sequence_number = packet.header.sequence_number;
    }

    fn clear_buffer(&mut self) {
        self.buffer.clear();
    }
}

pub(crate) struct RePayloadCodec {
    base: RePayloadBase,
    encoder: Box<dyn Payloader + Send>,
    decoder: Box<dyn Depacketizer + Send>,
}

impl RePayloadCodec {
    pub fn new(mime_type: String) -> RePayloadCodec {
        RePayloadCodec {
            base: RePayloadBase::new(),
            decoder: match mime_type.as_str() {
                MIME_TYPE_VP8 => Box::default() as Box<Vp8Packet>,
                MIME_TYPE_VP9 => Box::default() as Box<Vp9Packet>,
                MIME_TYPE_H264 => Box::default() as Box<H264Packet>,
                MIME_TYPE_OPUS => Box::default() as Box<OpusPacket>,
                "video/AV1" => Box::new(Av1Depacketizer),
                _ => Box::default() as Box<Vp8Packet>,
            },
            encoder: match mime_type.as_str() {
                MIME_TYPE_VP8 => Box::default() as Box<Vp8Payloader>,
                MIME_TYPE_VP9 => Box::default() as Box<Vp9Payloader>,
                MIME_TYPE_H264 => Box::default() as Box<H264Payloader>,
                MIME_TYPE_OPUS => Box::default() as Box<OpusPayloader>,
                "video/AV1" => Box::new(Av1Payloader {}), 
                _ => Box::default() as Box<Vp8Payloader>,
            },
        }
    }
}

impl RePayload for RePayloadCodec {
    fn payload(&mut self, packet: Packet) -> Vec<Packet> {
        self.base.verify_sequence_number(&packet);

        match self.decoder.depacketize(&packet.payload) {
            Ok(data) => self.base.buffer.push(data),
            Err(e) => error!("Depacketize error: {}", e),
        };

        if packet.header.marker {
            let packets = match self
                .encoder
                .payload(RTP_OUTBOUND_MTU, &Bytes::from(self.base.buffer.concat()))
            {
                Ok(payloads) => {
                    let length = payloads.len();
                    payloads
                        .into_iter()
                        .enumerate()
                        .map(|(i, payload)| {
                            let mut header = packet.clone().header;
                            header.sequence_number = self.base.sequence_number;
                            header.marker = i == length - 1;
                            self.base.sequence_number = self.base.sequence_number.wrapping_add(1);
                            Packet { header, payload }
                        })
                        .collect::<Vec<Packet>>()
                }
                Err(e) => {
                    error!("Payload error: {}", e);
                    vec![]
                }
            };
            self.base.clear_buffer();
            packets
        } else {
            vec![]
        }
    }
}