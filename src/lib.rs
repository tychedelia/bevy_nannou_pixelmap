use ::nannou::prelude::bevy_render::render_phase::ViewBinnedRenderPhases;
use ::nannou::prelude::render::NannouCamera;
use std::borrow::Cow;

use bevy::asset::load_internal_asset;
use bevy::core_pipeline::bloom::BloomSettings;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::core_3d::{Opaque3d, Opaque3dBinKey, CORE_3D_DEPTH_FORMAT};
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
use bevy::render::mesh::{GpuMesh, MeshVertexBufferLayoutRef};
use bevy::render::render_asset::{RenderAssetPlugin, RenderAssets};
use bevy::render::render_graph::{
    NodeRunError, RenderGraphApp, RenderGraphContext, ViewNode, ViewNodeRunner,
};
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, PhaseItem, PhaseItemExtraIndex,
    RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::binding_types::{
    sampler, storage_buffer_read_only, texture_2d, uniform_buffer,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::{BevyDefault, FallbackImage, GpuImage};
use bevy::render::view::{
    check_visibility, ExtractedView, NoFrustumCulling, RenderLayers, ViewTarget, ViewUniform,
    ViewUniformOffset, ViewUniforms, VisibleEntities, WithMesh,
};
use bevy::render::Extract;
use bevy::window::{
    PrimaryWindow, WindowClosing, WindowRef, WindowResized, WindowScaleFactorChanged,
};
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
pub use sacn;

use crate::ui::UiPlugin;

mod app;
mod sacn_src;
mod ui;

pub use crate::app::*;

const COMPUTE_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(966169125558327);
const MATERIAL_SHADER_HANDLE: Handle<Shader> = Handle::weak_from_u128(116169934631328);

pub struct NannouPixelmapPlugin;

impl Plugin for NannouPixelmapPlugin {
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
            UiPlugin,
            DefaultPickingPlugins,
            ExtractComponentPlugin::<LedArea>::default(),
            ExtractComponentPlugin::<ScreenTexture>::default(),
            ExtractComponentPlugin::<ScreenTextureCamera>::default(),
            ExtractComponentPlugin::<ScreenMaterialCamera>::default(),
        ))
        .add_systems(PostUpdate, check_visibility::<With<LedArea>>)
        .add_systems(
            PreUpdate,
            (spawn_screen_textures, update_cameras, resize_texture),
        );
    }

    fn finish(&self, app: &mut App) {
        let (s, r) = crossbeam_channel::unbounded();
        app.add_event::<ReceivedData>()
            .add_systems(First, send_led_data)
            .add_systems(
                Update,
                |mut commands: Commands,
                 images: Res<Assets<Image>>,
                 screen_texture_q: Query<&ScreenTexture>,
                 input: Res<ButtonInput<KeyCode>>| {
                    if input.just_pressed(KeyCode::Space) {
                        for screen_texture in screen_texture_q.iter() {
                            let image = images.get(&screen_texture.texture).unwrap();
                            let mut window = Window::default();
                            let scale_factor = window.resolution.scale_factor();
                            let window_size = image.size_f32() * scale_factor;
                            let render_layer = RenderLayers::layer(30);
                            window.resolution.set_physical_resolution(
                                window_size.x as u32,
                                window_size.y as u32,
                            );

                            commands.spawn((
                                SpriteBundle {
                                    texture: screen_texture.texture.clone(),
                                    ..default()
                                },
                                render_layer.clone(),
                            ));
                            let window = commands.spawn(window).id();
                            commands.spawn((
                                Camera2dBundle {
                                    camera: Camera {
                                        target: RenderTarget::Window(WindowRef::Entity(window)),
                                        ..default()
                                    },
                                    ..default()
                                },
                                render_layer.clone(),
                            ));
                        }
                    }
                },
            )
            .insert_resource(LedDataReceiver(r));

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
                    (queue_leds, queue_led_material)
                        .chain()
                        .in_set(RenderSet::Queue),
                    prepare_buffers.in_set(RenderSet::PrepareResources),
                    prepare_bind_groups.in_set(RenderSet::PrepareBindGroups),
                    map_and_read_buffer.after(RenderSet::Render),
                ),
            )
            .add_render_command::<Opaque3d, DrawLedMaterial>()
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

