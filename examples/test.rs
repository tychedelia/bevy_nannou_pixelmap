use bevy::prelude::*;
use bevy_nannou_artnet::{LedMaterial, LedZone, NannouArtnetPlugin, ScreenTexture};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NannouArtnetPlugin))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera3dBundle {
                transform: Transform::from_xyz(-2.0, 3., 5.0).looking_at(Vec3::ZERO, Vec3::Y),
                ..default()
            });
        })
        .add_systems(
            Update,
            |mut commands: Commands,
             mut meshes: ResMut<Assets<Mesh>>,
             screen_textue_q: Query<&ScreenTexture>,
             key_input: Res<ButtonInput<KeyCode>>| {
                if key_input.just_pressed(KeyCode::Space) {
                    commands.spawn((
                        LedZone {
                            count: 10,
                            position: Vec2::new(0.0, 0.0),
                            size: Vec2::new(10.0, 10.0),
                        },
                        meshes.add(Cuboid::default()),
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
