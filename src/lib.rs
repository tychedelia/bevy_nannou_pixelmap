mod artnet;
mod compute;
mod material;

use bevy::asset::load_internal_asset;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::ecs::entity::EntityHashMap;
use bevy::ecs::query::{QueryItem, ROQueryItem};
use bevy::ecs::system::lifetimeless::Read;
use bevy::ecs::system::SystemParamItem;
use bevy::pbr::{
    DrawMesh, MeshViewBindGroup, SetMaterialBindGroup, SetMeshBindGroup, SetMeshViewBindGroup,
    ViewFogUniformOffset, ViewLightProbesUniformOffset, ViewLightsUniformOffset,
    ViewScreenSpaceReflectionsUniformOffset,
};
use bevy::render::camera::RenderTarget;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_graph::{
    NodeRunError, RenderGraphApp, RenderGraphContext, ViewNode, ViewNodeRunner,
};
use bevy::render::render_phase::{
    AddRenderCommand, PhaseItem, RenderCommand, RenderCommandResult, SetItemPipeline,
    TrackedRenderPass,
};
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only, texture_2d, uniform_buffer,
};
use bevy::render::texture::{BevyDefault, GpuImage};
use bevy::render::view::{check_visibility, ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms, VisibleEntities};
use bevy::render::Extract;
use bevy::window::{PrimaryWindow, WindowRef};
use bevy::{
    prelude::*,
    render::{
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{binding_types::storage_buffer, *},
        renderer::{RenderContext, RenderDevice},
        Render, RenderApp, RenderSet,
    },
};
use crossbeam_channel::{Receiver, Sender};

#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<u32>>);
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<u32>>);

pub struct NannouArtnetPlugin;

const BUFFER_SIZE: usize = 256 * 4;
const COMPUTE_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(966169125558327);
const MATERIAL_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(116169934631328);

impl Plugin for NannouArtnetPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            COMPUTE_SHADER_HANDLE,
            "compute.wgsl",
            Shader::from_wgsl
        );
        load_internal_asset!(
            app,
            MATERIAL_SHADER_HANDLE,
            "material.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins((
            MaterialPlugin::<LedMaterial>::default(),
        ))
        .add_systems(PostUpdate, check_visibility::<With<LedZone>>)
        .add_systems(First, spawn_screen_textures);
    }

    fn finish(&self, app: &mut App) {
        let (s, r) = crossbeam_channel::unbounded();
        app.insert_resource(MainWorldReceiver(r));

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .insert_resource(RenderWorldSender(s))
            .init_resource::<ComputePipeline>()
            .init_resource::<WorkItemBuffers>()
            .init_resource::<GpuOutputBuffers>()
            .init_resource::<CpuReadbackBuffers>()
            .init_resource::<ComputeBindGroups>()
            .add_systems(
                Render,
                (
                    prepare_bind_groups.in_set(RenderSet::PrepareBindGroups),
                    map_and_read_buffer.after(RenderSet::Render),
                ),
            );

        render_app
            .add_render_command::<Transparent3d, DrawLedMaterial>()
            .add_render_graph_node::<ViewNodeRunner<ComputeNode>>(Core3d, ComputeNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::StartMainPass, ComputeNodeLabel, Node3d::EndMainPass),
            );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct LedMaterial {
    #[uniform(0)]
    pub color: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    pub color_texture: Option<Handle<Image>>,
}

impl Material for LedMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(MATERIAL_SHADER_HANDLE)
    }
}

type DrawLedMaterial = (
    SetItemPipeline,
    SetMeshBindGroup<1>,
    SetMaterialBindGroup<LedMaterial, 2>,
    SetBufferBindGroup<3>,
    SetMeshViewBindGroup<0>,
    DrawMesh,
);

pub struct SetBufferBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetBufferBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        _item: &P,
        _view_query: ROQueryItem<'w, Self::ViewQuery>,
        _entity: Option<()>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        RenderCommandResult::Success
    }
}

