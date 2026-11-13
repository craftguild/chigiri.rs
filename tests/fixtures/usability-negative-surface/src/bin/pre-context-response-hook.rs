fn pre_context_response_hook<I, H>(input: I, hook: H)
where
    I: chigiri::IntoEndpointInput<()>,
    H: chigiri::ResponseHook<chigiri::ContextPending>,
{
    let _ = chigiri::Endpoint::<()>::builder(input).inject(|components| {
        components.http().response_hook(hook);
    });
}

fn main() {}
