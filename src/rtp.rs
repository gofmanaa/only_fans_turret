use std::net::SocketAddr;
use crate::app_state::AppState;
use std::sync::Arc;
use tokio::net::{UdpSocket};
use tracing::{error, info};
use webrtc::rtp::packet::Packet;
use webrtc::util::Unmarshal;

// socket_addr = "127.0.0.1:5004"
pub fn rtp_thread(socket_addr: SocketAddr, app_state: Arc<AppState>) {
    // -------------------------
    // RTP packet receiver
    // -------------------------
    let rtp_state = app_state.clone();
    tokio::spawn(async move {
        // Bind to UDP port where GStreamer will send RTP
        let socket = match UdpSocket::bind(socket_addr).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to bind UDP socket for RTP: {}", e);
                return;
            }
        };
        info!("Listening for RTP packets on 127.0.0.1:5004");
        let mut buf = [0u8; 2048];

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, _src)) => {
                    let mut raw = &buf[..n];
                    match Packet::unmarshal(&mut raw) {
                        Ok(packet) => {
                            // println!("Received packet {:?}", packet.payload.len());
                            // Broadcast RTP packet to all clients
                            let _ = rtp_state.rtp_broadcast.send(packet);
                        }
                        Err(err) => {
                            error!("Failed to parse RTP packet: {err}");
                        }
                    }
                }
                Err(e) => {
                    error!("UDP recv error: {}", e);
                    break;
                }
            }
        }
    });
}
