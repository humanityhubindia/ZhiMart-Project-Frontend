//! Per-client open-connection accounting for the accept loop.
//!
//! The global connection cap (`app::MAX_CONNECTIONS`) protects the process, but
//! on its own one host could take every slot. This caps open connections per
//! peer IP (IPv6 bucketed by /64, like the rate limiter). Peers inside
//! `TRUSTED_PROXIES` are exempt: behind a load balancer every connection comes
//! from the proxy, and per-client limits are enforced at L7 instead.

use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    net::IpAddr,
    sync::{Arc, Mutex, MutexGuard},
};

use ipnetwork::IpNetwork;

use super::rate_limit::bucket_key;

/// Default per-IP cap. Browsers open ≤6 HTTP/1 connections per origin (one
/// for HTTP/2); 64 leaves room for an office behind one NAT address.
pub const MAX_CONNECTIONS_PER_IP: u32 = 64;
const SHARDS: usize = 64;

type Shard = Mutex<HashMap<IpAddr, u32>>;

pub struct PerIpConnections {
    max_per_ip: u32,
    trusted_proxies: Box<[IpNetwork]>,
    hasher: RandomState,
    shards: Box<[Shard]>,
}

/// Held for the lifetime of a connection; releases its slot on drop.
pub enum ConnSlot {
    /// Peer is a trusted proxy: not counted.
    Exempt,
    Counted {
        owner: Arc<PerIpConnections>,
        key: IpAddr,
    },
}

impl Drop for ConnSlot {
    fn drop(&mut self) {
        if let ConnSlot::Counted { owner, key } = self {
            owner.release(*key);
        }
    }
}

impl PerIpConnections {
    pub fn new(max_per_ip: u32, trusted_proxies: &[IpNetwork]) -> Arc<Self> {
        Arc::new(Self {
            max_per_ip: max_per_ip.max(1),
            trusted_proxies: trusted_proxies.into(),
            hasher: RandomState::new(),
            shards: (0..SHARDS).map(|_| Mutex::new(HashMap::new())).collect(),
        })
    }

    fn shard(&self, key: &IpAddr) -> MutexGuard<'_, HashMap<IpAddr, u32>> {
        let idx = (self.hasher.hash_one(key) as usize) % self.shards.len();
        // The critical sections cannot panic; recover from poisoning anyway.
        self.shards[idx].lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reserve a slot for a new connection from `peer`, or `None` if that
    /// client already has `max_per_ip` connections open.
    pub fn try_acquire(self: &Arc<Self>, peer: IpAddr) -> Option<ConnSlot> {
        let peer = peer.to_canonical();
        if self.trusted_proxies.iter().any(|net| net.contains(peer)) {
            return Some(ConnSlot::Exempt);
        }
        let key = bucket_key(peer);
        let mut shard = self.shard(&key);
        let open = shard.entry(key).or_insert(0);
        if *open >= self.max_per_ip {
            return None;
        }
        *open += 1;
        drop(shard);
        Some(ConnSlot::Counted {
            owner: Arc::clone(self),
            key,
        })
    }

    fn release(&self, key: IpAddr) {
        let mut shard = self.shard(&key);
        if let Some(open) = shard.get_mut(&key) {
            *open = open.saturating_sub(1);
            if *open == 0 {
                shard.remove(&key);
            }
        }
    }

    /// Open connections currently counted for `ip`'s bucket.
    pub fn open_for(&self, ip: IpAddr) -> u32 {
        let key = bucket_key(ip);
        self.shard(&key).get(&key).copied().unwrap_or(0)
    }

    /// Number of client buckets with at least one open connection.
    pub fn tracked_clients(&self) -> usize {
        self.shards
            .iter()
            .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    #[test]
    fn caps_per_ip_and_releases_on_drop() {
        let limits = PerIpConnections::new(2, &[]);
        let a = ip("198.51.100.1");
        let s1 = limits.try_acquire(a).expect("first");
        let s2 = limits.try_acquire(a).expect("second");
        assert!(limits.try_acquire(a).is_none(), "third is over the cap");
        assert!(
            limits.try_acquire(ip("198.51.100.2")).is_some(),
            "other IPs unaffected"
        );
        assert_eq!(limits.open_for(a), 2);

        drop(s1);
        assert_eq!(limits.open_for(a), 1);
        let s3 = limits.try_acquire(a).expect("slot freed");
        drop((s2, s3));
        assert_eq!(limits.open_for(a), 0);
        assert_eq!(limits.tracked_clients(), 0, "empty buckets are removed");
    }

    #[test]
    fn ipv6_shares_a_slash_64_and_mapped_v4_is_v4() {
        let limits = PerIpConnections::new(1, &[]);
        let _s = limits.try_acquire(ip("2001:db8:1:2::1")).expect("first");
        assert!(limits.try_acquire(ip("2001:db8:1:2:ffff::9")).is_none());
        assert!(limits.try_acquire(ip("2001:db8:1:3::1")).is_some());

        let _v4 = limits.try_acquire(ip("203.0.113.5")).expect("v4");
        assert!(limits.try_acquire(ip("::ffff:203.0.113.5")).is_none());
    }

    #[test]
    fn trusted_proxies_are_exempt() {
        let nets = [IpNetwork::from_str("10.0.0.0/8").expect("net")];
        let limits = PerIpConnections::new(1, &nets);
        let slots: Vec<_> = (0..10)
            .map(|_| limits.try_acquire(ip("10.1.2.3")).expect("exempt"))
            .collect();
        assert!(slots.iter().all(|s| matches!(s, ConnSlot::Exempt)));
        assert_eq!(limits.tracked_clients(), 0);
        let _held = limits
            .try_acquire(ip("198.51.100.9"))
            .expect("untrusted slot");
        assert!(limits.try_acquire(ip("198.51.100.9")).is_none());
    }

    #[test]
    fn concurrent_acquire_release_balances() {
        let limits = PerIpConnections::new(u32::MAX, &[]);
        let a = ip("198.51.100.77");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..1_000 {
                        let slot = limits.try_acquire(a).expect("slot");
                        drop(slot);
                    }
                });
            }
        });
        assert_eq!(limits.open_for(a), 0);
        assert_eq!(limits.tracked_clients(), 0);
    }
}
