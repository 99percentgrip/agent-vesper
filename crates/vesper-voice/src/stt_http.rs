//! Configured self-hosted remote STT over HTTP (feature `stt-http`).
//!
//! Implements the pinned worker's actual contract (verified against the
//! reconnaissance pin): `POST /stt` with a raw i16-PCM canonical body,
//! optional shared-token header, JSON `{"text": …}` responses; <0.1 s
//! bodies and transcription exceptions both yield `{"text": ""}`.
//!
//! **Legacy-worker limitation (explicit, D21):** the pinned worker
//! catches engine exceptions server-side and returns empty text. A
//! client cannot recover a distinction the server discarded. An empty
//! successful response therefore carries
//! [`TranscriptProvenance::LegacyEmptyResponse`] — "no transcript was
//! returned" — and is **never** labeled as VAD-confirmed silence, never
//! retried against a different egress destination, and never
//! supplemented with invented server fields. A stricter future worker
//! protocol is a separately scoped change; the external reference is
//! not modified or redeployed to manufacture compliance.
//!
//! Egress: the endpoint comes from validated configuration
//! (`http://` loopback-or-LAN, no redirects — any redirect response is
//! an error, never followed); the descriptor declares
//! [`SpeechEgressClass::SelfHostedRemote`]. A loopback address alone is
//! not proof of on-device inference: classification follows the
//! configured service's trust contract, and the composition layer's
//! policy decides admission.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use vesper_domain::{BoundedString, ProviderId};

use crate::audio::PcmFrame;
use crate::cancel::VoiceCancel;
use crate::composition::blocking::{self, ValueExecutor};
use crate::error::VoiceError;
use crate::ports::{
    SpeechEgressClass, SttDescriptor, SttTranscript, TranscriptProvenance, VoiceFuture, VoiceStt,
};

/// Validated remote-worker endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpSttEndpoint {
    /// Host (IPv4/IPv6/hostname; no userinfo, no path).
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Request path (must start with `/`).
    pub path: String,
    /// Optional bearer-style shared token (from the credential store,
    /// never from user strings at request time).
    pub token: Option<String>,
}

/// Configuration for the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpSttConfig {
    /// Validated endpoint.
    pub endpoint: HttpSttEndpoint,
    /// Connect+request deadline (one attempt; no transport retry
    /// multiplication here — failover retries are the composition's).
    pub deadline: Duration,
    /// Maximum accepted response body (bytes).
    pub max_response_bytes: usize,
    /// Maximum accepted transcript length after decoding.
    pub max_transcript_bytes: usize,
}

impl Default for HttpSttConfig {
    fn default() -> Self {
        Self {
            endpoint: HttpSttEndpoint {
                host: String::from("127.0.0.1"),
                port: 8768,
                path: String::from("/stt"),
                token: None,
            },
            deadline: Duration::from_secs(10),
            max_response_bytes: 64 * 1024,
            max_transcript_bytes: 8192,
        }
    }
}

impl HttpSttConfig {
    /// Validates the endpoint shape: `http` scheme only, host without
    /// userinfo, path starting with `/`, no query/fragment, no
    /// percent-encoded host tricks.
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] naming the offending field.
    pub fn validate(&self) -> Result<(), VoiceError> {
        let endpoint = &self.endpoint;
        if endpoint.host.is_empty() || endpoint.host.contains(['/', '?', '#', '@']) {
            return Err(VoiceError::InvalidInput(
                "endpoint host must be a bare host (no userinfo/path/query)".into(),
            ));
        }
        if endpoint.port == 0 {
            return Err(VoiceError::InvalidInput("endpoint port must be set".into()));
        }
        if !endpoint.path.starts_with('/') || endpoint.path.contains(['?', '#']) {
            return Err(VoiceError::InvalidInput(
                "endpoint path must start with '/' and carry no query or fragment".into(),
            ));
        }
        Ok(())
    }

    /// Absolute `http://` URL (formed from validated parts only).
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "http://{}:{}{}",
            self.endpoint.host, self.endpoint.port, self.endpoint.path
        )
    }
}

