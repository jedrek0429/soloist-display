//! Select the system output captured by the spectrum bridge, never Spotify's device.
//! Device IDs are CPAL identifiers; human names and list indices are transient.

use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device, Host,
};
use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Selection {
    Default,
    Id(String),
}

#[derive(Clone, Debug)]
struct DeviceInfo {
    id: String,
    raw_id: String,
    name: String,
    system_default: bool,
}

fn selection_path() -> PathBuf {
    crate::config_path().with_file_name("audio-device.json")
}

fn read_from(path: &Path) -> Result<Selection> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Selection::Default),
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_str(&contents)?;
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("invalid audio-device.json: expected a nonempty device ID")?;
    Ok(Selection::Id(id.to_owned()))
}

fn current_selection() -> Result<Selection> {
    read_from(&selection_path())
}

fn save_to(path: &Path, selected: &Selection) -> Result<()> {
    let Selection::Id(id) = selected else {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), stamp));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&serde_json::json!({"id": id}))?,
    )?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

fn host() -> Result<Host> {
    #[cfg(target_os = "linux")]
    {
        Ok(cpal::host_from_id(cpal::HostId::PulseAudio)?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(cpal::default_host())
    }
}

/// Only output devices, or the PulseAudio output monitors on Linux.
/// Never expose microphone inputs.
fn available() -> Result<Vec<(Device, DeviceInfo)>> {
    let host = host()?;
    let default_raw = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.id().to_owned());

    #[cfg(target_os = "linux")]
    let devices: Vec<Device> = host.input_devices()?.collect();
    #[cfg(not(target_os = "linux"))]
    let devices: Vec<Device> = host.output_devices()?.collect();

    let mut result = Vec::new();
    for device in devices {
        let id = device.id()?;
        let raw_id = id.id().to_owned();
        #[cfg(target_os = "linux")]
        if !raw_id.ends_with(".monitor") {
            continue;
        }
        #[cfg(target_os = "linux")]
        let system_default = default_raw
            .as_ref()
            .is_some_and(|raw| raw_id == format!("{raw}.monitor"));
        #[cfg(not(target_os = "linux"))]
        let system_default = default_raw.as_deref() == Some(raw_id.as_str());

        result.push((
            device.clone(),
            DeviceInfo {
                id: id.to_string(),
                raw_id,
                name: device.to_string(),
                system_default,
            },
        ));
    }
    Ok(result)
}

fn default_index(devices: &[(Device, DeviceInfo)]) -> Result<usize> {
    #[cfg(target_os = "linux")]
    if let Some(preferred) = env::var("SOLOIST_DISPLAY_MONITOR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return devices
            .iter()
            .position(|(_, d)| d.id == preferred || d.raw_id == preferred || d.name == preferred)
            .ok_or_else(|| "requested PulseAudio monitor source does not exist".into());
    }

    devices
        .iter()
        .position(|(_, d)| d.system_default)
        .or_else(|| {
            #[cfg(target_os = "linux")]
            {
                (!devices.is_empty()).then_some(0)
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        })
        .ok_or_else(|| "no system audio output found (microphones are not supported)".into())
}

fn active_index(devices: &[(Device, DeviceInfo)], selected: &Selection) -> Result<usize> {
    match selected {
        Selection::Default => default_index(devices),
        Selection::Id(id) => devices
            .iter()
            .position(|(_, d)| d.id == *id)
            .ok_or_else(|| format!("selected audio output {id} is unavailable; use 'device list' or 'device default'").into()),
    }
}

/// Preserve the OS's default output handle rather than pinning it to an
/// enumerated endpoint. The key also includes the selection mode, so switching
/// from a pinned device to default reopens the stream even if IDs are equal.
pub fn active_device() -> Result<(Device, bool, String, String)> {
    let selection = current_selection()?;
    #[cfg(not(target_os = "linux"))]
    if selection == Selection::Default {
        let device = host()?
            .default_output_device()
            .ok_or("no default system audio output device")?;
        let id = device.id()?.to_string();
        return Ok((device, true, id.clone(), format!("default:{id}")));
    }

    let devices = available()?;
    let index = active_index(&devices, &selection)?;
    let (device, info) = devices
        .into_iter()
        .nth(index)
        .ok_or("audio output vanished")?;
    let key = match selection {
        Selection::Default => format!("default:{}", info.id),
        Selection::Id(_) => format!("pinned:{}", info.id),
    };
    Ok((device, cfg!(not(target_os = "linux")), info.id, key))
}

fn choose_index(devices: &[(Device, DeviceInfo)], selector: &str) -> Result<usize> {
    let info = devices.iter().map(|(_, info)| info).collect::<Vec<_>>();
    resolve_selector(&info, selector)
}

fn resolve_selector(devices: &[&DeviceInfo], selector: &str) -> Result<usize> {
    if let Ok(number) = selector.parse::<usize>() {
        return number
            .checked_sub(1)
            .filter(|&index| index < devices.len())
            .ok_or_else(|| "device number is out of range; run 'device list'".into());
    }
    let matches: Vec<usize> = devices
        .iter()
        .enumerate()
        .filter(|(_, info)| info.id == selector || info.name == selector)
        .map(|(index, _)| index)
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err("no matching audio output; run 'device list'".into()),
        _ => Err("several audio outputs have that name; select a number or device ID".into()),
    }
}

