use std::collections::HashMap;
use std::net::IpAddr;

use crate::http::ip_key;
use wirehaus_crypto::base32;
use wirehaus_crypto::link::{ID_BYTES, new_id, verification_code};

pub const TTL_SECS: u64 = 600;
pub const MAX_PENDING: usize = 16;
pub const MIN_GAP_SECS: u64 = 1;
pub const MAX_PENDING_PER_IP: usize = 8;
pub const IP_GAP_SECS: u64 = 2;
const MANUAL_LEN: usize = 8;
const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    Busy,
    TooSoon,
    Random,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub id: [u8; ID_BYTES],
    pub manual: String,
    pub verification: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Pending,
    Ready(Vec<u8>),
}

#[derive(Debug)]
struct Entry {
    id: [u8; ID_BYTES],
    public: [u8; KEY_BYTES],
    created: u64,
    origin: IpAddr,
    sealed: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct Links {
    entries: Vec<Entry>,
    last_start: Option<u64>,
    last_by_ip: HashMap<IpAddr, u64>,
}

fn manual_of(id: &[u8; ID_BYTES]) -> String {
    base32::encode(id).chars().take(MANUAL_LEN).collect()
}

pub fn group_manual(manual: &str) -> String {
    let (head, tail) = manual.split_at(manual.len().min(MANUAL_LEN / 2));
    format!("{head}-{tail}")
}

pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

impl Links {
    pub fn start(
        &mut self,
        public: &[u8],
        now: u64,
        client: IpAddr,
    ) -> Result<Started, StartError> {
        self.prune(now);
        let client = ip_key(client);
        let public: [u8; KEY_BYTES] = public.try_into().map_err(|_| StartError::Random)?;
        if self.entries.len() >= MAX_PENDING {
            return Err(StartError::Busy);
        }
        if self
            .last_start
            .is_some_and(|last| now.saturating_sub(last) < MIN_GAP_SECS)
        {
            return Err(StartError::TooSoon);
        }
        if self
            .last_by_ip
            .get(&client)
            .is_some_and(|last| now.saturating_sub(*last) < IP_GAP_SECS)
        {
            return Err(StartError::TooSoon);
        }
        if self.entries.iter().filter(|e| e.origin == client).count() >= MAX_PENDING_PER_IP {
            return Err(StartError::Busy);
        }
        let id = new_id().map_err(|_| StartError::Random)?;
        let manual = manual_of(&id);
        if self.entries.iter().any(|e| manual_of(&e.id) == manual) {
            return Err(StartError::Busy);
        }
        self.last_start = Some(now);
        self.last_by_ip.insert(client, now);
        self.entries.push(Entry {
            id,
            public,
            created: now,
            origin: client,
            sealed: None,
        });
        Ok(Started {
            id,
            verification: verification_code(&public, &id),
            manual: group_manual(&manual),
        })
    }

    pub fn find(&mut self, key: &str, now: u64) -> Option<([u8; ID_BYTES], String)> {
        self.prune(now);
        let normalized: String = key
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let entry = if normalized.len() == ID_BYTES * 2 {
            let id: [u8; ID_BYTES] = from_hex(&normalized)?.try_into().ok()?;
            self.entries.iter().find(|e| e.id == id)?
        } else if normalized.len() == MANUAL_LEN {
            let mut hits = self
                .entries
                .iter()
                .filter(|e| manual_of(&e.id) == normalized);
            let first = hits.next()?;
            if hits.next().is_some() {
                return None;
            }
            first
        } else {
            return None;
        };
        if entry.sealed.is_some() {
            return None;
        }
        Some((entry.id, verification_code(&entry.public, &entry.id)))
    }

    pub fn public_of(&mut self, id: &[u8; ID_BYTES], now: u64) -> Option<[u8; KEY_BYTES]> {
        self.prune(now);
        self.entries
            .iter()
            .find(|e| e.id == *id && e.sealed.is_none())
            .map(|e| e.public)
    }

    pub fn approve(&mut self, id: &[u8; ID_BYTES], sealed: Vec<u8>, now: u64) -> bool {
        self.prune(now);
        match self
            .entries
            .iter_mut()
            .find(|e| e.id == *id && e.sealed.is_none())
        {
            Some(entry) => {
                entry.sealed = Some(sealed);
                entry.created = now;
                true
            }
            None => false,
        }
    }

