use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::SinkExt;
mod audio;

use serde_json::Value;
use std::{env, path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::broadcast, time::Duration};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const DEFAULT_CONFIG: &str = include_str!("../../config.json");
const INDEX: &str = include_str!("../../index.html");
const CSS: &str = include_str!("../../styles.css");
const SETTINGS_HTML: &str = include_str!("../../settings.html");
const SETTINGS_JS: &str = include_str!("../../src/settings.js");
const MAIN_JS: &str = include_str!("../../src/main.js");
const MOTION_JS: &str = include_str!("../../src/motion.js");
const SPECTRUM_JS: &str = include_str!("../../src/spectrum.js");
const SOLOIST_JS: &str = include_str!("../../src/soloist.js");

#[derive(Clone)]
struct WebState {
    path: Arc<PathBuf>,
}

fn config_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("soloist-display/config.json")
}

fn read_settings(path: &PathBuf) -> Value {
    let defaults: Value = serde_json::from_str(DEFAULT_CONFIG).expect("valid built-in settings");
    let mut settings = defaults;
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(Value::Object(values)) = serde_json::from_str::<Value>(&contents) {
            if let Value::Object(ref mut object) = settings {
                let old_dot = values.get("discAppearance").and_then(Value::as_str) == Some("dot");
                for (key, value) in &values {
                    if object.contains_key(key) {
                        object.insert(key.clone(), value.clone());
                    }
                }
                for key in [
                    "transitionStyle",
                    "backdropTransitionStyle",
                    "panelTransitionStyle",
                ] {
                    if object.get(key).and_then(Value::as_str) == Some("instant") {
                        object.insert(key.into(), Value::String("off".into()));
                    }
                }
                if old_dot {
                    object.insert("discAppearance".into(), Value::String("basic".into()));
                    if let Some(size) = values.get("centerHoleDot").and_then(Value::as_f64) {
                        object.insert("centerHoleBasic".into(), Value::from(size));
                    }
                }
            }
        }
    }
    settings
}

async fn get_defaults() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        [(header::CONTENT_TYPE, "application/json")],
        DEFAULT_CONFIG,
    )
}

async fn get_config(State(state): State<WebState>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(read_settings(&state.path)),
    )
}

async fn put_config(
    State(state): State<WebState>,
    Json(input): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let Value::Object(incoming) = input else {
        return Err((StatusCode::BAD_REQUEST, "Expected settings object".into()));
    };
    if state.path.exists() {
        let saved = std::fs::read_to_string(&*state.path).map_err(internal_error)?;
        if !matches!(serde_json::from_str::<Value>(&saved), Ok(Value::Object(_))) {
            return Err((StatusCode::CONFLICT, "Existing configuration is invalid; fix or back it up before saving. No changes were written.".into()));
        }
    }
    let defaults: Value = serde_json::from_str(DEFAULT_CONFIG).expect("valid built-in settings");
    let mut output = read_settings(&state.path);
    let output_map = output.as_object_mut().unwrap();
    let default_map = defaults.as_object().unwrap();
    for (key, value) in incoming {
        let Some(default) = default_map.get(&key) else {
            return Err((StatusCode::BAD_REQUEST, format!("Unknown setting: {key}")));
        };
        if !match default {
            Value::Bool(_) => value.is_boolean(),
            Value::Number(_) => value.as_f64().is_some_and(|x| x.is_finite()),
            Value::String(_) => value.is_string(),
            _ => false,
        } {
            return Err((StatusCode::BAD_REQUEST, format!("Invalid type for {key}")));
        }
        if let Some((lo, hi)) = match key.as_str() {
            "visualizerSensitivity" => Some((0., 10.)),
            "visualizerSmoothing" => Some((0., 1.)),
            "discSize" => Some((50., 200.)),
            "centerHoleBasic" | "centerHoleCd" | "centerHoleVinyl" => Some((0., 20.)),
            "panelFontSize" => Some((0.6, 1.6)),
            "discRotationSeconds" => Some((1., 120.)),
            "transitionDuration" | "backdropTransitionDuration" | "panelTransitionDuration" => {
                Some((100., 10000.))
            }
            "backgroundDim" => Some((0., 1.)),
            "backgroundBlur" => Some((0., 150.)),
            "metadataOpacity" => Some((0., 1.)),
            "metadataScale" => Some((0.5, 2.)),
            _ => None,
        } {
            if !value.as_f64().is_some_and(|n| n >= lo && n <= hi) {
                return Err((StatusCode::BAD_REQUEST, format!("Out of range: {key}")));
            }
        }
        if key == "transitionStyle"
            && !["fly", "slide", "fade", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported transition style".into(),
            ));
        }
        if key == "backdropTransitionStyle"
            && !["fade", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported backdrop transition".into(),
            ));
        }
        if key == "panelTransitionStyle"
            && !["slide", "fade", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported panel transition".into(),
            ));
        }
        if key == "timeDisplay"
            && !["off", "remaining", "total"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((StatusCode::BAD_REQUEST, "Unsupported time display".into()));
        }
        if key == "timeLayout" && !["split", "together"].contains(&value.as_str().unwrap_or("")) {
            return Err((StatusCode::BAD_REQUEST, "Unsupported time layout".into()));
        }
        if key == "panelPosition"
            && !["bottom-right", "bottom-left", "top-right", "top-left"]
                .contains(&value.as_str().unwrap_or(""))
        {
            return Err((StatusCode::BAD_REQUEST, "Unsupported panel position".into()));
        }
        if key.starts_with("centerHoleColor")
            && !["white", "black", "transparent"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((StatusCode::BAD_REQUEST, "Unsupported hole colour".into()));
        }
        if key == "sleeveTransitionStyle"
            && !["in-out", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported sleeve transition".into(),
            ));
        }
        if key == "discAppearance"
            && !["basic", "cd", "vinyl", "vinyl-cover"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported disc appearance".into(),
            ));
        }
        output_map.insert(key, value);
    }
    let parent = state.path.parent().unwrap();
    std::fs::create_dir_all(parent).map_err(internal_error)?;
    let temporary = state.path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&output).map_err(internal_error)?,
    )
    .map_err(internal_error)?;
    std::fs::rename(&temporary, &*state.path).map_err(internal_error)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(output)))
}
fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

