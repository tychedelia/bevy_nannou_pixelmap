use bevy::tasks::futures_lite::StreamExt;
use bevy_nannou_pixelmap::*;
use nannou::prelude::*;
use sacn::packet::ACN_SDT_MULTICAST_PORT;
use sacn::source::SacnSource;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use artnet_protocol::{ArtCommand, Output};

fn main() {
    nannou::app(model)
        .add_plugin(NannouPixelmapPlugin)
        .update(update)
        .run();
}

struct Model {
    window: Entity,
    camera: Entity,
    pixelmap: Entity,
    socket: UdpSocket,
}

fn model(app: &App) -> Model {
    let camera = app
        .new_camera()
        // HDR is required for bloom to work.
        .hdr(true)
        // Pick a default bloom settings. This also can be configured manually.
        .bloom_settings(BloomSettings::OLD_SCHOOL)
        .build();

    let window = app
        .new_window()
        .primary()
        .camera(camera)
        .size_pixels(1024, 1024)
        .view(view)
        .build();

    let pixelmap = app
        .new_pixelmap()
        .count(12)
        .x_y(100.0, 100.0)
        .w_h(200.0, 20.0)
        .build(|evt, model: &mut Model| {
            let broadcast_addr = ("192.168.2.4", 6454).to_socket_addrs().unwrap().next().unwrap();
            let command = ArtCommand::Output(Output {
                data: evt.event().iter().map(|x|(*x * 255.0) as u8).collect::<Vec<u8>>().into(),
                ..Output::default()
            });
            let bytes = command.write_to_buffer().unwrap();
            model
                .socket
                .send_to(&bytes, broadcast_addr).unwrap();
        });

    let socket = UdpSocket::bind(("0.0.0.0", 6454)).unwrap();
    socket.set_broadcast(true).unwrap();


    Model {
        camera,
        window,
        pixelmap,
        socket,
    }
}

fn update(app: &App, model: &mut Model) {
    let camera = app.camera(model.camera);
    let window_rect = app.window_rect();
    let norm_mouse_y = (app.mouse().y / window_rect.w()) + 0.5;

    camera.bloom_intensity(norm_mouse_y.clamp(0.0, 0.8));
}

fn view(app: &App, model: &Model) {
    let draw = app.draw();
    draw.background().color(Color::gray(0.1));
    let t = app.elapsed_seconds();
    let window_rect = app.window_rect();
    let norm_mouse_x = (app.mouse().x / window_rect.w()) + 0.5;
    let color_hsl = Color::hsl((1.0 - norm_mouse_x) * 360.0, 1.0, 0.5);
    let mut color_linear_rgb: LinearRgba = color_hsl.into();
    color_linear_rgb = color_linear_rgb * 5.0;

    let x1 = 300.0 * (t * 1.0).sin();
    let y1 = 200.0 * (t * 1.5).cos();
    draw.ellipse()
        .x_y(x1, y1)
        .w_h(100.0, 100.0)
        .emissive(color_linear_rgb);

    let color_hsl = Color::hsl(norm_mouse_x * 360.0, 1.0, 0.5);
    let mut color_linear_rgb: LinearRgba = color_hsl.into();
    color_linear_rgb = color_linear_rgb * 5.0;

    let x2 = -300.0 * (t * 1.0).cos();
    let y2 = -200.0 * (t * 1.5).sin();
    draw.ellipse()
        .x_y(x2, y2)
        .w_h(100.0, 100.0)
        .emissive(color_linear_rgb);

}
