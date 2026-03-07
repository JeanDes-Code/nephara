/// Diagnostic tool: streams raw Ollama chunks and reports thinking vs content token counts.
///
/// Usage:
///   cargo run --bin probe_ollama -- [--model NAME] [--url URL] [--think false|true] [--prompt TEXT]
///
/// Examples:
///   cargo run --bin probe_ollama -- --model gemma3:4b
///   cargo run --bin probe_ollama -- --model qwen3.5:9b --think false
///   cargo run --bin probe_ollama -- --model qwen3.5:9b
use std::error::Error;

use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_MODEL:  &str = "gemma3:4b";
const DEFAULT_URL:    &str = "http://localhost:11434";
const DEFAULT_PROMPT: &str = concat!(
    "You are an agent in a village simulation. Respond with a JSON action:\n",
    r#"{"action": "eat", "target": null, "intent": null, "reason": "hungry"}"#, "\n",
    "Pick one action from: eat, rest, forage, explore, wander. Return only JSON.",
);

#[derive(Serialize)]
struct ChatRequest<'a> {
    model:    &'a str,
    messages: Vec<UserMessage<'a>>,
    stream:   bool,
    options:  Options,
    #[serde(skip_serializing_if = "Option::is_none")]
    think:    Option<bool>,
}

#[derive(Serialize)]
struct UserMessage<'a> {
    role:    &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct Options {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct ChatChunk {
    message: ChunkContent,
    #[serde(default)]
    done:    bool,
}

#[derive(Deserialize)]
struct ChunkContent {
    #[serde(default)]
    content:  String,
    #[serde(default)]
    thinking: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    let mut model  = DEFAULT_MODEL.to_string();
    let mut url    = DEFAULT_URL.to_string();
    let mut think: Option<bool> = None;
    let mut prompt = DEFAULT_PROMPT.to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--model"  => { i += 1; model  = args[i].clone(); }
            "--url"    => { i += 1; url    = args[i].clone(); }
            "--prompt" => { i += 1; prompt = args[i].clone(); }
            "--think"  => {
                i += 1;
                think = match args[i].as_str() {
                    "true"  => Some(true),
                    "false" => Some(false),
                    other   => {
                        eprintln!("--think must be true or false, got: {}", other);
                        std::process::exit(1);
                    }
                };
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }

    println!("--- probe_ollama ---");
    println!("model  : {}", model);
    println!("url    : {}", url);
    println!("think  : {:?}", think);
    println!("prompt : {} chars", prompt.len());
    println!();

    let request_body = ChatRequest {
        model:    &model,
        messages: vec![UserMessage { role: "user", content: &prompt }],
        stream:   true,
        options:  Options { temperature: 0.7, num_predict: 512 },
        think,
    };

    println!("--- request body ---");
    println!("{}", serde_json::to_string_pretty(&request_body)?);
    println!();

    let client   = Client::new();
    let endpoint = format!("{}/api/chat", url);

    let mut resp = client
        .post(&endpoint)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {}: {}", status, text).into());
    }

    println!("--- streaming chunks ---");

    let mut content_total  = String::new();
    let mut thinking_total = 0usize;
    let mut chunk_index    = 0usize;
    let mut buf            = Vec::<u8>::new();

    while let Some(bytes) = resp.chunk().await.map_err(|e| format!("Stream error: {}", e))? {
        buf.extend_from_slice(&bytes);

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim();
            if line.is_empty() { continue; }

            match serde_json::from_str::<ChatChunk>(line) {
                Ok(chunk) => {
                    chunk_index += 1;
                    let c_len = chunk.message.content.len();
                    let t_len = chunk.message.thinking.len();
                    thinking_total += t_len;

                    let preview = if c_len > 0 {
                        let s   = &chunk.message.content;
                        let end = s.char_indices().nth(40).map(|(i, _)| i).unwrap_or(s.len());
                        format!("\"{}\"", s[..end].replace('\n', "\\n"))
                    } else if t_len > 0 {
                        "(thinking)".to_string()
                    } else if chunk.done {
                        "(done)".to_string()
                    } else {
                        "(empty)".to_string()
                    };

                    println!("chunk {:4}: thinking={:5}  content={:5}   >> {}",
                        chunk_index, t_len, c_len, preview);

                    content_total.push_str(&chunk.message.content);
                }
                Err(e) => {
                    eprintln!("  [parse error] {}: {:?}", e, line);
                }
            }
        }
    }

    // Flush any remaining partial line
    if !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        if let Ok(chunk) = serde_json::from_str::<ChatChunk>(line.trim()) {
            chunk_index += 1;
            let c_len = chunk.message.content.len();
            let t_len = chunk.message.thinking.len();
            thinking_total += t_len;
            println!("chunk {:4}: thinking={:5}  content={:5}   >> (final flush)",
                chunk_index, t_len, c_len);
            content_total.push_str(&chunk.message.content);
        }
    }

    println!();
    println!("--- done ---");
    println!("accumulated content ({} chars):", content_total.len());
    if content_total.is_empty() {
        println!("  (empty)");
    } else {
        println!("{}", content_total);
    }
    println!();
    println!("thinking total : {} chars", thinking_total);
    println!("content  total : {} chars", content_total.len());

    Ok(())
}
