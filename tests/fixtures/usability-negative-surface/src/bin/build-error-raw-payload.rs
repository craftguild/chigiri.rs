fn inspect(error: chigiri::EndpointBuildError) {
    if let chigiri::EndpointBuildError::IdentityInitialization(raw) = error {
        let _: String = raw;
    }
}

fn main() {}
