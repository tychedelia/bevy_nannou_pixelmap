use crate::LedArea;
use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::primitives::Aabb;
use bevy::render::view::{NoFrustumCulling, RenderLayers};
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_mod_picking::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_ui).add_systems(
            Update,
            (
                despawn_removed_areas,
                propagate_movement,
                update_corner_positions,
                spawn_led,
                update_cursor_state,
                update_cursor_icon,
            ),
        );
    }
}

#[derive(Component)]
struct DragCorner(Entity);
#[derive(Component)]
struct InitialDimensions(Vec2);

#[derive(Component, Debug, Copy, Clone)]
enum ResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Component)]
struct RotationHandle;

#[derive(Component, Default)]
struct CursorState {
    resize_handle: Option<ResizeHandle>,
    rotate: bool,
    main_rectangle: bool,
}

#[derive(Component, Copy, Clone)]
struct Hover;

#[derive(Component)]
pub struct AreaRef(Entity);

#[derive(Component)]
pub struct MeshRef(Entity);

fn spawn_led(
    mut commands: Commands,
    added_leds_q: Query<(Entity, &LedArea), Added<LedArea>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (entity, led) in added_leds_q.iter() {
        // Find the led's center transform from it's size and position (top left corner)
        let center = led.position + led.size * 0.5;

        let rect = commands
            .spawn((
                InitialDimensions(led.size),
                MaterialMesh2dBundle {
                    mesh: meshes.add(Rectangle::new(led.size.x, led.size.y)).into(),
                    transform: Transform::from_xyz(center.x, center.y, 0.0)
                        .with_rotation(Quat::from_rotation_z(led.rotation)),
                    material: materials.add(ColorMaterial::from(Color::NONE)),
                    ..default()
                },
                PickableBundle::default(), // <- Makes the mesh pickable.
                On::<Pointer<DragStart>>::target_insert(Pickable::IGNORE), // Disable picking
                On::<Pointer<DragEnd>>::target_insert(Pickable::default()), // Re-enable picking
                On::<Pointer<Drag>>::run(drag_body),
                On::<Pointer<Over>>::target_insert(Hover),
                On::<Pointer<Out>>::target_remove::<Hover>(),
            ))
            .insert(AreaRef(entity))
            .id();
        [
            (Vec2::new(-250.0, -25.0), ResizeHandle::BottomLeft),
            (Vec2::new(250.0, -25.0), ResizeHandle::BottomRight),
            (Vec2::new(-250.0, 25.0), ResizeHandle::TopLeft),
            (Vec2::new(250.0, 25.0), ResizeHandle::TopRight),
        ]
        .into_iter()
        .for_each(|(corner, handle)| {
            let drag_circle = commands
                .spawn((
                    handle,
                    DragCorner(rect),
                    MaterialMesh2dBundle {
                        mesh: meshes.add(Circle::new(7.0)).into(),
                        transform: Transform::from_translation(corner.extend(0.0)),
                        material: materials.add(ColorMaterial::from(Color::NONE)),
                        ..default()
                    },
                    PickableBundle::default(),
                    On::<Pointer<DragStart>>::target_insert(Pickable::IGNORE),
                    On::<Pointer<DragEnd>>::target_insert(Pickable::default()),
                    On::<Pointer<Drag>>::run(drag_corner),
                    On::<Pointer<Over>>::target_insert(Hover),
                    On::<Pointer<Out>>::target_remove::<Hover>(),
                ))
                .id();

            // Spawn rotation circle as a child of the drag circle
            commands
                .spawn((
                    RotationHandle,
                    MaterialMesh2dBundle {
                        mesh: meshes.add(Circle::new(15.0)).into(), // 2x the size of drag circle
                        transform: Transform::from_xyz(0.0, 0.0, -0.1), // Slightly behind the drag circle
                        material: materials.add(ColorMaterial::from(Color::NONE)), // Transparent
                        ..default()
                    },
                    PickableBundle::default(),
                    On::<Pointer<DragStart>>::target_insert(Pickable::IGNORE),
                    On::<Pointer<DragEnd>>::target_insert(Pickable::default()),
                    On::<Pointer<Drag>>::run(rotate_rectangle),
                    On::<Pointer<Over>>::target_insert(Hover),
                    On::<Pointer<Out>>::target_remove::<Hover>(),
                ))
                .set_parent(drag_circle);
        });

        commands.entity(entity).insert(MeshRef(rect));
    }
}

fn update_cursor_state(
    mut cursor_state: Query<&mut CursorState, With<UiCamera>>,
    resize_handles: Query<(&ResizeHandle, &Hover)>,
    rotation_handles: Query<(), (With<RotationHandle>, With<Hover>)>,
    main_rectangles: Query<(), (With<InitialDimensions>, With<Hover>)>,
) {
    let mut cursor_state = cursor_state.single_mut();
    cursor_state.resize_handle = resize_handles.iter().next().map(|(handle, _)| *handle);
    cursor_state.rotate = !rotation_handles.is_empty();
    cursor_state.main_rectangle = !main_rectangles.is_empty();
}

