use std::time::Instant;

// Simple client-side token bucket to mirror server limit.
// Tokens refill continuously at `rate_per_min` per minute up to `capacity`.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    rate_per_sec: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(rate_per_min: u32) -> Self {
        let rate = rate_per_min as f64;
        Self {
            capacity: rate,
            tokens: rate,
            rate_per_sec: rate / 60.0,
            last: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last);
        let add = self.rate_per_sec * dt.as_secs_f64();
        self.tokens = (self.tokens + add).min(self.capacity);
        self.last = now;
    }

    pub fn try_consume(&mut self, n: f64) -> bool {
        self.refill();
        if self.tokens + 1e-9 >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }

    pub fn peek_tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }

    pub fn capacity(&self) -> f64 {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn bucket_basic() {
        let mut b = TokenBucket::new(6); // 6/min = 0.1/s
        for _ in 0..6 {
            assert!(b.try_consume(1.0));
        }
        assert!(!b.try_consume(1.0));
        thread::sleep(Duration::from_millis(1200)); // ~0.12 tokens
        assert!(b.try_consume(0.1));
    }
}
