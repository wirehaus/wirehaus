use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use blake2::Blake2s256;
use blake2::digest::Digest;
use subtle::ConstantTimeEq;

use crate::error::CryptoError;
use crate::keys::PublicKey;
use crate::random;

pub const TICKET_BYTES: usize = 32;
pub const DIGEST_BYTES: usize = 32;
pub const REMEMBER_SECONDS: u64 = 30 * 24 * 3600;
pub const OVERLAP_SECONDS: u64 = 600;
pub const MAX_PER_DEVICE: usize = 4;
const MAX_ENTRIES: usize = 64;

#[derive(Clone)]
struct Entry {
    device: PublicKey,
    digest: [u8; DIGEST_BYTES],
    valid_until: Option<u64>,
    used: bool,
    superseded: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub device: PublicKey,
    pub digest: [u8; DIGEST_BYTES],
    pub until: u64,
}

#[derive(Default)]
pub struct Tickets {
    entries: Vec<Entry>,
}

impl std::fmt::Debug for Tickets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tickets")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

fn digest_of(secret: &[u8; TICKET_BYTES]) -> [u8; DIGEST_BYTES] {
    let mut hasher = Blake2s256::new();
    hasher.update(b"wirehaus resume ticket");
    hasher.update(secret);
    hasher.finalize().into()
}

impl Entry {
    fn matches(&self, device: &PublicKey, secret: &[u8; TICKET_BYTES]) -> bool {
        self.device == *device && bool::from(self.digest.ct_eq(&digest_of(secret)))
    }
}

impl Tickets {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(
        &mut self,
        device: PublicKey,
        now: u64,
    ) -> Result<[u8; TICKET_BYTES], CryptoError> {
        self.prune(now);
        while self
            .entries
            .iter()
            .filter(|entry| entry.device == device)
            .count()
            >= MAX_PER_DEVICE
        {
            let victim = self
                .entries
                .iter()
                .position(|entry| entry.device == device && entry.used)
                .or_else(|| self.entries.iter().position(|entry| entry.device == device));
            if let Some(index) = victim {
                self.entries.remove(index);
            }
        }
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        let mut secret = [0_u8; TICKET_BYTES];
        random::fill(&mut secret)?;
        self.entries.push(Entry {
            device,
            digest: digest_of(&secret),
            valid_until: None,
            used: false,
            superseded: Arc::new(AtomicBool::new(false)),
        });
        Ok(secret)
    }

    #[must_use]
    pub fn superseded_flag(
        &self,
        device: &PublicKey,
        secret: &[u8; TICKET_BYTES],
    ) -> Option<Arc<AtomicBool>> {
        self.entries
            .iter()
            .find(|entry| entry.matches(device, secret))
            .map(|entry| entry.superseded.clone())
    }

