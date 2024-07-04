use crate::{LedArea, LedBundle};
use bevy::prelude::{Entity, Vec2};
use bevy::render::view::RenderLayers;
use bevy::utils::default;

pub trait SetPixelmap: Sized {
    fn count(self, count: u32) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.count = count;
            bundle
        })
    }

    fn x_y(self, x: f32, y: f32) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.position = Vec2::new(x, y);
            bundle
        })
    }

    fn xy(self, p: Vec2) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.position = p;
            bundle
        })
    }

    fn w_h(self, width: f32, height: f32) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.size = Vec2::new(width, height);
            bundle
        })
    }

    fn wh(self, size: Vec2) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.size = size;
            bundle
        })
    }

    fn map_leds(self, f: impl FnOnce(LedBundle) -> LedBundle) -> Self;
}

pub struct Builder<'a, 'w> {
    app: &'a nannou::App<'w>,
    leds: LedBundle,
}

impl<'a, 'w> Builder<'a, 'w> {
    pub fn new(app: &'a nannou::App<'w>) -> Self {
        Self {
            app,
            leds: Default::default(),
        }
    }

    pub fn build(self) -> Entity {
        let world = unsafe { self.app.unsafe_world_mut() };
        world
            .spawn(
                (
                    self.leds
                    // , RenderLayers::layer(32)
                ),
            )
            .id()
    }
}

impl SetPixelmap for Builder<'_, '_> {
    fn map_leds(self, f: impl FnOnce(LedBundle) -> LedBundle) -> Self {
        Self {
            leds: f(self.leds),
            ..self
        }
    }
}

pub struct PixelmapArea<'a, 'w> {
    entity: Entity,
    app: &'a nannou::App<'w>,
}

impl<'a, 'w> PixelmapArea<'a, 'w> {
    pub fn new(app: &'a nannou::App<'w>, entity: Entity) -> Self {
        Self { app, entity }
    }
}

impl SetPixelmap for PixelmapArea<'_, '_> {
    fn map_leds(self, f: impl FnOnce(LedBundle) -> LedBundle) -> Self {
        let world = unsafe { self.app.unsafe_world_mut() };
        let mut camera_q = world.query::<(&LedArea,)>();
        let (area,) = camera_q.get_mut(world, self.entity).unwrap();
        let bundle = LedBundle {
            area: area.clone(),
            ..default()
        };

        let bundle = f(bundle);
        world.entity_mut(self.entity).insert(bundle);
        self
    }
}

pub trait AppPixelmapExt<'w> {
    /// Begin building a new camera.
    fn new_pixelmap<'a>(&'a self) -> Builder<'a, 'w>;
}

impl<'w> AppPixelmapExt<'w> for nannou::App<'w> {
    fn new_pixelmap<'a>(&'a self) -> Builder<'a, 'w> {
        Builder::new(self)
    }
}
