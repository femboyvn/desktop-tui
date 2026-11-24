//! Provider-native request encoding and sanitized, cancellable HTTPS streaming.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde_json::{Value, json};

#[derive(Debug)]
pub(super) enum Update { Text(String), Complete, Failed(String), Interrupted(String) }
#[derive(Debug)]
pub(super) struct Request { pub provider: AiProvider, pub model: String, pub body: Vec<u8> }

pub(super) fn compatible(provider: AiProvider, attachment: &Attachment) -> Result<(), String> {
    if let Some(error) = &attachment.error { return Err(format!("{}: {}", attachment.name, error)); }
    if attachment.size > MAX_FILE { return Err(format!("{} exceeds 10 MiB", attachment.name)); }
    let mime = attachment.mime.as_str();
    let image = matches!(mime, "image/png" | "image/jpeg" | "image/webp") || mime == "image/gif" && provider != AiProvider::Gemini || matches!(mime, "image/heic" | "image/heif") && provider == AiProvider::Gemini;
    let supported = mime == "text/plain" || mime == "application/pdf" || image || match provider {
        AiProvider::OpenAi => matches!(mime, "application/vnd.openxmlformats-officedocument.wordprocessingml.document" | "application/vnd.openxmlformats-officedocument.presentationml.presentation" | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" | "application/msword" | "application/vnd.ms-excel" | "application/vnd.ms-powerpoint"),
        AiProvider::Claude => false,
        AiProvider::Gemini => matches!(mime, "audio/wav" | "audio/mpeg" | "audio/flac" | "audio/ogg" | "audio/m4a" | "audio/aiff" | "audio/aac" | "video/mp4" | "video/webm" | "video/avi" | "video/mov" | "video/3gpp" | "video/mpeg" | "video/x-flv"),
    };
    if !supported { return Err(format!("{}: {} does not support this file type ({}); exclude it to send", attachment.name, provider.label(), mime)); }
    if provider == AiProvider::Claude && image && attachment.size > 5 * 1024 * 1024 { return Err(format!("{}: Claude images must be at most 5 MiB", attachment.name)); }
    Ok(())
}
pub(super) fn prepare(chat: &Conversation, store: &store::Store) -> Result<Request, String> {
    if chat.model.trim().is_empty() { return Err("Configure a model ID in AI Chat Settings".into()); }
    if chat.model.len() > 256 || chat.model.chars().any(char::is_control) { return Err("Invalid model ID".into()); }
    let mut raw = 0u64;
    for attachment in chat.attachments.iter().filter(|attachment| attachment.included) {
        compatible(chat.provider, attachment)?;
        raw = raw.checked_add(attachment.size).ok_or("Attachment size overflow")?;
        if raw > MAX_RAW { return Err("Included attachments exceed 20 MiB".into()); }
    }
    let mut messages = Vec::new();
    for (index, message) in chat.messages.iter().enumerate() {
        if message.role == Role::Assistant && message.status != MessageStatus::Complete { continue; }
        // Responses EasyInputMessage accepts assistant visible-text content as a
        // string; output_text belongs to the distinct response-output item shape.
        if chat.provider == AiProvider::OpenAi && message.role == Role::Assistant {
            if !message.text.is_empty() { messages.push(json!({"role":"assistant","content":message.text})); }
            continue;
        }
        let mut parts = Vec::new();
        if !message.text.is_empty() {
            parts.push(match chat.provider {
                AiProvider::OpenAi => json!({"type":"input_text","text":message.text}),
                AiProvider::Claude => json!({"type":"text","text":message.text}),
                AiProvider::Gemini => json!({"text":message.text}),
            });
        }
        for attachment in chat.attachments.iter().filter(|attachment| attachment.included && attachment.message == Some(index)) {
            let bytes = store.bytes(&chat.id, attachment).map_err(|_| format!("{}: cannot read saved attachment snapshot", attachment.name))?;
            if attachment.mime == "text/plain" {
                let text = std::str::from_utf8(&bytes).map_err(|_| format!("{}: saved text is not UTF-8", attachment.name))?;
                let text = format!("Attached file: {}\n{}", attachment.name, text);
                parts.push(match chat.provider { AiProvider::OpenAi => json!({"type":"input_text","text":text}), AiProvider::Claude => json!({"type":"text","text":text}), AiProvider::Gemini => json!({"text":text}) });
                continue;
            }
            if chat.provider == AiProvider::OpenAi && attachment.mime == "image/gif" && bytes.windows(11).any(|window| window == b"NETSCAPE2.0") { return Err(format!("{}: animated GIF inputs are unsupported by OpenAI", attachment.name)); }
            let encoded = STANDARD.encode(&bytes);
            let image = attachment.mime.starts_with("image/");
            let data_url = || format!("data:{};base64,{}", attachment.mime, encoded);
            parts.push(match chat.provider {
                AiProvider::OpenAi if image => json!({"type":"input_image","image_url":data_url()}),
                AiProvider::OpenAi => json!({"type":"input_file","filename":attachment.name,"file_data":data_url()}),
                AiProvider::Claude => json!({"type":if image {"image"} else {"document"},"source":{"type":"base64","media_type":attachment.mime,"data":encoded}}),
                AiProvider::Gemini => json!({"inline_data":{"mime_type":attachment.mime,"data":encoded}}),
            });
        }
        if parts.is_empty() { continue; }
        let role = match (chat.provider, message.role) { (AiProvider::Gemini, Role::Assistant) => "model", (_, Role::Assistant) => "assistant", _ => "user" };
        messages.push(match chat.provider { AiProvider::Gemini => json!({"role":role,"parts":parts}), _ => json!({"role":role,"content":parts}) });
    }
    let body = match chat.provider {
        AiProvider::OpenAi => json!({"model":chat.model,"input":messages,"stream":true,"store":false}),
        AiProvider::Claude => json!({"model":chat.model,"messages":messages,"stream":true,"max_tokens":8192}),
        AiProvider::Gemini => json!({"contents":messages}),
    };
    let body = serde_json::to_vec(&body).map_err(|_| "Cannot encode AI request")?;
    if body.len() > MAX_REQUEST { return Err("Encoded request exceeds 32 MiB".into()); }
    Ok(Request { provider: chat.provider, model: chat.model.clone(), body })
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().https_only(true).retry(reqwest::retry::never()).redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(600)).build().map_err(|_| "Cannot initialize secure AI connection".into())
}
fn endpoint(provider: AiProvider, model: &str) -> Result<String, String> {
    Ok(match provider {
        AiProvider::OpenAi => "https://api.openai.com/v1/responses".into(),
        AiProvider::Claude => "https://api.anthropic.com/v1/messages".into(),
        AiProvider::Gemini => {
            if model.is_empty() || !model.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')) { return Err("Gemini model ID must contain only letters, digits, dots, underscores or hyphens".into()); }
            format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse")
        }
    })
}
fn headers(provider: AiProvider, key: &str) -> Result<reqwest::header::HeaderMap, String> {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("accept", HeaderValue::from_static("text/event-stream"));
    let (name, value) = match provider { AiProvider::OpenAi => ("authorization", format!("Bearer {key}")), AiProvider::Claude => ("x-api-key", key.into()), AiProvider::Gemini => ("x-goog-api-key", key.into()) };
    let mut value = HeaderValue::from_str(&value).map_err(|_| "API key is not a valid HTTP credential")?; value.set_sensitive(true);
    headers.insert(name, value);
    if provider == AiProvider::Claude { headers.insert("anthropic-version", HeaderValue::from_static("2026-10-21")); }
    Ok(headers)
}
pub(super) async fn stream(request: Request, key: crate::config::ai::ApiKey, chat: String, generation: u64, tx: mpsc::Sender<NetworkEvent>) {
    let result = tokio::time::timeout(Duration::from_secs(600), stream_inner(&request, key.expose(), &chat, generation, &tx)).await;
    let update = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => Update::Interrupted(error),
        Err(_) => Update::Interrupted("AI request exceeded 10 minutes".into()),
    };
    let _ = tx.send(NetworkEvent { chat, generation, event: update }).await;
}
async fn stream_inner(request: &Request, key: &str, chat: &str, generation: u64, tx: &mpsc::Sender<NetworkEvent>) -> Result<(), String> {
    let sending = client()?.post(endpoint(request.provider, &request.model)?).headers(headers(request.provider, key)?).body(request.body.clone()).send();
    let response = tokio::time::timeout(Duration::from_secs(120), sending).await.map_err(|_| "AI response idle timeout after 120 seconds")?.map_err(|_| "AI connection failed; check network and provider settings")?;
    if !response.status().is_success() {
        let code = response.status().as_u16();
        let explanation = match code { 401 | 403 => "check API key and access", 404 => "check model ID and access", 413 => "request is too large", 429 => "rate or quota limit; retry manually", 500..=599 => "provider is unavailable; retry manually", _ => "provider rejected the request" };
        tx.send(NetworkEvent { chat: chat.into(), generation, event: Update::Failed(format!("{} HTTP {}: {}", request.provider.label(), code, explanation)) }).await.map_err(|_| "AI Chat closed")?;
        return Ok(());
    }
    consume(request.provider, response.bytes_stream(), chat, generation, tx).await
}
async fn consume<S, B, E>(provider: AiProvider, mut bytes: S, chat: &str, generation: u64, tx: &mpsc::Sender<NetworkEvent>) -> Result<(), String>
where S: futures_util::Stream<Item = Result<B, E>> + Unpin, B: AsRef<[u8]> {
    let mut decoder = sse::Decoder::default();
    let mut size = 0usize;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(120), bytes.next()).await.map_err(|_| "AI response idle timeout after 120 seconds")? {
        let chunk = chunk.map_err(|_| "AI response connection interrupted")?;
        for event in decoder.push(chunk.as_ref())? {
            if event.data == "[DONE]" || matches!(event.name.as_str(), "ping" | "heartbeat") { continue; }
            let mut json: Value = serde_json::from_str(&event.data).map_err(|_| "Invalid JSON in AI response stream")?;
            if !json.is_object() { return Err("Invalid event object in AI response stream".into()); }
            if json.get("type").is_none() && !event.name.is_empty() && provider != AiProvider::Gemini { json["type"] = event.name.into(); }
            for update in decode(provider, &json)? {
                let terminal = !matches!(update, Update::Text(_));
                if let Update::Text(text) = &update { size = size.saturating_add(text.len()); if size > MAX_RESPONSE { return Err("AI response exceeds 1 MiB".into()); } }
                tx.send(NetworkEvent { chat: chat.into(), generation, event: update }).await.map_err(|_| "AI Chat closed")?;
                if terminal { return Ok(()); }
            }
        }
    }
    decoder.finish()?;
    Err("AI response ended before completion".into())
}
fn decode(provider: AiProvider, value: &Value) -> Result<Vec<Update>, String> {
    let mut updates = Vec::new();
    let mut text = |value: &Value| { if let Some(text) = value.as_str() && !text.is_empty() { updates.push(Update::Text(text.into())); } };
    match provider {
        AiProvider::OpenAi => match value["type"].as_str().unwrap_or("") {
            "response.output_text.delta" | "response.refusal.delta" => text(&value["delta"]),
            "response.completed" => updates.push(Update::Complete),
            "response.incomplete" => updates.push(Update::Interrupted("Provider stopped before completing its answer".into())),
            "response.failed" | "error" => updates.push(Update::Failed("OpenAI reported a response error; retry manually".into())),
            _ => (),
        },
        AiProvider::Claude => match value["type"].as_str().unwrap_or("") {
            "content_block_start" if value["content_block"]["type"] == "text" => text(&value["content_block"]["text"]),
            "content_block_delta" if value["delta"]["type"] == "text_delta" => text(&value["delta"]["text"]),
            "message_delta" if matches!(value["delta"]["stop_reason"].as_str(), Some("max_tokens" | "pause_turn" | "tool_use")) => updates.push(Update::Interrupted("Claude stopped before a complete text answer".into())),
            "message_stop" => updates.push(Update::Complete),
            "error" => updates.push(Update::Failed("Claude reported a response error; retry manually".into())),
            _ => (),
        },
        AiProvider::Gemini => {
            if value.get("error").is_some() { return Ok(vec![Update::Failed("Gemini reported a response error; retry manually".into())]); }
            if value["promptFeedback"]["blockReason"].is_string() { return Ok(vec![Update::Failed("Gemini declined this request".into())]); }
            if let Some(candidate) = value["candidates"].as_array().and_then(|array| array.first()) {
                if let Some(parts) = candidate["content"]["parts"].as_array() { for part in parts { if part["thought"] != true { text(&part["text"]); } } }
                if let Some(reason) = candidate["finishReason"].as_str() { updates.push(if reason == "STOP" { Update::Complete } else { Update::Interrupted("Gemini stopped before a complete text answer".into()) }); }
            }
        },
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chat() -> Conversation {
        Conversation { id: "c-1".into(), title: "test".into(), provider: AiProvider::OpenAi, model: "manual-model".into(), draft: String::new(), attachments: Vec::new(), messages: vec![
            ChatMessage { role: Role::User, text: "first".into(), provider: AiProvider::Claude, model: "earlier".into(), status: MessageStatus::Complete },
            ChatMessage { role: Role::Assistant, text: "complete visible answer".into(), provider: AiProvider::Claude, model: "earlier".into(), status: MessageStatus::Complete },
            ChatMessage { role: Role::Assistant, text: "partial excluded".into(), provider: AiProvider::Gemini, model: "earlier".into(), status: MessageStatus::Stopped },
            ChatMessage { role: Role::User, text: "second".into(), provider: AiProvider::OpenAi, model: "manual-model".into(), status: MessageStatus::Complete },
        ] }
    }
    #[test]
    fn native_schemas_replay_only_complete_visible_history_without_secrets() {
        let temp = crate::test_support::TestDir::new();
        let store = store::Store::open(temp.path().join("ai")).unwrap();
        for provider in AiProvider::ALL {
            let mut chat = chat(); chat.provider = provider;
            let request = prepare(&chat, &store).unwrap();
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert!(!String::from_utf8(request.body).unwrap().contains("partial excluded"));
            match provider {
                AiProvider::OpenAi => { assert_eq!(body["store"], false); assert_eq!(body["input"].as_array().unwrap().len(), 3); assert_eq!(body["input"][1], json!({"role":"assistant","content":"complete visible answer"})); }
                AiProvider::Claude => { assert_eq!(body["stream"], true); assert_eq!(body["messages"][1]["role"], "assistant"); assert!(body["max_tokens"].is_number()); }
                AiProvider::Gemini => { assert_eq!(body["contents"][1]["role"], "model"); assert!(body.get("tools").is_none()); }
            }
            let request = client().unwrap().post(endpoint(provider, "manual-model").unwrap()).headers(headers(provider, "sample-secret").unwrap()).body(Vec::new()).build().unwrap();
            assert!(!request.url().as_str().contains("sample-secret"));
            let header = match provider { AiProvider::OpenAi => "authorization", AiProvider::Claude => "x-api-key", AiProvider::Gemini => "x-goog-api-key" };
            assert!(request.headers()[header].to_str().unwrap().contains("sample-secret"));
            if provider == AiProvider::Claude { assert_eq!(request.headers()["anthropic-version"], "2026-10-21"); }
        }
        assert!(endpoint(AiProvider::Gemini, "bad?key=secret").is_err());
    }
    #[test]
    fn unsupported_included_history_blocks_route_and_exclusion_allows_it() {
        let temp = crate::test_support::TestDir::new();
        let store = store::Store::open(temp.path().join("ai")).unwrap();
        let mut chat = chat();
        chat.attachments.push(Attachment { id: "a-1".into(), name: "movie.mp4".into(), mime: "video/mp4".into(), size: 3, included: true, message: Some(0), error: None });
        assert!(prepare(&chat, &store).unwrap_err().contains("movie.mp4"));
        chat.attachments[0].included = false;
        assert!(prepare(&chat, &store).is_ok());
    }
    #[test]
    fn provider_stream_events_errors_refusals_and_completion() {
        assert!(matches!(decode(AiProvider::OpenAi, &json!({"type":"response.output_text.delta","delta":"ไทย"})).unwrap().as_slice(), [Update::Text(text)] if text == "ไทย"));
        assert!(matches!(decode(AiProvider::OpenAi, &json!({"type":"response.refusal.delta","delta":"Cannot help"})).unwrap().as_slice(), [Update::Text(_)]));
        assert!(matches!(decode(AiProvider::OpenAi, &json!({"type":"error","error":{"message":"secret raw content"}})).unwrap().as_slice(), [Update::Failed(text)] if !text.contains("secret")));
        assert!(matches!(decode(AiProvider::Claude, &json!({"type":"message_stop"})).unwrap().as_slice(), [Update::Complete]));
        assert!(decode(AiProvider::Claude, &json!({"type":"ping"})).unwrap().is_empty());
        assert!(matches!(decode(AiProvider::Gemini, &json!({"candidates":[{"content":{"parts":[{"text":"private thought","thought":true},{"text":"visible"}]},"finishReason":"STOP"}]})).unwrap().as_slice(), [Update::Text(text),Update::Complete] if text == "visible"));
    }
    #[test]
    fn image_caps_and_sniffed_binary_compatibility() {
        let attachment = Attachment { id: "a".into(), name: "large.png".into(), mime: "image/png".into(), size: 6 * 1024 * 1024, included: true, message: None, error: None };
        assert!(compatible(AiProvider::Claude, &attachment).is_err());
        assert!(compatible(AiProvider::OpenAi, &attachment).is_ok());
        assert!(compatible(AiProvider::Gemini, &Attachment { mime: "application/octet-stream".into(), ..attachment }).is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fixture_stream_byte_splits_errors_eof_and_response_cap() {
        let fixture = "event: heartbeat\ndata: alive\n\nevent: response.output_text.delta\ndata: {\"delta\":\"ไทย😀\"}\n\nevent: response.completed\ndata: {}\n\n".as_bytes();
        let (tx, mut rx) = mpsc::channel(8);
        let chunks = futures_util::stream::iter(fixture.iter().map(|byte| Ok::<_, ()>(vec![*byte])));
        consume(AiProvider::OpenAi, chunks, "origin", 4, &tx).await.unwrap();
        let first = rx.recv().await.unwrap(); assert_eq!(first.chat, "origin"); assert_eq!(first.generation, 4);
        assert!(matches!(first.event, Update::Text(text) if text == "ไทย😀"));
        assert!(matches!(rx.recv().await.unwrap().event, Update::Complete));
        let chunks = futures_util::stream::iter([Ok::<_, ()>(b"data: {\"type\":\"ping\"}\n\n".to_vec())]);
        assert!(consume(AiProvider::Claude, chunks, "origin", 4, &tx).await.unwrap_err().contains("before completion"));
        let chunks = futures_util::stream::iter([Ok::<_, ()>(b"event: error\ndata: {\"message\":\"raw-secret\"}\n\n".to_vec())]);
        consume(AiProvider::Claude, chunks, "origin", 4, &tx).await.unwrap();
        assert!(matches!(rx.recv().await.unwrap().event, Update::Failed(text) if !text.contains("raw-secret")));
        let chunk = format!("data: {}\n\n", json!({"type":"response.output_text.delta","delta":"x".repeat(MAX_RESPONSE + 1)}));
        let chunks = futures_util::stream::iter([Ok::<_, ()>(chunk.into_bytes())]);
        assert!(consume(AiProvider::OpenAi, chunks, "origin", 4, &tx).await.unwrap_err().contains("1 MiB"));
        let chunks = futures_util::stream::iter([Ok::<_, ()>(b"event: error\ndata: null\n\n".to_vec())]);
        assert!(consume(AiProvider::Claude, chunks, "origin", 4, &tx).await.unwrap_err().contains("event object"));
    }
    #[test]
    fn raw_and_encoded_request_caps_reject_before_network() {
        let temp = crate::test_support::TestDir::new();
        let store = store::Store::open(temp.path().join("ai")).unwrap();
        let mut chat = chat();
        for index in 0..3 { chat.attachments.push(Attachment { id: format!("a-{index}"), name: "large.pdf".into(), mime: "application/pdf".into(), size: MAX_FILE, included: true, message: Some(0), error: None }); }
        assert!(prepare(&chat, &store).unwrap_err().contains("20 MiB"));
        chat.attachments.clear();
        chat.messages[0].text = "\u{1}".repeat(MAX_REQUEST / 6 + 1);
        assert!(prepare(&chat, &store).unwrap_err().contains("32 MiB"));
    }
}
