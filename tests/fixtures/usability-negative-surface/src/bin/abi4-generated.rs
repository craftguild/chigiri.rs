const _: () = assert!(
    chigiri::GENERATED_ABI_VERSION == 4,
    "Chigiri generated ABI mismatch; regenerate the schema.",
);

fn main() {}