fn spawn_screen_textures(
    mut commands: Commands,
    camera_q: Query<(Entity, &Camera), Without<ScreenTextureCamera>>,
    mut images: ResMut<Assets<Image>>,
    windows_q: Query<&Window>,
    primary_window_q: Query<&Window, With<PrimaryWindow>>,
) {
    for (entity, cam) in camera_q.iter() {
        let RenderTarget::Window(window_target) = cam.target else {
            panic!("Camera target should be a window");
        };
        let window = match window_target {
            WindowRef::Primary => {
                primary_window_q.single()
            }
            WindowRef::Entity(window) => {
                windows_q.get(window).unwrap()
            }
        };

        let size = Extent3d {
            width: window.physical_width(),
            height: window.physical_height(),
            ..default()
        };
        let mut image = Image {
            texture_descriptor: TextureDescriptor {
                label: None,
                size,
                dimension: TextureDimension::D2,
                format: if cam.hdr {
                    ViewTarget::TEXTURE_FORMAT_HDR
                } else {
                    TextureFormat::bevy_default()
                },
                mip_level_count: 1,
                sample_count: 1,
                usage: TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_DST
                    | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
            ..default()
        };

        image.resize(size);
        let image = images.add(image);

        commands.spawn((
            Camera3dBundle {
                camera: Camera {
                    order: cam.order - 1, // always render before the camera
                    target: RenderTarget::Image(image.clone()),
                    ..default()
                },
                ..default()
            },
            ScreenTextureCamera,
        ));

        commands.entity(entity).insert(ScreenTexture(image));
    }
}

fn receive(receiver: Res<MainWorldReceiver>) {
    if let Ok(data) = receiver.try_recv() {
        println!("Received data from render world: {data:?}");
    }
}

#[derive(Resource, Deref, DerefMut, Default)]
struct WorkItemBuffers(EntityHashMap<BufferVec<LedWorkItem>>);

#[derive(Resource, Deref, DerefMut, Default)]
struct GpuOutputBuffers(EntityHashMap<UninitBufferVec<LinearRgba>>);

#[derive(Resource, Deref, DerefMut, Default)]
struct CpuReadbackBuffers(EntityHashMap<RawBufferVec<LinearRgba>>);

#[derive(Component, ExtractComponent, Clone)]
struct ScreenTextureCamera;

#[derive(Component, ExtractComponent, Clone)]
pub struct ScreenTexture(pub Handle<Image>);

#[derive(Resource, Deref, DerefMut, Default)]
struct ComputeBindGroups(EntityHashMap<BindGroup>);

#[derive(Component)]
pub struct LedZone {
    pub count: u32,
    pub position: Vec2,
    pub size: Vec2,
}

#[derive(Component, Deref, DerefMut, Default)]
pub struct ViewLeds(pub Vec<LedWorkItem>);

fn extract_leds(
    mut commands: Commands,
    views: Extract<Query<(Entity, &ExtractedView, &VisibleEntities)>>,
    leds: Extract<Query<&LedZone>>,
) {
    for (view_entity, view, visible_entities) in views.iter() {
        let mut view_leds = ViewLeds::default();
        for visible in visible_entities.iter::<With<LedZone>>() {
            if let Ok(led) = leds.get(*visible) {
                view_leds.push(LedWorkItem {
                    start_index: 0,
                    num_leds: led.count,
                    num_samples: 1,
                    total_area_size: led.size,
                    area_position: led.position,
                });
            }
        }
    }
}

fn prepare_buffers(
    mut work_items: ResMut<WorkItemBuffers>,
    mut gpu_output: ResMut<GpuOutputBuffers>,
    mut cpu_readback: ResMut<CpuReadbackBuffers>,
    mut views: Query<(Entity, &mut ViewLeds)>,
) {
    for (entity, mut leds) in &mut views {
        let (Some(mut work_items), Some(mut gpu_output)) =
            (work_items.get_mut(&entity), gpu_output.get_mut(&entity))
        else {
            continue;
        };

        work_items.clear();
        gpu_output.clear();

        for led in leds.drain(..) {
            let mut offset_index = gpu_output.len();
            for _ in 0..led.num_leds {
                offset_index += gpu_output.add();
            }

            work_items.push(led);
        }
    }
}

fn prepare_bind_groups(
    views: Query<(Entity, &ExtractedView, &ScreenTexture)>,
    view_uniforms: Res<ViewUniforms>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    pipeline: Res<ComputePipeline>,
    render_device: Res<RenderDevice>,
    work_items: Res<WorkItemBuffers>,
    gpu_output: Res<GpuOutputBuffers>,
    mut bind_groups: ResMut<ComputeBindGroups>,
) {
    for (entity, view, screen_texture) in &views {
        let screen_texture = gpu_images
            .get(&screen_texture.0)
            .expect("image should exist");

        let Some(view_uniforms_binding) = view_uniforms.uniforms.binding() else {
            continue;
        };

        let bind_group = render_device.create_bind_group(
            None,
            &pipeline.layout,
            &BindGroupEntries::sequential((
                screen_texture.texture_view.into_binding(),
                gpu_output
                    .get(&entity)
                    .expect("buffer should exist")
                    .buffer()
                    .expect("buffer should exist")
                    .as_entire_binding(),
                work_items
                    .get(&entity)
                    .expect("buffer should exist")
                    .buffer()
                    .expect("buffer should exist")
                    .as_entire_binding(),
                view_uniforms_binding.into_binding(),
            )),
        );

        bind_groups.insert(entity, bind_group);
    }
}

#[derive(Resource)]
struct ComputePipeline {
    layout: BindGroupLayout,
    pipeline: CachedComputePipelineId,
}

#[derive(Component, ShaderType, Clone)]
pub struct LedWorkItem {
    start_index: u32,
    num_leds: u32,
    num_samples: u32,
    total_area_size: Vec2,
    area_position: Vec2,
}

impl FromWorld for ComputePipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();
        let layout = render_device.create_bind_group_layout(
            None,
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    storage_buffer::<LinearRgba>(false),
                    storage_buffer_read_only::<LedWorkItem>(false),
                    uniform_buffer::<ViewUniform>(true)
                ),
            ),
        );
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("led_material_compute".into()),
            layout: vec![layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: COMPUTE_SHADER_HANDLE.clone(),
            shader_defs: Vec::new(),
            entry_point: "main".into(),
        });
        ComputePipeline { layout, pipeline }
    }
}

