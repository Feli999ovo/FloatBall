use super::*;

impl Task {
    pub(super) fn price_for(
        &self,
        base: &str,
        model: &str,
        provider: &str,
        prompt: u64,
        completion: u64,
    ) -> Option<f64> {
        if let Some(cache) = self.catalog.lock().ok().and_then(|guard| guard.clone()) {
            return crate::catalog::estimate_cny(
                cache.price(base, model, provider),
                prompt,
                completion,
                self.cfg.usd_cny,
            );
        }
        None
    }

    pub fn tokens(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
    }

    pub fn estimate(&self) -> Option<f64> {
        self.has_usage
            .load(Ordering::Relaxed)
            .then(|| *self.cost.lock().unwrap())
            .flatten()
    }

    pub(super) fn add_estimate(&self, amount: Option<f64>) {
        let mut total = self.cost.lock().unwrap();
        *total = match (*total, amount) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
    }
}