pub fn list() -> Result<()> {
    let devices = available()?;
    let selection = current_selection()?;
    let default_choice = if selection == Selection::Default {
        default_index(&devices).ok()
    } else {
        None
    };
    println!("Audio bridge devices (system output captured for the audio visualiser):");
    if devices.is_empty() {
        println!("  No audio outputs available.");
    }
    for (index, (_, info)) in devices.iter().enumerate() {
        let selected = match &selection {
            Selection::Default => default_choice == Some(index),
            Selection::Id(id) => id == &info.id,
        };
        println!(
            "  {}. {}{}{}",
            index + 1,
            info.name,
            if info.system_default {
                " [system default]"
            } else {
                ""
            },
            if selected { " [selected]" } else { "" }
        );
        println!("     ID: {}", info.id);
    }
    match selection {
        Selection::Default => println!("Selection: system default output"),
        Selection::Id(id) => {
            let present = devices.iter().any(|(_, info)| info.id == id);
            println!(
                "Selection: {id}{}",
                if present {
                    ""
                } else {
                    " (unavailable; no fallback)"
                }
            );
        }
    }
    println!("Select: soloist-display device use <number|ID|exact-name>");
    println!("Reset:  soloist-display device default");
    Ok(())
}

pub fn use_device(selector: &str) -> Result<()> {
    let devices = available()?;
    let index = choose_index(&devices, selector)?;
    let (device, info) = &devices[index];
    #[cfg(target_os = "linux")]
    device.default_input_config()?;
    #[cfg(not(target_os = "linux"))]
    device.default_output_config()?;
    save_to(&selection_path(), &Selection::Id(info.id.clone()))?;
    println!("Audio visualiser input: {} ({})", info.name, info.id);
    println!("The running bridge will switch shortly, if it is active.");
    Ok(())
}

pub fn use_default() -> Result<()> {
    save_to(&selection_path(), &Selection::Default)?;
    println!("Audio visualiser input: system default output");
    println!("The running bridge will switch shortly, if it is active.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            raw_id: id.to_owned(),
            name: name.to_owned(),
            system_default: false,
        }
    }

    // Exercise the same selector used for real CPAL devices, without audio hardware.
    fn select_info(devices: &[DeviceInfo], selector: &str) -> Result<usize> {
        let info = devices.iter().collect::<Vec<_>>();
        resolve_selector(&info, selector)
    }

    #[test]
    fn stable_id_number_and_unique_name_can_select_the_same_device() {
        let items = [fake("wasapi:a", "Speakers"), fake("wasapi:b", "Headphones")];
        assert_eq!(select_info(&items, "2").unwrap(), 1);
        assert_eq!(select_info(&items, "wasapi:b").unwrap(), 1);
        assert_eq!(select_info(&items, "Headphones").unwrap(), 1);
        assert!(select_info(&items, "0").is_err());
        assert!(select_info(&items, "3").is_err());
    }

    #[test]
    fn duplicate_names_are_not_silently_resolved() {
        let items = [fake("id-a", "HDMI"), fake("id-b", "HDMI")];
        assert!(select_info(&items, "HDMI").is_err());
        assert_eq!(select_info(&items, "id-b").unwrap(), 1);
    }

    #[test]
    fn selection_is_persistent_and_default_clears_override() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-device.json");
        assert_eq!(read_from(&path).unwrap(), Selection::Default);
        save_to(&path, &Selection::Id("wasapi:long-id".into())).unwrap();
        assert_eq!(
            read_from(&path).unwrap(),
            Selection::Id("wasapi:long-id".into())
        );
        save_to(&path, &Selection::Id("wasapi:new-id".into())).unwrap();
        assert_eq!(
            read_from(&path).unwrap(),
            Selection::Id("wasapi:new-id".into())
        );
        save_to(&path, &Selection::Default).unwrap();
        assert_eq!(read_from(&path).unwrap(), Selection::Default);
    }

    #[test]
    fn malformed_selection_never_becomes_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-device.json");
        fs::write(&path, r#"{"id":null}"#).unwrap();
        assert!(read_from(&path).is_err());
        fs::write(&path, r#"{"id":"wasapi:a"}"#).unwrap();
        assert_eq!(read_from(&path).unwrap(), Selection::Id("wasapi:a".into()));
    }
}
