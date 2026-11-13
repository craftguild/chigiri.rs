struct ApplicationContext;

fn missing_context<I>(input: I)
where
    I: chigiri::IntoEndpointInput<ApplicationContext>,
{
    let _ = chigiri::Endpoint::<ApplicationContext>::builder(input).build();
}

fn main() {}
