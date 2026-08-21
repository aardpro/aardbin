//! In-memory sliding-window rate limiter for POST /login (PRD §7.4).
//!
//! 5 failed attempts within a 5-minute window → locked out until the
//! window drains. Successful login clears the counter.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_FAILURES: usize = 5;

pub struct LoginRateLimiter {
    failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginRateLimiter {
    pub fn new() -> Self {
        LoginRateLimiter {
            failures: Mutex::new(HashMap::new()),
        }
    }

    /// Returns Ok(()) if the IP may attempt a login, or Err(remaining lockout).
    pub fn check(&self, ip: IpAddr) -> Result<(), Duration> {
        let mut map = self.failures.lock().unwrap();
        let now = Instant::now();
        let queue = map.entry(ip).or_default();
        drain_old(queue, now);
        if queue.len() >= MAX_FAILURES {
            let oldest = *queue.front().unwrap();
            let remaining = WINDOW.saturating_sub(now.duration_since(oldest));
            Err(remaining.max(Duration::from_secs(1)))
        } else {
            Ok(())
        }
    }

    pub fn record_failure(&self, ip: IpAddr) {
        let mut map = self.failures.lock().unwrap();
        let now = Instant::now();
        let queue = map.entry(ip).or_default();
        drain_old(queue, now);
        queue.push_back(now);
    }

    pub fn record_success(&self, ip: IpAddr) {
        let mut map = self.failures.lock().unwrap();
        map.remove(&ip);
    }
}

fn drain_old(queue: &mut VecDeque<Instant>, now: Instant) {
    while let Some(&front) = queue.front() {
        if now.duration_since(front) >= WINDOW {
            queue.pop_front();
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn locks_out_after_five_failures() {
        let rl = LoginRateLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        for _ in 0..5 {
            assert!(rl.check(ip).is_ok());
            rl.record_failure(ip);
        }
        let err = rl.check(ip).unwrap_err();
        assert!(err.as_secs() > 0);
        assert!(err.as_secs() <= 300);
    }

    #[test]
    fn success_clears_counter() {
        let rl = LoginRateLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        for _ in 0..5 {
            rl.record_failure(ip);
        }
        rl.record_success(ip);
        assert!(rl.check(ip).is_ok());
    }

    #[test]
    fn ips_are_independent() {
        let rl = LoginRateLimiter::new();
        let a = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let b = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));
        for _ in 0..5 {
            rl.record_failure(a);
        }
        assert!(rl.check(a).is_err());
        assert!(rl.check(b).is_ok());
    }
}
