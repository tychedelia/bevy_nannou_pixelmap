use crate::artnet::{ArtNetPlugin, ArtNetServer};
use artnet_protocol::{ArtCommand, Output, PortAddress};
use bevy::asset::load_internal_asset;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::core_3d::{Transparent3d, CORE_3D_DEPTH_FORMAT};
use bevy::core_pipeline::fullscreen_vertex_shader::fullscreen_shader_vertex_state;
use bevy::ecs::entity::EntityHashMap;
use bevy::ecs::query::{QueryItem, ROQueryItem};
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::SystemParamItem;
use bevy::pbr::{
    DrawMesh, MeshPipeline, MeshPipelineKey, MeshPipelineViewLayoutKey, MeshUniform,
    MeshViewBindGroup, PreparedMaterial, RenderMeshInstances, SetMeshBindGroup,
    SetMeshViewBindGroup, ViewFogUniformOffset, ViewLightProbesUniformOffset,
    ViewLightsUniformOffset, ViewScreenSpaceReflectionsUniformOffset,
};
use bevy::render::camera::RenderTarget;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::extract_instances::ExtractInstancesPlugin;
use bevy::render::mesh::{GpuMesh, MeshVertexBufferLayoutRef};
use bevy::render::render_asset::{RenderAssetPlugin, RenderAssets};
use bevy::render::render_graph::{
    NodeRunError, RenderGraphApp, RenderGraphContext, ViewNode, ViewNodeRunner,
};
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only, texture_2d, uniform_buffer,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::{BevyDefault, FallbackImage, GpuImage};
use bevy::render::view::{
    check_visibility, ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms,
    VisibleEntities, WithMesh,
};
use bevy::render::Extract;
use bevy::window::{PrimaryWindow, WindowClosing, WindowRef};
use bevy::{
    prelude::*,
    render::{
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{binding_types::storage_buffer, *},
        renderer::{RenderContext, RenderDevice},
        Render, RenderApp, RenderSet,
    },
};
use bevy_mod_picking::DefaultPickingPlugins;
use crossbeam_channel::{Receiver, Sender};
use std::borrow::Cow;

mod artnet;
mod compute;
mod material;
mod sacn;

pub struct NannouArtnetPlugin;

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
            ArtNetPlugin,
            DefaultPickingPlugins,
            ExtractComponentPlugin::<LedZone>::default(),
            ExtractComponentPlugin::<ScreenTexture>::default(),
            ExtractComponentPlugin::<ScreenTextureCamera>::default(),
        ))
        .add_systems(Update, receive)
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
                    queue_led_material.in_set(RenderSet::QueueMeshes),
                    queue_leds.in_set(RenderSet::Queue),
                    prepare_buffers.in_set(RenderSet::PrepareResources),
                    prepare_bind_groups.in_set(RenderSet::PrepareBindGroups),
                    map_and_read_buffer.after(RenderSet::Render),
                ),
            )
            .add_render_command::<Transparent3d, DrawLedMaterial>()
            .init_resource::<SpecializedRenderPipelines<LedMaterialPipeline>>()
            .init_resource::<LedMaterialPipeline>()
            .add_render_graph_node::<ViewNodeRunner<ComputeNode>>(Core3d, ComputeNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::StartMainPass,
                    ComputeNodeLabel,
                    Node3d::MainOpaquePass,
                ),
            );
    }
}
#[derive(Resource, Deref)]
struct MainWorldReceiver(Receiver<Vec<f32>>);
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<Vec<f32>>);

type DrawLedMaterial = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMaterialBindGroup<1>,
    DrawMaterial,
);

pub struct SetMaterialBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetMaterialBindGroup<I> {
    type Param = ();
    type ViewQuery = (Entity);
    type ItemQuery = (Read<LedMaterialBindGroup>);

    #[inline]
    fn render<'w>(
        _item: &P,
        (view_entity): ROQueryItem<'w, Self::ViewQuery>,
        (bind_group): Option<ROQueryItem<'w, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Failure;
        };

        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

pub struct DrawMaterial;
impl<P: PhaseItem> RenderCommand<P> for DrawMaterial {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.draw(0..4, 0..1);
        RenderCommandResult::Success
    }
}