fn update_cursor_icon(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    cursor_state: Query<&CursorState, With<UiCamera>>,
) {
    let mut window = windows.single_mut();
    let cursor_state = cursor_state.single();

    if cursor_state.rotate {
        window.cursor.icon = CursorIcon::Grab;
    } else if let Some(resize_handle) = cursor_state.resize_handle {
        window.cursor.icon = match resize_handle {
            ResizeHandle::TopLeft => CursorIcon::NwResize,
            ResizeHandle::TopRight => CursorIcon::NeResize,
            ResizeHandle::BottomLeft => CursorIcon::SwResize,
            ResizeHandle::BottomRight => CursorIcon::SeResize,
        };
    } else if cursor_state.main_rectangle {
        window.cursor.icon = CursorIcon::Move;
    } else {
        window.cursor.icon = CursorIcon::Default;
    }
}

fn rotate_rectangle(
    mut event: ListenerMut<Pointer<Drag>>,
    rotation_handle_query: Query<&Parent, With<RotationHandle>>,
    corner_query: Query<&DragCorner>,
    mut rectangle_query: Query<&mut Transform>,
) {
    if let Ok(parent) = rotation_handle_query.get(event.target) {
        if let Ok(drag_corner) = corner_query.get(parent.get()) {
            if let Ok(mut rectangle_transform) = rectangle_query.get_mut(drag_corner.0) {
                // Calculate the rotation angle based on the drag delta
                let rotation_angle = -event.delta.x * 0.01; // Adjust this multiplier to control rotation speed

                // Get the center of the rectangle
                let center = rectangle_transform.translation.xy();

                // Rotate around the center
                rectangle_transform.translation -= center.extend(0.0);
                rectangle_transform.rotate_z(rotation_angle);
                rectangle_transform.translation += center.extend(0.0);
            }
        }
    }
}

fn drag_body(event: Listener<Pointer<Drag>>, mut transform_q: Query<&mut Transform>) {
    let mut transform = transform_q.get_mut(event.target).unwrap();
    transform.translation.x += event.delta.x; // Make the square follow the mouse
    transform.translation.y -= event.delta.y;
}

fn resize_rectangle(
    handle: &ResizeHandle,
    rectangle_transform: &mut Transform,
    initial_dimensions: &InitialDimensions,
    drag_delta: Vec2,
) {
    let current_scale = rectangle_transform.scale.xy();
    let current_position = rectangle_transform.translation.xy();
    let current_size = current_scale * initial_dimensions.0;
    let drag_delta = Vec2::new(drag_delta.x, -drag_delta.y); // Invert y axis

    let (new_size, position_change) = match handle {
        ResizeHandle::TopRight => {
            let new_size = (current_size + drag_delta).max(Vec2::splat(1.0));
            let position_change = (new_size - current_size) * 0.5;
            (new_size, position_change)
        }
        ResizeHandle::BottomLeft => {
            let new_size = (current_size - drag_delta).max(Vec2::splat(1.0));
            let position_change = (current_size - new_size) * 0.5;
            (new_size, position_change)
        }
        ResizeHandle::BottomRight => {
            let new_size = Vec2::new(current_size.x + drag_delta.x, current_size.y - drag_delta.y)
                .max(Vec2::splat(1.0));
            let position_change = Vec2::new(
                (new_size.x - current_size.x) * 0.5,
                (current_size.y - new_size.y) * 0.5,
            );
            (new_size, position_change)
        }
        ResizeHandle::TopLeft => {
            let new_size = Vec2::new(current_size.x - drag_delta.x, current_size.y + drag_delta.y)
                .max(Vec2::splat(1.0));
            let position_change = Vec2::new(
                (current_size.x - new_size.x) * 0.5,
                (new_size.y - current_size.y) * 0.5,
            );
            (new_size, position_change)
        }
    };

    let new_scale = new_size / initial_dimensions.0;
    rectangle_transform.scale = new_scale.extend(1.0);
    rectangle_transform.translation =
        (current_position + position_change).extend(rectangle_transform.translation.z);
}

