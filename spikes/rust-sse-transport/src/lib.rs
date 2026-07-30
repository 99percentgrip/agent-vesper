//! Disposable bounded SSE parser and local transport probes.

use std::{
    fmt,
    time::{Duration, SystemTime},
};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

const MAX_LINE: usize = 4096;
const MAX_BODY: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Reasoning(String),
    Content(String),
    ToolDelta {
        index: u64,
        arguments: String,
    },
    Usage {
        input: u64,
        output: u64,
        cached: u64,
    },
    Finish(String),
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    Cancelled,
    Timeout(&'static str),
    Http(u16),
    Transport { visible: bool },
    Incomplete { visible: bool },
    MalformedJson,
    Limit(&'static str),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StreamError {}

fn visible(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::Reasoning(_) | Event::Content(_)))
}

fn parse_data(data: &str, events: &mut Vec<Event>) -> Result<bool, StreamError> {
    if data == "[DONE]" {
        events.push(Event::Done);
        return Ok(true);
    }
    let value: Value = serde_json::from_str(data).map_err(|_| StreamError::MalformedJson)?;
    if let Some(usage) = value.get("usage") {
        let details = usage.get("prompt_tokens_details").unwrap_or(&Value::Null);
        events.push(Event::Usage {
            input: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cached: details
                .get("cached_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        });
    }
    let Some(choice) = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|v| v.first())
    else {
        return Ok(false);
    };
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
        if !text.is_empty() {
            events.push(Event::Reasoning(text.to_owned()));
        }
    }
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            events.push(Event::Content(text.to_owned()));
        }
    }
    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            events.push(Event::ToolDelta {
                index: call.get("index").and_then(Value::as_u64).unwrap_or(0),
                arguments: arguments.to_owned(),
            });
        }
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        events.push(Event::Finish(reason.to_owned()));
        return Ok(true);
    }
    Ok(false)
}

fn parse_line(line: &[u8], events: &mut Vec<Event>) -> Result<bool, StreamError> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty() || line.starts_with(b":") || !line.starts_with(b"data:") {
        return Ok(false);
    }
    let text = std::str::from_utf8(&line[5..]).map_err(|_| StreamError::MalformedJson)?;
    let data = text.trim();
    if data.is_empty() {
        return Ok(false);
    }
    match parse_data(data, events) {
        // Python deliberately ignores malformed JSON SSE lines.
        Err(StreamError::MalformedJson) => Ok(false),
        result => result,
    }
}

pub async fn read_sse(
    client: &reqwest::Client,
    url: &str,
    cancellation: CancellationToken,
) -> Result<Vec<Event>, StreamError> {
    if cancellation.is_cancelled() {
        return Err(StreamError::Cancelled);
    }
    let request = client.post(url).json(&serde_json::json!({"fixture": true}));
    let mut response = tokio::select! {
        _ = cancellation.cancelled() => return Err(StreamError::Cancelled),
        result = request.send() => result.map_err(|error| {
            if error.is_timeout() { StreamError::Timeout("request") }
            else { StreamError::Transport { visible: false } }
        })?,
    };
    if !response.status().is_success() {
        return Err(StreamError::Http(response.status().as_u16()));
    }

    let mut buffer = Vec::new();
    let mut total = 0usize;
    let mut events = Vec::new();
    let mut terminal = false;
    loop {
        let next = tokio::select! {
            _ = cancellation.cancelled() => return Err(StreamError::Cancelled),
            result = response.chunk() => result.map_err(|error| {
                if error.is_timeout() { StreamError::Timeout("read") }
                else { StreamError::Transport { visible: visible(&events) } }
            })?,
        };
        let Some(bytes) = next else { break };
        total = total.saturating_add(bytes.len());
        if total > MAX_BODY {
            return Err(StreamError::Limit("body"));
        }
        buffer.extend_from_slice(&bytes);
        while let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=index).collect();
            if line.len() - 1 > MAX_LINE {
                return Err(StreamError::Limit("line"));
            }
            terminal |= parse_line(&line[..line.len() - 1], &mut events)?;
            if cancellation.is_cancelled() {
                return Err(StreamError::Cancelled);
            }
        }
        if buffer.len() > MAX_LINE {
            return Err(StreamError::Limit("line"));
        }
    }
    if !buffer.is_empty() {
        terminal |= parse_line(&buffer, &mut events)?;
    }
    if terminal {
        Ok(events)
    } else {
        Err(StreamError::Incomplete {
            visible: visible(&events),
        })
    }
}

