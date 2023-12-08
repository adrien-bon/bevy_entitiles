use bevy::{
    ecs::component::Component,
    prelude::{Assets, Commands, Entity, IVec2, Image, ResMut, UVec2, Vec2},
    render::render_resource::TextureUsages,
};

use crate::{
    math::{aabb::AabbBox2d, FillArea},
    render::texture::TilemapTexture,
};

use super::{
    layer::UpdateLayer,
    tile::{TileBuilder, TileFlip, TileType},
};

pub struct TilemapBuilder {
    pub name: String,
    pub tile_type: TileType,
    pub size: UVec2,
    pub tile_render_scale: Vec2,
    pub tile_slot_size: Vec2,
    pub pivot: Vec2,
    pub render_chunk_size: u32,
    pub texture: Option<TilemapTexture>,
    pub translation: Vec2,
    pub z_order: i32,
    pub flip: u32,
}

impl TilemapBuilder {
    /// Create a new tilemap builder.
    pub fn new(ty: TileType, size: UVec2, tile_slot_size: Vec2, name: String) -> Self {
        Self {
            name,
            tile_type: ty,
            size,
            tile_render_scale: Vec2::ONE,
            tile_slot_size,
            pivot: Vec2 { x: 0.5, y: 0. },
            texture: None,
            render_chunk_size: 32,
            translation: Vec2::ZERO,
            z_order: 0,
            flip: 0,
        }
    }

    /// Override z order. Default is 10.
    /// The higher the value of z_order, the less likely to be covered.
    pub fn with_z_order(&mut self, z_order: i32) -> &mut Self {
        self.z_order = z_order;
        self
    }

    /// Override render chunk size. Default is 32.
    pub fn with_render_chunk_size(&mut self, size: u32) -> &mut Self {
        self.render_chunk_size = size;
        self
    }

    /// Override translation. Default is `Vec2::ZERO`.
    pub fn with_translation(&mut self, translation: Vec2) -> &mut Self {
        self.translation = translation;
        self
    }

    /// Assign a texture to the tilemap.
    pub fn with_texture(&mut self, texture: TilemapTexture) -> &mut Self {
        self.texture = Some(texture);
        self
    }

    /// Flip the uv of tiles. Use `TileFlip::Horizontal | TileFlip::Vertical` to flip both.
    pub fn with_flip(&mut self, flip: TileFlip) -> &mut Self {
        self.flip |= flip as u32;
        self
    }

    /// Override the pivot of the tile. Default is `[0.5, 0.]`.
    ///
    /// This can be useful when rendering `non_uniform` tilemaps. ( See the example )
    pub fn with_pivot(&mut self, pivot: Vec2) -> &mut Self {
        assert!(
            pivot.x >= 0. && pivot.x <= 1. && pivot.y >= 0. && pivot.y <= 1.,
            "pivot must be in range [0, 1]"
        );
        self.pivot = pivot;
        self
    }

    /// Override the tile render scale. Default is `Vec2::ONE` which means the render size is equal to the pixel size.
    pub fn with_tile_render_scale(&mut self, tile_render_scale: Vec2) -> &mut Self {
        self.tile_render_scale = tile_render_scale;
        self
    }

    /// Build the tilemap and spawn it into the world.
    /// You can modify the component and insert it back.
    pub fn build(&self, commands: &mut Commands) -> (Entity, Tilemap) {
        let mut entity = commands.spawn_empty();
        let tilemap = Tilemap {
            id: entity.id(),
            name: self.name.clone(),
            tile_render_scale: self.tile_render_scale,
            tile_type: self.tile_type,
            size: self.size,
            tiles: vec![None; (self.size.x * self.size.y) as usize],
            render_chunk_size: self.render_chunk_size,
            tile_slot_size: self.tile_slot_size,
            pivot: self.pivot,
            texture: self.texture.clone(),
            flip: self.flip,
            aabb: AabbBox2d::from_tilemap_builder(&self),
            translation: self.translation,
            z_order: self.z_order,
        };
        entity.insert((WaitForTextureUsageChange, tilemap.clone()));
        (entity.id(), tilemap)
    }
}

#[cfg(feature = "serializing")]
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct TilemapPattern {
    pub size: UVec2,
    pub tiles: Vec<Option<crate::serializing::SerializedTile>>,
}

#[cfg(feature = "serializing")]
impl TilemapPattern {
    pub fn get(&self, index: UVec2) -> Option<&crate::serializing::SerializedTile> {
        self.tiles[(index.y * self.size.x + index.x) as usize].as_ref()
    }
}

#[derive(Component, Clone)]
pub struct Tilemap {
    pub(crate) id: Entity,
    pub(crate) name: String,
    pub(crate) tile_type: TileType,
    pub(crate) size: UVec2,
    pub(crate) tile_render_scale: Vec2,
    pub(crate) tile_slot_size: Vec2,
    pub(crate) pivot: Vec2,
    pub(crate) render_chunk_size: u32,
    pub(crate) texture: Option<TilemapTexture>,
    pub(crate) tiles: Vec<Option<Entity>>,
    pub(crate) flip: u32,
    pub(crate) aabb: AabbBox2d,
    pub(crate) translation: Vec2,
    pub(crate) z_order: i32,
}

