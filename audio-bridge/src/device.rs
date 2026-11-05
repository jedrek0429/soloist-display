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
    if id.is_empty() || id.len() > 2048 || id.contains(['\r', '\n']) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid saved audio device ID"));
    }
    Ok(Some(id.to_owned()))
}

fn write_selection(path: &Path, id: &str) -> io::Result<()> {
    if id.is_empty() || id.len() > 2048 || id.contains(['\r', '\n']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid audio device ID"));
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "audio device path has no parent")
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

pub fn set(id: &str) -> Result<String> {
    let output = outputs()?
        .into_iter()
        .find(|device| device.id == id)
        .ok_or_else(|| "unknown output device ID; run 'soloist-display device list'".to_owned())?;
    write_selection(&selection_path(), id)?;
    Ok(output.name)
}

pub fn clear() -> io::Result<()> {
    clear_selection(&selection_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_survives_restart_and_can_be_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio-device.id");
        assert_eq!(read_selection(&path).unwrap(), None);
        write_selection(&path, "wasapi:{test-device}").unwrap();
        assert_eq!(read_selection(&path).unwrap().as_deref(), Some("wasapi:{test-device}"));
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
