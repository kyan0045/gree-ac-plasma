//! A headless controller daemon for Gree air conditioners, plus D-Bus and
//! CLI front ends for the Plasma applet.
//!
//! With no arguments it runs the daemon. The `--list`, `--read-state` and
//! `--self-test` commands are headless helpers.

mod config;
mod controller;
mod dbus;
mod gree;
mod model;
mod notify;
mod state;
mod wifi;

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use config::Config;
use controller::Controller;
use gree::GreeClient;
use tokio::sync::mpsc::unbounded_channel;

use crate::state::{Command, Snapshot};

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");

    let result = match command {
        "--list" => block_on(list_units()),
        "--read-state" => block_on(read_state()),
        "--self-test" => block_on(self_test()),
        "--current-ssid" => block_on(current_ssid()),
        "--set-home-ssid" => block_on(set_home_ssid(args.get(1))),
        "--set-name" => block_on(set_name(args.get(1), args.get(2))),
        "--daemon" => block_on(run_daemon()),
        "--state-json" => block_on(print_state()),
        "--refresh" => block_on(daemon_simple("Refresh", &(), "Refresh requested")),
        "--set-linked" => block_on(cmd_set_linked(args.get(1))),
        "--set-power" => block_on(cmd_set_power(args.get(1), &args)),
        "--set-mode" => block_on(cmd_set_mode(args.get(1), &args)),
        "--set-temp" => block_on(cmd_set_temp(args.get(1), &args)),
        "--set-fan" => block_on(cmd_set_fan(args.get(1), &args)),
        "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "" => block_on(run_daemon()),
        _ => {
            eprintln!("unknown command: {command}\n");
            print_help();
            std::process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

fn print_help() {
    println!(
        "gree-ac-plasma: controller daemon for Gree air conditioners\n\n\
         USAGE:\n  \
         gree-ac-plasma                    run the daemon (serves the Plasma applet)\n  \
         gree-ac-plasma --daemon           same as above\n\n\
         Applet / scripting commands (talk to the running daemon):\n  \
         gree-ac-plasma --state-json                 print the UI state as JSON\n  \
         gree-ac-plasma --refresh                    re-read everything now\n  \
         gree-ac-plasma --set-linked on|off          link or unlink the two ACs\n  \
         gree-ac-plasma --set-power on|off [--unit N|both]\n  \
         gree-ac-plasma --set-mode MODE [--unit N|both]\n         \
         (Auto, Cool, Dry, Fan, Heat)\n  \
         gree-ac-plasma --set-temp 16..30 [--unit N|both]\n  \
         gree-ac-plasma --set-fan FAN [--unit N|both]\n          \
         (Auto, Low, Medium-low, Medium, Medium-high, High)\n\n\
         Diagnostics (talk to the units directly):\n  \
         gree-ac-plasma --list             discover units and exit\n  \
         gree-ac-plasma --read-state       discover units and print their state\n  \
         gree-ac-plasma --self-test        re-send each unit its own settings\n  \
         gree-ac-plasma --current-ssid     print the connected Wi-Fi network name\n  \
         gree-ac-plasma --set-home-ssid N  remember N as the home network\n  \
         gree-ac-plasma --set-name MAC NAME\n                     \
         name a unit (MAC from --list; empty NAME clears it)\n\n\
         Add the \"Gree AC\" widget to the panel for the native popup UI.\n\
         Settings live in ~/.config/gree-ac-plasma/config.json."
    );
}

async fn list_units() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let devices = gree::discover(Duration::from_secs(3)).await?;
    if devices.is_empty() {
        println!(
            "No Gree units replied. Check that this computer and the ACs share a LAN and that Wi-Fi client isolation is off."
        );
        return Ok(());
    }
    println!("Found {} Gree unit(s):", devices.len());
    for (index, device) in devices.iter().enumerate() {
        println!(
            "  {}. {} at {} (MAC {})",
            index + 1,
            if device.name.is_empty() {
                "Gree unit"
            } else {
                &device.name
            },
            device.ip,
            device.mac,
        );
    }
    Ok(())
}

async fn read_state() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let devices = gree::discover(Duration::from_secs(3)).await?;
    if devices.is_empty() {
        println!("No Gree units replied.");
        return Ok(());
    }
    for device in devices {
        let label = if device.name.is_empty() {
            device.ip.to_string()
        } else {
            device.name.clone()
        };
        match GreeClient::connect(device).await {
            Ok(client) => match client.read_state().await {
                Ok(state) => {
                    println!("{label}: {}", state.summary());
                    match state.room_raw {
                        Some(raw) => println!("  room sensor: {raw} (raw)"),
                        None => println!("  room sensor: unavailable"),
                    }
                }
                Err(error) => eprintln!("{label}: could not read state: {error}"),
            },
            Err(error) => eprintln!("{label}: could not bind: {error}"),
        }
    }
    Ok(())
}

