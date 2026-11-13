//! The third-party operation registry is an independent compliance fixture.

use std::sync::Arc;

use chigiri::DocumentDigest;
use chigiri_http::BoxFuture;
use chigiri_operation::{OperationRegistry, PersistedOperationId, ProviderError, RegistryDocument};
use chigiri_testkit::{OperationProviderCase, assert_operation_provider_conformance};
use sha2::{Digest as _, Sha256};

struct ThirdPartyRegistry {
    known: PersistedOperationId,
    document: Arc<str>,
}

impl OperationRegistry for ThirdPartyRegistry {
    fn lookup<'a>(
        &'a self,
        id: &'a PersistedOperationId,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            Ok((id == &self.known).then(|| RegistryDocument::new(Arc::clone(&self.document))))
        })
    }

    fn lookup_by_document_digest<'a>(
        &'a self,
        digest: &'a DocumentDigest,
    ) -> BoxFuture<'a, Result<Option<RegistryDocument>, ProviderError>> {
        Box::pin(async move {
            let expected =
                DocumentDigest::from_bytes(Sha256::digest(self.document.as_bytes()).into());
            Ok((digest == &expected).then(|| RegistryDocument::new(Arc::clone(&self.document))))
        })
    }

    fn revision(&self) -> Option<&str> {
        Some("third-party-1")
    }
}

#[tokio::test]
async fn third_party_operation_provider_runs_public_contract() {
    let known = PersistedOperationId::new("known").unwrap();
    let missing = PersistedOperationId::new("missing").unwrap();
    let provider = ThirdPartyRegistry {
        known: known.clone(),
        document: Arc::from("query Known { value }"),
    };
    let known_digest = DocumentDigest::from_bytes(Sha256::digest(b"query Known { value }").into());
    let missing_digest = DocumentDigest::from_bytes([0; 32]);
    assert_operation_provider_conformance(
        &provider,
        OperationProviderCase {
            known_id: &known,
            known_digest: &known_digest,
            expected_document: "query Known { value }",
            missing_id: &missing,
            missing_digest: &missing_digest,
        },
    )
    .await
    .unwrap();
}