    pub fn approve_key(&mut self, public: &[u8; KEY_BYTES], sealed: Vec<u8>, now: u64) -> bool {
        self.prune(now);
        let mut id = [0_u8; ID_BYTES];
        id.copy_from_slice(&public[..ID_BYTES]);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            if entry.public != *public {
                return false;
            }
            entry.sealed = Some(sealed);
            entry.created = now;
            return true;
        }
        if self.entries.len() >= MAX_PENDING {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            id,
            public: *public,
            created: now,
            origin: IpAddr::from([0, 0, 0, 0]),
            sealed: Some(sealed),
        });
        true
    }

    pub fn poll(&mut self, id: &[u8; ID_BYTES], now: u64) -> Poll {
        self.prune(now);
        match self.entries.iter().find(|e| e.id == *id) {
            None => Poll::Pending,
            Some(entry) => match &entry.sealed {
                None => Poll::Pending,
                Some(sealed) => Poll::Ready(sealed.clone()),
            },
        }
    }

    fn prune(&mut self, now: u64) {
        self.entries
            .retain(|e| now.saturating_sub(e.created) <= TTL_SECS);
        self.last_by_ip
            .retain(|_, last| now.saturating_sub(*last) < IP_GAP_SECS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([198, 51, 100, n])
    }

    fn key(n: u8) -> [u8; KEY_BYTES] {
        [n; KEY_BYTES]
    }

    #[test]
    fn a_started_link_can_be_found_by_id_or_manual_code_and_shows_the_same_verification() {
        let mut links = Links::default();
        let started = links.start(&key(1), 100, ip(1)).unwrap();
        let by_id = links.find(&to_hex(&started.id), 101).unwrap();
        let by_manual = links.find(&started.manual.to_lowercase(), 101).unwrap();
        assert_eq!(by_id.0, started.id);
        assert_eq!(by_manual.0, started.id);
        assert_eq!(by_id.1, started.verification);
        assert!(links.find("ZZZZ-ZZZZ", 101).is_none());
        assert!(links.find("short", 101).is_none());
        assert!(links.find(&"0".repeat(32), 101).is_none());
    }

    #[test]
    fn approving_hands_the_sealed_text_over_until_the_link_expires() {
        let mut links = Links::default();
        let started = links.start(&key(2), 0, ip(1)).unwrap();
        assert_eq!(links.poll(&started.id, 1), Poll::Pending);
        assert!(links.public_of(&started.id, 1).is_some());
        assert!(links.approve(&started.id, vec![1, 2, 3], 2));
        assert!(!links.approve(&started.id, vec![9], 2));
        assert!(links.find(&to_hex(&started.id), 2).is_none());
        assert_eq!(links.poll(&started.id, 3), Poll::Ready(vec![1, 2, 3]));
        assert_eq!(links.poll(&started.id, 4), Poll::Ready(vec![1, 2, 3]));
        assert_eq!(
            links.poll(&started.id, 2 + TTL_SECS),
            Poll::Ready(vec![1, 2, 3])
        );
        assert_eq!(links.poll(&started.id, 2 + TTL_SECS + 1), Poll::Pending);
    }

    #[test]
    fn links_expire_and_unknown_ids_are_gone() {
        let mut links = Links::default();
        let started = links.start(&key(3), 0, ip(1)).unwrap();
        assert_eq!(links.poll(&started.id, TTL_SECS), Poll::Pending);
        assert_eq!(links.poll(&started.id, TTL_SECS + 1), Poll::Pending);
        assert!(links.find(&to_hex(&started.id), TTL_SECS + 1).is_none());
        assert_eq!(links.poll(&[0; ID_BYTES], 5), Poll::Pending);
    }

    #[test]
    fn pending_links_are_capped_and_spaced() {
        let mut links = Links::default();
        assert_eq!(links.start(&key(1), 0, ip(1)).map(|_| ()), Ok(()));
        assert_eq!(links.start(&key(2), 0, ip(2)), Err(StartError::TooSoon));
        for n in 1..MAX_PENDING {
            let now = u64::try_from(n).unwrap() * (MIN_GAP_SECS + 1);
            let seed = u8::try_from(n).unwrap();
            assert!(links.start(&key(10 + seed), now, ip(10 + seed)).is_ok());
        }
        assert_eq!(links.start(&key(99), 100, ip(99)), Err(StartError::Busy));
        assert!(links.start(&key(99), TTL_SECS + 10, ip(99)).is_ok());
    }

    #[test]
    fn one_address_cannot_fill_the_mailbox() {
        let mut links = Links::default();
        assert!(links.start(&key(1), 0, ip(1)).is_ok());
        assert_eq!(links.start(&key(2), 1, ip(1)), Err(StartError::TooSoon));
        assert!(links.start(&key(3), 1, ip(2)).is_ok());
        let mut now = 1;
        for n in 1..MAX_PENDING_PER_IP {
            now += IP_GAP_SECS + 1;
            let seed = u8::try_from(n).unwrap();
            assert!(links.start(&key(20 + seed), now, ip(1)).is_ok());
        }
        now += IP_GAP_SECS + 1;
        assert_eq!(links.start(&key(5), now, ip(1)), Err(StartError::Busy));
        assert!(links.start(&key(6), now, ip(3)).is_ok());
    }

    #[test]
    fn a_wrong_sized_key_is_refused() {
        let mut links = Links::default();
        assert!(links.start(&[1; 5], 0, ip(1)).is_err());
    }

    #[test]
    fn hex_round_trips_and_rejects_garbage() {
        assert_eq!(from_hex(&to_hex(&[0, 255, 16])), Some(vec![0, 255, 16]));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn a_key_can_be_approved_without_registering_first_and_only_that_key_reads_it() {
        let mut links = Links::default();
        let public = [7_u8; KEY_BYTES];
        let mut id = [0_u8; ID_BYTES];
        id.copy_from_slice(&public[..ID_BYTES]);
        assert_eq!(links.poll(&id, 0), Poll::Pending);
        assert!(links.approve_key(&public, vec![4, 5], 1));
        assert_eq!(links.poll(&id, 2), Poll::Ready(vec![4, 5]));
        let mut impostor = public;
        impostor[31] = 8;
        assert!(!links.approve_key(&impostor, vec![9], 3));
        assert_eq!(links.poll(&id, 4), Poll::Ready(vec![4, 5]));
        assert_eq!(links.poll(&id, 1 + TTL_SECS + 1), Poll::Pending);
    }

    #[test]
    fn approvals_never_pile_up_beyond_the_cap() {
        let mut links = Links::default();
        for n in 0..(u8::try_from(MAX_PENDING).unwrap() + 5) {
            assert!(links.approve_key(&[n; KEY_BYTES], vec![n], 10));
        }
        assert_eq!(links.entries.len(), MAX_PENDING);
    }
}