// -------------------------
// Components & Resources
// -------------------------

#[derive(Event, Deref, DerefMut, Debug)]
pub struct ReceivedData(pub Vec<f32>);

#[derive(Resource, Deref)]
struct LedDataReceiver(Receiver<(Entity, Vec<f32>)>);
#[derive(Resource, Deref)]
struct RenderWorldSender(Sender<(Entity, Vec<f32>)>);

#[derive(Resource, Deref, DerefMut, Default)]
struct WorkItemBuffers(EntityHashMap<BufferVec<LedWorkItem>>);

#[derive(Resource, Deref, DerefMut, Default)]
struct GpuOutputBuffers(EntityHashMap<UninitBufferVec<LinearRgba>>);

#[derive(Resource, Deref, DerefMut, Default)]
struct CpuReadbackBuffers(EntityHashMap<RawBufferVec<LinearRgba>>);

#[derive(Component, ExtractComponent, Clone)]
struct ScreenTextureCamera;

#[derive(Component, ExtractComponent, Clone)]
struct ScreenMaterialCamera;

#[derive(Component, Clone)]
struct ScreenTextureCameraRef(pub Entity);

#[derive(Component, Clone)]
struct ScreenMaterialCameraRef(pub Entity);

#[derive(Component, ExtractComponent, Clone)]
pub struct ScreenTexture {
    window: Entity,
    texture: Handle<Image>,
}

#[derive(Resource, Deref, DerefMut, Default)]
struct ComputeBindGroups(EntityHashMap<BindGroup>);

#[derive(Bundle, Default)]
pub struct LedBundle {
    /// The led's area.
    pub area: LedArea,
    /// The visibility of the entity.
    pub visibility: Visibility,
    /// The inherited visibility of the entity.
    pub inherited_visibility: InheritedVisibility,
    /// The view visibility of the entity.
    pub view_visibility: ViewVisibility,
    /// The transform of the entity.
    pub transform: Transform,
    /// The global transform of the entity.
    pub global_transform: GlobalTransform,
    /// No frustum culling.
    pub no_frustum_culling: NoFrustumCulling,
}

#[derive(Component, ExtractComponent, Clone)]
pub struct LedArea {
    pub count: u32,
    pub rotation: f32,
    pub position: Vec2,
    pub size: Vec2,
    pub num_samples: u32,
}

