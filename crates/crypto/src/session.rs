use snow::TransportState;

use crate::error::CryptoError;
use crate::keys::PublicKey;

pub const MAX_RECORD: usize = 65535;
pub const TAG_LEN: usize = 16;
pub const MAX_CHUNK: usize = MAX_RECORD - TAG_LEN - 1;
pub const REKEY_INTERVAL: u64 = 1 << 20;

const MORE: u8 = 0;
const LAST: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub send: usize,
    pub receive: usize,
}

pub struct Session {
    transport: TransportState,
    remote: PublicKey,
    pending: Vec<u8>,
    limits: Limits,
    sent: u64,
    received: u64,
    rekey_interval: u64,
    failed: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("remote", &self.remote)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(crate) fn new(transport: TransportState, remote: PublicKey, limits: Limits) -> Self {
        Self {
            transport,
            remote,
            pending: Vec::new(),
            limits,
            sent: 0,
            received: 0,
            rekey_interval: REKEY_INTERVAL,
            failed: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_rekey_interval(mut self, interval: u64) -> Self {
        self.rekey_interval = interval;
        self
    }

    #[must_use]
    pub fn remote(&self) -> PublicKey {
        self.remote
    }

    pub fn seal(&mut self, message: &[u8]) -> Result<Vec<Vec<u8>>, CryptoError> {
        self.ensure_open()?;
        if message.len() > self.limits.send {
            return Err(CryptoError::MessageTooLarge {
                len: message.len(),
                max: self.limits.send,
            });
        }
        let chunks: Vec<&[u8]> = if message.is_empty() {
            vec![message]
        } else {
            message.chunks(MAX_CHUNK).collect()
        };
        let last = chunks.len() - 1;
        let mut records = Vec::with_capacity(chunks.len());
        for (index, chunk) in chunks.into_iter().enumerate() {
            records.push(self.seal_record(if index == last { LAST } else { MORE }, chunk)?);
        }
        Ok(records)
    }

    pub fn open(&mut self, record: &[u8]) -> Result<Option<Vec<u8>>, CryptoError> {
        self.ensure_open()?;
        let result = self.open_record(record);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn ensure_open(&self) -> Result<(), CryptoError> {
        if self.failed {
            Err(CryptoError::SessionClosed)
        } else {
            Ok(())
        }
    }

    fn seal_record(&mut self, flag: u8, chunk: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if self.sent > 0 && self.sent.is_multiple_of(self.rekey_interval) {
            self.transport.rekey_outgoing();
        }
        let mut plain = Vec::with_capacity(1 + chunk.len());
        plain.push(flag);
        plain.extend_from_slice(chunk);
        let mut out = vec![0_u8; plain.len() + TAG_LEN];
        let written = self
            .transport
            .write_message(&plain, &mut out)
            .map_err(|e| {
                self.failed = true;
                CryptoError::from(e)
            })?;
        out.truncate(written);
        self.sent += 1;
        Ok(out)
    }

    fn open_record(&mut self, record: &[u8]) -> Result<Option<Vec<u8>>, CryptoError> {
        if record.len() > MAX_RECORD {
            return Err(CryptoError::InvalidRecord("record is too large"));
        }
        if record.len() < TAG_LEN + 1 {
            return Err(CryptoError::InvalidRecord("record is too short"));
        }
        if self.received > 0 && self.received.is_multiple_of(self.rekey_interval) {
            self.transport.rekey_incoming();
        }
        let mut plain = vec![0_u8; record.len()];
        let length = self.transport.read_message(record, &mut plain)?;
        self.received += 1;
        let (flag, chunk) = plain[..length]
            .split_first()
            .ok_or(CryptoError::InvalidRecord("record is empty"))?;
        if self.pending.len() + chunk.len() > self.limits.receive {
            return Err(CryptoError::MessageTooLarge {
                len: self.pending.len() + chunk.len(),
                max: self.limits.receive,
            });
        }
        self.pending.extend_from_slice(chunk);
        match *flag {
            MORE => Ok(None),
            LAST => Ok(Some(std::mem::take(&mut self.pending))),
            _ => Err(CryptoError::InvalidRecord("unknown record flag")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::{Accepted, Replied, ViewerHandshake};
    use crate::keys::StaticKey;

    const LIMIT: usize = 1 << 20;

    fn both(limit: usize) -> Limits {
        Limits {
            send: limit,
            receive: limit,
        }
    }

    fn sessions(viewer_max: Limits, agent_max: Limits) -> (Session, Session) {
        let agent = StaticKey::generate().unwrap();
        let device = StaticKey::generate().unwrap();
        let (handshake, first) =
            ViewerHandshake::reconnect(&device, &agent.public(), viewer_max).unwrap();
        let accepted = Accepted::begin(&first, &agent, None, agent_max).unwrap();
        let Replied::Established {
            message,
            established,
        } = accepted.reply().unwrap()
        else {
            panic!("a reconnection finishes in two messages");
        };
        let viewer = handshake.complete(&message).unwrap().session;
        (viewer, established.session)
    }

    fn pattern(length: usize) -> Vec<u8> {
        (0..length)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect()
    }

    fn open_all(session: &mut Session, records: &[Vec<u8>]) -> Vec<u8> {
        let mut message = None;
        for record in records {
            message = session.open(record).unwrap();
        }
        message.expect("the last record completes the message")
    }

    #[test]
    fn a_small_message_travels_in_one_record_both_ways() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let records = viewer.seal(b"hello agent").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(open_all(&mut agent, &records), b"hello agent");
        let records = agent.seal(b"hello viewer").unwrap();
        assert_eq!(open_all(&mut viewer, &records), b"hello viewer");
    }

    #[test]
    fn an_empty_message_is_a_valid_message() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let records = viewer.seal(b"").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(open_all(&mut agent, &records), b"");
    }

    #[test]
    fn a_large_message_is_split_and_reassembled() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let message = pattern(LIMIT);
        let records = viewer.seal(&message).unwrap();
        assert_eq!(records.len(), LIMIT.div_ceil(MAX_CHUNK));
        assert!(records.iter().all(|record| record.len() <= MAX_RECORD));
        assert_eq!(open_all(&mut agent, &records), message);
    }

    #[test]
    fn a_message_exactly_at_the_chunk_size_uses_one_record() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let message = pattern(MAX_CHUNK);
        let records = viewer.seal(&message).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].len(), MAX_RECORD);
        assert_eq!(open_all(&mut agent, &records), message);
        let longer = pattern(MAX_CHUNK + 1);
        assert_eq!(viewer.seal(&longer).unwrap().len(), 2);
    }

