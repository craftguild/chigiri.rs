fn main() {
    let _ = chigiri::rate::transport(TransportKey)
        .token_bucket()
        .window(std::time::Duration::from_secs(1));
}

struct TransportKey;

impl chigiri::rate::TransportQuotaExtractor for TransportKey {
    fn extract(
        &self,
        _: chigiri::TransportRequest<'_>,
    ) -> Result<chigiri::rate::QuotaKey, chigiri::spi::AdmissionError> {
        unreachable!("Compile failure fixture does not run.")
    }
}
