use crate::asset_manager::AssetManager;

use atelier_loader::{
    handle::{AssetHandle, Handle},
    rpc_loader::RpcLoader,
    LoadStatus, Loader
};
use std::collections::HashMap;
use legion::prelude::*;
use crate::clone_merge::CloneMergeImpl;

use legion::storage::ComponentTypeId;
use prefab_format::ComponentTypeUuid;
use legion_prefab::{ComponentRegistration, CookedPrefab};
use crate::prefab::PrefabAsset;
use atelier_core::AssetUuid;

pub fn cook_prefab(
    asset_manager: &mut AssetManager,
    registered_components: &HashMap<ComponentTypeId, ComponentRegistration>,
    registered_components_by_uuid: &HashMap<ComponentTypeUuid, ComponentRegistration>,
    prefab_uuid: AssetUuid
) -> CookedPrefab {
    // Create the clone_merge impl. For prefab cooking, we will clone everything so we don't need to
    // set up any transformations
    let clone_merge_impl = CloneMergeImpl::new(registered_components.clone());

    // This will allow us to look up prefabs by AssetUuid
    let mut prefab_lookup = HashMap::new();

    // This will allow us to look up the cooked entity ID by the entity's original UUID
    let mut entity_lookup = HashMap::new();

    // This will hold the asset IDs sorted with dependencies first. This ensures that
    // prefab_lookup and entity_lookup are populated with all dependent prefabs/entities
    let mut prefab_cook_order = vec![];

    // Recursively do a blocking load on the prefab and the other prefabs it depends on. This
    // populates prefab_lookup and prefab_cook_order
    request_prefab_dependency(
        asset_manager,
        prefab_uuid,
        &mut prefab_lookup,
        &mut prefab_cook_order
    );

    for id in &prefab_cook_order {
        println!("prefabs_in_cook_order: {}", id);
    }

    // Create a new world to hold the cooked data
    let universe = Universe::new();
    let mut world = universe.create_world();

    // merge all entity data from all prefabs. This data doesn't include any overrides, so order
    // doesn't matter
    for (_, prefab_handle) in &prefab_lookup {
        let prefab_asset: &PrefabAsset = prefab_handle.asset(asset_manager.storage()).unwrap();

        println!("Cloning entities from prefab {}", AssetUuid(prefab_asset.prefab.prefab_meta.id));
        println!("{:#?}", prefab_asset.prefab.prefab_meta.entities);

        // Clone all the entities from the prefab into the cooked world. As the data is copied,
        // entity will get a new Entity assigned to it in the cooked world. result_mappings will
        // be populated as this happens so that we can trace where data in the prefab landed in
        // the cooked world
        let mut result_mappings = HashMap::new();
        world.clone_merge(&prefab_asset.prefab.world, &clone_merge_impl, None, Some(&mut result_mappings));

        // Iterate the entities in this prefab. Determine where they are stored in the cooked
        // world and store this in entity_lookup
        for (entity_uuid, prefab_entity) in &prefab_asset.prefab.prefab_meta.entities {
            let cooked_entity = result_mappings[prefab_entity];
            entity_lookup.insert(*entity_uuid, cooked_entity);
            println!("entity {} ({:?}) will be {:?} in cooked data", uuid::Uuid::from_bytes(*entity_uuid), prefab_entity, cooked_entity);
        }
    }

    // apply component override data. iteration of prefabs is in order such that "base" prefabs
    // are processed first
    for prefab_id in &prefab_cook_order {
        // fetch the data for the prefab
        let prefab_handle = &prefab_lookup[prefab_id];
        let prefab_asset: &PrefabAsset = prefab_handle.asset(asset_manager.storage()).unwrap();

        // Iterate all the other prefabs that this prefab references
        println!("Iterating prefabs referenced by prefab {}", uuid::Uuid::from_bytes(prefab_asset.prefab.prefab_meta.id));
        for (dependency_prefab_id, dependency_prefab_ref) in &prefab_asset.prefab.prefab_meta.prefab_refs {
            // Iterate all the entities for which we have override data
            println!("Processing reference to prefab {}", uuid::Uuid::from_bytes(*dependency_prefab_id));
            for (entity_id, component_overrides) in &dependency_prefab_ref.overrides {
                println!("Processing referenced entity {}", uuid::Uuid::from_bytes(*entity_id));

                // Find where this entity is stored within the cooked data
                let cooked_entity = entity_lookup[entity_id];
                println!("This entity is stored at {:?}", cooked_entity);

                // Iterate all the component types for which we have override data
                for component_override in component_overrides {
                    println!("processing component type {}", uuid::Uuid::from_bytes(component_override.component_type));
                    let component_registration = &registered_components_by_uuid[&component_override.component_type];

                    // Apply the override data to the component
                    //TODO: Implement this
                    match &component_override.data.0 {
                        legion_prefab::BincodeOrSerdeValue::Bincode(x) => {
                            println!("deserialize bincode");

                            let slice_reader = bincode::SliceReader::new(x);
                            let de_acceptor = ApplyComponentDiffDeserializerAcceptor {
                                component_registration: component_registration,
                                world: &mut world,
                                entity: cooked_entity
                            };

                            bincode::with_deserializer(slice_reader, de_acceptor);
                        },
                        legion_prefab::BincodeOrSerdeValue::SerdeValue(_value) => {
                            println!("deserialize serde_value");
                            unimplemented!();

                        }
                    }
                }
            }
        }
    }

    // the resulting world can now be saved
    let cooked_prefab = legion_prefab::CookedPrefab {
        world: world,
        entities: entity_lookup
    };

    // Verify that the data can properly round-trip
    {
        let cooked_prefab_string =
            ron::ser::to_string_pretty(&cooked_prefab, ron::ser::PrettyConfig::default())
                .unwrap();

        let restored = ron::de::from_str::<legion_prefab::CookedPrefab>(&cooked_prefab_string).unwrap();

        let cooked_prefab_string2 =
            ron::ser::to_string_pretty(&restored, ron::ser::PrettyConfig::default())
                .unwrap();

        assert_eq!(cooked_prefab_string, cooked_prefab_string2);
        println!("{}", cooked_prefab_string2);
    }

    cooked_prefab
}

