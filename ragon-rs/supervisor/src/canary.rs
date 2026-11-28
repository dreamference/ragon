//! The tool-call canary in Rust (specs/DREAMFERENCE_MAC_NODE.md §4.2, the twin of
//! dreamference/vllm_server/tool_call_canary.py): one streamed `/v1/responses` request asking for a
//! `get_weather` call, after each start of the engine. It passes when the stream carries a
//! well-formed function call (the name, a call id, JSON arguments with a non-empty `city`). It never
//! gates the engine: the supervisor runs it in a thread and reports the verdict.
//!
//! The request and the verdict are pure and tested on every platform; [`run`] is plain std::net.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const TOOL_NAME: &str = "get_weather";
pub const REQUIRED_ARGUMENT: &str = "city";
pub const PROMPT: &str = "What is the weather in Paris right now? Use the tool.";
/// Seconds the whole check may take (the Python canary's `TOOL_CALL_CANARY_TIMEOUT_S`).
pub const TIMEOUT_S: u64 = 60;
pub const CONNECT_TIMEOUT_S: u64 = 5;
/// The most of a response the canary reads; a stream past it is cut off and judged as it is.
const MAX_BODY: usize = 4 << 20;

/// Text a Qwen-family model writes for a call. Seen in the answer, the server did not read it.
const CALL_MARKUP: [&str; 3] = ["<tool_call>", "<function=", "\"name\": \"get_weather\""];

/// The request, the same as the Python canary's.
pub fn request_body(model: &str) -> String {
    json!({
        "model": model,
        "instructions": "You are a helpful assistant. Use the tools you are given.",
        "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": PROMPT}]}],
        "tools": [{
            "type": "function",
            "name": TOOL_NAME,
            "description": "Get the current weather for a city.",
            "strict": false,
            "parameters": {
                "type": "object",
                "properties": {"city": {"type": "string", "description": "The city."}},
                "required": ["city"],
                "additionalProperties": false,
            },
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": {"effort": "none", "summary": "auto"},
        "store": false,
        "stream": true,
        "max_output_tokens": 256,
    })
    .to_string()
}

/// The first model id of a `/v1/models` body.
pub fn model_id(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value.get("data")?.as_array()?.first()?.get("id")?.as_str().map(str::to_string)
}

/// The events of a server-sent stream: every `data:` line that is JSON.
pub fn events(stream: &str) -> Vec<Value> {
    stream
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

/// The output items: each `response.output_item.done`, else the final response's `output`.
fn output_items(events: &[Value]) -> Vec<Value> {
    let done: Vec<Value> = events
        .iter()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("response.output_item.done"))
        .filter_map(|e| e.get("item").cloned())
        .collect();
    if !done.is_empty() {
        return done;
    }
    events
        .iter()
        .rev()
        .find_map(|e| e.get("response")?.get("output")?.as_array().cloned())
        .unwrap_or_default()
}

fn text_of(items: &[Value], events: &[Value]) -> String {
    let mut text: String = items
        .iter()
        .filter(|i| i.get("type").and_then(Value::as_str) == Some("message"))
        .filter_map(|i| i.get("content")?.as_array().cloned())
        .flatten()
        .filter_map(|c| c.get("text")?.as_str().map(str::to_string))
        .collect();
    if text.is_empty() {
        text = events
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("response.output_text.delta"))
            .filter_map(|e| e.get("delta")?.as_str().map(str::to_string))
            .collect();
    }
    text
}

