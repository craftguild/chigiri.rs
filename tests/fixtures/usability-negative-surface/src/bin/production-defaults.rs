fn missing<I, C>(input: I)
where
    C: Send + Sync + 'static,
    I: chigiri::IntoEndpointInput<C>,
{
    let _ = chigiri::Endpoint::<C>::builder(input).production_defaults();
}

fn main() {}
