//! The HTTP side of WHIP (RFC 9725): POST the offer, DELETE the session.

use std::fmt;
use std::time::Duration;

use ureq::http::header::{AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MATCH, LINK, LOCATION};
use ureq::Agent;

#[derive(Debug, Clone)]
pub(crate) struct WhipSession {
    /// Absolute URL of the session resource, used to end the session.
    pub resource_url: String,
    pub etag: Option<String>,
    pub answer: String,
    /// Raw `Link: rel="ice-server"` headers; logged, not used yet.
    pub ice_servers: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum WhipError {
    Status { status: u16, message: String },
    Transport(String),
    Protocol(String),
}

impl fmt::Display for WhipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhipError::Status { status, message } => {
                let hint = match status {
                    401 | 403 => " (check the token and that it allows presenting)",
                    404 => " (check the endpoint URL, group name and token)",
                    _ => "",
                };
                write!(f, "server returned HTTP {status}{hint}")?;
                if !message.is_empty() {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            WhipError::Transport(e) => write!(f, "{e}"),
            WhipError::Protocol(e) => write!(f, "{e}"),
        }
    }
}

fn agent(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

fn bearer(token: &str) -> Option<String> {
    let t = token.trim();
    (!t.is_empty()).then(|| format!("Bearer {t}"))
}

/// Publishes `offer` to the WHIP endpoint and returns the session.
pub(crate) fn publish(
    endpoint: &str,
    token: &str,
    offer: &str,
    timeout: Duration,
) -> Result<WhipSession, WhipError> {
    let endpoint = endpoint.trim();
    let mut req = agent(timeout)
        .post(endpoint)
        .header(CONTENT_TYPE, "application/sdp");
    if let Some(auth) = bearer(token) {
        req = req.header(AUTHORIZATION, auth);
    }
    let mut resp = req
        .send(offer)
        .map_err(|e| WhipError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| WhipError::Transport(e.to_string()))?;
    if status != 201 && status != 200 {
        // Keep plain-text error messages; drop HTML error pages.
        let first = body.lines().next().unwrap_or("").trim();
        let message = if first.starts_with('<') {
            String::new()
        } else {
            first.chars().take(200).collect()
        };
        return Err(WhipError::Status { status, message });
    }
    let headers = resp.headers();
    let location = headers
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| WhipError::Protocol("response has no Location header".into()))?;
    let resource_url = crate::url::resolve(endpoint, location)
        .ok_or_else(|| WhipError::Protocol(format!("cannot resolve Location {location:?}")))?;
    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let ice_servers = headers
        .get_all(LINK)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter(|v| v.contains("ice-server"))
        .map(str::to_string)
        .collect();
    if !body.trim_start().starts_with("v=") {
        return Err(WhipError::Protocol(
            "response body is not an SDP answer".into(),
        ));
    }
    Ok(WhipSession {
        resource_url,
        etag,
        answer: body,
        ice_servers,
    })
}

/// Ends a WHIP session.
pub(crate) fn delete(
    resource_url: &str,
    token: &str,
    etag: Option<&str>,
    timeout: Duration,
) -> Result<(), WhipError> {
    let mut req = agent(timeout).delete(resource_url);
    if let Some(auth) = bearer(token) {
        req = req.header(AUTHORIZATION, auth);
    }
    if let Some(etag) = etag {
        req = req.header(IF_MATCH, etag);
    }
    let resp = req
        .call()
        .map_err(|e| WhipError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    // 404: the server already dropped the session.
    if (200..300).contains(&status) || status == 404 {
        Ok(())
    } else {
        Err(WhipError::Status {
            status,
            message: String::new(),
        })
    }
}
