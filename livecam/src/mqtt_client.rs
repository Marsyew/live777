#![cfg(feature = "net4mqtt")]

use super::config::MqttConfig;
use rumqttc::{AsyncClient, EventLoop, LastWill, MqttOptions, QoS};
use std::future::Future;
use std::time::Duration;

pub fn get_client(config: &MqttConfig) -> anyhow::Result<(AsyncClient, EventLoop)> {
    let mut mqtt_options = MqttOptions::new(
        format!("livecam-{}", config.alias),
        config.url.split(':').next().unwrap_or("localhost"),
        config
            .url
            .split(':')
            .nth(1)
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(1883),
    );

    mqtt_options.set_keep_alive(Duration::from_secs(5));

    let last_will_topic = format!("livecam/status/{}", config.alias);
    let last_will_payload = serde_json::json!({
        "status": "offline",
        "reason": "connection_lost",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
    .to_string();
    let will = LastWill::new(last_will_topic, last_will_payload, QoS::AtLeastOnce, false);
    mqtt_options.set_last_will(will);

    let (client, eventloop) = AsyncClient::new(mqtt_options, 10);
    Ok((client, eventloop))
}

pub async fn run_loop(
    config: MqttConfig,
    client: AsyncClient,
    mut eventloop: EventLoop,
    shutdown_signal: impl Future<Output = ()>,
) {
    tracing::info!(url = %config.url, "MQTT client loop starting...");

    let status_client = client.clone();
    let status_alias = config.alias.clone();
    let status_task = tokio::spawn(async move {
        loop {
            let payload = serde_json::json!({
                "status": "online",
                "timestamp": chrono::Utc::now().to_rfc3339(),
            })
            .to_string();

            if let Err(e) = status_client
                .publish(
                    format!("livecam/status/{}", status_alias),
                    QoS::AtLeastOnce,
                    true,
                    payload,
                )
                .await
            {
                tracing::error!(error = %e, "Failed to publish MQTT status.");
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    });

    tokio::pin!(shutdown_signal);

    loop {
        tokio::select! {
            _ = &mut shutdown_signal => {
                tracing::info!("MQTT client received shutdown signal...");
                break;
            },
            result = eventloop.poll() => {
                match result {
                    Ok(notification) => {
                        tracing::debug!(?notification, "Received MQTT event.");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "MQTT connection error. Reconnecting in 5 seconds...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }

    status_task.abort();

    let final_payload = serde_json::json!({
        "status": "offline",
        "reason": "graceful_shutdown",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
    .to_string();

    tracing::info!("Publishing 'offline' status message...");
    if let Err(e) = client
        .publish(
            format!("livecam/status/{}", config.alias),
            QoS::AtLeastOnce,
            true,
            final_payload,
        )
        .await
    {
        tracing::error!(error = %e, "Failed to publish 'offline' message.");
    }

    if let Err(e) = client.disconnect().await {
        tracing::error!(error = %e, "Error during MQTT client disconnection.");
    }

    tracing::info!("MQTT client has shut down successfully.");
}