async fn static_file(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let (mime, data) = match path.as_str() {
        "" | "index.html" => ("text/html; charset=utf-8", INDEX),
        "settings" | "settings.html" => ("text/html; charset=utf-8", SETTINGS_HTML),
        "styles.css" => ("text/css; charset=utf-8", CSS),
        "src/settings.js" => ("text/javascript; charset=utf-8", SETTINGS_JS),
        "src/main.js" => ("text/javascript; charset=utf-8", MAIN_JS),
        "src/motion.js" => ("text/javascript; charset=utf-8", MOTION_JS),
        "src/spectrum.js" => ("text/javascript; charset=utf-8", SPECTRUM_JS),
        "src/soloist.js" => ("text/javascript; charset=utf-8", SOLOIST_JS),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store"),
        ],
        data,
    )
        .into_response()
}

async fn web_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = env::var("SOLOIST_DISPLAY_HTTP_PORT").unwrap_or_else(|_| "8080".into());
    let state = WebState {
        path: Arc::new(config_path()),
    };
    let app = Router::new()
        .route("/config.json", get(get_config).put(put_config))
        .route("/defaults.json", get(get_defaults))
        .route("/", get(|| static_file(axum::extract::Path(String::new()))))
        .route(
            "/settings",
            get(|| static_file(axum::extract::Path("settings".into()))),
        )
        .route("/*path", get(static_file))
        .with_state(state);
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    eprintln!("Display and settings: http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut web_task = tokio::spawn(async { web_server().await });
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<String>(64);
    let capture_tx = sender.clone();
    tokio::spawn(async move {
        loop {
            let next_sender = capture_tx.clone();
            let result = tokio::task::spawn_blocking(move || audio::run(next_sender)).await;
            if let Err(error) = result
                .map_err(|error| error.into())
                .and_then(|result| result)
            {
                eprintln!("Audio capture stopped: {error}; retrying");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    eprintln!(
        "Spectrum WebSocket listening on ws://127.0.0.1:{port} (6 bands; {})",
        audio::capture_backend()
    );
    loop {
        let (socket, _) = tokio::select! {
            result = listener.accept() => result?,
            result = &mut web_task => {
                return Err(format!("HTTP server exited unexpectedly: {result:?}").into());
            }
        };
        let mut receiver = sender.subscribe();
        tokio::spawn(async move {
            if let Ok(mut ws) = accept_async(socket).await {
                loop {
                    let frame = match receiver.recv().await {
                        Ok(frame) => frame,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    };
                    if ws.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_transitions_and_dot_without_losing_durations() {
        let path = env::temp_dir().join(format!(
            "soloist-display-migration-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"transitionStyle":"instant","backdropTransitionStyle":"instant","panelTransitionStyle":"instant","transitionDuration":3400,"discAppearance":"dot","centerHoleDot":8,"visualizerSensitivity":2.5}"#).unwrap();
        let migrated = read_settings(&path);
        std::fs::remove_file(&path).unwrap();
        for key in [
            "transitionStyle",
            "backdropTransitionStyle",
            "panelTransitionStyle",
        ] {
            assert_eq!(migrated[key], "off");
        }
        assert_eq!(migrated["discAppearance"], "basic");
        assert_eq!(migrated["centerHoleBasic"].as_f64(), Some(8.0));
        assert_eq!(migrated["transitionDuration"], 3400);
        assert_eq!(migrated["visualizerSensitivity"], 2.5);
        assert!(migrated.get("centerHoleDot").is_none());
    }

    #[test]
    fn preserves_defaults_for_missing_fields() {
        let path = env::temp_dir().join(format!(
            "soloist-display-defaults-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"backgroundDim":0.2}"#).unwrap();
        let settings = read_settings(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(settings["backgroundDim"], 0.2);
        assert_eq!(settings["progressBar"], true);
        assert_eq!(settings["timeDisplay"], "off");
    }
}
