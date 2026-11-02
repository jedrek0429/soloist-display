use crate::{
    paths::Paths,
    state::{atomic_json, now},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

const CURRENT_SCHEMA: u32 = 2;

#[derive(Serialize, Deserialize)]
struct Schema {
    schema: u32,
}

pub fn migrate(paths: &Paths) -> Result<()> {
    let config = paths.config.join("config.json");
    let schema_path = paths.state.join("config-schema.json");
    let schema = if schema_path.exists() {
        serde_json::from_slice::<Schema>(&fs::read(&schema_path)?)?.schema
    } else {
        0
    };
    if schema > CURRENT_SCHEMA {
        bail!("configuration schema {schema} is newer than this installer supports ({CURRENT_SCHEMA}); refusing a destructive downgrade");
    }
    if schema == CURRENT_SCHEMA {
        return Ok(());
    }
    if config.exists() {
        let bytes = fs::read(&config).context("read display configuration for migration")?;
        let mut value: Value = serde_json::from_slice(&bytes).map_err(|error| {
            anyhow!("display configuration is invalid; it was not changed: {error}")
        })?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("display configuration must be a JSON object"))?;
        let backups = paths.state.join("migration-backups");
        fs::create_dir_all(&backups)?;
        fs::write(
            backups.join(format!("config-schema-{schema}-{}.json", now())),
            &bytes,
        )?;
        if schema < 1 {
            for key in [
                "transitionStyle",
                "backdropTransitionStyle",
                "panelTransitionStyle",
            ] {
                if object.get(key).and_then(Value::as_str) == Some("instant") {
                    object.insert(key.into(), Value::String("off".into()));
                }
            }
        }
        if schema < 2 && object.get("discAppearance").and_then(Value::as_str) == Some("dot") {
            object.insert("discAppearance".into(), Value::String("basic".into()));
            if let Some(size) = object.remove("centerHoleDot") {
                object.insert("centerHoleBasic".into(), size);
            }
        }
        atomic_json(&config, &value, Some(0o600))?;
    }
    atomic_json(
        &schema_path,
        &Schema {
            schema: CURRENT_SCHEMA,
        },
        Some(0o600),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn migration_is_idempotent_and_keeps_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", directory.path());
        std::env::set_var("SOLOIST_DISPLAY_TEST_ROOT", directory.path().join("root"));
        let paths = Paths::discover().unwrap();
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(
            paths.config.join("config.json"),
            br#"{"transitionStyle":"instant","discAppearance":"dot","centerHoleDot":8}"#,
        )
        .unwrap();
        migrate(&paths).unwrap();
        migrate(&paths).unwrap();
        let value: Value =
            serde_json::from_slice(&fs::read(paths.config.join("config.json")).unwrap()).unwrap();
        assert_eq!(value["transitionStyle"], "off");
        assert_eq!(value["discAppearance"], "basic");
        assert_eq!(value["centerHoleBasic"], 8);
        assert_eq!(
            fs::read_dir(paths.state.join("migration-backups"))
                .unwrap()
                .count(),
            1
        );
        std::env::remove_var("SOLOIST_DISPLAY_TEST_ROOT");
    }
}
