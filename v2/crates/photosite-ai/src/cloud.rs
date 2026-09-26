//! The three cloud providers, each spoken to in its own dialect.
//!
//! All three take the same prompt and the same schema, and all three answer
//! with the same JSON object; what differs is the envelope — where the
//! image goes, what the key is called, how the schema is imposed and where
//! the answer is found in the reply. That is all this module is: three
//! envelopes and a shared way of retrying.
//!
//! **The key never leaves this module except in a header**, and never
//! appears in an error, a log line or a message on screen. A complaint from
//! a provider is quoted; the request that provoked it is not.

use crate::http::{self, Answer, Unreachable};
use crate::prompt::{self, Insights};
use crate::{Provider, Server};
use anyhow::{Context, Result, bail};
use std::time::Duration;

/// What the JPEG is called on the wire.
const MEDIA_TYPE: &str = "image/jpeg";

/// The tool Anthropic's models are made to answer through. A model held to
/// a tool answers with the tool's input and nothing else, which is the
/// closest that API comes to a schema.
const TOOL: &str = "describe_photograph";

/// The models a provider offers, as it names them.
///
/// A provider does not say which of its models can see, so the list is what
/// it is; the one that cannot will say so on the first photograph.
pub(crate) fn models(server: &Server, timeout: Duration) -> Result<Vec<String>> {
    let agent = http::agent(timeout);
    let auth = bearer(server);
    let (url, headers) = match server.provider {
        Provider::OpenAi => (url(server, "models")?, vec![("authorization", auth.as_str())]),
        Provider::Anthropic => (url(server, "models?limit=100")?, anthropic_headers(server)),
        Provider::Gemini => (
            url(server, "models?pageSize=100")?,
            vec![("x-goog-api-key", server.api_key.as_str())],
        ),
        Provider::Ollama => unreachable!("Ollama has a module of its own"),
    };

    let answer = http::get(&agent, &url, &headers)
        .map_err(|error| anyhow::anyhow!("{}: {error}", unreachable(server)))?;
    said(&answer, server)?;

    let parsed: serde_json::Value =
        serde_json::from_str(&answer.body).context("the model list was not JSON")?;
    let mut names: Vec<String> = match server.provider {
        Provider::Gemini => parsed
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter(|model| {
                        model
                            .get("supportedGenerationMethods")
                            .and_then(serde_json::Value::as_array)
                            .is_none_or(|methods| methods.iter().any(|m| m == "generateContent"))
                    })
                    .filter_map(|model| model.get("name").and_then(serde_json::Value::as_str))
                    .map(|name| name.trim_start_matches("models/").to_owned())
                    .collect()
            })
            .unwrap_or_default(),
        _ => parsed
            .get("data")
            .and_then(serde_json::Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|model| model.get("id").and_then(serde_json::Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    };
    names.sort();
    names.dedup();
    Ok(names)
}

/// Describes one photograph.
pub(crate) fn describe(
    server: &Server,
    model: &str,
    jpeg: &[u8],
    text: &str,
    english_too: bool,
    timeout: Duration,
) -> Result<Insights> {
    anyhow::ensure!(
        !server.api_key.trim().is_empty(),
        "{} needs an API key",
        server.provider.name()
    );

    let agent = http::agent(timeout);
    let image = crate::base64::encode(jpeg);
    match server.provider {
        Provider::OpenAi => {
            let url = url(server, "chat/completions")?;
            let auth = bearer(server);
            let headers = [("authorization", auth.as_str())];
            let body = openai_request(model, &image, text, english_too, true);
            let mut answer = ask(&agent, &url, &headers, &body, server)?;
            // A server that speaks the OpenAI dialect without the strict
            // schema mode — most of the compatible ones — refuses it with a
            // 400. The plainer JSON mode works everywhere, so a refusal is
            // worth one retry rather than a failed photograph.
            if answer.status == 400 {
                tracing::debug!("the server would not take a JSON schema; asking for plain JSON");
                let body = openai_request(model, &image, text, english_too, false);
                answer = ask(&agent, &url, &headers, &body, server)?;
            }

            said(&answer, server)?;
            read_openai(&answer.body)
        }
        Provider::Anthropic => {
            let url = url(server, "messages")?;
            let body = anthropic_request(model, &image, text, english_too);
            let answer = ask(&agent, &url, &anthropic_headers(server), &body, server)?;
            said(&answer, server)?;
            read_anthropic(&answer.body)
        }
        Provider::Gemini => {
            let url = url(server, &format!("models/{model}:generateContent"))?;
            let headers = [("x-goog-api-key", server.api_key.as_str())];
            let body = gemini_request(&image, text, english_too);
            let answer = ask(&agent, &url, &headers, &body, server)?;
            said(&answer, server)?;
            read_gemini(&answer.body)
        }
        Provider::Ollama => unreachable!("Ollama has a module of its own"),
    }
}

