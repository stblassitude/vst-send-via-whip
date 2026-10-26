//! Publishes audio handed over from a real-time thread to a WHIP endpoint.
//!
//! The audio thread pushes samples through a [`Feeder`]; a [`Sender`] owns a
//! worker thread that encodes them with Opus and runs the WebRTC session.
//! See `plugin-proposal.md` for the design rules this follows.

mod drift;
mod encoder;
mod eventlog;
mod feeder;
mod pin;
mod resample;
mod settings;
mod shared;
mod tls;
mod url;
mod whip;
mod worker;

pub use eventlog::{log_dir, log_file_path, EventLog};
pub use feeder::{audio_channel, Feeder};
pub use settings::{ChannelMode, Content, StreamSettings};
pub use shared::{ConnState, IntervalStats, Shared, Status};
pub use worker::{Sender, SenderOptions};
