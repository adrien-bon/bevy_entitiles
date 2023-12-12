use bevy::{
    core_pipeline::core_2d::Transparent2d,
    prelude::{Commands, Component, Entity, Msaa, Query, Res, ResMut},
    render::{
        render_phase::{DrawFunctions, RenderPhase},
        render_resource::{BindGroup, BindGroupEntry, PipelineCache, SpecializedRenderPipelines},
        renderer::RenderDevice,
        view::ViewUniforms,
    },
    utils::{nonmax::NonMaxU32, FloatOrd},
};

use super::{
    draw::{DrawTilemap, DrawTilemapPureColor},
    extract::ExtractedTilemap,
    pipeline::{EntiTilesPipeline, EntiTilesPipelineKey},
    resources::TilemapBindGroups,
    texture::TilemapTexturesStorage,
    uniform::TilemapUniformsStorage,
};

#[derive(Component)]
pub struct TileViewBindGroup {
    pub value: BindGroup,
}

pub fn queue(
    mut commands: Commands,
    mut views_query: Query<(Entity, &mut RenderPhase<Transparent2d>)>,
    tilemaps_query: Query<(Entity, &ExtractedTilemap)>,
    pipeline_cache: Res<PipelineCache>,
    draw_functions: Res<DrawFunctions<Transparent2d>>,
    mut sp_entitiles_pipeline: ResMut<SpecializedRenderPipelines<EntiTilesPipeline>>,
    entitile_pipeline: Res<EntiTilesPipeline>,
    view_uniforms: Res<ViewUniforms>,
    render_device: Res<RenderDevice>,
    mut bind_groups: ResMut<TilemapBindGroups>,
    textures_storage: Res<TilemapTexturesStorage>,
    msaa: Res<Msaa>,
    mut tilemap_uniform_storage: ResMut<TilemapUniformsStorage>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        return;
    };

    for (view_entity, mut transparent_phase) in views_query.iter_mut() {
        commands.entity(view_entity).insert(TileViewBindGroup {
            value: render_device.create_bind_group(
                "tilemap_view_bind_group",
                &entitile_pipeline.view_layout,
                &[BindGroupEntry {
                    binding: 0,
                    resource: view_binding.clone(),
                }],
            ),
        });

        let mut tilemaps = tilemaps_query.iter().collect::<Vec<_>>();
        tilemaps.sort_by(|lhs, rhs| lhs.1.z_order.cmp(&rhs.1.z_order));

        for (entity, tilemap) in tilemaps_query.iter() {
            bind_groups.queue_uniforms(
                &tilemap,
                &render_device,
                &mut tilemap_uniform_storage,
                &entitile_pipeline,
            );

            let (is_pure_color, is_uniform) = bind_groups.queue_textures(
                &tilemap,
                &render_device,
                &textures_storage,
                &entitile_pipeline,
            );

            let pipeline = sp_entitiles_pipeline.specialize(
                &pipeline_cache,
                &entitile_pipeline,
                EntiTilesPipelineKey {
                    msaa: msaa.samples(),
                    map_type: tilemap.tile_type,
                    is_pure_color,
                    is_uniform,
                },
            );

            let draw_function = {
                if is_pure_color {
                    draw_functions
                        .read()
                        .get_id::<DrawTilemapPureColor>()
                        .unwrap()
                } else {
                    draw_functions.read().get_id::<DrawTilemap>().unwrap()
                }
            };

            transparent_phase.add(Transparent2d {
                sort_key: FloatOrd(tilemap.z_order as f32),
                entity,
                pipeline,
                draw_function,
                batch_range: 0..1,
                dynamic_offset: NonMaxU32::new(0),
            });
        }
    }
}
