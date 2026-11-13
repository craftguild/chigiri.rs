fn missing<C>(schema: chigiri::spi::ExecutableSchema<C>)
where
    C: Send + Sync + 'static,
{
    let _ = chigiri_http::GraphQlEndpoint::__builder_from_schema(schema);
}

fn main() {}
