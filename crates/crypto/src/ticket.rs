use subtle::ConstantTimeEq;

use crate::error::CryptoError;
use crate::keys::PublicKey;
use crate::random;

pub const TICKET_BYTES: usize = 32;
pub const GRACE_SECONDS: u64 = 300;
const MAX_ENTRIES: usize = 64;

#[derive(Clone, Copy)]
struct Entry {
    device: PublicKey,
    secret: [u8; TICKET_BYTES],
    valid_until: Option<u64>,
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
        self.entries.retain(|entry| entry.device != device);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        let mut secret = [0_u8; TICKET_BYTES];
        random::fill(&mut secret)?;
        self.entries.push(Entry {
            device,
            secret,
            valid_until: None,
        });
        Ok(secret)
    }

    pub fn suspend(&mut self, device: &PublicKey, now: u64) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.device == *device)
        {
            entry.valid_until = Some(now.saturating_add(GRACE_SECONDS));
        }
    }

    pub fn revoke(&mut self, device: &PublicKey) {
        self.entries.retain(|entry| entry.device != *device);
    }

    pub fn redeem(&mut self, device: &PublicKey, presented: &[u8; TICKET_BYTES], now: u64) -> bool {
        self.prune(now);
        let Some(position) = self
            .entries
            .iter()
            .position(|entry| entry.device == *device && entry.valid_until.is_some())
        else {
            return false;
        };
        let matches: bool = self.entries[position].secret.ct_eq(presented).into();
        if matches {
            self.entries.remove(position);
        }
        matches
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
    fn a_ticket_is_random_and_unusable_while_its_session_is_active() {
        let mut tickets = Tickets::new();
        let key = device();
        let first = tickets.issue(key, 100).unwrap();
        assert_ne!(first, [0; TICKET_BYTES]);
        assert!(!tickets.redeem(&key, &first, 100));
        let second = tickets.issue(key, 100).unwrap();
        assert_ne!(first, second);
        assert_eq!(tickets.len(), 1);
    }

    #[test]
    fn a_suspended_ticket_works_once_within_the_grace_period() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, 1000);
        assert!(tickets.redeem(&key, &ticket, 1000 + GRACE_SECONDS));
        assert!(!tickets.redeem(&key, &ticket, 1000 + GRACE_SECONDS));
        assert!(tickets.is_empty());
    }

    #[test]
    fn a_ticket_expires_after_the_grace_period() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, 1000);
        assert!(!tickets.redeem(&key, &ticket, 1000 + GRACE_SECONDS + 1));
        assert!(tickets.is_empty());
    }

    #[test]
    fn a_wrong_ticket_or_another_device_is_refused_and_keeps_the_real_one() {
        let mut tickets = Tickets::new();
        let (key, other) = (device(), device());
        let ticket = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, 0);
        let mut wrong = ticket;
        wrong[0] ^= 1;
        assert!(!tickets.redeem(&key, &wrong, 1));
        assert!(!tickets.redeem(&other, &ticket, 1));
        assert!(tickets.redeem(&key, &ticket, 2));
    }

    #[test]
    fn revoking_removes_the_ticket() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, 0);
        tickets.revoke(&key);
        assert!(!tickets.redeem(&key, &ticket, 1));
    }

    #[test]
    fn a_new_login_replaces_the_previous_ticket_of_the_device() {
        let mut tickets = Tickets::new();
        let key = device();
        let old = tickets.issue(key, 0).unwrap();
        tickets.suspend(&key, 0);
        let new = tickets.issue(key, 10).unwrap();
        tickets.suspend(&key, 10);
        assert!(!tickets.redeem(&key, &old, 11));
        assert!(tickets.redeem(&key, &new, 11));
    }

    #[test]
    fn tickets_of_each_device_are_independent_and_bounded() {
        let mut tickets = Tickets::new();
        let keys: Vec<PublicKey> = (0..MAX_ENTRIES + 10).map(|_| device()).collect();
        let mut issued = Vec::new();
        for key in &keys {
            issued.push(tickets.issue(*key, 0).unwrap());
            tickets.suspend(key, 0);
        }
        assert_eq!(tickets.len(), MAX_ENTRIES);
        assert!(!tickets.redeem(&keys[0], &issued[0], 1));
        let last = keys.len() - 1;
        assert!(tickets.redeem(&keys[last], &issued[last], 1));
    }
}
