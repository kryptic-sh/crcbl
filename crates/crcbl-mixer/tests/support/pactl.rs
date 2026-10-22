//! `pactl`, the witness the live tests check the mixer against: what it says
//! about the server that `PULSE_SERVER` names, or the session's own.

use crcbl_mixer::Mixer;

/// One `pactl` sink or source: index, name, mean volume percent, mute.
#[derive(Debug)]
pub struct Witness {
    pub index: u32,
    pub name: String,
    pub volume: u32,
    pub muted: bool,
    pub active_port: Option<String>,
}

/// `pactl list short`-free parsing: `pactl list sinks` prints blocks of
/// `Sink #N`, `Name:`, `Mute:` and `Volume:` lines, which is all this reads.
pub fn pactl(kind: &str) -> Vec<Witness> {
    let output = crcbl_spawn::command("pactl")
        .args(["list", kind])
        .env("LC_ALL", "C")
        .output()
        .expect("pactl runs");
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    let mut current: Option<Witness> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(index) = line
            .strip_prefix("Sink #")
            .or_else(|| line.strip_prefix("Source #"))
        {
            found.extend(current.take());
            current = Some(Witness {
                index: index.parse().expect("an index"),
                name: String::new(),
                volume: 0,
                muted: false,
                active_port: None,
            });
        } else if let Some(witness) = current.as_mut() {
            if let Some(name) = line.strip_prefix("Name: ") {
                witness.name = name.to_owned();
            } else if let Some(port) = line.strip_prefix("Active Port: ") {
                witness.active_port = Some(port.to_owned());
            } else if let Some(mute) = line.strip_prefix("Mute: ") {
                witness.muted = mute == "yes";
            } else if let Some(volume) = line.strip_prefix("Volume: ") {
                let percents: Vec<u32> = volume
                    .split_whitespace()
                    .filter_map(|word| word.strip_suffix('%')?.parse().ok())
                    .collect();
                let count = u32::try_from(percents.len()).unwrap().max(1);
                witness.volume = (percents.iter().sum::<u32>() + count / 2) / count;
            }
        }
    }
    found.extend(current);
    found
        .into_iter()
        .filter(|witness| !witness.name.ends_with(".monitor"))
        .collect()
}

pub fn default(kind: &str) -> String {
    let output = crcbl_spawn::command("pactl")
        .arg(format!("get-default-{kind}"))
        .output()
        .expect("pactl runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Polls `mixer` until `ready` holds or two seconds pass.
pub fn until(mixer: &Mixer, ready: impl Fn(&crcbl_mixer::MixerState) -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if ready(&mixer.state()) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// The sink each playing stream is on, as `(stream, sink)` indices, from
/// `pactl list short sink-inputs`.
pub fn sink_inputs() -> Vec<(u32, u32)> {
    let output = crcbl_spawn::command("pactl")
        .args(["list", "short", "sink-inputs"])
        .output()
        .expect("pactl runs");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect()
}
