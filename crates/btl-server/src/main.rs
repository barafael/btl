mod server;

use bevy::prelude::*;
use btl_protocol::FIXED_TIMESTEP_HZ;
use btl_shared::SharedPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
        ))
        .add_plugins(SharedPlugin)
        .add_plugins(server::ServerPlugin)
        .run();
}
