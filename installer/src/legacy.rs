use crate::{
    paths::Paths,
    service::{self, ManagedRuntime},
    soloist,
};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
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
    let prefix = format!("{}{}", '
    }
    Ok(value.to_owned())
}

fn parse_unit(paths: &Paths, unit: &str) -> Result<ExistingSettings> {
    let mut env = HashMap::new();
    let mut env_files = Vec::new();
    let mut exec = None;
    let mut service_section = false;
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            service_section = line == "[Service]";
        }
        if !service_section || line.starts_with('#') {
            continue;
        }
        if let Some(line) = line.strip_prefix("Environment=") {
            for assignment in words(line)? {
                let (key, value) = assignment
                    .split_once('=')
                    .context("invalid Environment assignment")?;
                env.insert(key.to_owned(), value.to_owned());
            }
        } else if let Some(line) = line.strip_prefix("EnvironmentFile=") {
            env_files.extend(words(line)?);
        } else if let Some(line) = line.strip_prefix("ExecStart=") {
            exec = if line.is_empty() {
                None
            } else {
                Some(line.to_owned())
            };
        }
    }
    for env_file in env_files {
        let optional = env_file.starts_with('-');
        let path = PathBuf::from(env_file.trim_start_matches('-'));
        if !path.is_absolute() {
            bail!("relative Soloist EnvironmentFile cannot be migrated");
        }
        if optional && !path.exists() {
            continue;
        }
        for line in fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?
            .lines()
            .map(str::trim)
        {
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
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
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
            "--device-name" | "--name" => settings.name = Some(value),
            "--api-key" => settings.api_key = Some(value),
            "--data-dir" | "-D" => settings.runtime.data_dir = PathBuf::from(value),
            "--cache-dir" => settings.runtime.cache_dir = PathBuf::from(value),
            "--ws" => {
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
, '{');
    if let Some(key) = value
        .strip_prefix(&prefix)
        .and_then(|v| v.strip_suffix('}'))
    {
        return env
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("unset environment variable in existing Soloist service"));
    }
    if let Some(key) = value.strip_prefix('
    }
    Ok(value.to_owned())
}

