//! The conversation with the server.
//!
//! Everything that can be decided without one is in [`crate::prompt`]; what
//! is left here is the socket, and the handful of things that go wrong on
//! one.

use crate::prompt;
use anyhow::{Context, Result, bail};
use photosite_core::gazetteer::Nearby;
use std::time::Duration;

pub use crate::prompt::Insights;

/// Where Ollama listens out of the box.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

/// The models a server has, vision-capable first.
pub fn models(endpoint: &str, timeout: Duration) -> Result<Vec<String>> {
    let url = address(endpoint, "api/tags")?;
    let reply = agent(timeout)
        .get(&url)
        .call()
        .with_context(|| unreachable(endpoint))?
        .body_mut()
        .read_to_string()
        .context("the server's answer could not be read")?;
    Ok(prompt::read_models(&reply))
}

/// Describes one photograph.
///
/// `place` is a fact told to the model, never a question asked of it — see
/// the crate documentation for why that matters more than it sounds.
#[allow(clippy::too_many_arguments)]
pub fn describe(
    endpoint: &str,
    model: &str,
    jpeg: &[u8],
    language: &str,
    english_too: bool,
    place: Option<&Nearby>,
    approximate: bool,
    direction: &str,
    timeout: Duration,
) -> Result<Insights> {
    anyhow::ensure!(!model.trim().is_empty(), "no model has been chosen");
    let context = place.and_then(|place| prompt::place_context(place, approximate, direction));
    let url = address(endpoint, "api/chat")?;

    // `think` is a newer switch and a model without a thinking mode rejects
    // it outright. The request works without it, only slower, so a refusal
    // is worth one retry rather than a failed photograph.
    let mut reply = post(
        &url,
        &prompt::request(model, jpeg, language, english_too, context.as_deref(), true),
        timeout,
        endpoint,
    );
    if let Err(Rejected::Refused) = &reply {
        tracing::debug!("the server would not take `think`; asking again without it");
        reply = post(
            &url,
            &prompt::request(
                model,
                jpeg,
                language,
                english_too,
                context.as_deref(),
                false,
            ),
            timeout,
            endpoint,
        );
    }

    let body = match reply {
        Ok(body) => body,
        Err(Rejected::Refused) => bail!("the server would not take the request"),
        Err(Rejected::Said(said)) => bail!("{said}"),
        Err(Rejected::Unreachable(said)) => bail!("{said}"),
    };

    Ok(prompt::with_place(prompt::read(&body)?, place, approximate))
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
    url: &str,
    body: &serde_json::Value,
    timeout: Duration,
    endpoint: &str,
) -> std::result::Result<String, Rejected> {
    let answer = agent(timeout)
        .post(url)
        .header("content-type", "application/json")
        .send(body.to_string())
        .map_err(|error| match &error {
            ureq::Error::StatusCode(400) => Rejected::Refused,
            ureq::Error::StatusCode(_) => Rejected::Said(format!(
                "{unreachable}: {error}",
                unreachable = unreachable(endpoint)
            )),
            _ => Rejected::Unreachable(unreachable_with(endpoint, &error)),
        })?;

    answer
        .into_body()
        .read_to_string()
        .map_err(|error| Rejected::Unreachable(format!("the answer could not be read: {error}")))
}

fn agent(timeout: Duration) -> ureq::Agent {
    // A vision model on a modest machine takes tens of seconds a photograph,
    // so the timeout is minutes rather than the seconds an HTTP client would
    // pick — but it is a timeout, because a run of a thousand photographs
    // must not stop dead on a server that has wedged.
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn unreachable(endpoint: &str) -> String {
    format!("Ollama at {endpoint} did not answer")
}

fn unreachable_with(endpoint: &str, error: &ureq::Error) -> String {
    format!("{}: {error}", unreachable(endpoint))
}

/// The address of one endpoint.
///
/// A bare `localhost:11434` is what somebody types, and it is not a URL. It
/// becomes one here rather than failing with something about a missing
/// scheme.
pub fn address(endpoint: &str, path: &str) -> Result<String> {
    let mut base = endpoint.trim().trim_end_matches('/').to_owned();
    if base.is_empty() {
        base = DEFAULT_ENDPOINT.to_owned();
    }

    if !base.contains("://") {
        base = format!("http://{base}");
    }

    anyhow::ensure!(
        base.starts_with("http://") || base.starts_with("https://"),
        "{endpoint:?} is not an address this can reach"
    );
    Ok(format!("{base}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_somebody_types_becomes_an_address() {
        assert_eq!(
            address("localhost:11434", "api/chat").unwrap(),
            "http://localhost:11434/api/chat"
        );
        assert_eq!(
            address("http://box:1234/", "api/tags").unwrap(),
            "http://box:1234/api/tags"
        );
        assert_eq!(
            address("  ", "api/tags").unwrap(),
            "http://localhost:11434/api/tags"
        );
    }

    #[test]
    fn an_address_this_cannot_reach_says_so_rather_than_failing_later() {
        assert!(address("ftp://box", "api/tags").is_err());
    }

    #[test]
    fn a_run_with_no_model_chosen_says_which_thing_is_missing() {
        let error = describe(
            DEFAULT_ENDPOINT,
            "  ",
            b"x",
            "English",
            false,
            None,
            false,
            "east",
            Duration::from_secs(1),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("model"), "{error}");
    }
}
