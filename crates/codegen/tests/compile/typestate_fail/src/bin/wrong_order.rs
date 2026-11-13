#[path = "../support.rs"]
mod support;

use support::{Candidate, Types, generated};

fn main() {
    let _ =
        generated::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>)
            .with_query(Candidate);
}