/// One request, asked again when the provider is busy.
///
/// A 429 and a 5xx are the provider's problem and pass: a bulk run over a
/// folder hits a rate limit as a matter of course, and failing the
/// photograph for it would fail most of the folder. Three tries, with a
/// pause that grows.
fn ask(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
    server: &Server,
) -> Result<Answer> {
    let mut attempt = 0u32;
    loop {
        let answer = http::post(agent, url, headers, body)
            .map_err(|Unreachable(said)| anyhow::anyhow!("{}: {said}", unreachable(server)))?;
        attempt += 1;
        if attempt < 3 && (answer.status == 429 || answer.status >= 500) {
            let pause = Duration::from_secs(2u64.pow(attempt) + 1);
            tracing::debug!(
                status = answer.status,
                seconds = pause.as_secs(),
                "the provider is busy; waiting before asking again"
            );
            std::thread::sleep(pause);
            continue;
        }

        return Ok(answer);
    }
}

/// An answer that is not a success is an error carrying what the provider
/// said, and never what was sent.
fn said(answer: &Answer, server: &Server) -> Result<()> {
    anyhow::ensure!(
        answer.ok(),
        "{} answered: {}",
        server.provider.name(),
        answer.complaint()
    );
    Ok(())
}

fn url(server: &Server, path: &str) -> Result<String> {
    http::address(&server.endpoint, server.provider.default_endpoint(), path)
}

fn bearer(server: &Server) -> String {
    format!("Bearer {}", server.api_key.trim())
}

fn anthropic_headers(server: &Server) -> Vec<(&'static str, &str)> {
    vec![
        ("x-api-key", server.api_key.trim()),
        ("anthropic-version", "2023-06-01"),
    ]
}

fn unreachable(server: &Server) -> String {
    format!(
        "{} at {} did not answer",
        server.provider.name(),
        server.endpoint_or_default()
    )
}

// ------------------------------------------------------------- OpenAI

/// A chat completion with the image inline and the answer held to the
/// schema — strictly, or in plain JSON mode for a server that has no strict
/// mode.
pub(crate) fn openai_request(
    model: &str,
    image: &str,
    text: &str,
    english_too: bool,
    strict: bool,
) -> serde_json::Value {
    let response_format = if strict {
        serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "photograph",
                "strict": true,
                "schema": prompt::schema(english_too, true),
            }
        })
    } else {
        serde_json::json!({ "type": "json_object" })
    };

    // Plain JSON mode wants the word said in the prompt, and the schema is
    // no longer imposed, so the shape is spelled out instead.
    let text = if strict {
        text.to_owned()
    } else {
        format!(
            "{text}\nAnswer with one JSON object holding {} and nothing else.",
            fields(english_too)
        )
    };

    serde_json::json!({
        "model": model,
        "temperature": prompt::TEMPERATURE,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": text },
                { "type": "image_url", "image_url": { "url": format!("data:{MEDIA_TYPE};base64,{image}") } },
            ],
        }],
        "response_format": response_format,
    })
}

