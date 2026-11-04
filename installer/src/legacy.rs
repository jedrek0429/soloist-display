use crate::{
    paths::Paths,
    service::{self, ManagedRuntime},
    soloist,
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::HashMap,
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub struct ExistingSettings {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub runtime: ManagedRuntime,
}

fn words(input: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quoted = None;
    let mut escaped = false;
    let mut started = false;
    for c in input.chars() {
        if escaped {
            word.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if let Some(q) = quoted {
            if c == q {
                quoted = None;
            } else {
                word.push(c);
            }
        } else if c == '"' || c == '\'' {
            quoted = Some(c);
        } else if c.is_whitespace() {
            if started {
                result.push(std::mem::take(&mut word));
                started = false;
            }
            continue;
        } else {
            word.push(c);
        }
        started = true;
    }
    if quoted.is_some() || escaped {
        bail!("invalid quoting in existing Soloist service");
    }
    if started {
        result.push(word);
    }
    Ok(result)
}

fn expand(value: &str, env: &HashMap<String, String>) -> Result<String> {
    if let Some(raw) = value.strip_prefix(char::from(36)) {
        let key = raw.trim_start_matches('{').trim_end_matches('}');
        return env
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("unset environment variable in existing Soloist service"));
    }
    if value.contains(char::from(36)) || value.contains('%') {
        bail!("unsupported expansion in existing Soloist service; use external mode");
    }
    Ok(value.to_owned())
}

fn environment_file_path(paths: &Paths, value: &str) -> Result<PathBuf> {
    let path = if let Some(relative) = value.strip_prefix("%h/") {
        paths.home.join(relative)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute()
        || path.to_string_lossy().contains('%')
        || path.components().any(|part| part == Component::ParentDir)
    {
        bail!("unsupported Soloist EnvironmentFile path: {value}; use external mode");
    }
    Ok(path)
}

fn parse_unit(paths: &Paths, unit: &str) -> Result<ExistingSettings> {
    let mut env = HashMap::new();
    let mut env_files = Vec::new();
    let mut exec = None;
    let mut exec_count = 0;
    let mut section = "";
    for line in unit.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = match line {
                "[Unit]" => "Unit",
                "[Service]" => "Service",
                "[Install]" => "Install",
                _ => bail!("existing Soloist has an unsupported systemd section: {line}"),
            };
            continue;
        }
        let (directive, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid systemd directive"))?;
        match section {
            "Unit" => match directive {
                "Description" => {}
                "After" => {
                    for dependency in value.split_whitespace() {
                        if !matches!(
                            dependency,
                            "sound.target"
                                | "pipewire.service"
                                | "pipewire-pulse.service"
                                | "pulseaudio.service"
                        ) {
                            bail!("existing Soloist depends on {dependency}; use external mode");
                        }
                    }
                }
                _ => bail!("existing Soloist has an unsupported [Unit] directive: {directive}"),
            },
            "Install" => {
                if directive != "WantedBy" || value.trim() != "default.target" {
                    bail!("existing Soloist uses an unsupported [Install] directive: {directive}");
                }
            }
            "Service" => {
                match directive {
                    "Environment" | "EnvironmentFile" | "ExecStart" => {}
                    "Type" if value == "simple" => {}
                    "Restart" if value == "on-failure" => {}
                    "RestartSec" if matches!(value, "5" | "5s") => {}
                    "StartLimitIntervalSec" if matches!(value, "60" | "60s") => {}
                    "StartLimitBurst" if value == "5" => {}
                    _ => bail!("existing Soloist uses an unsupported [Service] setting: {directive}={value}"),
                }
                if directive == "Environment" {
                    for assignment in words(value)? {
                        let (key, setting) = assignment
                            .split_once('=')
                            .context("invalid Environment assignment")?;
                        env.insert(key.to_owned(), setting.to_owned());
                    }
                } else if directive == "EnvironmentFile" {
                    if value.is_empty() {
                        env_files.clear();
                    } else {
                        env_files.extend(words(value)?);
                    }
                } else if directive == "ExecStart" {
                    if value.is_empty() {
                        exec = None;
                        exec_count = 0;
                    } else {
                        exec = Some(value.to_owned());
                        exec_count += 1;
                    }
                }
            }
            _ => bail!("existing Soloist has settings outside a recognized systemd section"),
        }
    }
    for env_file in env_files {
        let (optional, value) = match env_file.strip_prefix('-') {
            Some(value) => (true, value),
            None => (false, env_file.as_str()),
        };
        let path = environment_file_path(paths, value)?;
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if optional && error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        for line in contents.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, raw) = line
                .split_once('=')
                .context("invalid EnvironmentFile assignment")?;
            let values = words(raw.trim())?;
            if values.len() != 1 {
                bail!("ambiguous EnvironmentFile value");
            }
            env.insert(key.trim().to_owned(), values[0].clone());
        }
    }
    for key in env.keys() {
        if !matches!(
            key.as_str(),
            "SOLOIST_NAME" | "SOLOIST_API_KEY" | "HOME" | "STATE_DIRECTORY" | "CACHE_DIRECTORY"
        ) {
            bail!("existing Soloist uses an unsupported environment variable: {key}");
        }
    }
    for key in ["SOLOIST_NAME", "SOLOIST_API_KEY"] {
        env.entry(key.to_owned()).or_default();
    }
    if exec_count != 1 {
        bail!("existing Soloist has multiple ExecStart commands; use external mode");
    }
    let exec = exec.context("existing Soloist has no recognizable ExecStart")?;
    let args = words(&exec)?;
    if args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .is_none_or(|n| n != "soloist")
    {
        bail!("existing unit does not start a recognizable Soloist binary");
    }
    let mut settings = ExistingSettings {
        name: env.get("SOLOIST_NAME").cloned(),
        api_key: env.get("SOLOIST_API_KEY").cloned(),
        runtime: ManagedRuntime::default_for(paths),
    };
    if let Some(home) = env.get("HOME") {
        if Path::new(home) != paths.home {
            bail!("existing Soloist uses a different HOME; specify its data directory explicitly");
        }
    }
    if let Some(state_dir) = env.get("STATE_DIRECTORY") {
        settings.runtime.data_dir = PathBuf::from(state_dir);
    }
    if let Some(cache_dir) = env.get("CACHE_DIRECTORY") {
        settings.runtime.cache_dir = PathBuf::from(cache_dir);
    }
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--verbose" || arg == "-v" {
            settings.runtime.verbose = true;
            i += 1;
            continue;
        }
        let (flag, raw) = if let Some((flag, value)) = arg.split_once('=') {
            (flag, value.to_owned())
        } else {
            i += 1;
            (
                arg.as_str(),
                args.get(i)
                    .cloned()
                    .context("missing Soloist option value")?,
            )
        };
        let value = expand(&raw, &env)?;
        match flag {
            "--device-name" | "-n" => settings.name = Some(value),
            "--api-key" | "-k" => settings.api_key = Some(value),
            "--data-dir" | "-D" => settings.runtime.data_dir = PathBuf::from(value),
            "--cache-dir" | "-C" => settings.runtime.cache_dir = PathBuf::from(value),
            "--cache-size" | "-z" => {
                let size = value.parse::<u64>().context("invalid Soloist cache size")?;
                if size != 0 && size < 100 {
                    bail!("invalid Soloist cache size");
                }
                settings.runtime.cache_size_mb = Some(size);
            }
            "--pipewire-device" | "-d" => settings.runtime.pipewire_device = Some(value),
            "--initial-volume" | "-i" => {
                let volume = value.parse::<u8>().context("invalid Soloist volume")?;
                if volume > 100 {
                    bail!("Soloist initial volume must be at most 100");
                }
                settings.runtime.initial_volume = Some(volume);
            }
            "--ws" | "-w" => {
                let ws = if value.starts_with("ws://") { value } else { format!("ws://{value}") };
                settings.runtime.websocket_url = soloist::validate_websocket_url(&ws)?;
            }
            _ => bail!("existing Soloist uses unsupported options; use --external-soloist or supply a compatible unit"),
        }
        i += 1;
    }
    if !settings.runtime.data_dir.is_absolute() || !settings.runtime.cache_dir.is_absolute() {
        bail!("existing Soloist data/cache paths are not absolute; use external mode");
    }
    Ok(settings)
}

