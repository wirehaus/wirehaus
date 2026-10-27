use subtle::ConstantTimeEq;

use crate::error::CryptoError;
use crate::keys::PublicKey;
use crate::random;

pub const TICKET_BYTES: usize = 32;
pub const GRACE_SECONDS: u64 = 300;
pub const MAX_PER_DEVICE: usize = 4;
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
        while self
            .entries
            .iter()
            .filter(|entry| entry.device == device)
            .count()
            >= MAX_PER_DEVICE
        {
            if let Some(oldest) = self.entries.iter().position(|entry| entry.device == device) {
                self.entries.remove(oldest);
            }
        }
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

    pub fn suspend(&mut self, device: &PublicKey, secret: &[u8; TICKET_BYTES], now: u64) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.device == *device && bool::from(entry.secret.ct_eq(secret)))
        {
            entry.valid_until = Some(now.saturating_add(GRACE_SECONDS));
        }
    }

    pub fn revoke(&mut self, device: &PublicKey, secret: &[u8; TICKET_BYTES]) {
        self.entries
            .retain(|entry| !(entry.device == *device && bool::from(entry.secret.ct_eq(secret))));
    }

    pub fn revoke_device(&mut self, device: &PublicKey) {
        self.entries.retain(|entry| entry.device != *device);
    }

    pub fn verify(&mut self, device: &PublicKey, presented: &[u8; TICKET_BYTES], now: u64) -> bool {
        self.prune(now);
        let found = self.entries.iter().position(|entry| {
            entry.device == *device
                && entry.valid_until.is_some()
                && bool::from(entry.secret.ct_eq(presented))
        });
        match found {
            Some(index) => {
                self.entries.remove(index);
                true
            }
            None => false,
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
    fn a_ticket_is_random_and_unusable_while_its_session_is_active() {
        let mut tickets = Tickets::new();
        let key = device();
        let first = tickets.issue(key, 100).unwrap();
        assert_ne!(first, [0; TICKET_BYTES]);
        assert!(!tickets.verify(&key, &first, 100));
        let second = tickets.issue(key, 100).unwrap();
        assert_ne!(first, second);
        assert_eq!(tickets.len(), 2);
    }

    #[test]
    fn a_suspended_ticket_verifies_once_within_the_grace_period() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, &ticket, 1000);
        assert!(tickets.verify(&key, &ticket, 1000 + GRACE_SECONDS));
        assert!(!tickets.verify(&key, &ticket, 1000 + GRACE_SECONDS));
    }

    #[test]
    fn a_ticket_expires_after_the_grace_period() {
        let mut tickets = Tickets::new();
        let key = device();
        let ticket = tickets.issue(key, 100).unwrap();
        tickets.suspend(&key, &ticket, 1000);
        assert!(!tickets.verify(&key, &ticket, 1000 + GRACE_SECONDS + 1));
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
}