fn call_problem(call: &Value) -> Option<String> {
    let name = call.get("name").and_then(Value::as_str).unwrap_or("");
    if name != TOOL_NAME {
        return Some(format!("a call to {name:?}, not {TOOL_NAME:?}: the tool-call parser misread the name"));
    }
    if call.get("call_id").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Some("the call has no call_id, which ragon needs to pair its output with it".into());
    }
    let raw = call.get("arguments").and_then(Value::as_str).unwrap_or("");
    let Ok(arguments) = serde_json::from_str::<Value>(raw) else {
        return Some(format!("the call's arguments are not JSON ({:?})", cut(raw, 120)));
    };
    match arguments.get(REQUIRED_ARGUMENT).and_then(Value::as_str) {
        Some(city) if !city.trim().is_empty() => None,
        _ => Some(format!("the call's arguments have no {REQUIRED_ARGUMENT:?} ({})", cut(raw, 120))),
    }
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect::<String>().replace('\n', " ")
}

/// The canary's verdict on a streamed response: Ok, or what is wrong in one line.
pub fn verdict(stream: &str) -> Result<(), String> {
    let events = events(stream);
    if let Some(failed) = events
        .iter()
        .find(|e| matches!(e.get("type").and_then(Value::as_str), Some("response.failed" | "error")))
    {
        let detail = failed
            .get("message")
            .or_else(|| failed.get("response")?.get("error"))
            .map(Value::to_string)
            .unwrap_or_else(|| failed.to_string());
        return Err(format!("the server reported an error mid-stream ({})", cut(&detail, 200)));
    }
    let items = output_items(&events);
    let calls: Vec<&Value> =
        items.iter().filter(|i| i.get("type").and_then(Value::as_str) == Some("function_call")).collect();
    if let Some(call) = calls.first() {
        return call_problem(call).map_or(Ok(()), Err);
    }
    let text = text_of(&items, &events);
    if CALL_MARKUP.iter().any(|mark| text.contains(mark)) {
        return Err("the model wrote the call as text and the server did not read it: the tool-call parser \
                    does not match this model's call format"
            .into());
    }
    if events.is_empty() {
        return Err("no events in the response".into());
    }
    Err(format!("no tool call, only prose ({:?})", cut(text.trim(), 120)))
}

/// Joins a `Transfer-Encoding: chunked` body; a body cut short keeps what arrived.
pub fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = body.windows(2).position(|w| w == b"\r\n") {
        let size_text = String::from_utf8_lossy(&body[..end]);
        let Ok(size) = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16) else { break };
        if size == 0 {
            break;
        }
        let start = end + 2;
        let stop = (start + size).min(body.len());
        out.extend_from_slice(&body[start..stop]);
        if stop + 2 > body.len() {
            break;
        }
        body = &body[stop + 2..];
    }
    out
}

/// One HTTP/1.1 exchange with `Connection: close`: the status code and the body.
pub fn exchange(
    host: &str,
    port: u16,
    request_line: &str,
    body: Option<&str>,
    deadline: Instant,
) -> Result<(u16, String), String> {
    let addr: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("{host} has no address"))?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(CONNECT_TIMEOUT_S)).map_err(|e| e.to_string())?;
    let mut head = format!("{request_line} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    if let Some(b) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    head.push_str(body.unwrap_or(""));
    stream.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 << 10];
    while raw.len() < MAX_BODY {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("timed out".into());
        }
        stream.set_read_timeout(Some(left)).map_err(|e| e.to_string())?;
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err("timed out".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let code = crate::probe::parse_status_line(head.as_bytes()).ok_or("no HTTP status line")?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = &raw[split + 4..];
    let body = if chunked { dechunk(body) } else { body.to_vec() };
    Ok((code, String::from_utf8_lossy(&body).to_string()))
}

