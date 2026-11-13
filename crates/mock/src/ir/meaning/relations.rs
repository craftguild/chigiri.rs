//! Bind paired relation declarations to foreign-key slots or implicit edges and retain output cardinality.

use super::{Build, fields, keys, relation_declarations, relation_matching};
use crate::ir::{Constraints, FieldId, FieldPlan, Relation, RelationId, RelationStorage, TypeRef};
use crate::schema::SchemaError;

pub(super) fn resolve(build: &mut Build) -> Result<(), SchemaError> {
    let declarations = relation_declarations::collect(build)?;
    let pairs = relation_matching::resolve(build, &declarations)?;
    for (index, declaration) in declarations.iter().enumerate() {
        if declaration.fields.is_empty()
            && pairs[index]
                .is_some_and(|pair| !declarations[pair].fields.is_empty() || pair < index)
        {
            continue;
        }
        let inverse = pairs[index].map(|pair| declarations[pair].field);
        let storage = if declaration.fields.is_empty() {
            RelationStorage::Implicit {
                owner_key: keys::identity(build, declaration.field, declaration.owner)?,
                target_key: keys::identity(build, declaration.field, declaration.target)?,
            }
        } else {
            let key = keys::register(
                build,
                declaration.field,
                declaration.target,
                &declaration.references,
            )?;
            let mut slots = Vec::new();
            for (position, name) in declaration.fields.iter().enumerate() {
                let target_slot = build.keys[key.0].slots[position];
                let relation_field = &build.graph.fields()[declaration.field.0];
                let ty = TypeRef {
                    named: build.slots[target_slot.0].ty.named,
                    wrappers: relation_field.ty.wrappers.clone(),
                };
                let slot = if let Some(slot) =
                    build.models[declaration.owner.0].slots.get(name).copied()
                {
                    if build.slots[slot.0].ty != ty {
                        return Err(build
                            .error(declaration.field, "public FK type or nullability mismatch"));
                    }
                    slot
                } else {
                    let owner = build.models[declaration.owner.0].ty;
                    if build.graph.types()[owner.0].fields.contains_key(name) {
                        return Err(
                            build.error(declaration.field, "FK collides with a computed field")
                        );
                    }
                    let slot = fields::add_slot(
                        build,
                        declaration.field,
                        declaration.owner,
                        name.clone(),
                        ty,
                    )?;
                    // Relation output constraints apply to the collection, not to each foreign-key component.
                    build.slots[slot.0].constraints = Constraints::default();
                    slot
                };
                slots.push(slot);
            }
            RelationStorage::ForeignKey { slots, key }
        };
        let id = RelationId(build.relations.len());
        let (min_refs, max_refs) = inverse.map_or((0, None), |field| cardinality(build, field));
        build.relations.push(Relation {
            field: declaration.field,
            owner: declaration.owner,
            target: declaration.target,
            storage,
            name: declaration.name.clone(),
            inverses: inverse.into_iter().collect(),
            unique: max_refs == Some(1),
            min_refs,
            max_refs,
        });
        build.plans[declaration.field.0] = Some(FieldPlan::Owner(id));
        if let Some(inverse) = inverse {
            build.plans[inverse.0] = Some(FieldPlan::Inverse { relation: id });
        }
    }
    Ok(())
}

fn cardinality(build: &Build, field: FieldId) -> (usize, Option<usize>) {
    let field = &build.graph.fields()[field.0];
    if field.ty.list_depth() == 0 {
        (usize::from(!field.ty.nullable()), Some(1))
    } else {
        (
            field.constraints.min_items.unwrap_or(0),
            field.constraints.max_items,
        )
    }
}
