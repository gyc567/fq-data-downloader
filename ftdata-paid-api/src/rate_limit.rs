//! Per-wallet rate limiting (Q5).
//!
//! DECISIONS.md: **Wallet primary; dual = IP coarse + wallet fine**
//!
//! - **Primary**: per `CallerIdentity.address` — counts requests, allows
//!   the free-tier cap from PricingConfig, rejects with 429 over the cap.
//! - **Secondary**: per source IP — coarse cap (higher) to bound abuse
//!   from a single IP even if the attacker rotates wallets.
//!
//! Both limits run independently. A request that hits either limit
//! gets a 429 with a `Retry-After` header derived from the per-wallet
//! window. IP fallback is implicit (the secondary limit).
//!
//! For Phase 1 the rate limiter is in-process and per-instance; Phase 2
//! moves to a shared store (Redis / Cloudflare Durable Object).

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// Free-tier limits per address, derived from PricingConfig.
#[derive(Debug, Clone)]
pub struct RateLimits {
    /// Per-wallet: requests per hour.
    pub per_wallet_per_hour: u32,
    /// Per-IP: requests per hour (higher than per-wallet to bound abuse).
    pub per_ip_per_hour: u32,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            per_wallet_per_hour: 10, // matches PricingConfig.free_tier.rate_limit_per_hour
            per_ip_per_hour: 100,    // 10x wallet cap for coarse IP limit
        }
    }
}

/// Token-bucket-ish sliding window. Stores the most recent N timestamps
/// and counts how many fall within the last `window`.
///
/// DashMap shard: address -> (window_start, request count, last refill).
/// This is a small optimization vs a full sliding window — the counter
/// is reset every hour rather than per-request, which is good enough
/// for the 10-req/hour free tier.
#[derive(Debug)]
struct WalletCounter {
    window_start: Instant,
    count: u32,
}

impl WalletCounter {
    fn fresh() -> Self {
        Self {
            window_start: Instant::now(),
            count: 0,
        }
    }
}

/// Thread-safe rate limiter keyed by wallet address and (secondary) IP.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// address -> counter
    wallet: Arc<DashMap<String, WalletCounter>>,
    /// ip -> counter
    ip: Arc<DashMap<IpAddr, WalletCounter>>,
    limits: RateLimits,
    window: Duration,
}

impl RateLimiter {
    pub fn new(limits: RateLimits) -> Self {
        Self {
            wallet: Arc::new(DashMap::new()),
            ip: Arc::new(DashMap::new()),
            limits,
            window: Duration::from_secs(3600),
        }
    }

    /// Try to consume one slot for this wallet+ip combo.
    /// Returns Ok(()) if allowed, Err with retry_after_secs if denied.
    pub fn check(&self, wallet: &str, ip: IpAddr) -> Result<(), RateLimitHit> {
        let wallet_key = wallet.to_string();
        self.check_inner(&self.wallet, &wallet_key, self.limits.per_wallet_per_hour)
            .map_err(|secs| RateLimitHit {
                scope: RateLimitScope::Wallet,
                retry_after_secs: secs,
            })?;
        self.check_inner(&self.ip, &ip, self.limits.per_ip_per_hour)
            .map_err(|secs| RateLimitHit {
                scope: RateLimitScope::Ip,
                retry_after_secs: secs,
            })?;
        Ok(())
    }

    fn check_inner<K: std::hash::Hash + Eq + Clone>(
        &self,
        map: &DashMap<K, WalletCounter>,
        key: &K,
        cap: u32,
    ) -> Result<(), u64> {
        let now = Instant::now();
        let mut entry = map.entry(key.clone()).or_insert_with(WalletCounter::fresh);
        if now.duration_since(entry.window_start) >= self.window {
            // Window expired — reset.
            entry.window_start = now;
            entry.count = 0;
        }
        if entry.count >= cap {
            let retry = self.window.saturating_sub(now.duration_since(entry.window_start));
            return Err(retry.as_secs().max(1));
        }
        entry.count += 1;
        Ok(())
    }

    /// Number of wallets currently tracked (for testing / metrics).
    pub fn wallet_count(&self) -> usize {
        self.wallet.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitHit {
    pub scope: RateLimitScope,
    pub retry_after_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitScope {
    Wallet,
    Ip,
}

impl std::fmt::Display for RateLimitScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wallet => write!(f, "wallet"),
            Self::Ip => write!(f, "ip"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_limiter(cap: u32) -> RateLimiter {
        RateLimiter::new(RateLimits {
            per_wallet_per_hour: cap,
            per_ip_per_hour: cap * 10,
            ..Default::default()
        })
    }

    #[test]
    fn allows_under_limit() {
        let rl = make_limiter(3);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert!(rl.check("0xA", ip).is_ok());
        assert!(rl.check("0xA", ip).is_ok());
        assert!(rl.check("0xA", ip).is_ok());
    }

    #[test]
    fn rejects_at_limit_with_retry_after() {
        let rl = make_limiter(2);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert!(rl.check("0xA", ip).is_ok());
        assert!(rl.check("0xA", ip).is_ok());
        let hit = rl.check("0xA", ip).unwrap_err();
        assert_eq!(hit.scope, RateLimitScope::Wallet);
        assert!(hit.retry_after_secs > 0);
    }

    #[test]
    fn different_wallets_have_separate_buckets() {
        let rl = make_limiter(1);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert!(rl.check("0xA", ip).is_ok());
        assert!(rl.check("0xB", ip).is_ok());
        // Both A and B have used their 1 request, but from the same IP
        // the IP cap (10) is still not hit.
    }

    #[test]
    fn same_wallet_different_ips_share_wallet_bucket() {
        let rl = make_limiter(1);
        let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
        assert!(rl.check("0xA", ip1).is_ok());
        // Wallet bucket already hit 1; same wallet from different IP also fails.
        let hit = rl.check("0xA", ip2).unwrap_err();
        assert_eq!(hit.scope, RateLimitScope::Wallet);
    }

    #[test]
    fn ip_cap_independent_of_wallet() {
        let rl = RateLimiter::new(RateLimits {
            per_wallet_per_hour: 100,
            per_ip_per_hour: 2,
            ..Default::default()
        });
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        // Two different wallets, same IP — wallet caps not hit but IP cap will.
        assert!(rl.check("0xA", ip).is_ok());
        assert!(rl.check("0xB", ip).is_ok());
        let hit = rl.check("0xC", ip).unwrap_err();
        assert_eq!(hit.scope, RateLimitScope::Ip);
    }

    #[test]
    fn wallet_count_tracks_unique_addresses() {
        let rl = make_limiter(100);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        rl.check("0xA", ip).unwrap();
        rl.check("0xB", ip).unwrap();
        rl.check("0xA", ip).unwrap();
        assert_eq!(rl.wallet_count(), 2);
    }
}
