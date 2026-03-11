#![allow(clippy::type_complexity, clippy::too_many_arguments)]

mod server;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use btl_protocol::FIXED_TIMESTEP_HZ;
use btl_shared::SharedPlugin;

fn main() {
    App::new()
        .add_plugins(
            MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_secs_f64(1.0 / FIXED_TIMESTEP_HZ),
            )),
        )
        .add_plugins(TransformPlugin)
        .add_plugins(LogPlugin {
            filter: "btl_server=debug,btl_shared=debug,btl_protocol=debug,\
                     lightyear=info"
                .into(),
            ..default()
        })
        .add_plugins(SharedPlugin)
        .add_plugins(server::ServerPlugin)
        .run();
}
