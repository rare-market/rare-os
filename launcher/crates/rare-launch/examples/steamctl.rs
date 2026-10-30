//! Device checks for Rare's Steam install, queue and uninstall code (run on the SuiPlay under the
//! device lock): steamctl up | eval '<js>' | snapshot | folders | install <appid> <folder> |
//! control <appid> pause|resume|top|cancel|update|accept|cancelinstall | uninstall <appid>
use rare_launch::steamdl::{self, Control};
use std::time::Duration;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let id = || arg(1).parse::<u32>().expect("an app id");
    let ctx = rare_core::Ctx::headless();
    let runner = rare_launch::launch::Runner::default();
    match arg(0).as_str() {
        "eval" => println!("{:?}", rare_launch::steamcef::eval(&arg(1), Duration::from_secs(30))),
        "snapshot" => println!("{}", serde_json::to_string_pretty(&steamdl::snapshot()).unwrap()),
        "folders" => println!("{:?}", steamdl::folders()),
        "install" => println!("{:?}", steamdl::install(&ctx, &runner, id(), &arg(2))),
        "control" => {
            let c = match arg(2).as_str() {
                "pause" => Control::Pause,
                "resume" => Control::Resume,
                "top" => Control::Top,
                "cancel" => Control::Cancel,
                "accept" => Control::AcceptEula,
                "cancelinstall" => Control::CancelInstall,
                _ => Control::UpdateNow,
            };
            println!("{:?}", steamdl::control(&ctx, &runner, id(), c));
        }
        "uninstall" => println!("{:?}", steamdl::uninstall_quiet(id())),
        _ => println!("up: {}", rare_launch::steamcef::up()),
    }
}
