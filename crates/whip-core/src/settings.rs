use std::fmt;

use serde::{Deserialize, Serialize};

/// Connection and encoder settings for one stream. These are persisted in the
/// host's project file, including the token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamSettings {
    /// WHIP endpoint, e.g. `https://galene.example.org/group/a/.whip`.
    pub endpoint: String,
    /// Bearer token; may be empty if the server does not require one.
    pub token: String,
    /// Opus target bitrate in bits per second.
    pub bitrate: u32,
    pub content: Content,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            token: String::new(),
            bitrate: 64_000,
            content: Content::Voice,
        }
    }
}

impl StreamSettings {
    pub const MIN_BITRATE: u32 = 16_000;
    pub const MAX_BITRATE: u32 = 256_000;

    pub fn is_complete(&self) -> bool {
        let e = self.endpoint.trim();
        e.starts_with("http://") || e.starts_with("https://")
    }

    /// A short name for log lines: the group name for Galene endpoints, the
    /// last path segment otherwise.
    pub fn label(&self) -> String {
        let path = self
            .endpoint
            .trim()
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or("")
            .split(['?', '#'])
            .next()
            .unwrap_or("");
        let segments: Vec<&str> = path.split('/').skip(1).filter(|s| !s.is_empty()).collect();
        match segments.as_slice() {
            [.., group, ".whip"] => group.to_string(),
            [.., last] => last.to_string(),
            [] => "unconfigured".to_string(),
        }
    }

    pub(crate) fn connection_differs(&self, other: &StreamSettings) -> bool {
        self.endpoint.trim() != other.endpoint.trim() || self.token.trim() != other.token.trim()
    }
}

// Never print the token.
impl fmt::Debug for StreamSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamSettings")
            .field("endpoint", &self.endpoint)
            .field(
                "token",
                &if self.token.is_empty() {
                    ""
                } else {
                    "<redacted>"
                },
            )
            .field("bitrate", &self.bitrate)
            .field("content", &self.content)
            .finish()
    }
}

/// Selects the Opus encoder tuning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    #[default]
    Voice,
    Music,
}

/// How the plugin's stereo input is mapped onto the transmitted stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ChannelMode {
    #[default]
    MonoLeft = 0,
    MonoRight = 1,
    MonoSum = 2,
    Stereo = 3,
}

impl ChannelMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::MonoRight,
            2 => Self::MonoSum,
            3 => Self::Stereo,
            _ => Self::MonoLeft,
        }
    }

    pub fn channels(self) -> usize {
        match self {
            Self::Stereo => 2,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_endpoint(e: &str) -> StreamSettings {
        StreamSettings {
            endpoint: e.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn label_uses_galene_group() {
        assert_eq!(
            with_endpoint("https://g.example.org:8443/group/voc-a/.whip").label(),
            "voc-a"
        );
        assert_eq!(
            with_endpoint("https://g.example.org/group/a/b/.whip").label(),
            "b"
        );
        assert_eq!(with_endpoint("https://x.example/ingest").label(), "ingest");
        assert_eq!(with_endpoint("").label(), "unconfigured");
    }

    #[test]
    fn debug_redacts_token() {
        let s = StreamSettings {
            token: "secret".into(),
            ..Default::default()
        };
        assert!(!format!("{s:?}").contains("secret"));
    }
}
