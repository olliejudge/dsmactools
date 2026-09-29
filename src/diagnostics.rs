use crate::{error::Result, hid};
use serde_json::{Value, json};
use std::process::Command;

fn command(program: &str, args: &[&str]) -> Value {
    match Command::new(program).args(args).output() {
        Ok(out) => {
            json!({"available":out.status.success(),"output":String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr }).trim()})
        }
        Err(err) => json!({"available":false,"error":err.to_string()}),
    }
}

#[cfg(target_os = "macos")]
fn native_controllers() -> Value {
    use objc2_foundation::{NSDate, NSRunLoop};
    use objc2_game_controller::{GCController, GCDevice};
    // Called on the application's main thread. Pump attachment notifications first.
    // Initialize discovery before waiting for its asynchronous notifications.
    let _initial = unsafe { GCController::controllers() };
    let run_loop = NSRunLoop::currentRunLoop();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < until {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
    }
    // SAFETY: framework-created, retained GCController objects are read on the main thread;
    // no callbacks, pointers, or mutable collections cross threads.
    let devices = unsafe { GCController::controllers() };
    let mut controllers = Vec::new();
    for device in devices.iter() {
        let value = unsafe {
            let profile = GCController::physicalInputProfile(&device);
            let buttons: Vec<_> = profile
                .buttons()
                .allKeys()
                .iter()
                .map(|key| key.to_string())
                .collect();
            let axes: Vec<_> = profile
                .axes()
                .allKeys()
                .iter()
                .map(|key| key.to_string())
                .collect();
            json!({"vendor":device.vendorName().map(|name| name.to_string()),"category":device.productCategory().to_string(),
                "extended_gamepad":device.extendedGamepad().is_some(),"motion":device.motion().is_some(),
                "haptics":device.haptics().is_some(),"light":device.light().is_some(),"battery":device.battery().is_some(),
                "buttons":buttons,"axes":axes})
        };
        controllers.push(value);
    }
    json!({"available":true,"controllers":controllers,"note":"A read-only snapshot of Apple's GameController framework; USB and Bluetooth devices may appear here."})
}
#[cfg(not(target_os = "macos"))]
fn native_controllers() -> Value {
    json!({"available":false,"reason":"GameController diagnostics require macOS"})
}

pub fn run() -> Result<()> {
    let devices = hid::controllers()?;
    let usb: Vec<_> = devices
        .iter()
        .map(|d| json!({"name":d.name,"pid":format!("{:04x}",d.pid),"transport":"USB"}))
        .collect();
    let report = json!({"schema":1,"tool_version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "macos":command("sw_vers",&["-productVersion"]),"developer_directory":command("xcode-select",&["-p"]),
        "xcode":command("xcodebuild",&["-version"]),"swift":command("xcrun",&["swift","--version"]),
        "metal_compiler":command("xcrun",&["--find","metal"]),"usb_controllers":usb,"game_controller":native_controllers()});
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(())
}