fn map_and_read_buffer(render_device: Res<RenderDevice>, sender: Res<RenderWorldSender>) {
    // let buffer_slice = buffers
    //     .cpu_buffers
    //     .buffer()
    //     .expect("buffer should exist")
    //     .slice(..);
    // let (s, r) = crossbeam_channel::unbounded::<()>();
    //
    // buffer_slice.map_async(MapMode::Read, move |r| match r {
    //     Ok(_) => s.send(()).expect("Failed to send map update"),
    //     Err(err) => panic!("Failed to map buffer {err}"),
    // });
    //
    // render_device.poll(Maintain::wait()).panic_on_timeout();
    //
    // r.recv().expect("Failed to receive the map_async message");
    //
    // {
    //     let buffer_view = buffer_slice.get_mapped_range();
    //     let data = buffer_view
    //         .chunks(std::mem::size_of::<u32>())
    //         .map(|chunk| u32::from_ne_bytes(chunk.try_into().expect("should be a u32")))
    //         .collect::<Vec<u32>>();
    //     sender
    //         .send(data)
    //         .expect("Failed to send data to main world");
    // }
    //
    // // We need to make sure all `BufferView`'s are dropped before we do what we're about
    // // to do.
    // // Unmap so that we can copy to the staging buffer in the next iteration.
    // buffers
    //     .cpu_buffer
    //     .buffer()
    //     .expect("buffer should exist")
    //     .unmap();
}

/// Label to identify the node in the render graph
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ComputeNodeLabel;

/// The node that will execute the compute shader
#[derive(Default)]
struct ComputeNode {}
impl ViewNode for ComputeNode {
    type ViewQuery = (Entity, Read<ViewUniformOffset>);

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (view_entity, view_uniform): QueryItem<'w, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipeline = world.resource::<ComputePipeline>();
        let bind_groups = world.resource::<ComputeBindGroups>();
        let work_items = world.resource::<WorkItemBuffers>();
        let gpu_output = world.resource::<GpuOutputBuffers>();
        let cpu_readback = world.resource::<CpuReadbackBuffers>();
        let Some(work_items) = work_items.get(&view_entity) else {
            return Ok(());
        };
        let Some(gpu_buffer) = gpu_output.get(&view_entity) else {
            return Ok(());
        };
        let Some(cpu_buffer) = cpu_readback.get(&view_entity) else {
            return Ok(());
        };

        if let Some(init_pipeline) = pipeline_cache.get_compute_pipeline(pipeline.pipeline) {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("led-material-compute-pass"),
                        ..default()
                    });

            pass.set_bind_group(
                0,
                bind_groups.get(&view_entity).as_ref().unwrap().clone(),
                &[view_uniform.offset],
            );
            pass.set_pipeline(init_pipeline);
            pass.dispatch_workgroups(work_items.capacity() as u32, 1, 1);
        }

        render_context.command_encoder().copy_buffer_to_buffer(
            &gpu_buffer.buffer().expect("buffer should exist"),
            0,
            &cpu_buffer.buffer().expect("buffer should exist"),
            0,
            (gpu_buffer.len() * std::mem::size_of::<LinearRgba>()) as u64,
        );

        Ok(())
    }
}
