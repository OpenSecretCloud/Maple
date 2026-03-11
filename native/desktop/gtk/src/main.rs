mod app;
mod manager;
mod theme;
mod wordmark;

use gtk::prelude::*;
use gtk4 as gtk;

fn main() -> gtk::glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("cloud.opensecret.maple.desktop")
        .build();

    application.connect_activate(|application| {
        app::launch(application);
    });

    application.run()
}