    pub fn suspend(&mut self, device: &PublicKey, secret: &[u8; TICKET_BYTES], now: u64) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.matches(device, secret) && !entry.used)
        {
            entry.valid_until = Some(now.saturating_add(REMEMBER_SECONDS));
        }
    }

    pub fn revoke(&mut self, device: &PublicKey, secret: &[u8; TICKET_BYTES]) {
        self.entries.retain(|entry| !entry.matches(device, secret));
    }

    pub fn revoke_device(&mut self, device: &PublicKey) {
        self.entries.retain(|entry| entry.device != *device);
    }

    pub fn verify(&mut self, device: &PublicKey, presented: &[u8; TICKET_BYTES], now: u64) -> bool {
        self.prune(now);
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.matches(device, presented))
        else {
            return false;
        };
        if !entry.used {
            entry.used = true;
            if entry.valid_until.is_none() {
                entry.superseded.store(true, Ordering::SeqCst);
            }
            let overlap = now.saturating_add(OVERLAP_SECONDS);
            entry.valid_until = Some(
                entry
                    .valid_until
                    .map_or(overlap, |until| until.min(overlap)),
            );
        }
        true
    }

    #[must_use]
    pub fn export(&self, now: u64) -> Vec<Stored> {
        self.entries
            .iter()
            .filter(|entry| !entry.used)
            .map(|entry| Stored {
                device: entry.device,
                digest: entry.digest,
                until: entry
                    .valid_until
                    .unwrap_or_else(|| now.saturating_add(REMEMBER_SECONDS)),
            })
            .collect()
    }

    pub fn import(&mut self, stored: &[Stored], now: u64) {
        for item in stored {
            if item.until <= now || self.entries.len() >= MAX_ENTRIES {
                continue;
            }
            if self.entries.iter().any(|entry| entry.digest == item.digest) {
                continue;
            }
            self.entries.push(Entry {
                device: item.device,
                digest: item.digest,
                valid_until: Some(item.until),
                used: false,
                superseded: Arc::new(AtomicBool::new(false)),
            });
        }
    }

    fn prune(&mut self, now: u64) {
        self.entries
            .retain(|entry| entry.valid_until.is_none_or(|until| now <= until));
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::StaticKey;

    fn device() -> PublicKey {
        StaticKey::generate().unwrap().public()
    }

    #[test]
    fn a_ticket_is_random_and_distinct_per_session() {
        let mut tickets = Tickets::new();
        let key = device();
        let first = tickets.issue(key, 100).unwrap();
        assert_ne!(first, [0; TICKET_BYTES]);
        let second = tickets.issue(key, 100).unwrap();
        assert_ne!(first, second);
        assert_eq!(tickets.len(), 2);
    }

    #[test]
    fn presenting_the_ticket_of_a_session_that_looks_alive_takes_it_over() {
        let mut tickets = Tickets::new();
        let (key, other) = (device(), device());
        let ticket = tickets.issue(key, 100).unwrap();
        let flag = tickets.superseded_flag(&key, &ticket).unwrap();
        assert!(!flag.load(Ordering::SeqCst));
        assert!(!tickets.verify(&other, &ticket, 100));
        assert!(!flag.load(Ordering::SeqCst));
        assert!(tickets.verify(&key, &ticket, 100));
        assert!(flag.load(Ordering::SeqCst));
        assert!(tickets.verify(&key, &ticket, 100 + OVERLAP_SECONDS));
        assert!(!tickets.verify(&key, &ticket, 100 + OVERLAP_SECONDS + 1));
    }

    #[test]
    fn a_suspended_ticket_does_not_raise_the_flag_of_a_finished_session() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 0).unwrap();
        let flag = tickets.superseded_flag(&key, &ticket).unwrap();
        tickets.suspend(&key, &ticket, 0);
        assert!(tickets.verify(&key, &ticket, 1));
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn a_used_ticket_still_works_for_a_short_overlap_so_a_lost_reply_does_not_log_anyone_out() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, &ticket, 1000);
        assert!(tickets.verify(&key, &ticket, 2000));
        assert!(tickets.verify(&key, &ticket, 2000 + OVERLAP_SECONDS));
        assert!(!tickets.verify(&key, &ticket, 2000 + OVERLAP_SECONDS + 1));
    }

    #[test]
    fn a_ticket_expires_after_the_grace_period() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, &ticket, 1000);
        assert!(!tickets.verify(&key, &ticket, 1000 + REMEMBER_SECONDS + 1));
        assert!(tickets.is_empty());
    }

    #[test]
    fn a_wrong_ticket_or_another_device_is_refused_and_keeps_the_real_one() {
        let mut tickets = Tickets::new();
        let (key, other) = (device(), device());
        let ticket = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, &ticket, 0);
        let mut wrong = ticket;
        wrong[0] ^= 1;
        assert!(!tickets.verify(&key, &wrong, 1));
        assert!(!tickets.verify(&other, &ticket, 1));
        assert!(tickets.verify(&key, &ticket, 2));
    }

    #[test]
    fn revoking_removes_only_that_ticket() {
        let mut tickets = Tickets::new();
        let key = device();
        let one = tickets.issue(key, 0).unwrap();
        let two = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, &one, 0);
        tickets.suspend(&key, &two, 0);
        tickets.revoke(&key, &one);
        assert!(!tickets.verify(&key, &one, 1));
        assert!(tickets.verify(&key, &two, 1));
        let three = tickets.issue(key, 2).unwrap();
        tickets.suspend(&key, &three, 2);
        tickets.revoke_device(&key);
        assert!(tickets.is_empty());
    }

    #[test]
    fn two_sessions_of_the_same_device_keep_their_own_tickets() {
        let mut tickets = Tickets::new();
        let key = device();
        let tab_a = tickets.issue(key, 0).unwrap();
        let tab_b = tickets.issue(key, 1).unwrap();
        tickets.suspend(&key, &tab_a, 10);
        tickets.suspend(&key, &tab_b, 10);
        assert!(tickets.verify(&key, &tab_a, 11));
        assert!(tickets.verify(&key, &tab_b, 11));
    }

    #[test]
    fn a_device_keeps_at_most_a_few_tickets_and_the_oldest_goes_first() {
        let mut tickets = Tickets::new();
        let key = device();
        let issued: Vec<_> = (0..=MAX_PER_DEVICE as u64)
            .map(|index| {
                let ticket = tickets.issue(key, index).unwrap();
                tickets.suspend(&key, &ticket, index);
                ticket
            })
            .collect();
        assert_eq!(tickets.len(), MAX_PER_DEVICE);
        assert!(!tickets.verify(&key, &issued[0], 10));
        assert!(tickets.verify(&key, &issued[MAX_PER_DEVICE], 10));
    }

    #[test]
    fn tickets_of_each_device_are_independent_and_bounded() {
        let mut tickets = Tickets::new();
        let keys: Vec<PublicKey> = (0..MAX_ENTRIES + 10).map(|_| device()).collect();
        let mut issued = Vec::new();
        for key in &keys {
            let ticket = tickets.issue(*key, 0).unwrap();
            tickets.suspend(key, &ticket, 0);
            issued.push(ticket);
        }
        assert_eq!(tickets.len(), MAX_ENTRIES);
        assert!(!tickets.verify(&keys[0], &issued[0], 1));
        let last = keys.len() - 1;
        assert!(tickets.verify(&keys[last], &issued[last], 1));
    }

    #[test]
    fn exported_tickets_survive_a_restart_without_keeping_the_secret() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        let exported = tickets.export(200);
        assert_eq!(exported.len(), 1);
        assert!(!exported[0].digest.windows(4).any(|w| w == &ticket[..4]));
        let mut restarted = Tickets::new();
        restarted.import(&exported, 300);
        assert!(restarted.verify(&key, &ticket, 400));
        let mut late = Tickets::new();
        late.import(&exported, 200 + REMEMBER_SECONDS + 1);
        assert!(late.is_empty());
    }

    #[test]
    fn a_used_ticket_is_not_exported_and_replacing_it_prunes_used_ones_first() {
        let mut tickets = Tickets::new();
        let key = device();
        let first = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, &first, 0);
        assert!(tickets.verify(&key, &first, 1));
        assert_eq!(tickets.export(2).len(), 0);
        let live: Vec<_> = (0..MAX_PER_DEVICE)
            .map(|_| tickets.issue(key, 3).unwrap())
            .collect();
        assert!(!tickets.verify(&key, &first, 4));
        assert!(tickets.verify(&key, &live[0], 4));
    }

    #[test]
    fn suspending_a_ticket_that_was_already_taken_over_does_not_extend_its_life() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 0).unwrap();
        assert!(tickets.verify(&key, &ticket, 10));
        tickets.suspend(&key, &ticket, 20);
        assert!(!tickets.verify(&key, &ticket, 10 + OVERLAP_SECONDS + 1));
    }
}