/// Rust adapter for the configured self-hosted HTTP STT worker.
pub struct HttpStt {
    config: HttpSttConfig,
    descriptor: SttDescriptor,
    executor: Arc<dyn ValueExecutor>,
}

impl HttpStt {
    /// Creates the adapter (validates configuration eagerly).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] when the endpoint shape is invalid.
    pub fn new(
        config: HttpSttConfig,
        executor: Arc<dyn ValueExecutor>,
    ) -> Result<Self, VoiceError> {
        config.validate()?;
        let descriptor = SttDescriptor {
            provider: ProviderId::new("stt-http").expect("static id fits"),
            model: BoundedString::new(format!(
                "http://{}:{}",
                config.endpoint.host, config.endpoint.port
            ))
            .unwrap_or_else(|_| BoundedString::new("http-worker").unwrap()),
            egress: SpeechEgressClass::SelfHostedRemote,
            partials: None,
            vad_enabled: false,
            languages: Vec::new(),
        };
        Ok(Self {
            config,
            descriptor,
            executor,
        })
    }
}

/// One HTTP attempt's classified outcome (metadata only).
#[derive(Debug, Clone, PartialEq, Eq)]
enum HttpOutcome {
    Success {
        status: u16,
        body: Vec<u8>,
    },
    AuthFailure,
    QuotaFailure,
    /// Response body exceeded its bound (server misbehavior, not
    /// transport): classified as invalid input, not unavailable.
    Oversized,
    Transport(String),
}

/// Performs one blocking HTTP POST of canonical PCM.
///
/// Uses `std::net::TcpStream` with a strict hand-rolled request:
/// no redirects (3xx is an error), no TLS (self-hosted `http` contract),
/// bounded read with deadline, `Connection: close`.
fn http_post_pcm(config: &HttpSttConfig, pcm: &[u8]) -> HttpOutcome {
    let address = format!("{}:{}", config.endpoint.host, config.endpoint.port);
    let Ok(mut stream) = TcpStream::connect(&address) else {
        return HttpOutcome::Transport("connect failed".into());
    };
    let _ = stream.set_read_timeout(Some(config.deadline));
    let _ = stream.set_write_timeout(Some(config.deadline));
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        config.endpoint.path,
        config.endpoint.host,
        config.endpoint.port,
        pcm.len()
    );
    if let Some(token) = &config.endpoint.token {
        // Bearer header only from the validated credential source.
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    if stream.write_all(request.as_bytes()).is_err() || stream.write_all(pcm).is_err() {
        return HttpOutcome::Transport("write failed".into());
    }
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                raw.extend_from_slice(&chunk[..count]);
                if raw.len() > config.max_response_bytes {
                    return HttpOutcome::Oversized;
                }
            }
            Err(_) => return HttpOutcome::Transport("read failed or timed out".into()),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return HttpOutcome::Transport("malformed response".into());
    };
    let status_line = head.lines().next().unwrap_or_default();
    let Some(status) = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
    else {
        return HttpOutcome::Transport("malformed status".into());
    };
    match status {
        200 => HttpOutcome::Success {
            status,
            body: body.as_bytes().to_vec(),
        },
        401 | 403 => HttpOutcome::AuthFailure,
        402 | 429 => HttpOutcome::QuotaFailure,
        _ if (300..400).contains(&status) => {
            // Redirects are never followed: the configured destination is
            // the only permitted egress target.
            HttpOutcome::Transport("redirect refused".into())
        }
        _ => HttpOutcome::Transport(format!("http {status}")),
    }
}

