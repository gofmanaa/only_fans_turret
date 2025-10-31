use crate::app_state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json as AxumJson;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info};
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::policy::ice_transport_policy::RTCIceTransportPolicy;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use crate::turn::{generate_turn_credentials, TurnCredentials};

#[derive(Deserialize)]
pub struct TurnRequest {
    pub client_id: String,
}

#[derive(Serialize)]
pub struct TurnResponse {
    pub turn: TurnCredentials,
}

#[derive(Deserialize)]
pub struct SdpRequest {
    pub client_id: String,
    pub sdp: String,
    #[serde(rename = "type")]
    pub sdp_type: String,
}

#[derive(Serialize)]
pub struct SdpResponse {
    #[serde(rename = "type")]
    pub sdp_type: String,
    pub sdp: String,
    pub client_id: String,
}


/// Handle SDP offer from client and create peer connection
pub async fn handle_sdp_offer(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<SdpRequest>,
) -> Result<AxumJson<SdpResponse>, (StatusCode, String)> {
    if request.sdp_type.to_lowercase() != "offer" {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Expected SDP offer, got: {}", request.sdp_type),
        ));
    }

    let user_session = app_state
        .get_user(&request.client_id)
        .await
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "User session not found".to_string(),
        ))?;

    let client_id = user_session.id.clone();
    info!("Processing SDP offer for client: {}", client_id);

    let turn_creds = generate_turn_credentials(app_state.web_config.clone());

    let ice_servers = vec![RTCIceServer {
        urls: turn_creds.urls.clone(),
        username: turn_creds.username.clone(),
        credential: turn_creds.credential.clone(),
        credential_type: RTCIceCredentialType::Password,
    }];

    // Create peer connection configuration
    let config = RTCConfiguration {
        ice_servers,
        ice_transport_policy: RTCIceTransportPolicy::All,
        ..Default::default()
    };

    let pc = app_state
        .api
        .new_peer_connection(config)
        .await
        .map(Arc::new)
        .map_err(|e| {
            error!("Failed to create peer connection: {:?}", e);
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

        tokio::spawn(async move {
            info!("Starting RTP forwarding for client: {}", client_id_clone);

            loop {
                match rtp_receiver.recv().await {
                    Ok(packet) => {
                        if let Err(e) = track_clone.write_rtp(&packet).await {
                            error!("Error writing RTP for client {}: {}", client_id_clone, e);
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        error!("RTP receiver lagged for client {}, skipped {} packets", client_id_clone, skipped);
                        continue;
                    }
                    Err(RecvError::Closed) => {
                        info!("RTP broadcast closed for client: {}", client_id_clone);
                        break;
                    }
                }
            }

            info!("RTP forwarding ended for client: {}", client_id_clone);
        });
    }

    setup_pc_monitoring(pc.clone(), client_id.clone());

    // Process SDP offer
    let offer_sdp = RTCSessionDescription::offer(request.sdp).map_err(|e| {
        error!("Failed to parse SDP offer from {}: {:?}", client_id, e);
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to parse offer SDP: {:?}", e),
        )
    })?;

    pc.set_remote_description(offer_sdp).await.map_err(|e| {
        error!("Failed to set remote description for {}: {:?}", client_id, e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to set remote description: {:?}", e),
        )
    })?;

    let answer = pc.create_answer(None).await.map_err(|e| {
        error!("Failed to set remote description for {}: {:?}", client_id, e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create answer: {:?}", e),
        )
    })?;

    pc.set_local_description(answer.clone())
        .await
        .map_err(|e| {
            error!("Failed to set local description for {}: {:?}", client_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to set local description: {:?}", e),
            )
        })?;

    // Wait for ICE gathering
    let mut gather_complete = pc.gathering_complete_promise().await;
    let _ =gather_complete.recv().await;

    // Get final local description
    let local_desc = pc.local_description().await.ok_or_else(|| {
        error!("No local description available for {}", client_id);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "No local description available".to_string(),
        )
    })?;

    info!("Successfully created answer for client: {client_id}",);
    Ok(AxumJson(SdpResponse {
        sdp_type: local_desc.sdp_type.to_string(),
        sdp: local_desc.sdp,
        client_id,
    }))
}


pub async fn get_turn_credentials(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<TurnRequest>,
) -> Result<AxumJson<TurnResponse>, (StatusCode, String)> {
    app_state
        .get_user(&request.client_id)
        .await
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "User session not found".to_string(),
        ))?;

    let turn_creds = generate_turn_credentials(app_state.web_config.clone());

    Ok(AxumJson(TurnResponse {
        turn: TurnCredentials {
            urls: turn_creds.urls,
            username: turn_creds.username,
            credential: turn_creds.credential,
        },
    }))
}

fn setup_pc_monitoring(
    pc: Arc<webrtc::peer_connection::RTCPeerConnection>,
    client_id: String,
) {
    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let client_id = client_id.clone();
        info!("Client {} PeerConnection state: {:?}", client_id, state);

        // Handle cleanup on failed/closed states
        if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
            info!("Peer connection {} terminated", client_id);
        }

        Box::pin(async {})
    }));
}