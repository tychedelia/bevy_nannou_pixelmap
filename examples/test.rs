use bevy::tasks::futures_lite::StreamExt;
use bevy_nannou_pixelmap::*;
use nannou::prelude::*;

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
}

fn model(app: &App) -> Model {
    let camera = app
        .new_camera()
        // HDR is required for bloom to work.
        .hdr(true)
        // Pick a default bloom settings. This also can be configured manually.
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
        .w_h(400.0, 40.0)
        .build();

    Model {
        camera,
        window,
        pixelmap,
    }
}

fn update(app: &App, model: &mut Model) {
    let camera = app.camera(model.camera);
    let window_rect = app.window_rect();
    let norm_mouse_y = (app.mouse().y / window_rect.w()) + 0.5;

}

fn view(app: &App, model: &Model) {
    // Begin drawing
    let draw = app.draw();

    // Clear the background to blue.
    draw.background().color(CORNFLOWER_BLUE);

    // Draw a purple triangle in the top left half of the window.
    let win = app.window_rect();
    draw.tri()
        .points(win.bottom_left(), win.top_left(), win.top_right())
        .color(VIOLET);

    // Draw an ellipse to follow the mouse.
    let t = app.elapsed_seconds();
    draw.ellipse()
        .x_y(app.mouse().x * t.cos(), app.mouse().y)
        .radius(win.w() * 0.125 * t.sin())
        .color(RED);

    // Draw a line!
    draw.line()
        .weight(10.0 + (t.sin() * 0.5 + 0.5) * 90.0)
        .caps_round()
        .color(PALE_GOLDENROD)
        .points(win.top_left() * t.sin(), win.bottom_right() * t.cos());

    // Draw a quad that follows the inverse of the ellipse.
    draw.quad()
        .x_y(-app.mouse().x, app.mouse().y)
        .color(DARK_GREEN)
        .rotate(t);

    // Draw a rect that follows a different inverse of the ellipse.
    draw.rect()
        .x_y(app.mouse().y, app.mouse().x)
        .w(app.mouse().x * 0.25)
        .hsv(t, 1.0, 1.0);
}
