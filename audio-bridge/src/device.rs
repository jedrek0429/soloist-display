//! A persistent output-device override for the audio visualiser.
//! The file is separate from web display settings so a browser save cannot
//! inadvertently change the selected audio interface.

use crate::config_path;
use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device,
};
use std::{fs, io, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[cfg(target_os = "linux")]
fn host() -> Result<cpal::Host> {
    Ok(cpal::host_from_id(cpal::HostId::PulseAudio)?)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn host() -> Result<cpal::Host> {
    Ok(cpal::default_host())
}

fn selection_path() -> std::path::PathBuf {
    config_path().with_file_name("audio-device.id")
}

fn read_selection(path: &Path) -> io::Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let id = content.trim_end_matches(['\n', '\r']);
    if id.is_empty() || id.len() > 2048 || (id.contains('\r') || id.contains('\n')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid saved audio device ID",
        ));
    }
    Ok(Some(id.to_owned()))
}

fn write_selection(path: &Path, id: &str) -> io::Result<()> {
    if id.is_empty() || id.len() > 2048 || (id.contains('\r') || id.contains('\n')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid audio device ID",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "audio device path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, format!("{id}\n"))?;
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

fn clear_selection(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub fn selected_id() -> io::Result<Option<String>> {
    read_selection(&selection_path())
}

pub fn outputs() -> Result<Vec<Output>> {
    let host = host()?;
    let default = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let mut result = Vec::new();
    for device in host.output_devices()? {
        let id = device.id()?.to_string();
        let name = device.to_string();
        result.push(Output {
            is_default: default.as_deref() == Some(id.as_str()),
            id,
            name,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(result)
}

pub fn output() -> Result<Device> {
    let host = host()?;
    if let Some(selected) = selected_id()? {
        let id = selected.parse::<cpal::DeviceId>()?;
        let found = host
            .output_devices()?
            .find(|device| device.id().ok().as_ref() == Some(&id));
        return found.ok_or_else(|| {
            format!("selected visualiser output {selected} is unavailable; use 'device list' or 'device default'").into()
        });
    }
    host.default_output_device()
        .ok_or_else(|| "no default system audio output device".into())
}

fn resolve<'a>(devices: &'a [Output], selector: &str) -> Result<&'a Output> {
    if let Ok(number) = selector.parse::<usize>() {
        return number
            .checked_sub(1)
            .and_then(|index| devices.get(index))
            .ok_or_else(|| "device number out of range; run 'device list'".into());
    }

    let matches: Vec<_> = devices
        .iter()
        .filter(|device| device.id == selector || device.name == selector)
        .collect();
    match matches.as_slice() {
        [device] => Ok(device),
        [] => Err("audio output not found; run 'device list'".into()),
        _ => Err("device name is ambiguous; choose its number or stable ID".into()),
    }
}

pub fn set(selector: &str) -> Result<String> {
    let devices = outputs()?;
    let output = resolve(&devices, selector)?;
    write_selection(&selection_path(), &output.id)?;
    Ok(output.name.clone())
}

pub fn clear() -> io::Result<()> {
    clear_selection(&selection_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_by_list_number_stable_id_or_unique_name() {
        let devices = vec![
            Output { id: "wasapi:first".into(), name: "Speakers".into(), is_default: true },
            Output { id: "wasapi:second".into(), name: "Headphones".into(), is_default: false },
        ];
        assert_eq!(resolve(&devices, "2").unwrap().id, "wasapi:second");
        assert_eq!(resolve(&devices, "wasapi:second").unwrap().name, "Headphones");
        assert_eq!(resolve(&devices, "Headphones").unwrap().id, "wasapi:second");
        assert!(resolve(&devices, "0").is_err());
        assert!(resolve(&devices, "3").is_err());
    }

    #[test]
    fn refuses_ambiguous_names() {
        let devices = vec![
            Output { id: "id:a".into(), name: "HDMI".into(), is_default: true },
            Output { id: "id:b".into(), name: "HDMI".into(), is_default: false },
        ];
        assert!(resolve(&devices, "HDMI").is_err());
        assert_eq!(resolve(&devices, "id:b").unwrap().id, "id:b");
    }

    #[test]
    fn selection_survives_restart_and_can_be_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-device.id");
        assert_eq!(read_selection(&path).unwrap(), None);
        write_selection(&path, "wasapi:{test-device}").unwrap();
        assert_eq!(
            read_selection(&path).unwrap().as_deref(),
            Some("wasapi:{test-device}")
        );
        clear_selection(&path).unwrap();
        assert_eq!(read_selection(&path).unwrap(), None);
        clear_selection(&path).unwrap();
    }
    #[test]
    fn refuses_empty_or_corrupt_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-device.id");
        assert!(write_selection(&path, "").is_err());
        assert!(write_selection(&path, "wasapi:id\nother").is_err());
        fs::write(&path, "\n").unwrap();
        assert!(read_selection(&path).is_err());
    }
}
