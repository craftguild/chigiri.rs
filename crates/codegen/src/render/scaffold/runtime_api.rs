//! Render the endpoint-input ABI for a generated resolver assembly.

pub(super) fn render_runtime_api(contents: &mut String) {
    contents.push_str(
        r#"
impl<C, T> ::chigiri::IntoEndpointInput<C> for Resolvers<C, T>
where
    C: ::std::marker::Send + ::std::marker::Sync + 'static,
    T: SchemaTypes,
{
    fn into_endpoint_input(self) -> ::chigiri::EndpointInput<C> {
        ::chigiri::EndpointInput::generated(
            &SCHEMA_DESCRIPTOR,
            GENERATED_RESOLVER_ABI_FINGERPRINT,
            self,
        )
    }
}
"#,
    );
}