/// Runs the canary against the engine at `host:port`: the time it took, or why it failed.
pub fn run(host: &str, port: u16, timeout: Duration) -> Result<Duration, String> {
    let started = Instant::now();
    let deadline = started + timeout;
    let (code, models) = exchange(host, port, "GET /v1/models", None, deadline)?;
    let model = (code == 200).then(|| model_id(&models)).flatten().ok_or("/v1/models names no model")?;
    let (code, stream) = exchange(host, port, "POST /v1/responses", Some(&request_body(&model)), deadline)?;
    if code != 200 {
        return Err(format!("/v1/responses answered {code} ({})", cut(stream.trim(), 120)));
    }
    verdict(&stream).map(|()| started.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sse(events: &[Value]) -> String {
        events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or(""))).collect()
    }

    fn call(name: &str, call_id: &str, arguments: &str) -> Value {
        json!({"type": "response.output_item.done", "item": {
            "type": "function_call", "name": name, "call_id": call_id, "arguments": arguments}})
    }

    #[test]
    fn the_request_is_the_python_canarys() {
        let body: Value = serde_json::from_str(&request_body("m")).unwrap();
        assert_eq!(body["model"], "m");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["parameters"]["required"][0], REQUIRED_ARGUMENT);
        assert_eq!(body["input"][0]["content"][0]["text"], PROMPT);
        assert_eq!(body["reasoning"]["effort"], "none");
        assert_eq!(body["max_output_tokens"], 256);
    }

    #[test]
    fn the_model_id_is_the_first_listed() {
        assert_eq!(model_id(r#"{"data":[{"id":"qwen"},{"id":"other"}]}"#).as_deref(), Some("qwen"));
        assert_eq!(model_id(r#"{"data":[]}"#), None);
        assert_eq!(model_id("not json"), None);
    }

    #[test]
    fn a_well_formed_call_passes() {
        assert_eq!(verdict(&sse(&[call(TOOL_NAME, "c1", r#"{"city":"Paris"}"#)])), Ok(()));
        // Only the final response carries the output.
        let completed = json!({"type": "response.completed", "response": {"output": [
            {"type": "function_call", "name": TOOL_NAME, "call_id": "c1", "arguments": "{\"city\": \"Paris\"}"}]}});
        assert_eq!(verdict(&sse(&[completed])), Ok(()));
    }

    #[test]
    fn a_bad_call_says_what_is_wrong() {
        let e = |events: &[Value]| verdict(&sse(events)).unwrap_err();
        assert!(e(&[call("get_time", "c1", r#"{"city":"Paris"}"#)]).contains("misread the name"));
        assert!(e(&[call(TOOL_NAME, "", r#"{"city":"Paris"}"#)]).contains("no call_id"));
        assert!(e(&[call(TOOL_NAME, "c1", "{city: Paris")]).contains("not JSON"));
        assert!(e(&[call(TOOL_NAME, "c1", r#"{"city":" "}"#)]).contains("no \"city\""));
        let failed = json!({"type": "response.failed", "response": {"error": {"message": "boom"}}});
        assert!(e(&[failed]).contains("error mid-stream"));
    }

    #[test]
    fn prose_and_unread_markup_are_told_apart() {
        let message = |text: &str| {
            json!({"type": "response.output_item.done", "item": {"type": "message", "content": [
                {"type": "output_text", "text": text}]}})
        };
        let prose = verdict(&sse(&[message("It is sunny.")])).unwrap_err();
        assert!(prose.starts_with("no tool call, only prose (\"It is sunny.\")"), "{prose}");
        let markup = verdict(&sse(&[message("<tool_call>{\"name\": \"get_weather\"}</tool_call>")])).unwrap_err();
        assert!(markup.contains("did not read it"), "{markup}");
        let deltas = json!({"type": "response.output_text.delta", "delta": "<function=get_weather>"});
        assert!(verdict(&sse(&[deltas])).unwrap_err().contains("did not read it"));
        assert_eq!(verdict(""), Err("no events in the response".into()));
    }

    #[test]
    fn a_chunked_body_is_joined() {
        assert_eq!(dechunk(b"5\r\nhello\r\n6;x=1\r\n world\r\n0\r\n\r\n"), b"hello world");
        assert_eq!(dechunk(b"5\r\nhel"), b"hel", "a body cut short keeps what arrived");
        assert_eq!(dechunk(b""), b"");
    }
}