pub fn inspect(paths: &Paths) -> Result<Option<ExistingSettings>> {
    if !service::soloist_unit_present(paths) {
        return Ok(None);
    }
    let unit = if paths.is_test() {
        fs::read_to_string(&paths.soloist_service)?
    } else {
        let drop_ins = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "soloist.service",
                "--property=DropInPaths",
                "--value",
            ])
            .output()
            .context("inspect Soloist drop-in configuration")?;
        if !drop_ins.status.success() {
            bail!("cannot check existing Soloist drop-in configuration");
        }
        if !String::from_utf8(drop_ins.stdout)
            .context("invalid systemd drop-in path")?
            .trim()
            .is_empty()
        {
            bail!("existing Soloist uses systemd drop-ins; use external mode to preserve them");
        }
        let output = Command::new("systemctl")
            .args(["--user", "cat", "soloist.service"])
            .output()
            .context("inspect existing Soloist service")?;
        if !output.status.success() {
            bail!("cannot inspect existing Soloist service; use --external-soloist");
        }
        String::from_utf8(output.stdout).context("Soloist unit contains invalid UTF-8")?
    };
    parse_unit(paths, &unit).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    #[test]
    #[serial]
    fn migrates_custom_data_cache_and_websocket() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let env_file = temp.path().join("soloist.env");
        fs::write(
            &env_file,
            "SOLOIST_NAME=\"Living Room\"\nSOLOIST_API_KEY=secret\n",
        )
        .unwrap();
        let unit = format!("[Service]\nEnvironmentFile={}\nExecStart=/usr/bin/soloist --device-name $SOLOIST_NAME --api-key $SOLOIST_API_KEY --data-dir {} --cache-dir {} --ws 127.0.0.1:9190\n", env_file.display(), temp.path().join("data").display(), temp.path().join("cache").display());
        let parsed = parse_unit(&paths, &unit).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Living Room"));
        assert_eq!(parsed.api_key.as_deref(), Some("secret"));
        assert_eq!(parsed.runtime.websocket_url, "ws://127.0.0.1:9190/");
        assert_eq!(parsed.runtime.data_dir, temp.path().join("data"));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }


    #[test]
    #[serial]
    fn migrates_home_specifier_environment_file() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let config = temp.path().join(".config/soloist");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("soloist.env"),
            "SOLOIST_NAME=\"Living Room\"\nSOLOIST_API_KEY=secret\n",
        )
        .unwrap();
        let unit = "[Service]\nEnvironmentFile=-%h/.config/soloist/missing.env %h/.config/soloist/soloist.env\nExecStart=/usr/bin/soloist --device-name $SOLOIST_NAME --api-key $SOLOIST_API_KEY\n";
        let parsed = parse_unit(&paths, unit).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("Living Room"));
        assert_eq!(parsed.api_key.as_deref(), Some("secret"));
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn refuses_relative_and_unknown_environment_file_paths() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        for file in [
            "soloist.env",
            "~/.config/soloist.env",
            "$HOME/.config/soloist.env",
            "%E/soloist.env",
            "%h/../other.env",
        ] {
            let unit = format!(
                "[Service]\nEnvironmentFile={file}\nExecStart=/usr/bin/soloist\n"
            );
            assert!(parse_unit(&paths, &unit).is_err(), "accepted {file}");
        }
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn refuses_missing_required_environment_file() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let unit = "[Service]\nEnvironmentFile=%h/missing.env\nExecStart=/usr/bin/soloist\n";
        assert!(parse_unit(&paths, unit).is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn missing_legacy_credentials_can_be_reentered() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let missing = temp.path().join("missing.env");
        let unit = format!(
            "[Service]\nEnvironmentFile=-{}\nExecStart=/usr/bin/soloist --device-name ${{SOLOIST_NAME}} --api-key ${{SOLOIST_API_KEY}} --ws 127.0.0.1:9090\n",
            missing.display()
        );
        let settings = parse_unit(&paths, &unit).unwrap();
        assert_eq!(settings.name.as_deref(), Some(""));
        assert_eq!(settings.api_key.as_deref(), Some(""));
        assert!(!missing.exists());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn migrates_documented_short_flags_and_environment_paths() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let unit = format!(
            "[Service]\nEnvironment=STATE_DIRECTORY={} CACHE_DIRECTORY={}\nExecStart=/usr/bin/soloist -n Speaker -k secret -w 127.0.0.1:9190 -C {} -z 500 -d alsa_output.usb -i 36 -v\n",
            temp.path().join("session").display(),
            temp.path().join("cache").display(),
            temp.path().join("cache").display()
        );
        let parsed = parse_unit(&paths, &unit).unwrap();
        assert_eq!(parsed.runtime.data_dir, temp.path().join("session"));
        assert_eq!(parsed.runtime.cache_size_mb, Some(500));
        assert_eq!(
            parsed.runtime.pipewire_device.as_deref(),
            Some("alsa_output.usb")
        );
        assert_eq!(parsed.runtime.initial_volume, Some(36));
        assert!(parsed.runtime.verbose);
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rejects_unsupported_systemd_specifiers_before_migration() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        assert!(parse_unit(
            &paths,
            "[Service]\nExecStart=/usr/bin/soloist -n %H -k secret\n"
        )
        .is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
    #[test]
    #[serial]
    fn refuses_units_with_unhandled_service_directives() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let unit = "[Service]\nType=simple\nStateDirectory=soloist\nExecStart=/usr/bin/soloist -n Speaker -k secret\n";
        assert!(parse_unit(&paths, unit).is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn refuses_unpreserved_unit_dependencies_and_targets() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        for unit in [
            "[Unit]\nRequires=pipewire.service\nAfter=pipewire.service\n[Service]\nExecStart=/usr/bin/soloist\n",
            "[Unit]\nAfter=network-online.target\n[Service]\nExecStart=/usr/bin/soloist\n",
            "[Unit]\nWants=network.target\n[Service]\nExecStart=/usr/bin/soloist\n",
            "[Service]\nExecStart=/usr/bin/soloist\n[Install]\nWantedBy=multi-user.target\n",
            "[Service]\nRestart=always\nExecStart=/usr/bin/soloist\n",
        ] {
            assert!(parse_unit(&paths, unit).is_err(), "migration accepted: {unit}");
        }
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn accepts_only_reproducible_unit_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        let unit = "[Unit]\nDescription=Spotify Soloist\nAfter=pipewire.service sound.target\n[Service]\nType=simple\nRestart=on-failure\nExecStart=/usr/bin/soloist\n[Install]\nWantedBy=default.target\n";
        assert!(parse_unit(&paths, unit).is_ok());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }

    #[test]
    #[serial]
    fn rejects_unknown_flags() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        assert!(parse_unit(
            &paths,
            "[Service]\nExecStart=/usr/bin/soloist --unknown yes\n"
        )
        .is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