/// Verify the write path by re-sending each unit its own current mode,
/// temperature and fan. This does not change how a unit behaves; it only
/// confirms that commands are accepted.
async fn self_test() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let devices = gree::discover(Duration::from_secs(3)).await?;
    if devices.is_empty() {
        println!("No Gree units replied.");
        return Ok(());
    }

    let mut failures = 0;
    for device in devices {
        let label = if device.name.is_empty() {
            device.ip.to_string()
        } else {
            device.name.clone()
        };
        let client = match GreeClient::connect(device).await {
            Ok(client) => client,
            Err(error) => {
                eprintln!("{label}: could not bind: {error}");
                failures += 1;
                continue;
            }
        };

        let before = match client.read_state().await {
            Ok(state) => state,
            Err(error) => {
                eprintln!("{label}: could not read state: {error}");
                failures += 1;
                continue;
            }
        };

        let props = vec![
            ("Mod".to_owned(), before.mode.raw()),
            ("SetTem".to_owned(), before.target as i64),
            ("WdSpd".to_owned(), before.fan.raw()),
        ];
        if let Err(error) = client.send_command(&props, true).await {
            eprintln!("{label}: could not send command: {error}");
            failures += 1;
            continue;
        }

        tokio::time::sleep(Duration::from_millis(800)).await;
        match client.read_state().await {
            Ok(after) => {
                let matched = after.mode == before.mode
                    && after.target == before.target
                    && after.fan == before.fan;
                println!(
                    "{label}: command accepted, state {} ({})",
                    after.summary(),
                    if matched {
                        "unchanged, as expected"
                    } else {
                        "read back different"
                    }
                );
            }
            Err(error) => {
                eprintln!("{label}: command sent but re-read failed: {error}");
                failures += 1;
            }
        }
    }

    if failures == 0 {
        println!("Write path OK.");
        Ok(())
    } else {
        Err(format!("{failures} unit(s) failed the write test").into())
    }
}

async fn current_ssid() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match wifi::current_ssid().await {
        Some(ssid) => println!("{ssid}"),
        None => println!("(not connected to Wi-Fi)"),
    }
    Ok(())
}

async fn set_home_ssid(
    ssid: Option<&String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ssid = ssid.ok_or("usage: gree-ac-plasma --set-home-ssid <network name>")?;
    if ssid.is_empty() {
        return Err("usage: gree-ac-plasma --set-home-ssid <network name>".into());
    }
    let mut config = Config::load();
    config.home_ssid = Some(ssid.clone());
    config.save();
    println!("Home network set to \"{ssid}\".");
    Ok(())
}

/// Name (or rename) a unit by MAC address as shown in `--list`.
/// An empty name clears the custom name.
async fn set_name(
    mac: Option<&String>,
    name: Option<&String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mac = mac.map(String::as_str).unwrap_or("");
    if mac.is_empty() {
        return Err("usage: gree-ac-plasma --set-name MAC NAME".into());
    }
    let mac = mac.to_lowercase();
    let mut config = Config::load();
    match name.map(String::as_str) {
        Some(name) if !name.is_empty() => {
            config.names.insert(mac.clone(), name.to_owned());
            println!("Named {mac} \"{name}\".");
        }
        _ => {
            config.names.remove(&mac);
            println!("Cleared the name for {mac}.");
        }
    }
    config.save();
    Ok(())
}

/// Run the headless daemon: the controller plus a D-Bus service that the
/// Plasma applet and the `--set-*` / `--state-json` commands talk to.
/// Runs until Ctrl-C or a `Quit` command.
async fn run_daemon() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::load();
    let (command_tx, command_rx) = unbounded_channel::<Command>();
    let (snapshot_tx, mut snapshot_rx) = unbounded_channel::<Snapshot>();

    let cached = Arc::new(Mutex::new("{\"units\":[]}".to_owned()));
    let api = dbus::ControllerApi::new(command_tx.clone(), cached.clone());

    let connection = zbus::Connection::session().await?;
    connection
        .object_server()
        .at(dbus::OBJECT_PATH, api)
        .await?;
    {
        use zbus::fdo::{RequestNameFlags, RequestNameReply};
        let reply = connection
            .request_name_with_flags(dbus::SERVICE_NAME, RequestNameFlags::DoNotQueue.into())
            .await?;
        if reply != RequestNameReply::PrimaryOwner {
            return Err("another gree-ac-plasma daemon is already running".into());
        }
    }

    // Ctrl-C stops the controller the same way Quit does.
    let quit_tx = command_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = quit_tx.send(Command::Quit);
    });

    // Keep the latest state JSON ready for `GetState` callers.
    let hub_cached = cached.clone();
    tokio::spawn(async move {
        while let Some(snapshot) = snapshot_rx.recv().await {
            *hub_cached.lock().unwrap_or_else(|e| e.into_inner()) = dbus::snapshot_json(&snapshot);
        }
    });

    Controller::new(config, snapshot_tx).run(command_rx).await;
    Ok(())
}

