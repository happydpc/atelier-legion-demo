use atelier_importer::{typetag, SerdeImportable};
use serde::{Deserialize, Serialize};
use serde_diff::SerdeDiff;
use type_uuid::TypeUuid;
use nphysics2d::object::DefaultBodyHandle;
use crate::clone_merge::SpawnFrom;
use crate::math::Vec2;
use crate::resources::{PhysicsResource, OpenedPrefabState};
use legion::prelude::*;
use std::ops::Range;
use legion::storage::ComponentStorage;
use skulpin_plugin_imgui::imgui;
use imgui_inspect_derive::Inspect;
use ncollide2d::shape::ShapeHandle;
use ncollide2d::shape::{Ball, Cuboid};
use ncollide2d::pipeline::{CollisionGroups, GeometricQueryType};
use legion::index::ComponentIndex;

use crate::components::{
    Position2DComponent, UniformScale2DComponent, NonUniformScale2DComponent, Rotation2DComponent,
};
use ncollide2d::world::CollisionWorld;

//
// Add a ball rigid body
//
#[derive(
    TypeUuid,
    Serialize,
    Deserialize,
    SerdeImportable,
    SerdeDiff,
    Debug,
    PartialEq,
    Clone,
    Inspect,
    Default,
)]
#[uuid = "fa518c0a-a65a-44c8-9d35-3f4f336b4de4"]
pub struct RigidBodyBallComponentDef {
    pub radius: f32,
    pub is_static: bool,
}

legion_prefab::register_component_type!(RigidBodyBallComponentDef);

#[derive(
    TypeUuid,
    Serialize,
    Deserialize,
    SerdeImportable,
    SerdeDiff,
    Debug,
    PartialEq,
    Clone,
    Inspect,
    Default,
)]
#[uuid = "36df3006-a5ad-4997-9ccc-0860f49195ad"]
pub struct RigidBodyBoxComponentDef {
    #[serde_diff(opaque)]
    pub half_extents: Vec2,
    pub is_static: bool,
}

legion_prefab::register_component_type!(RigidBodyBoxComponentDef);

pub struct RigidBodyComponent {
    pub handle: DefaultBodyHandle,
    delete_body_tx: crossbeam_channel::Sender<DefaultBodyHandle>,
}

impl Drop for RigidBodyComponent {
    fn drop(&mut self) {
        self.delete_body_tx.send(self.handle);
    }
}

fn transform_shape_to_rigid_body(
    physics: &mut PhysicsResource,
    into: &mut std::mem::MaybeUninit<RigidBodyComponent>,
    src_position: Option<&Position2DComponent>,
    src_rotation: Option<&Rotation2DComponent>,
    shape_handle: ShapeHandle<f32>,
    is_static: bool,
) {
    let position = if let Some(position) = src_position {
        position.position
    } else {
        Vec2::zero()
    };

    let mut collider_offset = Vec2::zero();

    // Build the rigid body.
    let rigid_body_handle = if is_static {
        *collider_offset += *position;
        physics.bodies.insert(nphysics2d::object::Ground::new())
    } else {
        physics.bodies.insert(
            nphysics2d::object::RigidBodyDesc::new()
                .translation(position.into())
                .build(),
        )
    };

    // Build the collider.
    let collider = nphysics2d::object::ColliderDesc::new(shape_handle.clone())
        .density(1.0)
        .translation(collider_offset.into())
        .build(nphysics2d::object::BodyPartHandle(rigid_body_handle, 0));

    // Insert the collider to the body set.
    physics.colliders.insert(collider);

    *into = std::mem::MaybeUninit::new(RigidBodyComponent {
        handle: rigid_body_handle,
        delete_body_tx: physics.delete_body_tx().clone(),
    })
}

impl SpawnFrom<RigidBodyBallComponentDef> for RigidBodyComponent {
    fn spawn_from(
        _src_world: &World,
        src_component_storage: &ComponentStorage,
        src_component_storage_indexes: Range<ComponentIndex>,
        resources: &Resources,
        _src_entities: &[Entity],
        _dst_entities: &[Entity],
        from: &[RigidBodyBallComponentDef],
        into: &mut [std::mem::MaybeUninit<Self>],
    ) {
        let mut physics = resources.get_mut::<PhysicsResource>().unwrap();

        let position_components = crate::components::try_iter_components_in_storage::<
            Position2DComponent,
        >(
            src_component_storage, src_component_storage_indexes.clone()
        );

        let uniform_scale_components =
            crate::components::try_iter_components_in_storage::<UniformScale2DComponent>(
                src_component_storage,
                src_component_storage_indexes.clone(),
            );

        let rotation_components = crate::components::try_iter_components_in_storage::<
            Rotation2DComponent,
        >(src_component_storage, src_component_storage_indexes);

        for (src_position, src_uniform_scale, src_rotation, from, into) in izip!(
            position_components,
            uniform_scale_components,
            rotation_components,
            from,
            into
        ) {
            let mut radius = from.radius;
            if let Some(src_uniform_scale) = src_uniform_scale {
                radius *= src_uniform_scale.uniform_scale;
            }

            //TODO: Warn if radius is 0
            let shape_handle = ShapeHandle::new(Ball::new(radius.max(0.01)));
            transform_shape_to_rigid_body(
                &mut physics,
                into,
                src_position,
                src_rotation,
                shape_handle,
                from.is_static,
            );
        }
    }
}

