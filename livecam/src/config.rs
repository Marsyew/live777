use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, str::FromStr};
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use rand::Rng;
use base64::{Engine as _, engine::general_purpose};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub http: Http,
    #[serde(default)]
    pub log: Log,
    pub cameras: Vec<CameraConfig>,
    #[cfg(feature = "net4mqtt")]
    pub net4mqtt: Option<MqttConfig>,
    #[serde(default)]
    pub ice_servers: Vec<liveion::config::IceServer>,
    #[serde(default)]
    pub auth: AuthConfig
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    #[serde(default = "default_log_level")]
    pub level: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Http {
    #[serde(default = "default_http_listen")]
    pub listen: SocketAddr,
    #[serde(default)]
    pub cors: bool,
    #[serde(default)]
    pub public: String,
}

fn default_http_listen() -> SocketAddr {
    SocketAddr::from_str(&format!(
        "0.0.0.0:{}",
        env::var("PORT").unwrap_or(String::from("9999"))
    ))
    .expect("invalid listen address")
}

impl Default for Http {
    fn default() -> Self {
        Self {
            listen: default_http_listen(),
            public: Default::default(),
            cors: Default::default(),
        }
    }
}

impl Default for Log {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            username: "admin".to_string(),

            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$bmljZXRyeQ$PqTT/n9ToBNVsdsoquTz1A/P5s9O4yvA9fym5Vd5s9s".to_string(),
            jwt_secret: default_jwt_secret(),
        }
    }
}

fn default_jwt_secret() -> String {
    let random_bytes: [u8; 32] = rand::rng().random();
    general_purpose::URL_SAFE_NO_PAD.encode(random_bytes)
}


fn default_log_level() -> String {
    env::var("LOG_LEVEL").unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "info".to_string()
        }
    })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CameraConfig {
    pub id: String,
    pub rtp_port: u16,
    pub codec: CodecConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodecConfig {
    pub mime_type: String,
    pub clock_rate: u32,
    pub channels: u16,
    pub sdp_fmtp_line: Option<String>,
}

impl From<CodecConfig> for RTCRtpCodecCapability {
    fn from(val: CodecConfig) -> Self {
        RTCRtpCodecCapability {
            mime_type: val.mime_type,
            clock_rate: val.clock_rate,
            channels: val.channels,
            sdp_fmtp_line: val.sdp_fmtp_line.unwrap_or_default(),
            rtcp_feedback: vec![], // TODO
        }
    }
}

#[cfg(feature = "net4mqtt")]
#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    pub url: String,
    pub alias: String,
}
impl Config {
    pub fn validate(&mut self) -> anyhow::Result<()> {
        if self.http.public.is_empty() {
            self.http.public = format!("http://{}", self.http.listen);
        }

        if self.auth.jwt_secret.is_empty() {
            tracing::warn!("auth.jwt_secret is empty or not set. A random secret will be used for this session.");
            self.auth.jwt_secret = default_jwt_secret();
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http: Http::default(),
            log: Log::default(),
            cameras: Vec::new(),
            #[cfg(feature = "net4mqtt")]
            net4mqtt: None,
            ice_servers: Vec::new(),
            auth: AuthConfig::default(),
        }
    }
}