fn parse_unit(paths: &Paths, unit: &str) -> Result<ExistingSettings> {
    let mut env = HashMap::new();
    let mut env_files = Vec::new();
    let mut exec = None;
    let mut service_section = false;
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            service_section = line == "[Service]";
        }
        if !service_section || line.starts_with('#') { continue; }
        if let Some(line) = line.strip_prefix("Environment=") {
            for assignment in words(line)? {
                let (key, value) = assignment.split_once('=').context("invalid Environment assignment")?;
                env.insert(key.to_owned(), value.to_owned());
            }
        } else if let Some(line) = line.strip_prefix("EnvironmentFile=") {
            env_files.extend(words(line)?);
        } else if let Some(line) = line.strip_prefix("ExecStart=") {
            exec = if line.is_empty() { None } else { Some(line.to_owned()) };
        }
    }
    for env_file in env_files {
        let optional = env_file.starts_with('-');
        let path = PathBuf::from(env_file.trim_start_matches('-'));
        if !path.is_absolute() { bail!("relative Soloist EnvironmentFile cannot be migrated"); }
        if optional && !path.exists() { continue; }
        for line in fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') { continue; }
            let (key, raw) = line.split_once('=').context("invalid EnvironmentFile assignment")?;
            let values = words(raw.trim())?;
            if values.len() != 1 { bail!("ambiguous EnvironmentFile value"); }
            env.insert(key.trim().to_owned(), values[0].clone());
        }
    }
    let exec = exec.context("existing Soloist has no recognizable ExecStart")?;
    let args = words(&exec)?;
    if args.first().and_then(|a| Path::new(a).file_name()).is_none_or(|n| n != "soloist") {
        bail!("existing unit does not start a recognizable Soloist binary");
    }
    let mut settings = ExistingSettings {
        name: env.get("SOLOIST_NAME").cloned(),
        api_key: env.get("SOLOIST_API_KEY").cloned(),
        runtime: ManagedRuntime::default_for(paths),
    };
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let (flag, raw) = if let Some((flag, value)) = arg.split_once('=') {
            (flag, value.to_owned())
        } else {
            i += 1;
            (arg.as_str(), args.get(i).cloned().context("missing Soloist option value")?)
        };
        let value = expand(&raw, &env)?;
        match flag {
            "--device-name" | "--name" => settings.name = Some(value),
            "--api-key" => settings.api_key = Some(value),
            "--data-dir" | "-D" => settings.runtime.data_dir = PathBuf::from(value),
            "--cache-dir" => settings.runtime.cache_dir = PathBuf::from(value),
            "--ws" => {
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
    if !service::soloist_unit_present(paths) { return Ok(None); }
    let unit = if paths.is_test() {
        fs::read_to_string(&paths.soloist_service)?
    } else {
        let output = Command::new("systemctl")
            .args(["--user", "cat", "soloist.service"]).output()
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
        fs::write(&env_file, "SOLOIST_NAME=\"Living Room\"\nSOLOIST_API_KEY=secret\n").unwrap();
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
    fn rejects_unknown_flags() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        assert!(parse_unit(&paths, "[Service]\nExecStart=/usr/bin/soloist --unknown yes\n").is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
) {
        return env
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("unset environment variable in existing Soloist service"));
    }
    if value.contains('
    }
    Ok(value.to_owned())
}

fn parse_unit(paths: &Paths, unit: &str) -> Result<ExistingSettings> {
    let mut env = HashMap::new();
    let mut env_files = Vec::new();
    let mut exec = None;
    let mut service_section = false;
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            service_section = line == "[Service]";
        }
        if !service_section || line.starts_with('#') { continue; }
        if let Some(line) = line.strip_prefix("Environment=") {
            for assignment in words(line)? {
                let (key, value) = assignment.split_once('=').context("invalid Environment assignment")?;
                env.insert(key.to_owned(), value.to_owned());
            }
        } else if let Some(line) = line.strip_prefix("EnvironmentFile=") {
            env_files.extend(words(line)?);
        } else if let Some(line) = line.strip_prefix("ExecStart=") {
            exec = if line.is_empty() { None } else { Some(line.to_owned()) };
        }
    }
    for env_file in env_files {
        let optional = env_file.starts_with('-');
        let path = PathBuf::from(env_file.trim_start_matches('-'));
        if !path.is_absolute() { bail!("relative Soloist EnvironmentFile cannot be migrated"); }
        if optional && !path.exists() { continue; }
        for line in fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') { continue; }
            let (key, raw) = line.split_once('=').context("invalid EnvironmentFile assignment")?;
            let values = words(raw.trim())?;
            if values.len() != 1 { bail!("ambiguous EnvironmentFile value"); }
            env.insert(key.trim().to_owned(), values[0].clone());
        }
    }
    let exec = exec.context("existing Soloist has no recognizable ExecStart")?;
    let args = words(&exec)?;
    if args.first().and_then(|a| Path::new(a).file_name()).is_none_or(|n| n != "soloist") {
        bail!("existing unit does not start a recognizable Soloist binary");
    }
    let mut settings = ExistingSettings {
        name: env.get("SOLOIST_NAME").cloned(),
        api_key: env.get("SOLOIST_API_KEY").cloned(),
        runtime: ManagedRuntime::default_for(paths),
    };
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let (flag, raw) = if let Some((flag, value)) = arg.split_once('=') {
            (flag, value.to_owned())
        } else {
            i += 1;
            (arg.as_str(), args.get(i).cloned().context("missing Soloist option value")?)
        };
        let value = expand(&raw, &env)?;
        match flag {
            "--device-name" | "--name" => settings.name = Some(value),
            "--api-key" => settings.api_key = Some(value),
            "--data-dir" | "-D" => settings.runtime.data_dir = PathBuf::from(value),
            "--cache-dir" => settings.runtime.cache_dir = PathBuf::from(value),
            "--ws" => {
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
    if !service::soloist_unit_present(paths) { return Ok(None); }
    let unit = if paths.is_test() {
        fs::read_to_string(&paths.soloist_service)?
    } else {
        let output = Command::new("systemctl")
            .args(["--user", "cat", "soloist.service"]).output()
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
        fs::write(&env_file, "SOLOIST_NAME=\"Living Room\"\nSOLOIST_API_KEY=secret\n").unwrap();
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
    fn rejects_unknown_flags() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        assert!(parse_unit(&paths, "[Service]\nExecStart=/usr/bin/soloist --unknown yes\n").is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
) || value.contains('%') {
        bail!("unsupported expansion in existing Soloist service; use external mode");
    }
    Ok(value.to_owned())
}

fn parse_unit(paths: &Paths, unit: &str) -> Result<ExistingSettings> {
    let mut env = HashMap::new();
    let mut env_files = Vec::new();
    let mut exec = None;
    let mut service_section = false;
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            service_section = line == "[Service]";
        }
        if !service_section || line.starts_with('#') { continue; }
        if let Some(line) = line.strip_prefix("Environment=") {
            for assignment in words(line)? {
                let (key, value) = assignment.split_once('=').context("invalid Environment assignment")?;
                env.insert(key.to_owned(), value.to_owned());
            }
        } else if let Some(line) = line.strip_prefix("EnvironmentFile=") {
            env_files.extend(words(line)?);
        } else if let Some(line) = line.strip_prefix("ExecStart=") {
            exec = if line.is_empty() { None } else { Some(line.to_owned()) };
        }
    }
    for env_file in env_files {
        let optional = env_file.starts_with('-');
        let path = PathBuf::from(env_file.trim_start_matches('-'));
        if !path.is_absolute() { bail!("relative Soloist EnvironmentFile cannot be migrated"); }
        if optional && !path.exists() { continue; }
        for line in fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') { continue; }
            let (key, raw) = line.split_once('=').context("invalid EnvironmentFile assignment")?;
            let values = words(raw.trim())?;
            if values.len() != 1 { bail!("ambiguous EnvironmentFile value"); }
            env.insert(key.trim().to_owned(), values[0].clone());
        }
    }
    let exec = exec.context("existing Soloist has no recognizable ExecStart")?;
    let args = words(&exec)?;
    if args.first().and_then(|a| Path::new(a).file_name()).is_none_or(|n| n != "soloist") {
        bail!("existing unit does not start a recognizable Soloist binary");
    }
    let mut settings = ExistingSettings {
        name: env.get("SOLOIST_NAME").cloned(),
        api_key: env.get("SOLOIST_API_KEY").cloned(),
        runtime: ManagedRuntime::default_for(paths),
    };
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        let (flag, raw) = if let Some((flag, value)) = arg.split_once('=') {
            (flag, value.to_owned())
        } else {
            i += 1;
            (arg.as_str(), args.get(i).cloned().context("missing Soloist option value")?)
        };
        let value = expand(&raw, &env)?;
        match flag {
            "--device-name" | "--name" => settings.name = Some(value),
            "--api-key" => settings.api_key = Some(value),
            "--data-dir" | "-D" => settings.runtime.data_dir = PathBuf::from(value),
            "--cache-dir" => settings.runtime.cache_dir = PathBuf::from(value),
            "--ws" => {
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
    if !service::soloist_unit_present(paths) { return Ok(None); }
    let unit = if paths.is_test() {
        fs::read_to_string(&paths.soloist_service)?
    } else {
        let output = Command::new("systemctl")
            .args(["--user", "cat", "soloist.service"]).output()
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
        fs::write(&env_file, "SOLOIST_NAME=\"Living Room\"\nSOLOIST_API_KEY=secret\n").unwrap();
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
    fn rejects_unknown_flags() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", temp.path().join("root"));
        let paths = Paths::discover().unwrap();
        assert!(parse_unit(&paths, "[Service]\nExecStart=/usr/bin/soloist --unknown yes\n").is_err());
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