impl Default for LedArea {
    fn default() -> Self {
        LedArea {
            count: 1,
            rotation: 0.0,
            position: Vec2::ZERO,
            size: Vec2::ONE,
            num_samples: 10,
        }
    }
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

// -------------------------
// Systems
// -------------------------

fn send_led_data(mut commands: Commands, mut receiver: ResMut<LedDataReceiver>) {
    while let Ok((entity, data)) = receiver.0.try_recv() {
        commands.trigger_targets(ReceivedData(data), entity);
    }
}

fn spawn_screen_textures(
    mut commands: Commands,
    camera_q: Query<
        (
            Entity,
            &Camera,
            &Transform,
            &Projection,
            Option<&RenderLayers>,
            Option<&BloomSettings>,
        ),
        (
            With<NannouCamera>,
            Without<ScreenTextureCameraRef>,
            Without<ScreenMaterialCameraRef>,
        ),
    >,
    mut images: ResMut<Assets<Image>>,
    windows_q: Query<(Entity, &Window)>,
    primary_window_q: Query<(Entity, &Window), With<PrimaryWindow>>,
) {
    for (entity, cam, cam_transform, projection, render_layers, bloom_settings) in camera_q.iter() {
        let RenderTarget::Window(window_target) = cam.target else {
            panic!("Camera target should be a window");
        };
        let (window_entity, window) = match window_target {
            WindowRef::Primary => primary_window_q.single(),
            WindowRef::Entity(window) => windows_q.get(window).unwrap(),
        };

        let size = Extent3d {
            width: window.width() as u32,
            height: window.height() as u32,
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
                        hdr: cam.hdr,
                        order: cam.order - 1, // always render before the camera
                        target: RenderTarget::Image(image.clone()),
                        clear_color: cam.clear_color,
                        ..cam.clone()
                    },
                    projection: projection.clone(),
                    ..default()
                },
                ScreenTextureCamera,
            ))
            .id();
        let screen_material_camera = commands
            .spawn((
                Camera3dBundle {
                    transform: cam_transform.clone(),
                    camera: Camera {
                        hdr: cam.hdr,
                        order: cam.order + 1, // always render after the camera
                        target: cam.target.clone(),
                        clear_color: ClearColorConfig::None,
                        ..cam.clone()
                    },
                    projection: projection.clone(),
                    ..default()
                },
                RenderLayers::layer(32),
                ScreenTexture {
                    window: window_entity,
                    texture: image,
                },
                ScreenMaterialCamera,
            ))
            .id();

        if let Some(render_layers) = render_layers {
            commands
                .entity(screen_texture_camera)
                .insert(render_layers.clone());
        }
        if let Some(bloom_settings) = bloom_settings {
            commands
                .entity(screen_texture_camera)
                .insert(bloom_settings.clone());
        }

        info!("Spawning screen texture camera {screen_texture_camera} for camera {entity}");
        info!("Spawning screen material camera {screen_material_camera} for camera {entity}");
        commands.entity(entity).insert((
            ScreenTextureCameraRef(screen_texture_camera),
            ScreenMaterialCameraRef(screen_material_camera),
        ));
    }
}

fn resize_texture(
    mut window_resized: EventReader<WindowResized>,
    mut window_scale_factor_changed: EventReader<WindowScaleFactorChanged>,
    mut images: ResMut<Assets<Image>>,
    mut screen_textures: Query<(&mut ScreenTexture)>,
    windows_q: Query<(&Window)>,
) {
    for resized in window_resized.read() {
        for (screen_texture) in screen_textures.iter() {
            if screen_texture.window != resized.window {
                continue;
            }

            let (window) = windows_q.get(screen_texture.window).unwrap();
            let size = Extent3d {
                width: window.width() as u32,
                height: window.height() as u32,
                ..default()
            };
            let mut image = images.get_mut(&screen_texture.texture).unwrap();
            image.resize(size);
        }
    }

    for scale_factor_changed in window_scale_factor_changed.read() {
        for (screen_texture) in screen_textures.iter() {
            if screen_texture.window != scale_factor_changed.window {
                continue;
            }

            let (window) = windows_q.get(screen_texture.window).unwrap();
            let size = Extent3d {
                width: window.width() as u32,
                height: window.height() as u32,
                ..default()
            };
            let mut image = images.get_mut(&screen_texture.texture).unwrap();
            image.resize(size);
        }
    }
}

fn update_cameras(
    camera_q: Query<
        (
            &Camera,
            &Transform,
            &Projection,
            Option<&BloomSettings>,
            &ScreenTextureCameraRef,
        ),
        With<NannouCamera>,
    >,
    mut update_camera_q: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut Projection,
            Option<&mut BloomSettings>,
        ),
        Without<NannouCamera>,
    >,
) {
    for (
        parent_cam,
        parent_transform,
        parent_projection,
        parent_bloom_settings,
        screen_texture_camera,
    ) in camera_q.iter()
    {
        let (mut cam, mut transform, mut projection, mut bloom_settings) =
            update_camera_q.get_mut(screen_texture_camera.0).unwrap();

        *transform = parent_transform.clone();
        *projection = parent_projection.clone();
        cam.clear_color = parent_cam.clear_color;
        if let (Some(mut bloom_settings), Some(parent_bloom_settings)) =
            (bloom_settings, parent_bloom_settings)
        {
            *bloom_settings = parent_bloom_settings.clone();
        }
    }
}

