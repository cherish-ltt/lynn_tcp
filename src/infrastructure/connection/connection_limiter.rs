use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// A rate limiter using a sliding window algorithm.
#[derive(Clone)]
pub struct RateLimiter {
    max_tokens: u64,
    window_duration: Duration,
    tokens: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
}

impl RateLimiter {
    /// Creates a new rate limiter.
    ///
    /// # Parameters
    ///
    /// * `max_tokens` - Maximum number of tokens (connections) allowed per window.
    /// * `window_duration_secs` - Window duration in seconds.
    ///
    /// # Returns
    ///
    /// A new RateLimiter instance.
    pub fn new(max_tokens: u64, window_duration_secs: u64) -> Self {
        Self {
            max_tokens,
            window_duration: Duration::from_secs(window_duration_secs),
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Checks if a connection from the given IP is allowed.
    ///
    /// # Parameters
    ///
    /// * `ip` - The IP address to check.
    ///
    /// # Returns
    ///
    /// `true` if the connection is allowed, `false` otherwise.
    pub async fn check_rate_limit(&self, ip: IpAddr) -> bool {
        if self.max_tokens == 0 {
            return true; // No rate limiting
        }

        let mut tokens = self.tokens.write().await;
        let now = Instant::now();
        let window_start = now - self.window_duration;

        // Get or create the timestamp vector for this IP
        let timestamps = tokens.entry(ip).or_insert_with(Vec::new);

        // Remove timestamps outside the window
        timestamps.retain(|&ts| ts > window_start);

        // Check if we can add a new connection
        if timestamps.len() < self.max_tokens as usize {
            timestamps.push(now);
            debug!(
                "Rate limit check passed for IP: {}, count: {}/{}",
                ip,
                timestamps.len(),
                self.max_tokens
            );
            true
        } else {
            warn!(
                "Rate limit exceeded for IP: {}, count: {}/{}",
                ip,
                timestamps.len(),
                self.max_tokens
            );
            false
        }
    }

    /// Removes all timestamps for a given IP (called when connection closes).
    pub async fn cleanup_ip(&self, ip: IpAddr) {
        let mut tokens = self.tokens.write().await;
        tokens.remove(&ip);
    }

    /// Periodically cleanup old timestamps to prevent memory leaks.
    pub async fn periodic_cleanup(&self) {
        let mut tokens = self.tokens.write().await;
        let now = Instant::now();
        let window_start = now - self.window_duration;

        for timestamps in tokens.values_mut() {
            timestamps.retain(|&ts| ts > window_start);
        }

        // Remove empty entries
        tokens.retain(|_, timestamps| !timestamps.is_empty());
    }
}

/// Limits the number of connections per IP address.
#[derive(Clone)]
pub struct IpConnectionLimiter {
    max_connections_per_ip: usize,
    ip_connections: Arc<DashMap<IpAddr, usize>>,
}

impl IpConnectionLimiter {
    /// Creates a new IP connection limiter.
    ///
    /// # Parameters
    ///
    /// * `max_connections_per_ip` - Maximum number of connections per IP.
    ///
    /// # Returns
    ///
    /// A new IpConnectionLimiter instance.
    pub fn new(max_connections_per_ip: usize) -> Self {
        Self {
            max_connections_per_ip,
            ip_connections: Arc::new(DashMap::new()),
        }
    }

    /// Checks if a connection from the given IP is allowed.
    ///
    /// # Parameters
    ///
    /// * `ip` - The IP address to check.
    ///
    /// # Returns
    ///
    /// `true` if the connection is allowed, `false` otherwise.
    pub fn check_connection_limit(&self, ip: IpAddr) -> bool {
        let mut count = self.ip_connections.entry(ip).or_insert(0);
        if *count < self.max_connections_per_ip {
            *count += 1;
            debug!(
                "Connection limit check passed for IP: {}, count: {}/{}",
                ip, *count, self.max_connections_per_ip
            );
            true
        } else {
            warn!(
                "Connection limit exceeded for IP: {}, count: {}/{}",
                ip, *count, self.max_connections_per_ip
            );
            false
        }
    }

    /// Records that a connection from the given IP has closed.
    pub fn release_connection(&self, ip: IpAddr) {
        if let Some(mut count) = self.ip_connections.get_mut(&ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                self.ip_connections.remove(&ip);
            }
            debug!(
                "Connection released for IP: {}, remaining count: {}",
                ip, *count
            );
        }
    }

    /// Gets the current number of connections for a given IP.
    pub fn get_connection_count(&self, ip: IpAddr) -> usize {
        self.ip_connections.get(&ip).map(|v| *v).unwrap_or(0)
    }
}

/// A combined limiter that manages both rate limiting and per-IP connection limits.
pub struct ConnectionLimiter {
    rate_limiter: Option<RateLimiter>,
    ip_limiter: Option<IpConnectionLimiter>,
}

impl ConnectionLimiter {
    /// Creates a new connection limiter.
    ///
    /// # Parameters
    ///
    /// * `rate_limit` - Maximum connections per second (0 to disable).
    /// * `max_connections_per_ip` - Maximum connections per IP (0 to disable).
    ///
    /// # Returns
    ///
    /// A new ConnectionLimiter instance.
    pub fn new(rate_limit: u64, max_connections_per_ip: usize) -> Self {
        let rate_limiter = if rate_limit > 0 {
            Some(RateLimiter::new(rate_limit, 1))
        } else {
            None
        };

        let ip_limiter = if max_connections_per_ip > 0 {
            Some(IpConnectionLimiter::new(max_connections_per_ip))
        } else {
            None
        };

        Self {
            rate_limiter,
            ip_limiter,
        }
    }

