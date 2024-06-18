use bevy::prelude::*;
use bevy_nannou_artnet::NannouArtnetPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NannouArtnetPlugin))
        .add_systems(Startup, || {

        })
        .run();
}