fn update_corner_positions(
    rectangles: Query<(Entity, &Transform, &InitialDimensions), Changed<Transform>>,
    mut corners: Query<(&mut Transform, &ResizeHandle, &DragCorner), Without<InitialDimensions>>,
) {
    for (rectangle, rectangle_transform, initial_dimensions) in rectangles.iter() {
        let half_size = (rectangle_transform.scale.xy() * initial_dimensions.0) * 0.5;
        for (mut corner_transform, handle, corner) in corners.iter_mut() {
            if corner.0 == rectangle {
                let corner_pos = match handle {
                    ResizeHandle::TopLeft => Vec3::new(-half_size.x, half_size.y, 0.0),
                    ResizeHandle::TopRight => Vec3::new(half_size.x, half_size.y, 0.0),
                    ResizeHandle::BottomLeft => Vec3::new(-half_size.x, -half_size.y, 0.0),
                    ResizeHandle::BottomRight => Vec3::new(half_size.x, -half_size.y, 0.0),
                };
                corner_transform.translation =
                    rectangle_transform.translation + rectangle_transform.rotation * corner_pos;
                corner_transform.rotation = rectangle_transform.rotation;
            }
        }
    }
}

fn drag_corner(
    mut event: ListenerMut<Pointer<Drag>>,
    corner_query: Query<(&ResizeHandle, &DragCorner)>,
    mut rectangles: Query<(&mut Transform, &InitialDimensions)>,
) {
    if let Ok((handle, corner)) = corner_query.get(event.target) {
        if let Ok((mut rectangle_transform, initial_dimensions)) = rectangles.get_mut(corner.0) {
            resize_rectangle(
                handle,
                &mut rectangle_transform,
                initial_dimensions,
                event.delta,
            );
        }
    }
}

#[derive(Component)]
pub struct UiCamera;

fn setup_ui(mut commands: Commands) {
    commands.spawn((
        Camera2dBundle {
            camera: Camera {
                hdr: true,
                order: 3,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 10.0),
            ..default()
        },
        UiCamera,
        CursorState::default(),
    ));
}

fn propagate_movement(
    camera_q: Query<(&Camera, &GlobalTransform), With<UiCamera>>,
    windows_q: Query<&Window>,
    primary_window_q: Query<&Window, With<PrimaryWindow>>,
    meshes_q: Query<(&Transform, &Aabb)>,
    mut led_q: Query<(&mut LedArea, &MeshRef)>,
) {
    let (ui_camera, ui_camera_transform) = camera_q.single();
    let RenderTarget::Window(window_ref) = ui_camera.target else {
        panic!("Expected a window render target");
    };
    let window = match window_ref {
        WindowRef::Primary => primary_window_q.single(),
        WindowRef::Entity(window) => windows_q.get(window).unwrap(),
    };

    for (mut led, mesh_ref) in led_q.iter_mut() {
        let Ok((parent_transform, parent_aabb)) = meshes_q.get(mesh_ref.0) else {
            continue;
        };

        // Compute the corners of the AABB in local space
        let half_extents = parent_aabb.half_extents;
        let local_corners = [
            Vec3::new(-half_extents.x, -half_extents.y, 0.0), // bottom-left
            Vec3::new(half_extents.x, -half_extents.y, 0.0),  // bottom-right
            Vec3::new(half_extents.x, half_extents.y, 0.0),   // top-right
            Vec3::new(-half_extents.x, half_extents.y, 0.0),  // top-left
        ];

        // Transform the corners to world space
        let world_corners: Vec<Vec3> = local_corners
            .iter()
            .map(|&corner| parent_transform.compute_matrix() * corner.extend(1.0))
            .map(|corner| corner.truncate())
            .collect();

        // Convert all corners to screen space
        let screen_corners: Vec<Vec2> = world_corners
            .iter()
            .map(|&corner| {
                ui_camera
                    .world_to_viewport(ui_camera_transform, corner)
                    .unwrap()
                    .xy()
                    * window.scale_factor()
            })
            .collect();

        // Calculate the size in screen space
        let size_screen = Vec2::new(
            (screen_corners[1].x - screen_corners[0].x).abs(),
            (screen_corners[3].y - screen_corners[0].y).abs(),
        );

        // Top-left corner in screen space
        let top_left_screen = screen_corners[3];

        // Extract the rotation from the transform
        let (_, _, rotation) = parent_transform.rotation.to_euler(EulerRot::XYZ); // Z rotation in radians

        // Update the LedArea
        led.position = top_left_screen;
        led.size = size_screen;
        led.rotation = rotation;
    }
}

fn despawn_removed_areas(
    mut commands: Commands,
    mut removed_areas: RemovedComponents<LedArea>,
    area_refs: Query<(Entity, &AreaRef)>,
    corner_query: Query<(Entity, &DragCorner)>,
) {
    for removed_area in removed_areas.read() {
        // Find the rectangle entity associated with the removed LedArea
        if let Some((rect_entity, _)) = area_refs
            .iter()
            .find(|(_, area_ref)| area_ref.0 == removed_area)
        {
            // Despawn the rectangle
            commands.entity(rect_entity).despawn_recursive();

            // Find and despawn all corner entities associated with this rectangle
            for (corner_entity, drag_corner) in corner_query.iter() {
                if drag_corner.0 == rect_entity {
                    commands.entity(corner_entity).despawn_recursive();
                }
            }
        }
    }
}
