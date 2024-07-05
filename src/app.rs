use crate::{LedArea, LedBundle, ReceivedData};
use bevy::prelude::{Entity, ResMut, Trigger, Vec2};
use bevy::render::view::RenderLayers;
use bevy::utils::default;
use nannou::app::ModelHolder;

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

    fn samples(self, samples: u32) -> Self {
        self.map_leds(|mut bundle| {
            bundle.area.num_samples = samples;
            bundle
        })
    }

    fn map_leds(self, f: impl FnOnce(LedBundle) -> LedBundle) -> Self;
}

pub struct Builder<'a, 'w, M>
where
    M: Send + Sync + 'static,
{
    app: &'a nannou::App<'w>,
    leds: LedBundle,
    _marker: std::marker::PhantomData<M>,
}

impl<'a, 'w, M> Builder<'a, 'w, M>
where
    M: Send + Sync + 'static,
{
    pub fn new(app: &'a nannou::App<'w>) -> Self {
        Self {
            app,
            leds: Default::default(),
            _marker: Default::default(),
        }
    }

    pub fn build(
        mut self,
        mut callback: impl FnMut(Trigger<ReceivedData>, &mut M) + Send + Sync + 'static,
    ) -> Entity {
        let world = unsafe { self.app.unsafe_world_mut() };
        world
            .spawn((self.leds, RenderLayers::layer(32)))
            .observe(
                move |trigger: Trigger<ReceivedData>, mut model: ResMut<ModelHolder<M>>| {
                    callback(trigger, &mut model.0);
                },
            )
            .id()
    }
}

impl<M> SetPixelmap for Builder<'_, '_, M>
where
    M: Send + Sync + 'static,
{
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
    fn new_pixelmap<'a, M>(&'a self) -> Builder<'a, 'w, M>
    where
        M: Send + Sync + 'static;
}

impl<'w> AppPixelmapExt<'w> for nannou::App<'w> {
    fn new_pixelmap<'a, M>(&'a self) -> Builder<'a, 'w, M>
    where
        M: Send + Sync + 'static,
    {
        Builder::new(self)
    }
}
