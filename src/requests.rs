//! Pending access requests, persisted to `requests.json`.

use crate::config::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::path::PathBuf;

pub const MAX_PENDING: usize = 100;
pub const RESUBMIT_SECS: u64 = 600;
pub const MAX_NOTE: usize = 200;
pub const MAX_UA: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AccessRequest {
    pub ip: IpAddr,
    pub note: String,
    pub user_agent: String,
    pub requested_at: u64,
}

#[derive(Debug, PartialEq)]
pub enum SubmitError {
    QueueFull,
    RateLimited,
    Io(String),
}

pub struct RequestQueue {
    path: PathBuf,
    items: Vec<AccessRequest>,
    last_submit: HashMap<IpAddr, u64>,
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl RequestQueue {
    /// Missing file → empty queue.
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let items = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { path, items, last_submit: HashMap::new() })
    }

    fn persist(&self, items: &[AccessRequest]) -> Result<(), SubmitError> {
        let bytes = serde_json::to_vec_pretty(items).map_err(|e| SubmitError::Io(e.to_string()))?;
        write_atomic(&self.path, &bytes).map_err(|e| SubmitError::Io(e.to_string()))
    }

    pub fn submit(&mut self, ip: IpAddr, note: &str, user_agent: &str, now: u64) -> Result<(), SubmitError> {
        if let Some(&last) = self.last_submit.get(&ip)
            && now.saturating_sub(last) < RESUBMIT_SECS
        {
            return Err(SubmitError::RateLimited);
        }
        let existing = self.items.iter().position(|r| r.ip == ip);
        if existing.is_none() && self.items.len() >= MAX_PENDING {
            return Err(SubmitError::QueueFull);
        }
        let req = AccessRequest {
            ip,
            note: truncate(note, MAX_NOTE),
            user_agent: truncate(user_agent, MAX_UA),
            requested_at: now,
        };
        let mut items = self.items.clone();
        match existing {
            Some(i) => items[i] = req,
            None => items.push(req),
        }
        self.persist(&items)?;
        self.items = items;
        self.last_submit.insert(ip, now);
        Ok(())
    }

    pub fn remove(&mut self, ip: IpAddr) -> Result<Option<AccessRequest>, SubmitError> {
        let Some(i) = self.items.iter().position(|r| r.ip == ip) else { return Ok(None) };
        let mut items = self.items.clone();
        let removed = items.remove(i);
        self.persist(&items)?;
        self.items = items;
        Ok(Some(removed))
    }

    /// Oldest first.
    pub fn list(&self) -> Vec<AccessRequest> {
        let mut v = self.items.clone();
        v.sort_by_key(|r| r.requested_at);
        v
    }

    pub fn is_pending(&self, ip: IpAddr) -> bool {
        self.items.iter().any(|r| r.ip == ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u32) -> IpAddr { IpAddr::V4(std::net::Ipv4Addr::from(0x0a00_0000 + n)) }

    #[test]
    fn submit_and_persist() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("requests.json");
        let mut q = RequestQueue::load(p.clone()).unwrap();
        q.submit(ip(1), "hi", "ua", 1000).unwrap();
        assert_eq!(q.list().len(), 1);
        assert!(q.is_pending(ip(1)));
        let q2 = RequestQueue::load(p).unwrap();
        assert_eq!(q2.list().len(), 1);
        assert_eq!(q2.list()[0].note, "hi");
    }

    #[test]
    fn note_truncated() {
        let d = tempfile::tempdir().unwrap();
        let mut q = RequestQueue::load(d.path().join("r.json")).unwrap();
        q.submit(ip(1), &"é".repeat(500), &"u".repeat(1000), 1).unwrap();
        assert_eq!(q.list()[0].note.chars().count(), 200);
        assert_eq!(q.list()[0].user_agent.chars().count(), 300);
    }

    #[test]
    fn rate_limited() {
        let d = tempfile::tempdir().unwrap();
        let mut q = RequestQueue::load(d.path().join("r.json")).unwrap();
        q.submit(ip(1), "", "", 1000).unwrap();
        assert_eq!(q.submit(ip(1), "", "", 1599), Err(SubmitError::RateLimited));
        assert_eq!(q.submit(ip(1), "", "", 1600), Ok(()));
        assert_eq!(q.list().len(), 1);
    }

    #[test]
    fn queue_full() {
        let d = tempfile::tempdir().unwrap();
        let mut q = RequestQueue::load(d.path().join("r.json")).unwrap();
        for n in 0..100 { q.submit(ip(n), "", "", 1).unwrap(); }
        assert_eq!(q.submit(ip(100), "", "", 1), Err(SubmitError::QueueFull));
    }

    #[test]
    fn remove_works() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.json");
        let mut q = RequestQueue::load(p.clone()).unwrap();
        q.submit(ip(1), "", "", 1).unwrap();
        assert!(q.remove(ip(1)).unwrap().is_some());
        assert!(!q.is_pending(ip(1)));
        assert!(RequestQueue::load(p).unwrap().list().is_empty());
    }

    #[test]
    fn list_oldest_first() {
        let d = tempfile::tempdir().unwrap();
        let mut q = RequestQueue::load(d.path().join("r.json")).unwrap();
        q.submit(ip(9), "", "", 5).unwrap();
        q.submit(ip(1), "", "", 10).unwrap();
        assert_eq!(q.list()[0].ip, ip(9));
    }
}