    #[test]
    fn many_messages_in_a_row_keep_their_boundaries() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        for length in [0, 1, 100, MAX_CHUNK, MAX_CHUNK + 5, 3 * MAX_CHUNK] {
            let message = pattern(length);
            let records = viewer.seal(&message).unwrap();
            assert_eq!(open_all(&mut agent, &records), message, "length {length}");
        }
    }

    #[test]
    fn sealing_refuses_a_message_over_the_limit() {
        let (mut viewer, _agent) = sessions(both(100), both(LIMIT));
        assert_eq!(
            viewer.seal(&pattern(101)),
            Err(CryptoError::MessageTooLarge { len: 101, max: 100 })
        );
        assert!(viewer.seal(&pattern(100)).is_ok());
    }

    #[test]
    fn the_receiver_enforces_its_own_limit_while_reassembling() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(100_000));
        let records = viewer.seal(&pattern(3 * MAX_CHUNK)).unwrap();
        assert!(agent.open(&records[0]).unwrap().is_none());
        assert!(matches!(
            agent.open(&records[1]),
            Err(CryptoError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn each_direction_can_have_its_own_limit() {
        let viewer_limits = Limits {
            send: 4096,
            receive: LIMIT,
        };
        let agent_limits = Limits {
            send: LIMIT,
            receive: 4096,
        };
        let (mut viewer, mut agent) = sessions(viewer_limits, agent_limits);
        let large = pattern(500_000);
        let records = agent.seal(&large).unwrap();
        assert_eq!(open_all(&mut viewer, &records), large);
        assert!(viewer.seal(&pattern(5000)).is_err());
        let small = viewer.seal(&pattern(4096)).unwrap();
        assert_eq!(open_all(&mut agent, &small), pattern(4096));
    }

    #[test]
    fn a_tampered_record_is_rejected_and_ends_the_session() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let mut records = viewer.seal(b"transfer money").unwrap();
        records[0][3] ^= 1;
        assert!(matches!(
            agent.open(&records[0]),
            Err(CryptoError::Noise(_))
        ));
        let genuine = viewer.seal(b"next").unwrap();
        assert_eq!(agent.open(&genuine[0]), Err(CryptoError::SessionClosed));
        assert_eq!(agent.seal(b"x"), Err(CryptoError::SessionClosed));
    }

    #[test]
    fn a_replayed_record_is_rejected() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let records = viewer.seal(b"once").unwrap();
        assert_eq!(agent.open(&records[0]).unwrap().unwrap(), b"once");
        assert!(agent.open(&records[0]).is_err());
    }

    #[test]
    fn reordered_records_are_rejected() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let first = viewer.seal(b"first").unwrap();
        let second = viewer.seal(b"second").unwrap();
        assert!(agent.open(&second[0]).is_err());
        assert_eq!(agent.open(&first[0]), Err(CryptoError::SessionClosed));
    }

    #[test]
    fn a_dropped_record_is_detected() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let _lost = viewer.seal(b"lost").unwrap();
        let next = viewer.seal(b"next").unwrap();
        assert!(agent.open(&next[0]).is_err());
    }

    #[test]
    fn a_record_from_the_other_direction_is_rejected() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let records = viewer.seal(b"mine").unwrap();
        assert!(viewer.open(&records[0]).is_err());
        let other = agent.seal(b"theirs").unwrap();
        assert!(agent.open(&other[0]).is_err());
    }

    #[test]
    fn a_record_from_another_session_is_rejected() {
        let (mut viewer_a, _agent_a) = sessions(both(LIMIT), both(LIMIT));
        let (_viewer_b, mut agent_b) = sessions(both(LIMIT), both(LIMIT));
        let records = viewer_a.seal(b"cross").unwrap();
        assert!(agent_b.open(&records[0]).is_err());
    }

    #[test]
    fn malformed_record_sizes_are_rejected() {
        let (_viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        assert!(matches!(
            agent.open(&[]),
            Err(CryptoError::InvalidRecord("record is too short"))
        ));
        let (_viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        assert!(matches!(
            agent.open(&[0; TAG_LEN]),
            Err(CryptoError::InvalidRecord("record is too short"))
        ));
        let (_viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        assert!(matches!(
            agent.open(&vec![0; MAX_RECORD + 1]),
            Err(CryptoError::InvalidRecord("record is too large"))
        ));
    }

    #[test]
    fn an_unknown_record_flag_is_rejected() {
        let (mut viewer, mut agent) = sessions(both(LIMIT), both(LIMIT));
        let record = viewer.seal_record(7, b"payload").unwrap();
        assert_eq!(
            agent.open(&record),
            Err(CryptoError::InvalidRecord("unknown record flag"))
        );
    }

    #[test]
    fn the_same_plaintext_never_produces_the_same_ciphertext_twice() {
        let (mut viewer, _agent) = sessions(both(LIMIT), both(LIMIT));
        let first = viewer.seal(b"same").unwrap();
        let second = viewer.seal(b"same").unwrap();
        assert_ne!(first, second);
        let (mut other, _agent) = sessions(both(LIMIT), both(LIMIT));
        assert_ne!(other.seal(b"same").unwrap(), first);
    }

    #[test]
    fn keys_are_rotated_in_step_on_both_sides() {
        let (viewer, agent) = sessions(both(LIMIT), both(LIMIT));
        let (mut viewer, mut agent) = (viewer.with_rekey_interval(3), agent.with_rekey_interval(3));
        for round in 0..20_u8 {
            let outbound = viewer.seal(&[round; 10]).unwrap();
            assert_eq!(open_all(&mut agent, &outbound), vec![round; 10]);
            let inbound = agent.seal(&[round; 5]).unwrap();
            assert_eq!(open_all(&mut viewer, &inbound), vec![round; 5]);
        }
    }

    #[test]
    fn a_side_that_does_not_rotate_cannot_read_after_the_rotation() {
        let (viewer, agent) = sessions(both(LIMIT), both(LIMIT));
        let mut viewer = viewer.with_rekey_interval(2);
        let mut agent = agent.with_rekey_interval(1000);
        for _ in 0..2 {
            let records = viewer.seal(b"early").unwrap();
            assert_eq!(open_all(&mut agent, &records), b"early");
        }
        let records = viewer.seal(b"late").unwrap();
        assert!(agent.open(&records[0]).is_err());
    }
}
