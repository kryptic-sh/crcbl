//! PipeWire's native protocol framing over its Unix socket.
//!
//! Every message is a 16-byte header — the object id, the opcode in the top
//! eight bits of a word whose low 24 are the body size, a sequence number and
//! a count of file descriptors — followed by the body, one SPA POD struct
//! ([`super::pod`]). That is protocol version 3, `module-protocol-native`'s
//! `connection.c`. A message may carry a footer POD after its body; this
//! client reads the body and ignores what follows it.
//!
//! The mixer never asks for file descriptors and the server sends none for the
//! objects it binds, so messages are read with plain reads; a descriptor the
//! kernel delivered with one would be closed by it, not leaked.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// The header's size in protocol version 3.
const HEADER: usize = 16;

/// The largest body this client accepts, `connection.c`'s `MAX_BUFFER_SIZE`.
const MAX_BODY: usize = 32 * 1024;

/// One message off the socket.
#[derive(Debug)]
pub(crate) struct Message {
    pub(crate) id: u32,
    pub(crate) opcode: u8,
    pub(crate) body: Vec<u8>,
}

/// Where PipeWire's socket is: `$PIPEWIRE_REMOTE` when it names one (an
/// absolute path, or a name under the runtime directory), else `pipewire-0`,
/// under `$PIPEWIRE_RUNTIME_DIR`, `$XDG_RUNTIME_DIR` or `$USERPROFILE`, the
/// order `module-protocol-native`'s `local-socket.c` searches.
pub(crate) fn socket_path() -> Option<PathBuf> {
    let name = std::env::var_os("PIPEWIRE_REMOTE").unwrap_or_else(|| "pipewire-0".into());
    let name = PathBuf::from(name);
    if name.is_absolute() {
        return Some(name);
    }
    ["PIPEWIRE_RUNTIME_DIR", "XDG_RUNTIME_DIR", "USERPROFILE"]
        .iter()
        .find_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join(name))
}

/// Writes messages, numbering each.
#[derive(Debug)]
pub(crate) struct Writer {
    stream: UnixStream,
    seq: u32,
}

impl Writer {
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self { stream, seq: 0 }
    }

    /// Sends `body` to object `id` as `opcode`, and returns the sequence
    /// number it went out with.
    pub(crate) fn send(&mut self, id: u32, opcode: u8, body: &[u8]) -> io::Result<u32> {
        let size = u32::try_from(body.len())
            .ok()
            .filter(|size| (*size as usize) < MAX_BODY)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let mut message = Vec::with_capacity(HEADER + body.len());
        message.extend_from_slice(&id.to_ne_bytes());
        message.extend_from_slice(&((u32::from(opcode) << 24) | size).to_ne_bytes());
        message.extend_from_slice(&seq.to_ne_bytes());
        message.extend_from_slice(&0u32.to_ne_bytes());
        message.extend_from_slice(body);
        self.stream.write_all(&message)?;
        Ok(seq)
    }
}

/// Reads messages off the socket, one at a time.
#[derive(Debug)]
pub(crate) struct Reader {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl Reader {
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    /// The next message, blocking until one has arrived whole. An
    /// end-of-file is [`io::ErrorKind::UnexpectedEof`]: the server went away.
    pub(crate) fn next(&mut self) -> io::Result<Message> {
        loop {
            if let Some(message) = self.take()? {
                return Ok(message);
            }
            let mut chunk = [0u8; 8192];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }

    /// A whole message from the front of the buffer, if one is there.
    fn take(&mut self) -> io::Result<Option<Message>> {
        if self.buffer.len() < HEADER {
            return Ok(None);
        }
        let word = |at: usize| u32::from_ne_bytes(self.buffer[at..at + 4].try_into().expect("4"));
        let id = word(0);
        let opcode_size = word(4);
        let size = (opcode_size & 0x00ff_ffff) as usize;
        if size > MAX_BODY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a message larger than the protocol allows",
            ));
        }
        if self.buffer.len() < HEADER + size {
            return Ok(None);
        }
        let body = self.buffer[HEADER..HEADER + size].to_vec();
        self.buffer.drain(..HEADER + size);
        Ok(Some(Message {
            id,
            opcode: (opcode_size >> 24) as u8,
            body,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a writer sends, a reader on the other end reads back, header and
    /// body, across two messages in one read.
    #[test]
    fn a_message_round_trips_through_a_socket_pair() {
        let (left, right) = UnixStream::pair().unwrap();
        let mut writer = Writer::new(left);
        let mut reader = Reader::new(right);
        assert_eq!(writer.send(0, 1, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap(), 0);
        assert_eq!(writer.send(2, 5, &[9; 16]).unwrap(), 1);
        let first = reader.next().unwrap();
        assert_eq!((first.id, first.opcode), (0, 1));
        assert_eq!(first.body, [1, 2, 3, 4, 5, 6, 7, 8]);
        let second = reader.next().unwrap();
        assert_eq!((second.id, second.opcode, second.body.len()), (2, 5, 16));
    }

    /// The header is the words `connection.c` writes: id, opcode over size,
    /// sequence, descriptor count.
    #[test]
    fn the_header_is_the_layout_connection_c_writes() {
        let (left, mut right) = UnixStream::pair().unwrap();
        let mut writer = Writer::new(left);
        writer.send(7, 3, &[0; 8]).unwrap();
        let mut header = [0u8; 16];
        right.read_exact(&mut header).unwrap();
        let words: Vec<u32> = header
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_ne_bytes(*word))
            .collect();
        assert_eq!(words, [7, (3 << 24) | 8, 0, 0]);
    }

    /// A closed peer is an end of file, which is how a server that went away
    /// is told apart from one that is quiet.
    #[test]
    fn a_closed_socket_is_an_end_of_file() {
        let (left, right) = UnixStream::pair().unwrap();
        drop(left);
        let mut reader = Reader::new(right);
        assert_eq!(
            reader.next().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