impl crate::selection::EditorSelectableTransformed<RigidBodyComponent>
    for RigidBodyBallComponentDef
{
    fn create_editor_selection_world(
        &self,
        collision_world: &mut CollisionWorld<f32, Entity>,
        resources: &Resources,
        opened_prefab: &OpenedPrefabState,
        prefab_world: &World,
        prefab_entity: Entity,
        transformed_world: &World,
        transformed_entity: Entity,
        transformed_component: &RigidBodyComponent,
    ) {
        if let Some(position) = prefab_world.get_component::<Position2DComponent>(prefab_entity) {
            let mut radius = self.radius;

            if let Some(uniform_scale) =
                prefab_world.get_component::<UniformScale2DComponent>(prefab_entity)
            {
                radius *= uniform_scale.uniform_scale;
            }

            let shape_handle = ShapeHandle::new(Ball::new(radius.max(0.01)));

            collision_world.add(
                ncollide2d::math::Isometry::new(position.position.into(), 0.0),
                shape_handle,
                CollisionGroups::new(),
                GeometricQueryType::Proximity(0.001),
                transformed_entity,
            );
        }
    }
}

impl SpawnFrom<RigidBodyBoxComponentDef> for RigidBodyComponent {
    fn spawn_from(
        _src_world: &World,
        src_component_storage: &ComponentStorage,
        src_component_storage_indexes: Range<ComponentIndex>,
        resources: &Resources,
        _src_entities: &[Entity],
        _dst_entities: &[Entity],
        from: &[RigidBodyBoxComponentDef],
        into: &mut [std::mem::MaybeUninit<Self>],
    ) {
        let mut physics = resources.get_mut::<PhysicsResource>().unwrap();

        let position_components = crate::components::try_iter_components_in_storage::<
            Position2DComponent,
        >(
            src_component_storage, src_component_storage_indexes.clone()
        );

        let uniform_scale_components =
            crate::components::try_iter_components_in_storage::<UniformScale2DComponent>(
                src_component_storage,
                src_component_storage_indexes.clone(),
            );

        let non_uniform_scale_components =
            crate::components::try_iter_components_in_storage::<NonUniformScale2DComponent>(
                src_component_storage,
                src_component_storage_indexes.clone(),
            );

        let rotation_components = crate::components::try_iter_components_in_storage::<
            Rotation2DComponent,
        >(src_component_storage, src_component_storage_indexes);

        for (src_position, src_uniform_scale, src_non_uniform_scale, src_rotation, from, into) in izip!(
            position_components,
            uniform_scale_components,
            non_uniform_scale_components,
            rotation_components,
            from,
            into
        ) {
            let mut half_extents = *from.half_extents;

            if let Some(src_uniform_scale) = src_uniform_scale {
                half_extents *= glam::Vec2::splat(src_uniform_scale.uniform_scale);
            }

            if let Some(src_non_uniform_scale) = src_non_uniform_scale {
                half_extents *= *src_non_uniform_scale.non_uniform_scale;
            }

            let shape_handle =
                ShapeHandle::new(Cuboid::new(crate::math::vec2_glam_to_glm(half_extents)));
            transform_shape_to_rigid_body(
                &mut physics,
                into,
                src_position,
                src_rotation,
                shape_handle,
                from.is_static,
            );
        }
    }
}

impl crate::selection::EditorSelectableTransformed<RigidBodyComponent>
    for RigidBodyBoxComponentDef
{
    fn create_editor_selection_world(
        &self,
        collision_world: &mut CollisionWorld<f32, Entity>,
        resources: &Resources,
        opened_prefab: &OpenedPrefabState,
        prefab_world: &World,
        prefab_entity: Entity,
        transformed_world: &World,
        transformed_entity: Entity,
        transformed_component: &RigidBodyComponent,
    ) {
        if let Some(position) = prefab_world.get_component::<Position2DComponent>(prefab_entity) {
            let mut half_extents = *self.half_extents;

            if let Some(uniform_scale) =
                prefab_world.get_component::<UniformScale2DComponent>(prefab_entity)
            {
                half_extents *= uniform_scale.uniform_scale;
            }

            if let Some(non_uniform_scale) =
                prefab_world.get_component::<NonUniformScale2DComponent>(prefab_entity)
            {
                half_extents *= *non_uniform_scale.non_uniform_scale;
            }

            let mut rotation = 0.0;
            if let Some(rotation_component) =
                prefab_world.get_component::<Rotation2DComponent>(prefab_entity)
            {
                rotation = rotation_component.rotation;
            }

            let shape_handle =
                ShapeHandle::new(Cuboid::new(crate::math::vec2_glam_to_glm(half_extents)));

            collision_world.add(
                ncollide2d::math::Isometry::new(position.position.into(), rotation),
                shape_handle,
                CollisionGroups::new(),
                GeometricQueryType::Proximity(0.001),
                transformed_entity,
            );
        }
    }
}
