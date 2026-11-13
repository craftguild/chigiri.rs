fn missing<I, C>(input: I)
where
    C: Send + Sync + 'static,
    I: chigiri::IntoEndpointInput<C>,
{
    let _ = chigiri::Endpoint::<C>::advanced(input);
}

fn main() {}