pub(crate) fn read_openai(reply: &str) -> Result<Insights> {
    let outer: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| anyhow::anyhow!("the reply was not JSON"))?;
    let message = outer
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .context("the reply carried no message")?;

    // The content is a string, or — from some compatible servers — a list
    // of parts of which the text ones are wanted.
    let content = match message.get("content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => {
            if message.get("refusal").and_then(serde_json::Value::as_str).is_some() {
                bail!("the model refused to describe the photograph");
            }

            bail!("the reply carried no content");
        }
    };
    prompt::read_answer(&strip_fence(&content))
}

// ---------------------------------------------------------- Anthropic

/// A message with the image first and the model made to answer through a
/// tool whose input is our schema.
pub(crate) fn anthropic_request(
    model: &str,
    image: &str,
    text: &str,
    english_too: bool,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "temperature": prompt::TEMPERATURE,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": MEDIA_TYPE, "data": image } },
                { "type": "text", "text": text },
            ],
        }],
        "tools": [{
            "name": TOOL,
            "description": "Records what is in the photograph.",
            "input_schema": prompt::schema(english_too, true),
        }],
        "tool_choice": { "type": "tool", "name": TOOL },
    })
}

pub(crate) fn read_anthropic(reply: &str) -> Result<Insights> {
    let outer: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| anyhow::anyhow!("the reply was not JSON"))?;
    let content = outer
        .get("content")
        .and_then(serde_json::Value::as_array)
        .context("the reply carried no content")?;
    if let Some(input) = content
        .iter()
        .find(|block| block.get("type").and_then(serde_json::Value::as_str) == Some("tool_use"))
        .and_then(|block| block.get("input"))
    {
        return prompt::read_object(input);
    }

    // A model that talked instead of using the tool may still have said it
    // in JSON.
    let text: String = content
        .iter()
        .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    prompt::read_answer(&strip_fence(&text))
}

// ------------------------------------------------------------- Gemini

/// One turn with the image inline and the answer held to the schema through
/// the generation config. The model is in the address, not the body.
pub(crate) fn gemini_request(image: &str, text: &str, english_too: bool) -> serde_json::Value {
    serde_json::json!({
        "contents": [{
            "parts": [
                { "text": text },
                { "inline_data": { "mime_type": MEDIA_TYPE, "data": image } },
            ],
        }],
        "generationConfig": {
            "temperature": prompt::TEMPERATURE,
            "response_mime_type": "application/json",
            "response_schema": prompt::schema(english_too, false),
        },
    })
}

pub(crate) fn read_gemini(reply: &str) -> Result<Insights> {
    let outer: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| anyhow::anyhow!("the reply was not JSON"))?;
    let candidate = outer
        .get("candidates")
        .and_then(serde_json::Value::as_array)
        .and_then(|candidates| candidates.first())
        .context("the reply carried no candidate")?;
    if candidate.get("content").is_none()
        && let Some(reason) = candidate.get("finishReason").and_then(serde_json::Value::as_str)
    {
        bail!("the model stopped without answering: {reason}");
    }

    let text: String = candidate
        .get("content")
        .and_then(|content| content.get("parts"))
        .and_then(serde_json::Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    prompt::read_answer(&strip_fence(&text))
}

// ------------------------------------------------------------- shared

fn fields(english_too: bool) -> &'static str {
    if english_too {
        "\"title\", \"description\", \"keywords\" and \"description_en\""
    } else {
        "\"title\", \"description\" and \"keywords\""
    }
}

