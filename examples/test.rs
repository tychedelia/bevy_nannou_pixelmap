use bevy::prelude::*;
use bevy_nannou_artnet::{LedMaterial, LedZone, NannouArtnetPlugin, ScreenTexture};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NannouArtnetPlugin))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            |mut commands: Commands,
             mut meshes: ResMut<Assets<Mesh>>,
             screen_textue_q: Query<&ScreenTexture>,
             key_input: Res<ButtonInput<KeyCode>>| {
                if key_input.just_pressed(KeyCode::Space) {
                    commands.spawn((
                        LedZone {
                            count: 100,
                            position: Vec2::new(150.0, 250.0),
                            size: Vec2::new(512.0,100.0),
                        },
                        meshes.add(Plane3d::new(Vec3::Y, Vec2::new(10.0, 10.0))),
                        SpatialBundle {
                            transform: Transform::from_xyz(-1.0, 0.5, 0.0),
                            ..default()
                        },
                    ));
                }
            },
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // circular base
    commands.spawn(PbrBundle {
        mesh: meshes.add(Circle::new(4.0)),
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