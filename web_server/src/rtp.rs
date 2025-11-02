use crate::app_state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing::{error, info};
use webrtc::rtp::packet::Packet;
use webrtc::util::Unmarshal;

pub fn rtp_thread(socket_addr: SocketAddr, app_state: Arc<AppState>) {

    let monitor_state = app_state.clone();
    // Thread for control start/stop video stream from device
    tokio::spawn(async move {
        loop {
            let user_count = monitor_state.users_count().await;
            if user_count == 0 && monitor_state.is_streaming.load(Ordering::Relaxed) {
                info!("No active users, stopping device stream...");
                monitor_state.is_streaming.store(false, Ordering::Relaxed);
                if let Err(e) = monitor_state.device_client.lock().await
                    .stop_stream(device::pb::StopStreamRequest {}).await {
                    error!("Failed to stop device stream: {}", e);
                }
            } else if user_count > 0 && !monitor_state.is_streaming.load(Ordering::Relaxed) {
                info!("Users detected, starting device stream...");
                monitor_state.is_streaming.store(true, Ordering::Relaxed);
                if let Err(e) = monitor_state.device_client.lock().await
                    .start_stream(device::pb::StartStreamRequest {}).await {
                    error!("Failed to start device stream: {}", e);
                    monitor_state.is_streaming.store(false, Ordering::Relaxed);
                }
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
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

        let mut buf = [0u8; 65536];

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, _src)) => {

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
