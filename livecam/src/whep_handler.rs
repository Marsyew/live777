use axum::http::{header, StatusCode};
use axum::{
    body::Body,
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use std::sync::Arc;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::{
    configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
    sdp::session_description::RTCSessionDescription,
};

use super::auth::{Claims, AppState};

pub fn create_router() -> Router<AppState> {
    Router::new().route("/whep/:stream_id", post(whep_handler))
}

async fn whep_handler(
    State(app_state): State<AppState>,
    _claims: Claims,
    Path(stream_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    let offer = match RTCSessionDescription::offer(body) {
        Ok(offer) => offer,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse SDP offer.");
            return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
        }
    };

    let manager = &app_state.manager;
    let track = match manager.add_subscriber(&stream_id) {
        Some(track) => track,
        None => {
            tracing::warn!(stream_id, "Requested stream not found.");
            return (StatusCode::NOT_FOUND, "Stream not found".to_string()).into_response();
        }
    };

    let rtc_config = {
        let config = app_state.config.read().unwrap();
        RTCConfiguration {
            ice_servers: config.ice_servers.iter().map(|s| s.clone().into()).collect(),
            ..Default::default()
        }
    };

    let pc:Arc<RTCPeerConnection> = match manager.webrtc_api.new_peer_connection(rtc_config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            tracing::error!(error = %e, "Failed to create PeerConnection.");
            manager.remove_subscriber(&stream_id);

            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    if let Err(e) = pc.add_track(track).await {
        tracing::error!(error = %e, "Failed to add track.");
        manager.remove_subscriber(&stream_id);
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let manager_clone = manager.clone();
    let stream_id_clone = stream_id.clone();
    pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        let stream_id_clone = stream_id_clone.clone();
        let manager_clone2 = manager_clone.clone();
        Box::pin(async move {
            tracing::debug!(stream_id = stream_id_clone, state = %s, "PeerConnection state changed.");
            if s == RTCPeerConnectionState::Closed
                || s == RTCPeerConnectionState::Disconnected
                || s == RTCPeerConnectionState::Failed
            {
                manager_clone2.remove_subscriber(&stream_id_clone);
            }
        })
    }));

    if let Err(e) = pc.set_remote_description(offer).await {
        tracing::error!(error = %e, "Failed to set remote description.");
        manager.remove_subscriber(&stream_id);

        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let answer = match pc.create_answer(None).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create answer.");
            manager.remove_subscriber(&stream_id);

            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let mut gather_complete = pc.gathering_complete_promise().await;

    if let Err(e) = pc.set_local_description(answer).await {
        tracing::error!(error = %e, "Failed to set local description.");
        manager.remove_subscriber(&stream_id);

        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let _ = gather_complete.recv().await;

    if let Some(local_desc) = pc.local_description().await {
        Response::builder()
            .status(StatusCode::CREATED)
            .header(header::CONTENT_TYPE, "application/sdp")
            .body(Body::from(local_desc.sdp))
            .unwrap()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get local description".to_string(),
        )
            .into_response()
    }
}
