use crate::app_state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing::{error, info};
use webrtc::rtp::packet::Packet;
use webrtc::util::Unmarshal;

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
        info!(
            "Listening for RTP packets on {}:{}",
            socket_addr.ip(),
            socket_addr.port()
        );
        let mut last_check = tokio::time::Instant::now();
        let is_streaming = Arc::new(AtomicBool::new(false));
        let local_state = is_streaming.clone();

        let mut buf = [0u8; 65536];

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, _src)) => {
                    if n < 12 {
                        // RTP header must be at least 12 bytes
                        continue;
                    }
                    if last_check.elapsed() > Duration::from_secs(3) {
                        last_check = tokio::time::Instant::now();
                        let user_count = rtp_state.users_count().await;
                        info!("users count: {}", user_count);
                        if user_count == 0 && local_state.load(Ordering::Relaxed) {
                            info!("No active clients, stopping device stream...");
                            local_state.store(false, Ordering::Relaxed);

                            if let Err(e) = rtp_state
                                .device_client
                                .lock()
                                .await
                                .stop_stream(device::pb::StopStreamRequest {})
                                .await
                            {
                                error!("Failed to stop device stream: {}", e);
                            }
                        } else if user_count > 0 && !local_state.load(Ordering::Relaxed) {
                            info!("New client detected, starting device stream...");
                            local_state.store(true, Ordering::Relaxed);

                            if let Err(e) = rtp_state
                                .device_client
                                .lock()
                                .await
                                .start_stream(device::pb::StartStreamRequest {})
                                .await
                            {
                                error!("Failed to start device stream: {}", e);
                                local_state.store(false, Ordering::Relaxed);
                            }
                        }
                    }

                    if rtp_state.rtp_broadcast.receiver_count() == 0 {
                        tracing::warn!("No active RTP subscribers, skipping packet...");
                        continue;
                    }

                    let mut raw = &buf[..n];
                    match Packet::unmarshal(&mut raw) {
                        Ok(packet) => {
                            // Broadcast RTP packet to all clients
                            if let Err(e) = rtp_state.rtp_broadcast.send(packet)
                                && e.to_string().contains("no active receivers") {
                                    tracing::warn!("No RTP subscribers available.");
                            }
                        }
                        Err(err) => {
                            error!("Failed to parse RTP packet: {err}");
                        }
                    }
                }
                Err(e) => {
                    error!("UDP recv error: {}", e);
                    sleep(Duration::from_millis(250)).await;
                    continue;
                }
            }
        }
    });
}
