use std::collections::VecDeque;

#[derive(Default)]
pub struct Clock {
    samples: VecDeque<f64>,
}
impl Clock {
    pub fn sample(&mut self, t0: f64, received: f64, server: f64) {
        let rtt = received - t0;
        if !(0.0..=2000.0).contains(&rtt) || !server.is_finite() {
            return;
        }
        if self.samples.len() == 5 {
            self.samples.pop_front();
        }
        self.samples.push_back(server - (t0 + received) / 2.0);
    }
    pub fn offset(&self) -> f64 {
        let mut samples: Vec<_> = self.samples.iter().copied().collect();
        samples.sort_by(f64::total_cmp);
        samples.get(samples.len() / 2).copied().unwrap_or_default()
    }
    pub fn ready(&self) -> bool {
        self.samples.len() >= 3
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midpoint_median_rejects_slow_samples() {
        let mut c = Clock::default();
        for offset in [10.0, 11.0, 900.0, 9.0, 10.0] {
            c.sample(100.0, 120.0, 110.0 + offset);
        }
        c.sample(0.0, 5000.0, 99999.0);
        assert_eq!(c.offset(), 10.0);
        assert!(c.ready());
    }
}