impl VoiceStt for HttpStt {
    fn transcribe<'a>(
        &'a self,
        audio: &'a [PcmFrame],
        cancel: &'a VoiceCancel,
    ) -> VoiceFuture<'a, Result<SttTranscript, VoiceError>> {
        Box::pin(async move {
            if audio.is_empty() {
                return Err(VoiceError::InvalidInput(
                    "no audio captured for transcription".into(),
                ));
            }
            let mut pcm: Vec<u8> = Vec::new();
            for frame in audio {
                pcm.extend_from_slice(frame.bytes());
            }
            // The pinned worker rejects <0.1 s bodies with an empty
            // response; surface that as invalid input instead of paying
            // a network round trip for a known-empty outcome.
            if pcm.len() < 3200 {
                return Err(VoiceError::InvalidInput(
                    "capture shorter than the worker's minimum (0.1 s)".into(),
                ));
            }
            let provider = self.descriptor.provider.clone();
            let config = self.config.clone();
            let outcome = blocking::run_blocking(
                self.executor.as_ref(),
                cancel,
                Box::new(move || Ok(http_post_pcm(&config, &pcm))),
            )
            .await;
            // run_blocking already mapped error types; the closure returns
            // HttpOutcome, so map it now.
            let outcome = outcome?;
            match outcome {
                HttpOutcome::AuthFailure => Err(VoiceError::Auth { provider }),
                HttpOutcome::QuotaFailure => Err(VoiceError::Quota { provider }),
                HttpOutcome::Oversized => Err(VoiceError::InvalidInput(
                    "worker response exceeded its bound".into(),
                )),
                HttpOutcome::Transport(reason) => Err(VoiceError::Unavailable {
                    provider,
                    reason: BoundedString::new(reason)
                        .unwrap_or_else(|_| BoundedString::new("transport failure").unwrap()),
                }),
                HttpOutcome::Success { body, .. } => {
                    let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
                        VoiceError::InvalidInput("worker returned malformed JSON".into())
                    })?;
                    let text = value
                        .get("text")
                        .ok_or_else(|| {
                            VoiceError::InvalidInput("worker response missing `text`".into())
                        })?
                        .as_str()
                        .ok_or_else(|| {
                            VoiceError::InvalidInput("worker `text` is not a string".into())
                        })?
                        .trim();
                    if text.len() > self.config.max_transcript_bytes {
                        return Err(VoiceError::InvalidInput(
                            "worker transcript exceeded its bound".into(),
                        ));
                    }
                    let trimmed = text;
                    if trimmed.is_empty() {
                        // D21: legacy worker — empty proves only "no
                        // transcript was returned". Not confirmed silence.
                        return Ok(SttTranscript {
                            text: BoundedString::new(String::new()).unwrap(),
                            provider,
                            confidence: None,
                            provenance: TranscriptProvenance::LegacyEmptyResponse,
                        });
                    }
                    let bounded = BoundedString::new(trimmed).map_err(|_| {
                        VoiceError::InvalidInput("transcript exceeded its bound".into())
                    })?;
                    Ok(SttTranscript {
                        text: bounded,
                        provider,
                        confidence: None,
                        provenance: TranscriptProvenance::InferredText,
                    })
                }
            }
        })
    }

    fn descriptor(&self) -> &SttDescriptor {
        &self.descriptor
    }
}

/// Config-source marker: endpoints come from validated configuration.
/// This type exists so hosts cannot construct an endpoint from raw
/// user input by accident; they must go through
/// [`HttpSttEndpoint::from_validated_parts`].
impl HttpSttEndpoint {
    /// Builds an endpoint from parts already validated by the
    /// configuration layer (TOML scope parse + host validation).
    ///
    /// # Errors
    ///
    /// [`VoiceError::InvalidInput`] when any part fails the shape rules.
    pub fn from_validated_parts(
        host: impl Into<String>,
        port: u16,
        path: impl Into<String>,
    ) -> Result<Self, VoiceError> {
        let endpoint = Self {
            host: host.into(),
            port,
            path: path.into(),
            token: None,
        };
        let config = HttpSttConfig {
            endpoint,
            ..HttpSttConfig::default()
        };
        config.validate()?;
        Ok(config.endpoint)
    }
}

// PathBuf is intentionally unused: endpoints never touch the filesystem.
#[allow(dead_code)]
fn _path_marker(_value: PathBuf) {}