fn spawn_screen_textures(
    mut commands: Commands,
    camera_q: Query<
        (Entity, &Camera, &Transform),
        (
            With<Camera3d>,
            Without<ScreenTexture>,
            Without<ScreenTextureCamera>,
        ),
    >,
    mut images: ResMut<Assets<Image>>,
    windows_q: Query<&Window>,
    primary_window_q: Query<&Window, With<PrimaryWindow>>,
) {
    for (entity, cam, cam_transform) in camera_q.iter() {
        let RenderTarget::Window(window_target) = cam.target else {
            panic!("Camera target should be a window");
        };
        let window = match window_target {
            WindowRef::Primary => primary_window_q.single(),
            WindowRef::Entity(window) => windows_q.get(window).unwrap(),
        };

        let size = Extent3d {
            width: (window.physical_width() as f32 * window.scale_factor()) as u32,
            height: (window.physical_height() as f32 * window.scale_factor()) as u32,
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

        let screen_texture_camera = commands
            .spawn((
                Camera3dBundle {
                    transform: cam_transform.clone(),
                    camera: Camera {
                        order: cam.order - 1, // always render before the camera
                        target: RenderTarget::Image(image.clone()),
                        ..default()
                    },
                    ..default()
                },
                ScreenTextureCamera,
            ))
            .id();

        info!("Spawning screen texture camera {screen_texture_camera} for camera {entity}");
        commands.entity(entity).insert((
            ScreenTexture(image),
            ScreenTextureCameraRef(screen_texture_camera),
        ));
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

#[derive(Component, Clone)]
struct ScreenTextureCameraRef(pub Entity);

#[derive(Component, ExtractComponent, Clone)]
pub struct ScreenTexture(pub Handle<Image>);

#[derive(Resource, Deref, DerefMut, Default)]
struct ComputeBindGroups(EntityHashMap<BindGroup>);

#[derive(Component, ExtractComponent, Clone)]
pub struct LedZone {
    pub count: u32,
    pub rotation: f32,
    pub position: Vec2,
    pub size: Vec2,
}

#[derive(AsBindGroup, Debug, Clone)]
pub struct LedMaterial {
    #[uniform(0)]
    pub offset: u32,
    #[uniform(0)]
    pub rotation: f32,
    #[uniform(0)]
    pub count: u32,
    #[uniform(0)]
    pub position: Vec2,
    #[uniform(0)]
    pub size: Vec2,
    #[storage(1, read_only, buffer)]
    pub color_buffer: Buffer,
}

#[derive(Component)]
pub struct LedMaterialBindGroup(pub BindGroup);

#[derive(Component, Default, Debug)]
pub struct ViewLeds {
    work_items: EntityHashMap<LedWorkItem>,
    materials: EntityHashMap<LedMaterial>,
}

fn queue_leds(
    mut commands: Commands,
    views: Query<(Entity, &ExtractedView, &VisibleEntities), Without<ScreenTextureCamera>>,
    gpu_output: Res<GpuOutputBuffers>,
    leds: Query<&LedZone>,
) {
    for (view_entity, view, visible_entities) in views.iter() {
        let mut view_leds = ViewLeds::default();
        for visible in visible_entities.iter::<With<LedZone>>() {
            let mut idx = 0;
            if let Ok(led) = leds.get(*visible) {
                view_leds.work_items.insert(
                    *visible,
                    LedWorkItem {
                        start_index: 0,
                        rotation: led.rotation,
                        num_leds: led.count,
                        num_samples: 10,
                        total_area_size: led.size,
                        area_position: led.position,
                    },
                );

                let Some(gpu_output) = gpu_output.get(&view_entity) else {
                    continue;
                };
                let Some(buffer) = gpu_output.buffer() else {
                    continue;
                };

                view_leds.materials.insert(
                    *visible,
                    LedMaterial {
                        offset: idx as u32,
                        rotation: led.rotation,
                        count: led.count,
                        position: led.position,
                        size: led.size,
                        color_buffer: buffer.clone(),
                    },
                );

                idx += 1;
            }
        }
        commands.entity(view_entity).insert(view_leds);
    }
}

fn prepare_buffers(
    mut work_items: ResMut<WorkItemBuffers>,
    mut gpu_output: ResMut<GpuOutputBuffers>,
    mut cpu_readback: ResMut<CpuReadbackBuffers>,
    mut views: Query<(Entity, &mut ViewLeds), With<ExtractedView>>,
) {
    for (entity, mut leds) in &mut views {
        let (mut work_items, mut gpu_output, cpu_readback) = (
            work_items
                .entry(entity)
                .or_insert_with(|| BufferVec::new(BufferUsages::COPY_DST | BufferUsages::STORAGE)),
            gpu_output.entry(entity).or_insert_with(|| {
                UninitBufferVec::new(BufferUsages::STORAGE | BufferUsages::COPY_SRC)
            }),
            cpu_readback.entry(entity).or_insert_with(|| {
                RawBufferVec::new(BufferUsages::MAP_READ | BufferUsages::COPY_DST)
            }),
        );

        work_items.clear();
        gpu_output.clear();
        cpu_readback.clear();

        for (_, led) in leds.work_items.drain() {
            let mut offset_index = gpu_output.len();
            for _ in 0..led.num_leds {
                offset_index += gpu_output.add();
            }

            work_items.push(led);
        }
    }
}

fn prepare_bind_groups(
    mut commands: Commands,
    views: Query<(Entity, &ScreenTexture, &ViewLeds), With<ExtractedView>>,
    view_uniforms: Res<ViewUniforms>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    compute_pipeline: Res<ComputePipeline>,
    material_pipeline: Res<LedMaterialPipeline>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    fallback_img: Res<FallbackImage>,
    mut work_items: ResMut<WorkItemBuffers>,
    mut gpu_output: ResMut<GpuOutputBuffers>,
    mut cpu_readback: ResMut<CpuReadbackBuffers>,
    mut compute_bind_groups: ResMut<ComputeBindGroups>,
) {
    for (entity, screen_texture, view_leds) in &views {
        let screen_texture = gpu_images
            .get(&screen_texture.0)
            .expect("image should exist");

        let Some(view_uniforms_binding) = view_uniforms.uniforms.binding() else {
            continue;
        };
        let Some(mut gpu_output) = gpu_output.get_mut(&entity) else {
            continue;
        };
        let Some(work_items) = work_items.get_mut(&entity) else {
            continue;
        };
        let Some(mut cpu_readback) = cpu_readback.get_mut(&entity) else {
            continue;
        };
        if gpu_output.is_empty() || work_items.is_empty() {
            continue;
        }

        gpu_output.write_buffer(&render_device);
        work_items.write_buffer(&render_device, &render_queue);
        cpu_readback.reserve(gpu_output.len(), &render_device);

        let bind_group = render_device.create_bind_group(
            Some("compute_bind_group"),
            &compute_pipeline.layout,
            &BindGroupEntries::sequential((
                screen_texture.texture_view.into_binding(),
                gpu_output
                    .buffer()
                    .expect("buffer should exist")
                    .as_entire_binding(),
                work_items
                    .buffer()
                    .expect("buffer should exist")
                    .as_entire_binding(),
                view_uniforms_binding.into_binding(),
            )),
        );

        compute_bind_groups.insert(entity, bind_group);

        for (entity, material) in view_leds.materials.iter() {
            let bind_group = material
                .as_bind_group(
                    &material_pipeline.layout,
                    &render_device,
                    &gpu_images,
                    &fallback_img,
                )
                .expect("Failed to create bind group");
            commands
                .entity(*entity)
                .insert(LedMaterialBindGroup(bind_group.bind_group.clone()));
        }
    }
}

#[derive(Resource)]
struct ComputePipeline {
    layout: BindGroupLayout,
    pipeline: CachedComputePipelineId,
}

#[derive(Component, ShaderType, Clone, Debug)]
pub struct LedWorkItem {
    start_index: u32,
    rotation: f32,
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
                    uniform_buffer::<ViewUniform>(true),
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

fn f32_to_u8(value: f32) -> u8 {
    // Clamp the value to the range [0.0, 1.0] to ensure valid u8 conversion
    let clamped_value = value.clamp(0.0, 1.0);
    // Scale the clamped value to the range [0, 255] and cast to u8
    (clamped_value * 255.0).round() as u8
}

fn f32_vec_to_u8_vec(values: Vec<f32>) -> Vec<u8> {
    values.iter().map(|&v| f32_to_u8(v)).collect()
}

fn receive(receiver: Res<MainWorldReceiver>, artnet_server: ResMut<ArtNetServer>) {
    if let Ok(data) = receiver.try_recv() {
        // info!("data received: {data:?}");

        // artnet_server.send(ArtCommand::Output(Output {
        //     data: f32_vec_to_u8_vec(data).into(),
        //     ..default()
        // }))
    }
}
fn map_and_read_buffer(
    render_device: Res<RenderDevice>,
    cpu_readback_buffers: Res<CpuReadbackBuffers>,
    sender: ResMut<RenderWorldSender>,
    views_q: Query<(Entity), (With<ScreenTexture>, With<ExtractedView>)>,
) {
    for entity in views_q.iter() {
        let Some(buffer) = cpu_readback_buffers.get(&entity) else {
            continue;
        };
        let Some(buffer) = buffer.buffer() else {
            continue;
        };

        let buffer_slice = buffer.slice(..);
        let (s, r) = crossbeam_channel::unbounded::<()>();

        buffer_slice.map_async(MapMode::Read, move |r| match r {
            Ok(_) => s.send(()).expect("Failed to send map update"),
            Err(err) => panic!("Failed to map buffer {err}"),
        });

        render_device.poll(Maintain::wait()).panic_on_timeout();

        r.recv().expect("Failed to receive the map_async message");

        {
            let buffer_view = buffer_slice.get_mapped_range();
            let data = buffer_view
                .chunks(size_of::<f32>())
                .map(|chunk| f32::from_ne_bytes(chunk.try_into().expect("should be a u32")))
                .collect::<Vec<f32>>();
            let _ = sender.send(data);
        }
        buffer.unmap();
    }
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
        let Some(bind_group) = bind_groups.get(&view_entity) else {
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

            pass.set_bind_group(0, bind_group, &[view_uniform.offset]);
            pass.set_pipeline(init_pipeline);
            pass.dispatch_workgroups(work_items.capacity() as u32, 1, 1);
        }

        render_context.command_encoder().copy_buffer_to_buffer(
            &gpu_buffer.buffer().expect("buffer should exist"),
            0,
            &cpu_buffer.buffer().expect("buffer should exist"),
            0,
            (gpu_buffer.len() * size_of::<LinearRgba>()) as u64,
        );

        Ok(())
    }
}

#[derive(Resource)]
struct LedMaterialPipeline {
    mesh_pipeline: MeshPipeline,
    layout: BindGroupLayout,
}

impl FromWorld for LedMaterialPipeline {
    fn from_world(world: &mut World) -> Self {
        let mesh_pipeline = world.resource::<MeshPipeline>();
        let render_device = world.resource::<RenderDevice>();
        let mut layout_entries = LedMaterial::bind_group_layout_entries(render_device);
        layout_entries[0].visibility = ShaderStages::VERTEX | ShaderStages::FRAGMENT;
        let layout = render_device.create_bind_group_layout(LedMaterial::label(), &layout_entries);
        LedMaterialPipeline {
            mesh_pipeline: mesh_pipeline.clone(),
            layout,
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct LedMaterialPipelineKey {
    hdr: bool,
    samples: u32,
}

impl SpecializedRenderPipeline for LedMaterialPipeline {
    type Key = LedMaterialPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let view_layout = self
            .mesh_pipeline
            .view_layouts
            .get_view_layout(MeshPipelineViewLayoutKey::from(
                MeshPipelineKey::from_msaa_samples(key.samples),
            ))
            .clone();
        let layout = self.layout.clone();
        RenderPipelineDescriptor {
            label: Some("led_material_pipeline".into()),
            layout: vec![view_layout, layout],
            vertex: VertexState {
                shader: MATERIAL_SHADER_HANDLE,
                shader_defs: vec![],
                entry_point: Cow::Borrowed("vertex"),
                buffers: vec![],
            },
            fragment: Some(FragmentState {
                shader: MATERIAL_SHADER_HANDLE.clone(),
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState {
                    format: if key.hdr {
                        ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    },
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: bevy::render::render_resource::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: MultisampleState {
                count: key.samples,
                ..default()
            },
            push_constant_ranges: vec![],
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_led_material(
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    custom_pipeline: Res<LedMaterialPipeline>,
    msaa: Res<Msaa>,
    mut pipelines: ResMut<SpecializedRenderPipelines<LedMaterialPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    materials: Query<Entity, With<LedZone>>,
    mut transparent_render_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut views: Query<(Entity, &ExtractedView), Without<ScreenTextureCamera>>,
) {
    let draw_function = transparent_3d_draw_functions.read().id::<DrawLedMaterial>();

    for (view_entity, view) in &mut views {
        let Some(transparent_phase) = transparent_render_phases.get_mut(&view_entity) else {
            continue;
        };

        for entity in &materials {
            let key = LedMaterialPipelineKey {
                hdr: view.hdr,
                samples: msaa.samples(),
            };
            let pipeline = pipelines.specialize(&pipeline_cache, &custom_pipeline, key);
            transparent_phase.add(Transparent3d {
                entity,
                pipeline,
                draw_function,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::NONE,
            });
        }
    }
}
