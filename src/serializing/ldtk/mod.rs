use bevy::{
    asset::{AssetServer, Assets, Handle},
    ecs::{
        component::Component,
        entity::Entity,
        event::EventWriter,
        query::With,
        system::{Commands, NonSend, Query, Res, ResMut},
    },
    hierarchy::{BuildChildren, DespawnRecursiveExt},
    log::error,
    math::{UVec2, Vec2},
    prelude::SpatialBundle,
    sprite::{Sprite, SpriteBundle, TextureAtlas},
    transform::components::Transform,
};

use crate::{
    render::texture::{TilemapTexture, TilemapTextureDescriptor},
    tilemap::map::TilemapRotation,
};

use self::{
    components::{EntityIid, LdtkLoadedLevel, LevelIid},
    entities::LdtkEntityRegistry,
    events::{LdtkEvent, LevelEvent},
    json::{
        definitions::{LayerType, TilesetDef},
        level::{LayerInstance, Level},
        LdtkJson, WorldLayout,
    },
    layer::LdtkLayers,
    physics::analyze_physics_layer,
    resources::{LdtkLevelManager, LdtkTextures},
};

pub mod app_ext;
pub mod components;
pub mod entities;
pub mod enums;
pub mod events;
pub mod json;
pub mod layer;
pub mod physics;
pub mod resources;

#[derive(Component)]
pub struct LdtkLoader {
    pub(crate) level: String,
}

#[derive(Component)]
pub struct LdtkUnloader;

pub fn unload_ldtk_level(
    mut commands: Commands,
    mut query: Query<(Entity, &LdtkLoadedLevel), With<LdtkUnloader>>,
    mut ldtk_events: EventWriter<LdtkEvent>,
) {
    query.iter_mut().for_each(|(entity, level)| {
        ldtk_events.send(LdtkEvent::LevelUnloaded(LevelEvent {
            identifier: level.identifier.clone(),
            iid: level.iid.clone(),
        }));
        commands.entity(entity).despawn_recursive();
    });
}

pub fn load_ldtk_json(
    mut commands: Commands,
    loader_query: Query<(Entity, &LdtkLoader)>,
    asset_server: Res<AssetServer>,
    ident_mapper: NonSend<LdtkEntityRegistry>,
    mut atlas_asstes: ResMut<Assets<TextureAtlas>>,
    mut ldtk_events: EventWriter<LdtkEvent>,
    mut ldtk_textures: ResMut<LdtkTextures>,
    mut manager: ResMut<LdtkLevelManager>,
) {
    for (entity, loader) in loader_query.iter() {
        let loaded = load_levels(
            &mut commands,
            &mut manager,
            loader,
            &asset_server,
            &ident_mapper,
            &mut atlas_asstes,
            entity,
            &mut ldtk_events,
            &mut ldtk_textures,
        );

        let mut e_cmd = commands.entity(entity);
        if let Some(loaded) = loaded {
            e_cmd.insert((
                SpatialBundle::default(),
                LevelIid(loaded.iid.clone()),
                loaded,
            ));
        } else {
            error!("Failed to load level: {}!", loader.level);
        }
        e_cmd.remove::<LdtkLoader>();
    }
}

fn load_levels(
    commands: &mut Commands,
    manager: &mut LdtkLevelManager,
    loader: &LdtkLoader,
    asset_server: &AssetServer,
    ident_mapper: &LdtkEntityRegistry,
    atlas_asstes: &mut Assets<TextureAtlas>,
    level_entity: Entity,
    ldtk_events: &mut EventWriter<LdtkEvent>,
    ldtk_textures: &mut LdtkTextures,
) -> Option<LdtkLoadedLevel> {
    let ldtk_data = manager.get_cached_data();
    ldtk_data.defs.tilesets.iter().for_each(|tileset| {
        if let Some(texture) = load_texture(tileset, asset_server, &manager) {
            ldtk_textures.insert_tileset(tileset.uid, texture.clone());

            let handle = atlas_asstes.add(texture.as_texture_atlas());
            ldtk_textures.insert_atlas(tileset.uid, handle);
        }
    });

    for (level_index, level) in ldtk_data.levels.iter().enumerate() {
        // this cannot cannot be replaced by filter(), because the level_index matters.
        if level.identifier != loader.level {
            continue;
        }

        let translation = get_level_translation(&ldtk_data, level_index, &manager);

        let level_px = UVec2 {
            x: level.px_wid as u32,
            y: level.px_hei as u32,
        };

        load_background(
            commands,
            level_entity,
            level,
            translation,
            level_px,
            asset_server,
            &manager,
        );

        let mut collider_aabbs = None;
        let mut layer_grid = LdtkLayers::new(
            level_entity,
            level.layer_instances.len(),
            level_px,
            ldtk_textures,
            translation,
            manager.z_index,
        );
        for (layer_index, layer) in level.layer_instances.iter().enumerate() {
            if let Some(phy) = manager.physics_layer.as_ref() {
                if layer.identifier == phy.identifier {
                    collider_aabbs = Some(analyze_physics_layer(layer, phy));
                    continue;
                }
            }

            load_layer(
                commands,
                level_entity,
                layer_index,
                layer,
                &mut layer_grid,
                &ident_mapper,
                asset_server,
                ldtk_textures,
                &manager,
            );
        }

        if let Some(aabbs) = collider_aabbs {
            layer_grid.apply_physics_layer(
                commands,
                manager.physics_layer.as_ref().unwrap(),
                aabbs,
            );
        }
        layer_grid.apply_all(commands);
        ldtk_events.send(LdtkEvent::LevelLoaded(LevelEvent {
            identifier: level.identifier.clone(),
            iid: level.iid.clone(),
        }));

        return Some(LdtkLoadedLevel {
            identifier: level.identifier.clone(),
            iid: level.iid.clone(),
        });
    }

    None
}

