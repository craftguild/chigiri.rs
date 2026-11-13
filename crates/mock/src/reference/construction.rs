//! Assemble one bounded document without executing sources or callbacks.

use super::*;

impl ReferenceDocument {
    /// Build reference metadata without constructing a store or VM.
    pub fn build(ir: Option<&MockIr>) -> Result<Self, ReferenceError> {
        Self::from_input(
            if ir.is_some() {
                InputKind::Schema
            } else {
                InputKind::Common
            },
            ir,
        )
    }

    /// Build reference metadata without implying that a readiness check ran.
    pub fn from_input(input: InputKind, ir: Option<&MockIr>) -> Result<Self, ReferenceError> {
        Self::build_with(input, ir, None)
    }

    /// Build project or bundle reference metadata including configured state and hooks.
    pub fn from_configuration(
        input: InputKind,
        ir: &MockIr,
        state: &crate::hooks::StateContract,
        settings: &crate::hooks::HookSettings,
        events: &crate::events::EventSettings,
    ) -> Result<Self, ReferenceError> {
        Self::build_with(input, Some(ir), Some((state, settings, events)))
    }

    pub(super) fn build_with(
        input: InputKind,
        ir: Option<&MockIr>,
        configuration: Option<(
            &crate::hooks::StateContract,
            &crate::hooks::HookSettings,
            &crate::events::EventSettings,
        )>,
    ) -> Result<Self, ReferenceError> {
        if (input == InputKind::Common) != ir.is_none()
            || matches!(input, InputKind::Project | InputKind::Bundle) != configuration.is_some()
        {
            return Err(ReferenceError::contract());
        }
        crate::api::validate(crate::api::METHODS).map_err(|_| ReferenceError::contract())?;
        let mut doc = Self {
            format: info::REFERENCE_FORMAT.into(),
            api_version: info::API_VERSION.into(),
            mock_format: match input {
                InputKind::Common | InputKind::Schema => None,
                InputKind::Project => Some(crate::project::FORMAT.into()),
                InputKind::Bundle => Some(info::CONFIG_FORMAT.into()),
            },
            supported_formats: SupportedFormats {
                project: crate::project::FORMAT.into(),
                bundle: info::CONFIG_FORMAT.into(),
            },
            input_kind: input,
            config_executed: input == InputKind::Project,
            binary: BinaryDoc {
                version: info::BINARY_VERSION.into(),
                luaur: info::LUAUR_VERSION.into(),
            },
            readiness_checked: false,
            directives: Vec::new(),
            methods: Vec::new(),
            api_types: Vec::new(),
            options: Vec::new(),
            operators: Vec::new(),
            sort_directions: Vec::new(),
            rules: Vec::new(),
            examples: Vec::new(),
            limits: Vec::new(),
            contexts: Vec::new(),
            hooks: Vec::new(),
            state_configured: configuration.is_some(),
            rejection: hooks::RejectionDoc::build(),
            logging: hooks::LoggingDoc::build(),
            events: events::empty(configuration.map(|(_, _, events)| events)),
            globals: Vec::new(),
            schema: ir.map(schema::empty),
        };
        // Charge the outer frame first, then append only complete validated lines.
        let mut budget = budget::Budget::new(&doc)?;
        directives::append(&mut doc, &mut budget, ir)?;
        common::append(&mut doc, &mut budget)?;
        type_graph::append(&mut doc, &mut budget)?;
        context_graph::append(&mut doc, &mut budget)?;
        limits::append(&mut doc.limits, &mut budget)?;
        hooks::append(
            &mut doc,
            &mut budget,
            configuration.map(|(state, hooks, _)| (state, hooks)),
        )?;
        if let Some(ir) = ir {
            events::append(
                &mut doc.events,
                ir,
                configuration.map(|(_, _, events)| events),
                &mut budget,
            )?;
        }
        if let (Some(ir), Some(schema)) = (ir, &mut doc.schema) {
            schema::append(schema, ir, &mut budget)?;
            schema_models::append(schema, ir, &mut budget)?;
            for (index, model) in ir.models().iter().enumerate() {
                let name = &ir.graph().types()[model.ty.0].name;
                // Always use quoted table access so every valid GraphQL model name is represented uniformly.
                let quoted =
                    serde_json::to_string(name.as_ref()).map_err(|_| ReferenceError::contract())?;
                budget.push(
                    &mut doc.examples,
                    ExampleDoc {
                        id: format!("schema/model/{index}/count"),
                        title: "Count model records.".into(),
                        origin: ExampleOrigin::SchemaSpecific,
                        scope: crate::api::Effect::Read,
                        luau: format!("return ctx.models[{quoted}]:count({{}})"),
                        rule_ids: vec!["count.exact".into()],
                    },
                )?;
            }
        }
        Ok(doc)
    }
}