/// A model asked for JSON and not held to it sometimes wraps it in a code
/// fence. The fence is not the answer.
fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    let Some(inner) = trimmed.strip_prefix("```") else {
        return trimmed.to_owned();
    };

    let inner = inner.strip_prefix("json").unwrap_or(inner);
    inner.trim().strip_suffix("```").unwrap_or(inner).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"title":"A hill","description":"Green.","keywords":["hill"]}"#;

    #[test]
    fn the_openai_request_carries_the_image_and_the_schema() {
        let body = openai_request("gpt", "AAAA", "the prompt", true, true);
        assert_eq!(body["model"], "gpt");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["schema"]["additionalProperties"],
            false
        );
        assert!(
            body["response_format"]["json_schema"]["schema"]["properties"]["description_en"]
                .is_object()
        );
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,AAAA"
        );
    }

    #[test]
    fn plain_json_mode_says_json_in_the_prompt() {
        let body = openai_request("gpt", "AAAA", "the prompt", false, false);
        assert_eq!(body["response_format"]["type"], "json_object");
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("JSON"), "{text}");
        assert!(!text.contains("description_en"));
    }

    #[test]
    fn the_openai_answer_is_read_from_the_first_choice() {
        let reply = serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": ANSWER } }]
        })
        .to_string();
        assert_eq!(read_openai(&reply).unwrap().title.as_deref(), Some("A hill"));

        let fenced = serde_json::json!({
            "choices": [{ "message": { "content": format!("```json\n{ANSWER}\n```") } }]
        })
        .to_string();
        assert_eq!(read_openai(&fenced).unwrap().keywords, ["hill"]);

        let parts = serde_json::json!({
            "choices": [{ "message": { "content": [{ "type": "text", "text": ANSWER }] } }]
        })
        .to_string();
        assert!(read_openai(&parts).is_ok());
    }

    #[test]
    fn a_refusal_is_said_to_be_one() {
        let reply = serde_json::json!({
            "choices": [{ "message": { "content": null, "refusal": "I cannot" } }]
        })
        .to_string();
        let error = read_openai(&reply).unwrap_err().to_string();
        assert!(error.contains("refused"), "{error}");
    }

    #[test]
    fn the_anthropic_request_makes_the_model_use_the_tool() {
        let body = anthropic_request("claude", "AAAA", "the prompt", false);
        assert_eq!(body["tool_choice"]["name"], TOOL);
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["messages"][0]["content"][0]["source"]["data"], "AAAA");
        assert_eq!(body["messages"][0]["content"][1]["text"], "the prompt");
    }

    #[test]
    fn the_anthropic_answer_is_the_tools_input() {
        let reply = serde_json::json!({
            "content": [
                { "type": "text", "text": "Let me look." },
                { "type": "tool_use", "name": TOOL, "input": serde_json::from_str::<serde_json::Value>(ANSWER).unwrap() }
            ]
        })
        .to_string();
        assert_eq!(read_anthropic(&reply).unwrap().title.as_deref(), Some("A hill"));

        let talked = serde_json::json!({ "content": [{ "type": "text", "text": ANSWER }] }).to_string();
        assert_eq!(read_anthropic(&talked).unwrap().keywords, ["hill"]);
    }

    #[test]
    fn the_gemini_request_holds_the_answer_to_the_schema_without_strictness() {
        let body = gemini_request("AAAA", "the prompt", true);
        let schema = &body["generationConfig"]["response_schema"];
        assert!(schema.get("additionalProperties").is_none());
        assert!(schema["properties"]["description_en"].is_object());
        assert_eq!(body["contents"][0]["parts"][1]["inline_data"]["data"], "AAAA");
    }

    #[test]
    fn the_gemini_answer_is_the_first_candidates_text() {
        let reply = serde_json::json!({
            "candidates": [{ "content": { "parts": [{ "text": ANSWER }] }, "finishReason": "STOP" }]
        })
        .to_string();
        assert_eq!(read_gemini(&reply).unwrap().title.as_deref(), Some("A hill"));

        let blocked = serde_json::json!({ "candidates": [{ "finishReason": "SAFETY" }] }).to_string();
        let error = read_gemini(&blocked).unwrap_err().to_string();
        assert!(error.contains("SAFETY"), "{error}");
    }

    #[test]
    fn a_code_fence_is_not_part_of_the_answer() {
        assert_eq!(strip_fence("```json\n{}\n```"), "{}");
        assert_eq!(strip_fence("```\n{}```"), "{}");
        assert_eq!(strip_fence("  {} "), "{}");
    }
}
