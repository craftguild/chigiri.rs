#[path = "../support.rs"]
mod support;

use support::{Types, generated};

fn main() {
    let stage =
        generated::Resolvers::<(), Types>::builder(std::convert::identity::<chigiri::FieldError>);
    let _ = stage.clone();
}
