use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::primitives::Aabb;
use bevy::render::view::NoFrustumCulling;
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_mod_picking::prelude::*;
use bevy_nannou_pixelmap::{LedMaterial, LedZone, NannouArtnetPlugin, ScreenTexture};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NannouArtnetPlugin))
        .add_systems(Startup, (setup, setup_2d))
        .add_systems(
            Update,
            (
                propagate_movement,
                update_corner_positions,
                spawn_led,
                update_cursor_state,
                update_cursor_icon,
            ),
        )
        .run();
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

fn spawn_led(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    key_input: Res<ButtonInput<KeyCode>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if key_input.just_pressed(KeyCode::Space) {
        let rect = commands
            .spawn((
                InitialDimensions(Vec2::new(500.0, 50.0)),
                MaterialMesh2dBundle {
                    mesh: meshes.add(Rectangle::new(500.0, 50.0)).into(),
                    transform: Transform::from_xyz(0.0, 0.0, 0.0)
                        .with_rotation(Quat::from_rotation_z(10.0_f32.to_radians())),
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
                        material: materials
                            .add(ColorMaterial::from(Color::NONE)), // Transparent
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

        commands.entity(rect).with_children(|parent| {
            parent.spawn((
                LedZone {
                    count: 16,
                    rotation: 45.0,
                    position: Vec2::new(150.0, 300.0) * 2.0,
                    size: Vec2::new(512.0, 100.0) * 2.0,
                },
                NoFrustumCulling,
                SpatialBundle::INHERITED_IDENTITY,
            ));
        });
    }
}

#[derive(Component, Default)]
struct CursorState {
    resize_handle: Option<ResizeHandle>,
    rotate: bool,
    main_rectangle: bool,
}

#[derive(Component, Copy, Clone)]
struct Hover;

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

fn calculate_scale_factor(current_scale: Vec2, current_position: Vec2, new_position: Vec2) -> Vec2 {
    let delta = new_position - current_position;
    Vec2::new(
        (current_scale.x + delta.x) / current_scale.x,
        (current_scale.y + delta.y) / current_scale.y,
    )
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

    println!("New transform: {:?}", rectangle_transform);
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // circular base
    commands.spawn(PbrBundle {
        mesh: meshes.add(Circle::new(7.0)),
        material: materials.add(Color::WHITE),
        transform: Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ..default()
    });
    // cube
    commands.spawn(PbrBundle {
        mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        material: materials.add(Color::srgb_u8(124, 144, 255)),
        transform: Transform::from_xyz(0.0, 0.5, 0.0),
        ..default()
    });
    // light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });
    // camera
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(-2.5, 4.5, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });
}

#[derive(Component)]
pub struct UiCamera;

/// Set up a simple 2D scene
fn setup_2d(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Spawn camera
    commands.spawn((
        Camera2dBundle {
            camera: Camera {
                order: 10,
                ..default()
            },
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
    mut led_q: Query<(&mut LedZone, &Parent)>,
) {
    let (ui_camera, ui_camera_transform) = camera_q.single();
    let RenderTarget::Window(window_ref) = ui_camera.target else {
        panic!("Expected a window render target");
    };
    let window = match window_ref {
        WindowRef::Primary => primary_window_q.single(),
        WindowRef::Entity(window) => windows_q.get(window).unwrap(),
    };

    for (mut led, parent) in led_q.iter_mut() {
        let Ok((parent_transform, parent_aabb)) = meshes_q.get(parent.get()) else {
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

        // Calculate the width and height in world space
        let width = (world_corners[1] - world_corners[0]).length();
        let height = (world_corners[3] - world_corners[0]).length();

        // Convert the top-left corner to screen space
        let top_left_screen = ui_camera
            .world_to_viewport(ui_camera_transform, world_corners[3])
            .unwrap()
            .xy()
            * window.scale_factor() as f32;

        // Calculate the size in screen space using the width and height
        let size_screen = Vec2::new(width, height) * window.scale_factor() as f32;

        // Extract the rotation from the transform
        let (_, _, rotation) = parent_transform.rotation.to_euler(EulerRot::XYZ); // Z rotation in radians

        // Update the LedZone
        led.position = top_left_screen;
        led.size = size_screen;
        led.rotation = rotation;
    }
}
