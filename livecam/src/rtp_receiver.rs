use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use webrtc::rtp::packet::Packet;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::util::Unmarshal;
pub async fn start(
    rtp_port: u16,
    track: Arc<TrackLocalStaticRTP>,
    mut shutdown_rx: mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", rtp_port)).await?;
    tracing::info!(port = rtp_port, "RTP receiver listening.");

    let mut buffer = [0u8; 1500];

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!(port = rtp_port, "RTP receiver shutting down.");
                break;
            },
            Ok((size, _)) = socket.recv_from(&mut buffer) => {

                match Packet::unmarshal(&mut &buffer[..size]) {
                    Ok(rtp_packet) => {
                        if let Err(e) = track.write_rtp(&rtp_packet).await {
                            tracing::error!("Failed to write RTP packet: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to unmarshal RTP packet: {}", e);
                    }
                }
            }
        }
    }

    Ok(())
}