pub fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<f64>() {
        if seconds > 0.0 {
            return Some(Duration::from_secs_f64(seconds.min(60.0)));
        }
    }
    let date = httpdate::parse_http_date(value).ok()?;
    date.duration_since(now)
        .ok()
        .filter(|duration| !duration.is_zero())
        .map(|duration| duration.min(Duration::from_secs(60)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[derive(Clone)]
    struct Part {
        delay: Duration,
        bytes: Vec<u8>,
    }

    async fn server(
        status: u16,
        headers: &[(&str, &str)],
        parts: Vec<Part>,
        accepts: Arc<AtomicUsize>,
        declared_length: Option<usize>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let headers = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect::<String>();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            accepts.fetch_add(1, Ordering::SeqCst);
            let mut request = vec![0; 4096];
            let _ = socket.read(&mut request).await;
            let length = declared_length
                .map(|value| format!("Content-Length: {value}\r\n"))
                .unwrap_or_default();
            let response = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: text/event-stream\r\n{headers}{length}Connection: close\r\n\r\n"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            for part in parts {
                tokio::time::sleep(part.delay).await;
                if socket.write_all(&part.bytes).await.is_err() {
                    break;
                }
                let _ = socket.flush().await;
            }
        });
        format!("http://{address}/chat/completions")
    }

    fn part(bytes: impl Into<Vec<u8>>) -> Part {
        Part {
            delay: Duration::ZERO,
            bytes: bytes.into(),
        }
    }

    fn delayed(milliseconds: u64, bytes: impl Into<Vec<u8>>) -> Part {
        Part {
            delay: Duration::from_millis(milliseconds),
            bytes: bytes.into(),
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(300))
            .timeout(Duration::from_secs(2))
            .read_timeout(Duration::from_millis(500))
            .retry(reqwest::retry::never())
            .build()
            .unwrap()
    }

    fn line(value: Value) -> Vec<u8> {
        format!("data: {}\n", serde_json::to_string(&value).unwrap()).into_bytes()
    }

    #[tokio::test]
    async fn python_fixture_is_consumable() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/provider/glm/reasoning-then-content/result.python.json"
        ))
        .unwrap();
        assert_eq!(fixture["scenario_id"], "glm.reasoning-then-content");
    }

    #[tokio::test]
    async fn arbitrary_bytes_utf8_crlf_comments_and_order() {
        let json =
            serde_json::json!({"choices":[{"delta":{"reasoning_content":"思","content":"答"}}]});
        let mut bytes = format!(": note\r\nevent: x\r\ndata: {}\r\n", json).into_bytes();
        bytes.extend_from_slice(
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        );
        let parts = bytes.into_iter().map(|byte| part(vec![byte])).collect();
        let accepts = Arc::new(AtomicUsize::new(0));
        let url = server(200, &[], parts, Arc::clone(&accepts), None).await;
        let events = read_sse(&client(), &url, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            events,
            [
                Event::Reasoning("思".into()),
                Event::Content("答".into()),
                Event::Finish("stop".into())
            ]
        );
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_json_non_data_and_done_are_handled_like_python() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let url = server(
            200,
            &[],
            vec![
                part(b"data: {broken}\nnot-data\n\n".to_vec()),
                part(b"data: [DONE]\n".to_vec()),
            ],
            accepts,
            None,
        )
        .await;
        assert_eq!(
            read_sse(&client(), &url, CancellationToken::new())
                .await
                .unwrap(),
            [Event::Done]
        );
    }

    #[tokio::test]
    async fn fragmented_interleaved_tools_and_usage_are_ordered() {
        let one = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{"}}]}}]});
        let two = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}],"usage":{"prompt_tokens":9,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":4}}});
        let finish = serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]});
        let accepts = Arc::new(AtomicUsize::new(0));
        let url = server(
            200,
            &[],
            vec![part(line(one)), part(line(two)), part(line(finish))],
            accepts,
            None,
        )
        .await;
        let events = read_sse(&client(), &url, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            events[0],
            Event::ToolDelta {
                index: 1,
                arguments: "{".into()
            }
        );
        assert_eq!(
            events[1],
            Event::Usage {
                input: 9,
                output: 2,
                cached: 4
            }
        );
        assert_eq!(
            events[2],
            Event::ToolDelta {
                index: 0,
                arguments: "{}".into()
            }
        );
        assert_eq!(events[3], Event::Finish("tool_calls".into()));
    }

    #[tokio::test]
    async fn eof_and_transport_failure_preserve_visible_classification() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let empty = server(200, &[], vec![], Arc::clone(&accepts), None).await;
        assert_eq!(
            read_sse(&client(), &empty, CancellationToken::new()).await,
            Err(StreamError::Incomplete { visible: false })
        );
        let partial = server(
            200,
            &[],
            vec![part(line(
                serde_json::json!({"choices":[{"delta":{"content":"partial"}}]}),
            ))],
            accepts,
            None,
        )
        .await;
        assert_eq!(
            read_sse(&client(), &partial, CancellationToken::new()).await,
            Err(StreamError::Incomplete { visible: true })
        );
    }

    #[tokio::test]
    async fn declared_length_transport_failure_preserves_partial_output_class() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let before = server(200, &[], vec![], Arc::clone(&accepts), Some(100)).await;
        assert_eq!(
            read_sse(&client(), &before, CancellationToken::new()).await,
            Err(StreamError::Transport { visible: false })
        );
        let body = line(serde_json::json!({"choices":[{"delta":{"content":"partial"}}]}));
        let after = server(200, &[], vec![part(body)], accepts, Some(1000)).await;
        assert_eq!(
            read_sse(&client(), &after, CancellationToken::new()).await,
            Err(StreamError::Transport { visible: true })
        );
    }

    #[tokio::test]
    async fn status_and_retry_after_are_explicit() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let url = server(
            503,
            &[("Retry-After", "2")],
            vec![],
            Arc::clone(&accepts),
            None,
        )
        .await;
        assert_eq!(
            read_sse(&client(), &url, CancellationToken::new()).await,
            Err(StreamError::Http(503))
        );
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
        assert_eq!(
            retry_after("2", SystemTime::UNIX_EPOCH),
            Some(Duration::from_secs(2))
        );
        let date = httpdate::fmt_http_date(SystemTime::UNIX_EPOCH + Duration::from_secs(30));
        assert_eq!(
            retry_after(&date, SystemTime::UNIX_EPOCH),
            Some(Duration::from_secs(30))
        );
    }

    #[tokio::test]
    async fn cancellation_before_request_before_headers_and_mid_body_emits_nothing_after() {
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            read_sse(&client(), "http://127.0.0.1:9", token).await,
            Err(StreamError::Cancelled)
        );

        let accepts = Arc::new(AtomicUsize::new(0));
        let before_headers = server(
            200,
            &[],
            vec![delayed(300, b"data: [DONE]\n".to_vec())],
            Arc::clone(&accepts),
            None,
        )
        .await;
        let token = CancellationToken::new();
        let child = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            child.cancel();
        });
        assert_eq!(
            read_sse(&client(), &before_headers, token).await,
            Err(StreamError::Cancelled)
        );

        let mid = server(
            200,
            &[],
            vec![
                part(line(serde_json::json!({"choices":[{"delta":{"content":"first"}}]}))),
                delayed(300, line(serde_json::json!({"choices":[{"delta":{"content":"late"},"finish_reason":"stop"}]}))),
            ],
            accepts,
            None,
        )
        .await;
        let token = CancellationToken::new();
        let child = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            child.cancel();
        });
        assert_eq!(
            read_sse(&client(), &mid, token).await,
            Err(StreamError::Cancelled)
        );
    }

    #[tokio::test]
    async fn cancellation_during_continuation_keeps_first_output_and_emits_no_second_output() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let first = server(
            200,
            &[],
            vec![part(line(serde_json::json!({
                "choices":[{"delta":{"content":"part"},"finish_reason":"length"}]
            })))],
            Arc::clone(&accepts),
            None,
        )
        .await;
        let first_events = read_sse(&client(), &first, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            first_events,
            [
                Event::Content("part".into()),
                Event::Finish("length".into())
            ]
        );

        let second = server(
            200,
            &[],
            vec![delayed(
                300,
                line(serde_json::json!({
                    "choices":[{"delta":{"content":"late"},"finish_reason":"stop"}]
                })),
            )],
            accepts,
            None,
        )
        .await;
        let token = CancellationToken::new();
        let child = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            child.cancel();
        });
        assert_eq!(
            read_sse(&client(), &second, token).await,
            Err(StreamError::Cancelled)
        );
    }

    #[tokio::test]
    async fn explicit_read_timeout_and_memory_limits_apply() {
        let accepts = Arc::new(AtomicUsize::new(0));
        let delayed_url = server(
            200,
            &[],
            vec![delayed(700, b"data: [DONE]\n".to_vec())],
            Arc::clone(&accepts),
            None,
        )
        .await;
        assert_eq!(
            read_sse(&client(), &delayed_url, CancellationToken::new()).await,
            Err(StreamError::Timeout("read"))
        );
        let long = format!("data: {}\n", "x".repeat(MAX_LINE + 1));
        let long_url = server(200, &[], vec![part(long.into_bytes())], accepts, None).await;
        assert_eq!(
            read_sse(&client(), &long_url, CancellationToken::new()).await,
            Err(StreamError::Limit("line"))
        );
        let body = (0..5000)
            .map(|_| b": comment\n".as_slice())
            .collect::<Vec<_>>()
            .concat();
        let body_url = server(
            200,
            &[],
            vec![part(body.clone()), part(body)],
            Arc::new(AtomicUsize::new(0)),
            None,
        )
        .await;
        assert_eq!(
            read_sse(&client(), &body_url, CancellationToken::new()).await,
            Err(StreamError::Limit("body"))
        );
    }
}
