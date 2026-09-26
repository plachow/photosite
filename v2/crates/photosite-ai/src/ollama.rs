//! The conversation with Ollama.
//!
//! Everything that can be decided without a server is in [`crate::prompt`];
//! what is left here is Ollama's own shape of request and reply.

use crate::http::{self, Answer, Unreachable};
use crate::prompt::{self, Insights};
use anyhow::{Context, Result, bail};
use std::time::Duration;

/// Where Ollama listens out of the box.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

/// The models a server has, vision-capable first.
pub(crate) fn models(endpoint: &str, timeout: Duration) -> Result<Vec<String>> {
    let url = http::address(endpoint, DEFAULT_ENDPOINT, "api/tags")?;
    let answer = http::get(&http::agent(timeout), &url, &[])
        .map_err(|error| anyhow::anyhow!("{}: {error}", unreachable(endpoint)))?;
    anyhow::ensure!(
        answer.ok(),
        "{}: {}",
        unreachable(endpoint),
        answer.complaint()
    );
    Ok(prompt::read_models(&answer.body))
}

/// Describes one photograph.
pub(crate) fn describe(
    endpoint: &str,
    model: &str,
    jpeg: &[u8],
    text: &str,
    english_too: bool,
    timeout: Duration,
) -> Result<Insights> {
    let url = http::address(endpoint, DEFAULT_ENDPOINT, "api/chat")?;
    let agent = http::agent(timeout);

    // `think` is a newer switch and a model without a thinking mode rejects
    // it outright. The request works without it, only slower, so a refusal
    // is worth one retry rather than a failed photograph.
    let mut reply = post(
        &agent,
        &url,
        &prompt::ollama_request(model, jpeg, text, english_too, true),
        endpoint,
    );
    if let Err(Rejected::Refused) = &reply {
        tracing::debug!("the server would not take `think`; asking again without it");
        reply = post(
            &agent,
            &url,
            &prompt::ollama_request(model, jpeg, text, english_too, false),
            endpoint,
        );
    }

    let body = match reply {
        Ok(body) => body,
        Err(Rejected::Refused) => bail!("the server would not take the request"),
        Err(Rejected::Said(said)) => bail!("{said}"),
        Err(Rejected::Unreachable(said)) => bail!("{said}"),
    };

    read(&body)
}

/// Why a request did not come back with an answer.
///
/// The three are told apart because they mean different things to a bulk
/// run: a refusal is worth retrying differently, something the server *said*
/// is about this photograph, and an unreachable server means every remaining
/// photograph would wait out the same failure.
#[derive(Debug)]
enum Rejected {
    /// A 400: the request was not one this server takes.
    Refused,
    /// The server answered, and said why it would not.
    Said(String),
    Unreachable(String),
}

fn post(
    agent: &ureq::Agent,
    url: &str,
    body: &serde_json::Value,
    endpoint: &str,
) -> std::result::Result<String, Rejected> {
    let answer: Answer = http::post(agent, url, &[], body)
        .map_err(|Unreachable(said)| Rejected::Unreachable(format!("{}: {said}", unreachable(endpoint))))?;
    if answer.status == 400 {
        return Err(Rejected::Refused);
    }

    if !answer.ok() {
        return Err(Rejected::Said(format!(
            "{}: {}",
            unreachable(endpoint),
            answer.complaint()
        )));
    }

    Ok(answer.body)
}

/// Reads the answer out of a chat response.
///
/// Two layers of JSON: the chat reply, whose `message.content` is itself the
/// JSON the schema asked for. That is Ollama's shape, not ours.
fn read(reply: &str) -> Result<Insights> {
    let outer: serde_json::Value =
        serde_json::from_str(reply).map_err(|_| anyhow::anyhow!("the reply was not JSON"))?;
    let content = outer
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .context("the reply carried no message")?;
    prompt::read_answer(content)
}

fn unreachable(endpoint: &str) -> String {
    format!("Ollama at {endpoint} did not answer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_read_out_of_the_chat_reply() {
        let reply = r#"{"message":{"role":"assistant","content":"{\"title\":\"A hill\",\"description\":\"Green.\",\"keywords\":[\"hill\"]}"}}"#;
        let insights = read(reply).unwrap();
        assert_eq!(insights.title.as_deref(), Some("A hill"));
        assert_eq!(insights.keywords, ["hill"]);
    }

    #[test]
    fn a_reply_with_no_message_is_said_so() {
        assert!(read(r#"{"done":true}"#).is_err());
        assert!(read("not json").is_err());
    }
}
