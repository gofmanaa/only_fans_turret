use crate::app_state::{AppState, UserSession};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json as AxumJson;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};
use uuid::Uuid;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

/// Handle SDP offer from client and create peer connection
pub async fn handle_sdp_offer(
    State(app_state): State<Arc<AppState>>,
    Json(offer): Json<Value>,
) -> Result<AxumJson<Value>, (StatusCode, String)> {
    let client_id = offer
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap();
    let user_session = app_state.get_user(client_id).await.ok_or_else(|| {
        (
            StatusCode::NON_AUTHORITATIVE_INFORMATION,
            "User session not found.".to_string(),
        )
    })?;

    let client_id = user_session.id.clone();
    info!("Processing SDP offer for client: {}", client_id);

    // Create peer connection configuration
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".into()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let pc = app_state
        .api
        .new_peer_connection(config)
        .await
        .map(Arc::new)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create peer connection: {:?}", e),
            )
        })?;

    // Create video track
    let video_track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: "video/VP8".into(),
            ..Default::default()
        },
        "video".into(),
        "webrtc-rs".into(),
    ));

    pc.add_track(video_track.clone()).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to add track: {:?}", e),
        )
    })?;

    // RTP forwarding task
    {
        let track_clone = video_track.clone();
        let mut rtp_receiver = app_state.rtp_broadcast.subscribe();
        let client_id_clone = client_id.clone();
        let app_state_clone = app_state.clone();

        tokio::spawn(async move {
            info!("Starting RTP forwarding for client: {}", client_id_clone);

            while let Ok(packet) = rtp_receiver.recv().await {
                if let Err(e) = track_clone.write_rtp(&packet).await {
                    error!(
                        "Error writing RTP to track for client {}: {}",
                        client_id_clone, e
                    );
                    break;
                }
            }

            info!("RTP forwarding ended for client: {}", client_id_clone);
            // todo: dont need remove user session
            //app_state_clone.remove_user(&client_id_clone).await;
        });
    }

    // PeerConnection state monitoring
    {
        let client_id_monitor = client_id.clone();
        let app_state_monitor = app_state.clone();

        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let client_id = client_id_monitor.clone();
            let app_state = app_state_monitor.clone();
            info!("Client {} PeerConnection state: {:?}", client_id, state);

            // todo: dont need remove user session
            // if matches!(state, RTCPeerConnectionState::Failed
            //               | RTCPeerConnectionState::Disconnected
            //               | RTCPeerConnectionState::Closed) {
            //     tokio::spawn(async move {
            //         app_state.remove_user(&client_id).await;
            //     });
            // }

            Box::pin(async {})
        }));
    }

    // Process SDP offer
    let offer_sdp_str = offer
        .get("sdp")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "Missing SDP in offer".to_string()))?;

    let offer_sdp = RTCSessionDescription::offer(offer_sdp_str.to_string()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to parse offer SDP: {:?}", e),
        )
    })?;

    pc.set_remote_description(offer_sdp).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to set remote description: {:?}", e),
        )
    })?;

    let answer = pc.create_answer(None).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create answer: {:?}", e),
        )
    })?;

    pc.set_local_description(answer.clone())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to set local description: {:?}", e),
            )
        })?;

    // Wait for ICE gathering
    let mut gather_complete = pc.gathering_complete_promise().await;
    gather_complete.recv().await;

    let local_desc = pc.local_description().await.ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "No local description available".to_string(),
    ))?;

    // Register client in AppState
    // let client = UserSession {
    //     id: client_id.clone(),
    //     joined_at: Instant::now(),
    //     has_control: false,
    //     peer_connection: Some(pc),
    //     video_track, control_granted_at: None
    // };
    // app_state.add_user(client).await;

    let answer_json = serde_json::json!({
        "type": local_desc.sdp_type.to_string(),
        "sdp": local_desc.sdp,
        "client_id": client_id
    });

    info!("Successfully created answer for client: {client_id}",);
    Ok(AxumJson(answer_json))
}
