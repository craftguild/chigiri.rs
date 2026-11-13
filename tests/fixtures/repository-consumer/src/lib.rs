//! Verify public-crate boundaries through an ordinary path dependency.

pub fn product_contract() -> (&'static str, u32, &'static str) {
    (
        env!("CARGO_PKG_RUST_VERSION"),
        chigiri::GENERATED_ABI_VERSION,
        chigiri_opentelemetry::OPENTELEMETRY_SCHEMA_URL,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_repository_crate_versions_and_provider_contracts() {
        let (msrv, abi, schema) = super::product_contract();
        assert_eq!(msrv, "1.95");
        assert_eq!(abi, 8);
        assert!(!schema.is_empty());
        let config = chigiri_rate_memory::InMemoryRateLimitConfig::default();
        assert_eq!(config.max_keys(), 10_000);
        let provider = chigiri_rate_memory::InMemoryTokenBucket::new();
        let _: &dyn chigiri_rate::RateLimiter = &provider;
        let _: &dyn chigiri::rate::RateLimiter = &provider;
    }
}