    /// Checks if a connection from the given address is allowed.
    ///
    /// # Parameters
    ///
    /// * `ip` - The IP address to check.
    ///
    /// # Returns
    ///
    /// `true` if the connection is allowed, `false` otherwise.
    pub async fn check_connection(&self, ip: IpAddr) -> bool {
        // Check rate limit first
        if let Some(rate_limiter) = &self.rate_limiter {
            if !rate_limiter.check_rate_limit(ip).await {
                return false;
            }
        }

        // Check per-IP connection limit
        if let Some(ip_limiter) = &self.ip_limiter {
            if !ip_limiter.check_connection_limit(ip) {
                return false;
            }
        }

        true
    }

    /// Records that a connection from the given IP has closed.
    pub fn release_connection(&self, ip: IpAddr) {
        if let Some(ip_limiter) = &self.ip_limiter {
            ip_limiter.release_connection(ip);
        }
    }

    /// Starts the periodic cleanup task for the rate limiter.
    pub fn spawn_cleanup_task(self: Arc<Self>) {
        if let Some(rate_limiter) = &self.rate_limiter {
            let rate_limiter = rate_limiter.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    rate_limiter.periodic_cleanup().await;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(5, 1);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First 5 connections should succeed
        for i in 0..5 {
            assert!(
                limiter.check_rate_limit(ip).await,
                "Connection {} should succeed",
                i + 1
            );
        }

        // 6th connection should fail
        assert!(
            !limiter.check_rate_limit(ip).await,
            "6th connection should fail"
        );

        // Wait for window to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // After window expires, should work again
        assert!(
            limiter.check_rate_limit(ip).await,
            "Connection after window should succeed"
        );
    }

    #[tokio::test]
    async fn test_ip_connection_limiter() {
        let limiter = IpConnectionLimiter::new(3);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First 3 connections should succeed
        for i in 0..3 {
            assert!(
                limiter.check_connection_limit(ip),
                "Connection {} should succeed",
                i + 1
            );
        }

        // 4th connection should fail
        assert!(
            !limiter.check_connection_limit(ip),
            "4th connection should fail"
        );

        // Release one connection
        limiter.release_connection(ip);

        // Now should work again
        assert!(
            limiter.check_connection_limit(ip),
            "Connection after release should succeed"
        );
    }

    #[tokio::test]
    async fn test_connection_limiter_combined() {
        let limiter = Arc::new(ConnectionLimiter::new(5, 3));
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First 3 connections should succeed (limited by per-IP limit)
        for i in 0..3 {
            assert!(
                limiter.check_connection(ip).await,
                "Connection {} should succeed",
                i + 1
            );
        }

        // 4th connection should fail (exceeds per-IP limit)
        assert!(
            !limiter.check_connection(ip).await,
            "4th connection should fail (per-IP limit)"
        );
    }

    #[tokio::test]
    async fn test_rate_limit_disabled() {
        let limiter = RateLimiter::new(0, 1);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // With rate limit disabled, all connections should succeed
        for i in 0..100 {
            assert!(
                limiter.check_rate_limit(ip).await,
                "Connection {} should succeed",
                i + 1
            );
        }
    }

    #[tokio::test]
    async fn test_ipv6_rate_limiting() {
        let limiter = RateLimiter::new(2, 1);
        let ip = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));

        // Should work with IPv6 addresses
        assert!(limiter.check_rate_limit(ip).await);
        assert!(limiter.check_rate_limit(ip).await);
        assert!(!limiter.check_rate_limit(ip).await);
    }
}
