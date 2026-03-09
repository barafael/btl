mod server;

use bevy::log::LogPlugin;
use bevy::prelude::*;
use btl_protocol::FIXED_TIMESTEP_HZ;
use btl_shared::SharedPlugin;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    close_when_requested: false,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .set(LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_ecs=warn,\
                             btl_server=debug,btl_shared=debug,btl_protocol=debug,\
                             lightyear=info"
                        .into(),
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
