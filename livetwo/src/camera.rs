use anyhow::{anyhow, Result};
use bytes::Bytes;
use cli::Codec;
use rand::random; 
use rav1e::{prelude::*, Config, Context};
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{debug, error, info};
use url::Url;
use v4l::io::traits::CaptureStream;
use v4l::{video::Capture, Device, FourCC};
use webrtc::rtp::{header::Header, packet::Packet};
use webrtc::util::{Marshal, MarshalSize};

pub const MIME_TYPE_AV1: &str = "video/AV1";
const RTP_OUTBOUND_MTU: usize = 1200;

#[derive(Debug)]
pub struct CameraConfig {
    pub device_path: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: FourCC,
}

pub struct YUVFrame {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

pub async fn setup_camera_session(
    target_url: &str,
    complete_tx: &UnboundedSender<()>,
) -> Result<(rtsp::MediaInfo, UnboundedReceiver<Vec<u8>>)> {
    let input = Url::parse(target_url)?;
    let config = get_camera_parameters(input.path())?;
    info!("Camera parameters retrieved: {:?}", config);

    let device = Device::with_path(&config.device_path)?;
    info!("Device opened successfully: {}", config.device_path);

    info!(
        "Using format: {}x{} @ {}fps, FourCC: {:?}",
        config.width, config.height, config.fps, config.format
    );

    let aligned_width = (config.width + 63) & !63;
    let aligned_height = (config.height + 63) & !63;
    info!(
        "Aligned dimensions for encoding: {}x{}",
        aligned_width, aligned_height
    );

    let av1_config = Config::default()
        .with_threads(4)
        .with_encoder_config(EncoderConfig {
            width: aligned_width as usize,
            height: aligned_height as usize,
            time_base: Rational::new(1, config.fps as u64),
            chroma_sampling: ChromaSampling::Cs420,
            speed_settings: SpeedSettings::from_preset(10),
            ..Default::default()
        });
    let mut encoder: Context<u8> = av1_config.new_context()?;
    info!("AV1 encoder context created successfully");

    let (video_tx, video_rx) = unbounded_channel::<Vec<u8>>();

    let complete_tx = complete_tx.clone();
    tokio::spawn(async move {
        let mut stream = match v4l::io::mmap::Stream::with_buffers(
            &device,
            v4l::buffer::Type::VideoCapture,
            4,
        ) {
            Ok(stream) => {
                info!("Started video capture stream with 4 buffers");
                stream
            }
            Err(e) => {
                error!("Failed to start stream: {}", e);
                let _ = complete_tx.send(());
                return;
            }
        };

        let (signal_tx, mut signal_rx) = unbounded_channel::<()>();
        let complete_tx_clone = complete_tx.clone();
        tokio::spawn(async move {
            complete_tx_clone.closed().await;
            let _ = signal_tx.send(());
        });

        let mut seq_start = 0; 
        let ssrc = random::<u32>(); 
        let mut timestamp = 0;

        loop {
            if signal_rx.try_recv().is_ok() {
                info!("Received completion signal, stopping camera task");
                break;
            }
            match stream.next() {
                Ok((buf, _meta)) => {
                    info!(
                        "Captured frame, length: {}, expected: {}",
                        buf.len(),
                        config.width as usize * config.height as usize * 2
                    );
                    if buf.len() != config.width as usize * config.height as usize * 2 {
                        error!(
                            "Frame size mismatch: got {}, expected {}",
                            buf.len(),
                            config.width as usize * config.height as usize * 2
                        );
                        continue;
                    }
                    let yuv_frame = match &config.format.repr {
                        b"YUYV" => match convert_yuyv_to_yuv420p(
                            buf,
                            config.width as usize,
                            config.height as usize,
                        ) {
                            Ok(frame) => {
                                info!("Converted YUYV frame to YUV420p: Y={} bytes, U={} bytes, V={} bytes", frame.y.len(), frame.u.len(), frame.v.len());
                                frame
                            }
                            Err(e) => {
                                error!("Failed to convert frame to YUV420p: {}", e);
                                continue;
                            }
                        },
                        _ => {
                            error!("Unsupported format: {:?}", config.format);
                            break;
                        }
                    };

                    let mut av1_frame = Frame::new_with_padding(
                        aligned_width as usize,
                        aligned_height as usize,
                        ChromaSampling::Cs420,
                        128,
                    );
                    info!(
                        "Created AV1 frame: Y={}x{}, U={}x{}, V={}x{}",
                        av1_frame.planes[0].cfg.width,
                        av1_frame.planes[0].cfg.height,
                        av1_frame.planes[1].cfg.width,
                        av1_frame.planes[1].cfg.height,
                        av1_frame.planes[2].cfg.width,
                        av1_frame.planes[2].cfg.height
                    );

                    let y_padding =
                        aligned_width as usize * aligned_height as usize - yuv_frame.y.len();
                    let uv_padding = (aligned_width as usize / 2) * (aligned_height as usize / 2)
                        - yuv_frame.u.len();
                    info!(
                        "Padding sizes: Y={} bytes, U/V={} bytes",
                        y_padding, uv_padding
                    );

                    let mut y_padded = yuv_frame.y.clone();
                    let mut u_padded = yuv_frame.u.clone();
                    let mut v_padded = yuv_frame.v.clone();
                    y_padded.extend(vec![0; y_padding]);
                    u_padded.extend(vec![128; uv_padding]);
                    v_padded.extend(vec![128; uv_padding]);
                    info!(
                        "Padded frame data: Y={} bytes, U={} bytes, V={} bytes",
                        y_padded.len(),
                        u_padded.len(),
                        v_padded.len()
                    );

                    av1_frame.planes[0].copy_from_raw_u8(&y_padded, aligned_width as usize, 1);
                    av1_frame.planes[1].copy_from_raw_u8(&u_padded, aligned_width as usize / 2, 1);
                    av1_frame.planes[2].copy_from_raw_u8(&v_padded, aligned_width as usize / 2, 1);

                    match encoder.send_frame(Some(Arc::new(av1_frame))) {
                        Ok(_) => {
                            info!("Frame sent to encoder successfully");
                            if let Ok(packet) = encoder.receive_packet() {
                                debug!(
                                    "Encoded packet received, size: {} bytes",
                                    packet.data.len()
                                );
                                let rtp_packets = packetize_av1(
                                    &packet.data,
                                    90000,
                                    config.fps,
                                    &mut seq_start,
                                    ssrc,
                                    timestamp
                                );
                                debug!("Packetized into {} RTP packets", rtp_packets.len());
                                for pkt in rtp_packets {
                                    if video_tx.send(pkt).is_err() {
                                        error!("Failed to send RTP packet through channel");
                                    }
                                }
                                timestamp += 90000 / config.fps;
                            }
                        }
                        Err(e) => error!("AV1 encoding error: {}", e),
                    }
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    break;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
        debug!("Capture loop ended, sending completion signal");
        let _ = complete_tx.send(());
    });

    let media_info = rtsp::MediaInfo {
        video_rtp_client: None,
        audio_rtp_client: None,
        video_codec: Some(Codec::AV1),
        audio_codec: None,
        video_rtp_server: None,
        audio_rtp_server: None,
        video_rtcp_client: None,
        audio_rtcp_client: None,
    };

    Ok((media_info, video_rx))
}

fn get_camera_parameters(device_path: &str) -> Result<CameraConfig> {
    info!("Getting camera parameters for device: {}", device_path);

    let device = Device::with_path(device_path).map_err(|e| {
        error!("Failed to open device '{}': {}", device_path, e);
        e
    })?;

    let current_format = device.format()?;
    info!("Current camera format: {:?}", current_format);

    let width = current_format.width;
    let height = current_format.height;
    let fourcc = current_format.fourcc;

    let mut fps = 30; // Default FPS
    match device.enum_frameintervals(fourcc, width, height) {
        Ok(intervals) => {
            if let Some(first_interval) = intervals.first() {
                if let v4l::frameinterval::FrameIntervalEnum::Discrete(fraction) =
                    first_interval.interval
                {
                    fps = fraction.denominator / fraction.numerator;
                }
            }
        }
        Err(e) => {
            error!("Failed to retrieve frame intervals: {}", e);
        }
    }

    if fourcc != FourCC::new(b"YUYV") {
        let err = anyhow!("Unsupported format: {:?}", fourcc);
        error!("{}", err);
        return Err(err);
    }

    let config = CameraConfig {
        device_path: device_path.to_string(),
        width,
        height,
        fps,
        format: fourcc,
    };

    info!("Retrieved camera config: {:?}", config);
    Ok(config)
}

fn convert_yuyv_to_yuv420p(data: &[u8], width: usize, height: usize) -> Result<YUVFrame> {
    let aligned_width = (width + 1) & !1;
    let aligned_height = (height + 1) & !1;

    let expected_len = aligned_width * aligned_height * 2;
    if data.len() < expected_len {
        return Err(anyhow!(
            "Input data too short: got {}, expected {}",
            data.len(),
            expected_len
        ));
    }

    let mut yuv_frame = YUVFrame {
        y: vec![0; aligned_width * aligned_height],
        u: vec![0; aligned_width * aligned_height / 4],
        v: vec![0; aligned_width * aligned_height / 4],
    };

    for i in (0..data.len()).step_by(4) {
        let y0 = data[i];
        let u0 = data[i + 1];
        let y1 = data[i + 2];
        let v0 = data[i + 3];

        let x = (i / 4) % aligned_width;
        let y = (i / 4) / aligned_width;

        yuv_frame.y[y * aligned_width + x] = y0;
        if x + 1 < aligned_width {
            yuv_frame.y[y * aligned_width + x + 1] = y1;
        }

        if x % 2 == 0 && y % 2 == 0 && x / 2 < aligned_width / 2 && y / 2 < aligned_height / 2 {
            let uv_idx = (y / 2) * (aligned_width / 2) + (x / 2);
            yuv_frame.u[uv_idx] = u0;
            yuv_frame.v[uv_idx] = v0;
        }
    }

    Ok(yuv_frame)
}

// fn packetize_av1(data: &[u8], clock_rate: u32, fps: u32, seq_start: &mut u16, ssrc: u32) -> Vec<Vec<u8>> {
//     info!(
//         "Packetizing AV1 data: size={} bytes, clock_rate={}, fps={}",
//         data.len(),
//         clock_rate,
//         fps
//     );

//     let mut packets = Vec::new();
//     let mut timestamp = 0;

//     let chunks = data.chunks(RTP_OUTBOUND_MTU);
//     let total_chunks = chunks.len();

//     for (i, chunk) in chunks.enumerate() {
//         let payload = Bytes::copy_from_slice(chunk);
//         let header = Header {
//             version: 2,
//             padding: false,
//             extension: false,
//             marker: i == total_chunks - 1,
//             payload_type: 96,
//             sequence_number: *seq_start,
//             timestamp,
//             ssrc,
//             ..Default::default()
//         };

//         let pkt = Packet { header, payload };
//         let required_size = pkt.marshal_size();
//         let mut buf = vec![0; required_size];

//         info!(
//             "Attempting to marshal RTP packet: seq={}, chunk_size={}, required_size={}",
//             *seq_start,
//             chunk.len(),
//             required_size
//         );

//         match pkt.marshal_to(&mut buf) {
//             Ok(_) => {
//                 info!(
//                     "Created RTP packet: seq={}, size={} bytes, marker={}",
//                     *seq_start,
//                     buf.len(),
//                     i == total_chunks - 1
//                 );
//                 packets.push(buf);
//             }
//             Err(e) => {
//                 error!(
//                     "Failed to marshal RTP packet: seq={}, chunk_size={}, error={:?}",
//                     *seq_start,
//                     chunk.len(),
//                     e
//                 );
//                 continue;
//             }
//         }

//         *seq_start = seq_start.wrapping_add(1);
//         timestamp += clock_rate / fps;
//     }

//     info!("Packetization complete: {} packets created", packets.len());
//     packets
// }
fn packetize_av1(data: &[u8], clock_rate: u32, fps: u32, seq_start: &mut u16, ssrc: u32, timestamp: u32) -> Vec<Vec<u8>> {
    info!(
        "Packetizing AV1 data: size={} bytes, clock_rate={}, fps={}, timestamp={}",
        data.len(),
        clock_rate,
        fps,
        timestamp
    );

    let mut packets = Vec::new();
    let chunks = data.chunks(RTP_OUTBOUND_MTU - 1); // 预留 1 字节给 AV1 头
    let total_chunks = chunks.len();

    for (i, chunk) in chunks.enumerate() {
        let mut payload = Vec::new();
        // AV1 RTP 负载头
        let header_byte = ((i == 0) as u8) << 7 | // Z=1 表示起始分片
                         ((i == total_chunks - 1) as u8) << 6 | // Y=1 表示结束分片
                         ((total_chunks.min(3)) as u8) << 4; // W=分片计数（最大3）
        payload.push(header_byte);
        payload.extend_from_slice(chunk);
        let payload = Bytes::from(payload);

        let header = Header {
            version: 2,
            padding: false,
            extension: false,
            marker: i == total_chunks - 1,
            payload_type: 96,
            sequence_number: *seq_start,
            timestamp,
            ssrc,
            ..Default::default()
        };

        let pkt = Packet { header, payload };
        let required_size = pkt.marshal_size();
        let mut buf = vec![0; required_size];

        info!(
            "Attempting to marshal RTP packet: seq={}, chunk_size={}, required_size={}",
            *seq_start,
            chunk.len(),
            required_size
        );

        match pkt.marshal_to(&mut buf) {
            Ok(_) => {
                info!(
                    "Created RTP packet: seq={}, size={} bytes, marker={}, av1_header={:08b}",
                    *seq_start,
                    buf.len(),
                    i == total_chunks - 1,
                    header_byte
                );
                packets.push(buf);
            }
            Err(e) => {
                error!(
                    "Failed to marshal RTP packet: seq={}, chunk_size={}, error={:?}",
                    *seq_start,
                    chunk.len(),
                    e
                );
                continue;
            }
        }

        *seq_start = seq_start.wrapping_add(1);
    }

    info!("Packetization complete: {} packets created", packets.len());
    packets
}