impl Tilemap {
    /// Get a tile.
    pub fn get(&self, index: UVec2) -> Option<Entity> {
        if self.is_out_of_tilemap_uvec(index) {
            return None;
        }

        self.get_unchecked(index)
    }

    pub(crate) fn get_unchecked(&self, index: UVec2) -> Option<Entity> {
        self.tiles[self.grid_index_to_linear_index(index)]
    }

    /// Set a tile.
    ///
    /// Overwrites the tile if it already exists.
    pub fn set(&mut self, commands: &mut Commands, index: UVec2, tile_builder: &TileBuilder) {
        if self.is_out_of_tilemap_uvec(index) {
            return;
        }

        self.set_unchecked(commands, index, tile_builder);
    }

    /// Overwrites all the tiles.
    pub fn set_all(&mut self, commands: &mut Commands, tiles: &Vec<Option<TileBuilder>>) {
        assert_eq!(
            tiles.len(),
            self.tiles.len(),
            "tiles length must be equal to the tilemap size"
        );

        for (i, tile) in tiles.iter().enumerate() {
            let index = UVec2 {
                x: i as u32 % self.size.x,
                y: i as u32 / self.size.x,
            };

            if let Some(t) = tile {
                self.set_unchecked(commands, index, t);
            } else {
                self.remove(commands, index);
            }
        }
    }

    pub(crate) fn set_unchecked(
        &mut self,
        commands: &mut Commands,
        index: UVec2,
        tile_builder: &TileBuilder,
    ) {
        let linear_index = self.grid_index_to_linear_index(index);
        if let Some(previous) = self.tiles[linear_index] {
            commands.entity(previous).despawn();
        }
        let new_tile = tile_builder.build(commands, index, self);
        self.tiles[linear_index] = Some(new_tile);
    }

    /// Update the `texture_index` of a layer for a tile.
    /// Leave `texture_index` to `None` if you want to remove the layer.
    ///
    /// If the target tile is a `AnimatedTile`, this will changed the layer of the whole animation.
    /// But this method does nothing if you try to remove the layer of a `AnimatedTile`
    /// or the target tile only has one layer left.
    ///
    /// Use `Tilemap::set()` if you want to change more.
    pub fn update(
        &mut self,
        commands: &mut Commands,
        index: UVec2,
        layer: usize,
        texture_index: Option<u32>,
    ) {
        if self.is_out_of_tilemap_uvec(index) || self.get(index).is_none() {
            return;
        }

        self.update_unchecked(commands, index, layer, texture_index);
    }

    pub(crate) fn update_unchecked(
        &mut self,
        commands: &mut Commands,
        index: UVec2,
        layer: usize,
        texture_index: Option<u32>,
    ) {
        commands
            .entity(self.get(index).unwrap())
            .insert(UpdateLayer {
                target: layer,
                value: texture_index.unwrap_or_default(),
                is_remove: texture_index.is_none(),
            });
    }

    /// Remove a tile.
    pub fn remove(&mut self, commands: &mut Commands, index: UVec2) {
        if self.is_out_of_tilemap_uvec(index) || self.get(index).is_none() {
            return;
        }

        self.remove_unchecked(commands, index);
    }

    pub(crate) fn remove_unchecked(&mut self, commands: &mut Commands, index: UVec2) {
        let index = self.grid_index_to_linear_index(index);
        commands.entity(self.tiles[index].unwrap()).despawn();
        self.tiles[index] = None;
    }

    /// Fill a rectangle area with tiles.
    pub fn fill_rect(
        &mut self,
        commands: &mut Commands,
        area: FillArea,
        tile_builder: &TileBuilder,
    ) {
        let builder = tile_builder.clone();
        for y in area.origin.y..=area.dest.y {
            for x in area.origin.x..=area.dest.x {
                self.set_unchecked(commands, UVec2 { x, y }, &builder);
            }
        }
    }

    /// Fill a rectangle area with tiles returned by `tile_builder`.
    ///
    /// Set `relative_index` to true if your function takes index relative to the area origin.
    pub fn fill_rect_custom(
        &mut self,
        commands: &mut Commands,
        area: FillArea,
        mut tile_builder: impl FnMut(UVec2) -> TileBuilder,
        relative_index: bool,
    ) {
        for y in area.origin.y..=area.dest.y {
            for x in area.origin.x..=area.dest.x {
                let builder = tile_builder(if relative_index {
                    UVec2::new(x, y) - area.origin
                } else {
                    UVec2::new(x, y)
                });
                self.set_unchecked(commands, UVec2 { x, y }, &builder);
            }
        }
    }

    /// This is a method for debugging.
    /// You can check if the uvs of tiles are correct.
    ///
    /// Fill the tilemap with tiles from the atlas in a row.
    /// Notice that the method **won't** check if the tilemap is wide enough.
    #[cfg(feature = "debug")]
    pub fn fill_atlas(&mut self, commands: &mut Commands) {
        if let Some(texture) = &self.texture {
            for x in 0..texture.desc.tiles_uv.len() as u32 {
                self.set_unchecked(commands, UVec2 { x, y: 0 }, &TileBuilder::new(x));
            }
        }
    }

