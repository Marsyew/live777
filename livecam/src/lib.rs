pub mod config;
#[cfg(feature = "net4mqtt")]
pub mod mqtt_client;
pub mod rtp_receiver;
pub mod whep_handler;
pub mod auth; 
pub mod utils; 

use axum::Router;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::api::API;
use webrtc::interceptor::registry::Registry;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use auth::AppState;

use self::config::CameraConfig;
#[cfg(feature = "net4mqtt")]
use rumqttc::{AsyncClient, QoS};

pub struct StreamState {
    subscriber_count: usize,
    track: Arc<TrackLocalStaticRTP>,
    rtp_receiver_handle: Option<JoinHandle<()>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
    config: CameraConfig,
}

#[derive(Clone)]
pub struct LiveCamManager {
    streams: Arc<Mutex<HashMap<String, StreamState>>>,
    pub webrtc_api: Arc<API>,
    #[cfg(feature = "net4mqtt")]
    mqtt_client: Option<AsyncClient>,
}

impl LiveCamManager {
    pub fn new(
        cameras: Vec<CameraConfig>,
        webrtc_api: Arc<API>,
        #[cfg(feature = "net4mqtt")] mqtt_client: Option<AsyncClient>,
    ) -> Self {
        let streams = cameras
            .into_iter()
            .map(|config| {
                let track = Arc::new(TrackLocalStaticRTP::new(
                    config.codec.clone().into(),
                    config.id.clone(),
                    "livecam-stream".to_owned(),
                ));
                let state = StreamState {
                    subscriber_count: 0,
                    track,
                    rtp_receiver_handle: None,
                    shutdown_tx: None,
                    config: config.clone(),
                };
                (config.id, state)
            })
            .collect();

        Self {
            streams: Arc::new(Mutex::new(streams)),
            webrtc_api,
            #[cfg(feature = "net4mqtt")]
            mqtt_client,
        }
    }

    pub fn add_subscriber(&self, stream_id: &str) -> Option<Arc<TrackLocalStaticRTP>> {
        let mut streams = self.streams.lock().unwrap();
        if let Some(state) = streams.get_mut(stream_id) {
            state.subscriber_count += 1;
            tracing::info!(
                stream_id,
                subscribers = state.subscriber_count,
                "Subscriber added."
            );

            #[cfg(feature = "net4mqtt")]
            if let Some(client) = &self.mqtt_client {
                let topic = format!("livecam/stream/{}/status", stream_id);
                let viewers_count = state.subscriber_count;
                let payload = serde_json::json!({
                    "status": "active",
                    "viewers": viewers_count,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })
                .to_string();
                let client_clone = client.clone();
                tokio::spawn(async move {
                    if let Err(e) = client_clone
                        .publish(topic, QoS::AtLeastOnce, true, payload)
                        .await
                    {
                        tracing::error!(error = %e, "Failed to publish MQTT stream status");
                    }
                });
            }

            if state.subscriber_count == 1 {
                tracing::info!(
                    stream_id,
                    port = state.config.rtp_port,
                    "First subscriber arrived, starting RTP receiver."
                );
                let (tx, rx) = mpsc::channel(1);
                state.shutdown_tx = Some(tx);
                let track_clone = state.track.clone();
                let port = state.config.rtp_port;

                let handle = tokio::spawn(async move {
                    if let Err(e) = rtp_receiver::start(port, track_clone, rx).await {
                        tracing::error!(port, error = %e, "RTP receiver task failed.");
                    }
                });
                state.rtp_receiver_handle = Some(handle);
            }
            return Some(state.track.clone());
        }
        None
    }

