//! The socket, and the handful of things that go wrong on one.
//!
//! Every provider speaks JSON over HTTP and every one of them says why it
//! would not answer in the body of a 4xx. So a status is never an error here:
//! the caller gets the code and the body and decides, because "400" means
//! *try again without that switch* to Ollama and *your request is wrong* to
//! everybody else.

use std::time::Duration;

/// What came back.
#[derive(Debug, Clone)]
pub(crate) struct Answer {
    pub status: u16,
    pub body: String,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// What the server said was wrong, as a sentence rather than a page.
    ///
    /// The three cloud providers all put it under `error.message`; anything
    /// else is trimmed to a line, because a body can be a whole HTML page
    /// from a proxy and nobody wants that in a log.
    pub fn complaint(&self) -> String {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&self.body)
            && let Some(message) = parsed
                .get("error")
                .and_then(|error| {
                    error
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| error.as_str())
                })
                .filter(|message| !message.trim().is_empty())
        {
            return message.trim().to_owned();
        }

        let line = self.body.lines().next().unwrap_or("").trim();
        let mut short: String = line.chars().take(200).collect();
        if short.len() < line.len() {
            short.push('…');
        }

        if short.is_empty() {
            format!("HTTP {}", self.status)
        } else {
            format!("HTTP {}: {short}", self.status)
        }
    }
}

/// Could not even ask: no route, no socket, a timeout, a broken answer.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct Unreachable(pub String);

pub(crate) fn agent(timeout: Duration) -> ureq::Agent {
    // A vision model on a modest machine takes tens of seconds a photograph,
    // so the timeout is minutes rather than the seconds an HTTP client would
    // pick — but it is a timeout, because a run of a thousand photographs
    // must not stop dead on a server that has wedged.
    //
    // Certificates are judged by the operating system's own store and not
    // by a list compiled into the binary. A machine behind a corporate proxy
    // that re-signs every connection trusts that proxy through its own
    // store, and a built-in list has never heard of it — the very first
    // attempt from such a machine failed with "unknown issuer" against three
    // providers in a row.
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

pub(crate) fn get(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Answer, Unreachable> {
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }

    let mut response = request
        .call()
        .map_err(|error| Unreachable(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Unreachable(format!("the answer could not be read: {error}")))?;
    Ok(Answer { status, body })
}

pub(crate) fn post(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
) -> Result<Answer, Unreachable> {
    let mut request = agent.post(url).header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }

    let mut response = request
        .send(body.to_string())
        .map_err(|error| Unreachable(error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Unreachable(format!("the answer could not be read: {error}")))?;
    Ok(Answer { status, body })
}

/// The address of one endpoint under a base.
///
/// A bare `localhost:11434` is what somebody types, and it is not a URL. It
/// becomes one here rather than failing with something about a missing
/// scheme.
pub(crate) fn address(base: &str, fallback: &str, path: &str) -> anyhow::Result<String> {
    let mut base = base.trim().trim_end_matches('/').to_owned();
    if base.is_empty() {
        base = fallback.trim_end_matches('/').to_owned();
    }

    if !base.contains("://") {
        base = format!("http://{base}");
    }

    anyhow::ensure!(
        base.starts_with("http://") || base.starts_with("https://"),
        "{base} is not an address we can speak to"
    );
    Ok(format!("{base}/{}", path.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_complaint_is_the_providers_message_when_there_is_one() {
        let answer = Answer {
            status: 401,
            body: r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#.to_owned(),
        };
        assert_eq!(answer.complaint(), "Incorrect API key provided");
    }

    #[test]
    fn a_page_of_html_becomes_one_line() {
        let answer = Answer {
            status: 502,
            body: "<html>\n<body>Bad gateway</body></html>".to_owned(),
        };
        assert_eq!(answer.complaint(), "HTTP 502: <html>");
    }

    #[test]
    fn an_empty_body_is_just_the_status() {
        let answer = Answer {
            status: 429,
            body: String::new(),
        };
        assert_eq!(answer.complaint(), "HTTP 429");
    }

    #[test]
    fn a_bare_host_becomes_an_address() {
        assert_eq!(
            address("localhost:11434", "x", "api/tags").unwrap(),
            "http://localhost:11434/api/tags"
        );
        assert_eq!(
            address("", "https://api.example.com/v1/", "/models").unwrap(),
            "https://api.example.com/v1/models"
        );
        assert!(address("ftp://x", "y", "z").is_err());
    }
}