fn load_texture(
    tileset: &TilesetDef,
    asset_server: &AssetServer,
    manager: &LdtkLevelManager,
) -> Option<TilemapTexture> {
    let Some(path) = tileset.rel_path.as_ref() else {
        return None;
    };

    let texture = asset_server.load(format!("{}{}", manager.asset_path_prefix, path));
    let desc = TilemapTextureDescriptor {
        size: UVec2 {
            x: tileset.px_wid as u32,
            y: tileset.px_hei as u32,
        },
        tile_size: UVec2 {
            x: tileset.tile_grid_size as u32,
            y: tileset.tile_grid_size as u32,
        },
        filter_mode: manager.filter_mode.into(),
    };
    Some(TilemapTexture {
        texture,
        desc,
        rotation: TilemapRotation::None,
    })
}

fn load_background(
    commands: &mut Commands,
    level_entity: Entity,
    level: &Level,
    translation: Vec2,
    level_px: UVec2,
    asset_server: &AssetServer,
    manager: &LdtkLevelManager,
) {
    let texture = match level.bg_rel_path.as_ref() {
        Some(path) => asset_server.load(format!("{}{}", manager.asset_path_prefix, path)),
        None => Handle::default(),
    };

    let bg_entity = commands
        .spawn(SpriteBundle {
            sprite: Sprite {
                color: level.bg_color.into(),
                custom_size: Some(level_px.as_vec2()),
                ..Default::default()
            },
            texture,
            transform: Transform::from_translation(
                (translation + level_px.as_vec2() / 2.)
                    .extend(manager.z_index as f32 - level.layer_instances.len() as f32 - 1.),
            ),
            ..Default::default()
        })
        .id();
    commands.entity(level_entity).add_child(bg_entity);
}

fn load_layer(
    commands: &mut Commands,
    level_entity: Entity,
    layer_index: usize,
    layer: &LayerInstance,
    layer_grid: &mut LdtkLayers,
    ident_mapper: &LdtkEntityRegistry,
    asset_server: &AssetServer,
    ldtk_textures: &LdtkTextures,
    manager: &LdtkLevelManager,
) {
    match layer.ty {
        LayerType::IntGrid | LayerType::AutoLayer => {
            for tile in layer.auto_layer_tiles.iter() {
                layer_grid.set(commands, layer_index, layer, tile);
            }
        }
        LayerType::Entities => {
            for entity_instance in layer.entity_instances.iter() {
                let phantom_entity = {
                    if let Some(m) = ident_mapper.get(&entity_instance.identifier) {
                        m
                    } else if !manager.ignore_unregistered_entities {
                        panic!(
                            "Could not find entity type with entity identifier: {}! \
                            You need to register it using App::register_ldtk_entity::<T>() first!",
                            entity_instance.identifier
                        );
                    } else {
                        return;
                    }
                };

                let mut new_entity = commands.spawn(EntityIid(entity_instance.iid.clone()));

                let mut fields = entity_instance
                    .field_instances
                    .iter()
                    .map(|field| (field.identifier.clone(), field.clone()))
                    .collect();
                phantom_entity.spawn(
                    level_entity,
                    &mut new_entity,
                    entity_instance,
                    &mut fields,
                    asset_server,
                    ldtk_textures,
                );
            }
        }
        LayerType::Tiles => {
            for tile in layer.grid_tiles.iter() {
                layer_grid.set(commands, layer_index, layer, tile);
            }
        }
    }
}

fn get_level_translation(ldtk_data: &LdtkJson, index: usize, manager: &LdtkLevelManager) -> Vec2 {
    let level = &ldtk_data.levels[index];
    match ldtk_data.world_layout.unwrap() {
        WorldLayout::GridVania | WorldLayout::Free => Vec2 {
            x: level.world_x as f32,
            y: (-level.world_y - level.px_hei) as f32,
        },
        WorldLayout::LinearHorizontal => {
            let mut offset = 0;
            for i in 0..index {
                offset += ldtk_data.levels[i].px_wid + manager.level_spacing.unwrap();
            }
            Vec2 {
                x: offset as f32,
                y: 0.,
            }
        }
        WorldLayout::LinearVertical => {
            let mut offset = 0;
            for i in 0..index {
                offset += ldtk_data.levels[i].px_hei + manager.level_spacing.unwrap();
            }
            Vec2 {
                x: 0.,
                y: -offset as f32,
            }
        }
    }
}