    pub fn remove_subscriber(&self, stream_id: &str) {
        let mut streams = self.streams.lock().unwrap();
        if let Some(state) = streams.get_mut(stream_id) {
            if state.subscriber_count > 0 {
                state.subscriber_count -= 1;
                tracing::info!(
                    stream_id,
                    subscribers = state.subscriber_count,
                    "Subscriber removed."
                );
            }

            #[cfg(feature = "net4mqtt")]
            if let Some(client) = &self.mqtt_client {
                let topic = format!("livecam/stream/{}/status", stream_id);
                let viewers_count = state.subscriber_count;
                let payload = if viewers_count == 0 {
                    serde_json::json!({
                        "status": "inactive",
                        "viewers": 0,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    })
                } else {
                    serde_json::json!({
                        "status": "active",
                        "viewers": viewers_count,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    })
                }
                .to_string();

                let client_clone = client.clone();
                tokio::spawn(async move {
                    if let Err(e) = client_clone
                        .publish(topic, QoS::AtLeastOnce, true, payload)
                        .await
                    {
                        tracing::error!(error = %e, "Failed to publish MQTT stream status");
                    }
                });
            }

            if state.subscriber_count == 0 {
                tracing::info!(stream_id, "Last subscriber left, stopping RTP receiver.");
                if let Some(tx) = state.shutdown_tx.take() {
                    let _ = tx.try_send(());
                }
                if let Some(handle) = state.rtp_receiver_handle.take() {
                    handle.abort();
                }
            }
        }
    }
}

pub async fn serve(
    cfg: Arc<RwLock<config::Config>>,
    listener: TcpListener,
    shutdown_signal: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let registry = Registry::new();
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();
    let webrtc_api = Arc::new(api);

    #[cfg(feature = "net4mqtt")]
    let livecam_manager = {
        let (cameras, mqtt_config_opt) = {
            let config_guard = cfg.read().unwrap();
            (
                config_guard.cameras.clone(),
                config_guard.net4mqtt.clone(),
            )
        };

        let mqtt_client = if let Some(mqtt_config) = mqtt_config_opt {
            let (client, eventloop) = mqtt_client::new(mqtt_config).await?;
            tokio::spawn(async move {
                mqtt_client::poll(eventloop).await;
            });
            Some(client)
        } else {
            None
        };

        LiveCamManager::new(cameras, webrtc_api.clone(), mqtt_client)
    };

    #[cfg(not(feature = "net4mqtt"))]
    let livecam_manager = {
        let cameras = {
            let config_guard = cfg.read().unwrap();
            config_guard.cameras.clone()
        };
        LiveCamManager::new(cameras, webrtc_api.clone())
    };

    // let (cameras, mqtt_config_opt) = {
    //     let config_guard = cfg.read().unwrap();
    //     (
    //         config_guard.cameras.clone(),
    //         #[cfg(feature = "net4mqtt")]
    //         config_guard.net4mqtt.clone(),
    //     )
    // };

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(feature = "net4mqtt")]
    let mqtt_client = if let Some(mqtt_config) = mqtt_config_opt {
        let mut mqtt_shutdown_rx = shutdown_tx.subscribe();
        let (client, eventloop) = mqtt_client::get_client(mqtt_config)?;
        
        let client_for_loop = client.clone();
        
        let mqtt_config_clone = mqtt_config.clone();
        tokio::spawn(async move {
            let shutdown_future = async {
                let _ = mqtt_shutdown_rx.recv().await;
            };

            mqtt_client::run_loop(
                mqtt_config_clone,
                client_for_loop, 
                eventloop,
                shutdown_future,
            )
            .await;
        });

        Some(client)
    } else {
        None
    };

    // let livecam_manager = LiveCamManager::new(
    //     cameras,
    //     webrtc_api.clone(),
    //     #[cfg(feature = "net4mqtt")]
    //     mqtt_client,
    // );

    let app_state = AppState {
        config: cfg,
        manager: livecam_manager,
    };

    let app = Router::new()
        .merge(whep_handler::create_router())
        .merge(auth::create_auth_router()) 
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )        
        .with_state(app_state);

    tracing::info!("Server started, processing requests...");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    Ok(())
}
