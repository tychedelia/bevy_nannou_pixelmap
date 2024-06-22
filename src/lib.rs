use bevy::{
    prelude::*,
    render::{
        Render,
        render_graph::{self, RenderGraph, RenderLabel},
        render_resource::{*, binding_types::storage_buffer},
        RenderApp, renderer::{RenderContext, RenderDevice}, RenderSet,
    },
};
use bevy::asset::load_internal_asset;
use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::ecs::entity::EntityHashMap;
use bevy::ecs::query::{QueryItem, ROQueryItem};
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::SystemParamItem;
use bevy::pbr::{DrawMesh, MeshPipeline, MeshPipelineKey, MeshUniform, MeshViewBindGroup, PreparedMaterial, RenderMeshInstances, SetMeshBindGroup, SetMeshViewBindGroup, ViewFogUniformOffset, ViewLightProbesUniformOffset, ViewLightsUniformOffset, ViewScreenSpaceReflectionsUniformOffset};
use bevy::render::camera::RenderTarget;
use bevy::render::Extract;
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
use bevy::render::view::{check_visibility, ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms, VisibleEntities, WithMesh};
use bevy::window::{PrimaryWindow, WindowRef};
use crossbeam_channel::{Receiver, Sender};

mod artnet;
mod compute;
mod material;

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

        app
            .add_plugins((
            ExtractComponentPlugin::<LedZone>::default(),
            ExtractComponentPlugin::<ScreenTexture>::default(),
            ExtractComponentPlugin::<ScreenTextureCamera>::default(),
        ))
        .add_systems(PostUpdate, check_visibility::<With<LedZone>>)
        .add_systems(First, spawn_screen_textures);
    }

    fn finish(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        render_app
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
            .init_resource::<SpecializedMeshPipelines<LedMaterialPipeline>>()
            .init_resource::<LedMaterialPipeline>()
            .add_render_graph_node::<ViewNodeRunner<ComputeNode>>(Core3d, ComputeNodeLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::StartMainPass, ComputeNodeLabel, Node3d::MainOpaquePass),
            );
    }
}

type DrawLedMaterial = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshBindGroup<1>,
    SetMaterialBindGroup<2>,
    DrawMesh,
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

fn spawn_screen_textures(
    mut commands: Commands,
    camera_q: Query<(Entity, &Camera, &Transform), (Without<ScreenTexture>, Without<ScreenTextureCamera>)>,
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

        let screen_texture_camera = commands.spawn((
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
        )).id();

        commands.entity(entity).insert((ScreenTexture(image), ScreenTextureCameraRef(screen_texture_camera)));
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
    pub position: Vec2,
    pub size: Vec2,
}

#[derive(AsBindGroup, Debug, Clone)]
pub struct LedMaterial {
    #[uniform(0)]
    pub offset: u32,
    #[uniform(0)]
    pub count: u32,
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
        for visible in visible_entities.iter::<WithMesh>() {
            let mut idx = 0;
            if let Ok(led) = leds.get(*visible) {
                view_leds.work_items.insert(*visible, LedWorkItem {
                    start_index: 0,
                    num_leds: led.count,
                    num_samples: 100,
                    total_area_size: led.size,
                    area_position: led.position,
                });

                let Some(gpu_output) = gpu_output.get(&view_entity) else {
                    continue;
                };
                let Some(buffer) = gpu_output.buffer() else {
                    continue;
                };

                view_leds.materials.insert(*visible, LedMaterial {
                    offset: idx as u32,
                    count: led.count,
                    color_buffer: buffer.clone(),
                });

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
            gpu_output
                .entry(entity)
                .or_insert_with(|| UninitBufferVec::new(BufferUsages::STORAGE)),
            cpu_readback
                .entry(entity)
                .or_insert_with(|| RawBufferVec::new(BufferUsages::COPY_SRC)),
        );

        work_items.clear();
        gpu_output.clear();
        cpu_readback.clear();

        for (_, led ) in leds.work_items.drain() {
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
        if gpu_output.is_empty() || work_items.is_empty() {
            continue;
        }

        gpu_output.write_buffer(&render_device);
        work_items.write_buffer(&render_device, &render_queue);

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
            let bind_group = material.as_bind_group(
                &material_pipeline.layout,
                &render_device,
                &gpu_images,
                &fallback_img,
            ).expect("Failed to create bind group");
            commands.entity(*entity).insert(LedMaterialBindGroup(bind_group.bind_group.clone()));
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

fn map_and_read_buffer(render_device: Res<RenderDevice>) {
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

            pass.set_bind_group(
                0,
                bind_group,
                &[view_uniform.offset],
            );
            pass.set_pipeline(init_pipeline);
            pass.dispatch_workgroups(work_items.capacity() as u32, 1, 1);
        }

        // render_context.command_encoder().copy_buffer_to_buffer(
        //     &gpu_buffer.buffer().expect("buffer should exist"),
        //     0,
        //     &cpu_buffer.buffer().expect("buffer should exist"),
        //     0,
        //     (gpu_buffer.len() * size_of::<LinearRgba>()) as u64,
        // );

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
        LedMaterialPipeline {
            mesh_pipeline: mesh_pipeline.clone(),
            layout: LedMaterial::bind_group_layout(render_device),
        }
    }
}

impl SpecializedMeshPipeline for LedMaterialPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut descriptor = self.mesh_pipeline.specialize(key, layout)?;
        // descriptor.vertex.shader = MATERIAL_SHADER_HANDLE.clone();
        descriptor.layout.push(self.layout.clone());
        descriptor.fragment.as_mut().unwrap().shader = MATERIAL_SHADER_HANDLE.clone();
        Ok(descriptor)
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_led_material(
    transparent_3d_draw_functions: Res<DrawFunctions<Transparent3d>>,
    custom_pipeline: Res<LedMaterialPipeline>,
    msaa: Res<Msaa>,
    mut pipelines: ResMut<SpecializedMeshPipelines<LedMaterialPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    meshes: Res<RenderAssets<GpuMesh>>,
    render_mesh_instances: Res<RenderMeshInstances>,
    material_meshes: Query<Entity, With<LedZone>>,
    mut transparent_render_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut views: Query<(Entity, &ExtractedView), Without<ScreenTextureCamera>>,
) {
    let draw_custom = transparent_3d_draw_functions.read().id::<DrawLedMaterial>();

    let msaa_key = MeshPipelineKey::from_msaa_samples(msaa.samples());

    for (view_entity, view) in &mut views {
        let Some(transparent_phase) = transparent_render_phases.get_mut(&view_entity) else {
            continue;
        };

        let view_key = msaa_key | MeshPipelineKey::from_hdr(view.hdr);
        let rangefinder = view.rangefinder3d();
        for entity in &material_meshes {
            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(entity) else {
                continue;
            };
            let Some(mesh) = meshes.get(mesh_instance.mesh_asset_id) else {
                continue;
            };
            let key =
                view_key | MeshPipelineKey::from_primitive_topology(mesh.primitive_topology());
            let pipeline = pipelines
                .specialize(&pipeline_cache, &custom_pipeline, key, &mesh.layout)
                .unwrap();
            transparent_phase.add(Transparent3d {
                entity,
                pipeline,
                draw_function: draw_custom,
                distance: rangefinder.distance_translation(&mesh_instance.translation),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::NONE,
            });
        }
    }
}