async fn print_state() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state: String = dbus::call_daemon("GetState", &()).await?;
    println!("{state}");
    Ok(())
}

async fn daemon_simple<A>(
    method: &str,
    args: &A,
    done: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
{
    let (): () = dbus::call_daemon(method, args).await?;
    println!("{done}.");
    Ok(())
}

/// `--unit N`, `--unit both`, or absent (both).
fn parse_unit(args: &[String]) -> Result<Option<usize>, String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--unit" {
            let value = args
                .get(index + 1)
                .ok_or("--unit needs a value (0, 1, ... or both)")?;
            if value.eq_ignore_ascii_case("both") {
                return Ok(None);
            }
            return value
                .parse()
                .map(Some)
                .map_err(|_| format!("bad --unit value: {value}"));
        }
        index += 1;
    }
    Ok(None)
}

fn unit_code(unit: Option<usize>) -> Result<i32, String> {
    match unit {
        Some(index) => {
            i32::try_from(index).map_err(|_| format!("--unit index out of range: {index}"))
        }
        None => Ok(-1),
    }
}

fn parse_on_off(value: Option<&String>, usage: &str) -> Result<bool, String> {
    match value.map(String::as_str) {
        Some("on" | "true" | "1") => Ok(true),
        Some("off" | "false" | "0") => Ok(false),
        _ => Err(format!("usage: {usage}")),
    }
}

fn unit_suffix(unit: Option<usize>) -> String {
    match unit {
        Some(index) => format!("unit {index}"),
        None => "both ACs".to_owned(),
    }
}

async fn cmd_set_linked(
    value: Option<&String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let linked = parse_on_off(value, "gree-ac-plasma --set-linked on|off")?;
    daemon_simple(
        "SetLinked",
        &linked,
        if linked {
            "Linked control on"
        } else {
            "Linked control off"
        },
    )
    .await
}

async fn cmd_set_power(
    value: Option<&String>,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let on = parse_on_off(value, "gree-ac-plasma --set-power on|off [--unit N|both]")?;
    let unit = parse_unit(args)?;
    let code: i32 = unit_code(unit)?;
    daemon_simple(
        "SetPower",
        &(code, on),
        &format!(
            "Power {} sent to {}",
            if on { "on" } else { "off" },
            unit_suffix(unit)
        ),
    )
    .await
}

async fn cmd_set_mode(
    value: Option<&String>,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = value.map(String::as_str).unwrap_or("");
    let mode = dbus::parse_mode(text)
        .ok_or("usage: gree-ac-plasma --set-mode Auto|Cool|Dry|Fan|Heat [--unit N|both]")?;
    let unit = parse_unit(args)?;
    let code: i32 = unit_code(unit)?;
    daemon_simple(
        "SetMode",
        &(code, mode.label()),
        &format!("Mode set to {} on {}", mode.label(), unit_suffix(unit)),
    )
    .await
}

async fn cmd_set_temp(
    value: Option<&String>,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp: u8 = value
        .map(String::as_str)
        .unwrap_or("")
        .parse()
        .map_err(|_| "usage: gree-ac-plasma --set-temp 16..30 [--unit N|both]")?;
    if !(crate::model::TEMP_MIN..=crate::model::TEMP_MAX).contains(&temp) {
        return Err(format!(
            "temperature must be between {} and {}",
            crate::model::TEMP_MIN,
            crate::model::TEMP_MAX
        )
        .into());
    }
    let unit = parse_unit(args)?;
    let code: i32 = unit_code(unit)?;
    daemon_simple(
        "SetTemp",
        &(code, temp),
        &format!("Temperature set to {temp} on {}", unit_suffix(unit)),
    )
    .await
}

async fn cmd_set_fan(
    value: Option<&String>,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = value.map(String::as_str).unwrap_or("");
    let fan = dbus::parse_fan(text).ok_or(
        "usage: gree-ac-plasma --set-fan Auto|Low|Medium-low|Medium|Medium-high|High [--unit N|both]",
    )?;
    let unit = parse_unit(args)?;
    let code: i32 = unit_code(unit)?;
    daemon_simple(
        "SetFan",
        &(code, fan.label()),
        &format!("Fan set to {} on {}", fan.label(), unit_suffix(unit)),
    )
    .await
}
