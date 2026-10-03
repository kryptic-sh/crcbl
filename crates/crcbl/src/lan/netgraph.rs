//! The netgraph: a debug section showing how each of a session's links is
//! doing — round trip, jitter, recent loss and resends, bytes each way, and
//! the snapshot size — with a rolling graph of the round trip and the
//! snapshot under the figures.
//!
//! A [`LanHost`](super::LanHost) shows a row per peer; a
//! [`LanClient`](super::LanClient) one, for its link to the host. Each keeps
//! a [`Netgraph`] and feeds it every frame, so a sample shows it with one
//! `panel.add(lan.netgraph())` beside its "lan" section, and toggles it with
//! the rest of the panel on F3 — hidden by default in a release build, as
//! the whole overlay is.
//!
//! # Where the numbers come from
//!
//! Every figure is the packet layer's own ([`EndpointStats`]): round trip
//! and jitter are its RFC 6298 estimate, and loss, resends and bytes are its
//! counts over the last [`STATS_WINDOW`] ([`crate::net::reliable::window`]).
//! A host reads each peer's through
//! [`Host::peer_link_stats`](crate::server::Host::peer_link_stats), a client
//! its own through its transport. The snapshot size is the sealed length of
//! the last snapshot the host sent that peer, or the client opened.
//!
//! A link that measures nothing — a listen host's own player, whose
//! transport is an in-memory pair, or a link still connecting or down —
//! reads as dashes, never as a perfect link.
//!
//! # Here, not in `crcbl-client` or `crcbl-server`
//!
//! The section needs both halves of a session and the debug panel, and the
//! umbrella's [`crate::lan`] is the one place all of them meet; neither the
//! client nor the server crate depends on `crcbl-ui`, and this keeps it so.

use std::collections::VecDeque;
use std::time::Duration;

use crate::net::reliable::{EndpointStats, MAX_UNRELIABLE_PAYLOAD, STATS_WINDOW};
use crate::ui::{DebugModule, DebugSection};

/// How often a link's history takes a sample. Frames come faster; a graph
/// of every frame would scroll by in a second.
pub const HISTORY_INTERVAL: Duration = Duration::from_millis(100);

/// Samples a link's history holds: [`HISTORY_INTERVAL`] apart, a few seconds
/// of graph — enough to see a spike come and go.
pub const HISTORY_SAMPLES: usize = 64;

/// The round-trip graph's least full scale, in milliseconds: a LAN's
/// sub-millisecond round trip drawn against its own maximum would make
/// scheduling noise look like a spike.
pub const RTT_SCALE_FLOOR_MS: f32 = 20.0;

/// The column heads of a link's row: round trip and jitter in
/// milliseconds, loss as a percentage, resends, bytes a second in and out,
/// and the snapshot's bytes.
const HEADS: [&str; 7] = ["rtt", "jit", "loss", "rsnd", "in B/s", "out B/s", "snap B"];

/// One link's figures at one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinkReading {
    /// What the link's transport measures, or `None` when it measures
    /// nothing: an in-memory link, or one connecting or down.
    pub stats: Option<EndpointStats>,
    /// The sealed length of the link's last snapshot, in bytes.
    pub snapshot_bytes: usize,
}

/// Which end of a session a netgraph shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A host: a link per peer, named by peer number.
    Host,
    /// A client: its one link, to the host.
    Client,
}

/// A rolling, bounded series of samples, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    samples: VecDeque<f32>,
}

impl History {
    /// Appends `sample`, dropping the oldest once [`HISTORY_SAMPLES`] are
    /// held.
    pub fn push(&mut self, sample: f32) {
        if self.samples.len() == HISTORY_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// The samples, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        self.samples.iter().copied()
    }

    /// How many samples are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether none is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The largest sample held, or zero.
    #[must_use]
    pub fn max(&self) -> f32 {
        self.iter().fold(0.0, f32::max)
    }
}

/// One link the netgraph shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The peer's number on a host; zero for a client's link.
    pub id: u64,
    /// Its figures at the last frame.
    pub reading: LinkReading,
    /// Its round trip, in milliseconds, a sample per [`HISTORY_INTERVAL`];
    /// zero while it has none.
    pub rtt_ms: History,
    /// Its snapshot size, in bytes, a sample per [`HISTORY_INTERVAL`].
    pub snapshot_bytes: History,
}

/// The netgraph's state: every link's last figures and its history. See the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct Netgraph {
    role: Role,
    links: Vec<Link>,
    /// When the histories next take a sample.
    next_sample: Duration,
}