    // TODO implement this
    // #[cfg(feature = "serializing")]
    // pub fn set_pattern(&mut self, commands: &mut Commands, pattern: TilemapPattern, origin: UVec2) {
    //     for y in 0..pattern.size.y {
    //         for x in 0..pattern.size.x {
    //             let index = UVec2 { x, y };
    //             if let Some(tile) = pattern.get(index) {
    //                 self.set(
    //                     commands,
    //                     index + origin,
    //                     &TileBuilder::from_serialized_tile(tile),
    //                 );
    //             }
    //         }
    //     }
    // }

    /// Simlar to `Tilemap::fill_rect()`.
    pub fn update_rect(
        &mut self,
        commands: &mut Commands,
        area: FillArea,
        layer: usize,
        texture_index: Option<u32>,
    ) {
        for y in area.origin.y..=area.dest.y {
            for x in area.origin.x..=area.dest.x {
                self.update_unchecked(commands, UVec2 { x, y }, layer, texture_index);
            }
        }
    }

    /// Simlar to `Tilemap::fill_rect_custom()`.
    pub fn update_rect_custom(
        &mut self,
        commands: &mut Commands,
        area: FillArea,
        layer: usize,
        mut texture_index: impl FnMut(UVec2) -> Option<u32>,
        relative_index: bool,
    ) {
        for y in area.origin.y..=area.dest.y {
            for x in area.origin.x..=area.dest.x {
                self.update_unchecked(
                    commands,
                    UVec2 { x, y },
                    layer,
                    texture_index(if relative_index {
                        UVec2 { x, y } - area.origin
                    } else {
                        UVec2 { x, y }
                    }),
                );
            }
        }
    }

    /// Remove the whole tilemap.
    pub fn delete(&mut self, commands: &mut Commands) {
        for tile in self.tiles.iter() {
            if let Some(tile) = tile {
                commands.entity(*tile).despawn();
            }
        }
        commands.entity(self.id).despawn();
    }

    /// Get the id of the tilemap.
    #[inline]
    pub fn id(&self) -> Entity {
        self.id
    }

    /// Get the name of the tilemap.
    #[inline]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Get the world position of the center of a slot.
    pub fn index_to_world(&self, index: UVec2) -> Vec2 {
        let index = index.as_vec2();
        match self.tile_type {
            TileType::Square => (index - self.pivot) * self.tile_slot_size + self.translation,
            TileType::IsometricDiamond => {
                (Vec2 {
                    x: (index.x - index.y - 1.),
                    y: (index.x + index.y),
                } / 2.
                    - self.pivot)
                    * self.tile_slot_size
                    + self.translation
            }
        }
    }

    #[inline]
    pub fn grid_index_to_linear_index(&self, index: UVec2) -> usize {
        (index.y * self.size.x + index.x) as usize
    }

    #[inline]
    pub fn is_out_of_tilemap_uvec(&self, index: UVec2) -> bool {
        index.x >= self.size.x || index.y >= self.size.y
    }

    #[inline]
    pub fn is_out_of_tilemap_ivec(&self, index: IVec2) -> bool {
        index.x < 0 || index.y < 0 || index.x >= self.size.x as i32 || index.y >= self.size.y as i32
    }

    #[inline]
    pub fn get_tile_convex_hull(&self, index: UVec2) -> [Vec2; 4] {
        let offset = self.index_to_world(index);
        let (x, y) = (self.tile_slot_size.x, self.tile_slot_size.y);
        match self.tile_type {
            TileType::Square => [
                (Vec2 { x: 0., y: 0. } + offset).into(),
                (Vec2 { x: 0., y } + offset).into(),
                (Vec2 { x, y } + offset).into(),
                (Vec2 { x, y: 0. } + offset).into(),
            ],
            TileType::IsometricDiamond => [
                (Vec2 { x: 0., y: y / 2. } + offset).into(),
                (Vec2 { x: x / 2., y } + offset).into(),
                (Vec2 { x, y: y / 2. } + offset).into(),
                (Vec2 { x: x / 2., y: 0. } + offset).into(),
            ],
        }
    }

    /// Bevy doesn't set the `COPY_SRC` usage for images by default, so we need to do it manually.
    pub(crate) fn set_usage(
        &mut self,
        commands: &mut Commands,
        image_assets: &mut ResMut<Assets<Image>>,
    ) {
        let Some(texture) = &self.texture else {
            commands
                .entity(self.id)
                .remove::<WaitForTextureUsageChange>();
            return;
        };

        let Some(image) = image_assets.get(&texture.clone_weak()) else {
            return;
        };

        if !image
            .texture_descriptor
            .usage
            .contains(TextureUsages::COPY_SRC)
        {
            image_assets
                .get_mut(&texture.clone_weak())
                .unwrap()
                .texture_descriptor
                .usage
                .set(TextureUsages::COPY_SRC, true);
        }

        commands
            .entity(self.id)
            .remove::<WaitForTextureUsageChange>();
    }
}

#[derive(Component)]
pub struct WaitForTextureUsageChange;