fn queue_leds(
    mut commands: Commands,
    views: Query<(Entity, &ExtractedView, &VisibleEntities), With<ScreenMaterialCamera>>,
    gpu_output: Res<GpuOutputBuffers>,
    leds: Query<&LedArea>,
) {
    for (view_entity, view, visible_entities) in views.iter() {
        let is_orthographic = view.clip_from_view.w_axis.w == 1.0;
        let mut view_leds = ViewLeds::default();
        for visible in visible_entities.iter::<With<LedArea>>() {
            let mut idx = 0;
            if let Ok(led) = leds.get(*visible) {
                view_leds.work_items.insert(
                    *visible,
                    LedWorkItem {
                        start_index: 0,
                        rotation: led.rotation,
                        num_leds: led.count,
                        num_samples: led.num_samples,
                        total_area_size: if is_orthographic {
                            led.size / 2.0
                        } else {
                            led.size
                        },
                        area_position: if is_orthographic {
                            led.position / 2.0
                        } else {
                            led.position
                        },
                    },
                );

                let Some(gpu_output) = gpu_output.get(&view_entity) else {
                    warn!("No gpu_output for view {view_entity}");
                    continue;
                };
                let Some(buffer) = gpu_output.buffer() else {
                    warn!("No buffer for view {view_entity}");
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
            .get(&screen_texture.texture)
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
fn f32_to_u8(value: f32) -> u8 {
    // Clamp the value to the range [0.0, 1.0] to ensure valid u8 conversion
    let clamped_value = value.clamp(0.0, 1.0);
    // Scale the clamped value to the range [0, 255] and cast to u8
    (clamped_value * 255.0).round() as u8
}

fn f32_vec_to_u8_vec(values: Vec<f32>) -> Vec<u8> {
    values.iter().map(|&v| f32_to_u8(v)).collect()
}

fn map_and_read_buffer(
    render_device: Res<RenderDevice>,
    cpu_readback_buffers: Res<CpuReadbackBuffers>,
    sender: ResMut<RenderWorldSender>,
    views_q: Query<(Entity, &ViewLeds), (With<ScreenTexture>, With<ExtractedView>)>,
) {
    for (entity, view_leds) in views_q.iter() {
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

            for (led_entity, led) in view_leds.materials.iter() {
                let led_data =
                    data[led.offset as usize..(led.offset + led.count) as usize].to_vec();
                let _ = sender.send((*led_entity, led_data));
            }
        }
        buffer.unmap();
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_led_material(
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    custom_pipeline: Res<LedMaterialPipeline>,
    msaa: Res<Msaa>,
    mut pipelines: ResMut<SpecializedRenderPipelines<LedMaterialPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    mut views: Query<(Entity, &ExtractedView, &ViewLeds), With<ScreenMaterialCamera>>,
) {
    let draw_function = draw_functions.read().id::<DrawLedMaterial>();

    for (view_entity, view, view_leds) in &mut views {
        let Some(phase) = phases.get_mut(&view_entity) else {
            warn!("No phase for view {view_entity}");
            continue;
        };

        for (entity, _) in &view_leds.materials {
            let key = LedMaterialPipelineKey {
                hdr: view.hdr,
                samples: msaa.samples(),
            };
            let pipeline = pipelines.specialize(&pipeline_cache, &custom_pipeline, key);
            phase.add(
                Opaque3dBinKey {
                    draw_function,
                    pipeline,
                    asset_id: AssetId::<Mesh>::invalid().untyped(),
                    material_bind_group_id: None,
                    lightmap_image: None,
                },
                *entity,
                BinnedRenderPhaseType::NonMesh,
            );
        }
    }
}

// -------------------------
// ComputePipeline
// -------------------------

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
        _graph: &mut RenderGraphContext,
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

// -------------------------
// LedMaterialPipeline
// -------------------------

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