impl Netgraph {
    /// A netgraph for one end of a session, with no links yet.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self {
            role,
            links: Vec::new(),
            next_sample: Duration::ZERO,
        }
    }

    /// Which end it shows.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// The links it shows, in the order last recorded.
    #[must_use]
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// Records each link's figures at `now`, by id, in the order given. A
    /// link not named here is dropped with its history — the peer left —
    /// and one named for the first time starts an empty one. The histories
    /// take a sample once every [`HISTORY_INTERVAL`], on the interval's
    /// boundaries, however often this is called; a frame slower than the
    /// interval takes one sample, not several.
    pub fn record(
        &mut self,
        now: Duration,
        readings: impl IntoIterator<Item = (u64, LinkReading)>,
    ) {
        let sample = now >= self.next_sample;
        if sample {
            let interval = HISTORY_INTERVAL.as_nanos();
            let next = (now.as_nanos() / interval + 1) * interval;
            self.next_sample = Duration::from_nanos(u64::try_from(next).unwrap_or(u64::MAX));
        }
        let mut previous = std::mem::take(&mut self.links);
        for (id, reading) in readings {
            let mut link = match previous.iter().position(|link| link.id == id) {
                Some(index) => previous.swap_remove(index),
                None => Link {
                    id,
                    reading,
                    rtt_ms: History::default(),
                    snapshot_bytes: History::default(),
                },
            };
            link.reading = reading;
            if sample {
                let rtt = reading.stats.and_then(|stats| stats.rtt);
                link.rtt_ms
                    .push(rtt.map_or(0.0, |rtt| rtt.as_secs_f32() * 1_000.0));
                link.snapshot_bytes.push(reading.snapshot_bytes as f32);
            }
            self.links.push(link);
        }
    }

    /// What a link is called in the section: the host, from a client;
    /// the peer by number, from a host.
    fn name(&self, link: &Link) -> String {
        match self.role {
            Role::Host => format!("peer {}", link.id),
            Role::Client => "host".to_owned(),
        }
    }

    /// The round-trip graphs' shared full scale, in milliseconds: the
    /// largest sample any link holds, and no less than
    /// [`RTT_SCALE_FLOOR_MS`], so two peers' bars compare.
    #[must_use]
    pub fn rtt_scale_ms(&self) -> f32 {
        self.links
            .iter()
            .map(|link| link.rtt_ms.max())
            .fold(RTT_SCALE_FLOOR_MS, f32::max)
    }
}

/// `cells` right-aligned in the netgraph's columns, each as wide as the
/// widest figure it is expected to hold, so a row lines up under the heads
/// in the panel's fixed-advance font.
fn columns(cells: [&str; 7]) -> String {
    let [rtt, jitter, loss, resends, received, sent, snapshot] = cells;
    format!("{rtt:>6} {jitter:>5} {loss:>5} {resends:>4} {received:>7} {sent:>7} {snapshot:>6}")
}

/// A link's figures, lined up under [`HEADS`]: loss and resends are over the
/// last [`STATS_WINDOW`], and so are the rates. A figure the link has not
/// measured is a dash.
fn link_row(reading: &LinkReading) -> String {
    let snapshot = reading.snapshot_bytes.to_string();
    let Some(stats) = reading.stats else {
        return columns(["-", "-", "-", "-", "-", "-", &snapshot]);
    };
    let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1_000.0);
    let (rtt, jitter) = match stats.rtt {
        Some(rtt) => (ms(rtt), ms(stats.rtt_variance)),
        None => ("-".to_owned(), "-".to_owned()),
    };
    let loss = stats
        .recent
        .loss()
        .map_or_else(|| "-".to_owned(), |loss| format!("{:.1}", loss * 100.0));
    columns([
        &rtt,
        &jitter,
        &loss,
        &stats.recent.resends.to_string(),
        &stats.recent.received_per_second().to_string(),
        &stats.recent.sent_per_second().to_string(),
        &snapshot,
    ])
}

impl DebugModule for Netgraph {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title("net");
        if self.links.is_empty() {
            out.row_str("links", "none yet");
            return;
        }
        out.row("link", format_args!("{}", columns(HEADS)));
        for link in &self.links {
            out.row(
                &self.name(link),
                format_args!("{}", link_row(&link.reading)),
            );
        }
        let rtt_scale = self.rtt_scale_ms();
        out.row(
            "graphs",
            format_args!(
                "rtt to {rtt_scale:.0} ms, snap to {MAX_UNRELIABLE_PAYLOAD} B, {:.1} s",
                (HISTORY_INTERVAL * HISTORY_SAMPLES as u32).as_secs_f32(),
            ),
        );
        out.row(
            "window",
            format_args!("{:.1} s", STATS_WINDOW.as_secs_f32()),
        );
        for link in &self.links {
            let name = self.name(link);
            out.graph(&format!("{name} rtt"), link.rtt_ms.iter(), rtt_scale);
            out.graph(
                &format!("{name} snap"),
                link.snapshot_bytes.iter(),
                MAX_UNRELIABLE_PAYLOAD as f32,
            );
        }
    }
}

#[cfg(test)]
mod tests;