// This function does a recursive blocking load on the provided prefab asset and all prefabs
// that it references. As it does this, prefab_lookup and prefab_cook_order are populated
fn request_prefab_dependency(
    asset_manager: &mut AssetManager,
    id: AssetUuid,
    prefab_lookup: &mut HashMap<AssetUuid, Handle<PrefabAsset>>,
    prefab_cook_order: &mut Vec<AssetUuid>
) {
    // Request the asset
    let load_handle = asset_manager
        .loader()
        .add_ref(id);
    let handle = Handle::<PrefabAsset>::new(asset_manager.tx().clone(), load_handle);

    // Block until it loads
    loop {
        asset_manager.update();
        if let LoadStatus::Loaded = handle.load_status::<RpcLoader>(asset_manager.loader()) {
            break;
        }
    }

    // Grab a reference to the asset
    let prefab_asset: &PrefabAsset = handle.asset(asset_manager.storage()).unwrap();

    // Get a list of prefabs this asset references. We clone these into a new list due to borrowing restrictions
    let other_prefab_ids : Vec<_> = prefab_asset.prefab.prefab_meta.prefab_refs.iter().map(|(other_prefab_id, _)| AssetUuid(other_prefab_id.clone())).collect();

    // Use recursion to visit the tree ensuring that ancestor prefab data gets processed first
    for other_prefab_id in other_prefab_ids {
        if !prefab_lookup.contains_key(&other_prefab_id) {
            request_prefab_dependency(asset_manager, other_prefab_id, prefab_lookup, prefab_cook_order);
        }
    }

    // Write data.. this needs to happen after we visit prefabs that we reference
    prefab_lookup.insert(id, handle);
    prefab_cook_order.push(id);
}

// bincode API requires us to implement an acceptor in order to get a deserializer impl. We need
// the impl so that we can pass it to legion::de::deserialize()
struct ApplyComponentDiffDeserializerAcceptor<'b, 'c> {
    //world: &'b mut World,
    //deserialize_impl: &'c legion_prefab::DeserializeImpl
    component_registration: &'b ComponentRegistration,
    world: &'c mut World,
    entity: Entity
}

impl<'a, 'b, 'c> bincode::DeserializerAcceptor<'a> for ApplyComponentDiffDeserializerAcceptor<'b, 'c> {
    type Output = ();

    //TODO: Error handling needs to be passed back out
    fn accept<T: serde::Deserializer<'a>>(self, de: T) -> Self::Output {
        let mut de = erased_serde::Deserializer::erase(de);
        self.component_registration.apply_diff(&mut de, self.world, self.entity);
    }
}