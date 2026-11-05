use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::SinkExt;
mod capture;
mod cli;
mod device;
mod pcm;
mod spectrum;
use serde_json::Value;
use spectrum::{HOP, RATE};
use std::{env, path::PathBuf, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{broadcast, Mutex},
};
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
    write_lock: Arc<Mutex<()>>,
}

fn config_path() -> PathBuf {
    if let Some(directory) = env::var_os("SOLOIST_DISPLAY_CONFIG_DIR") {
        return PathBuf::from(directory).join("config.json");
    }

    #[cfg(target_os = "windows")]
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Roaming"))
        })
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(target_os = "macos")]
    let base =
        PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("Library/Application Support");

    #[cfg(target_os = "linux")]
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
                if values.get("idleText").and_then(Value::as_str) == Some("Ready when you are") {
                    object.insert("idleText".into(), Value::from(""));
                }
                // Existing global diameters become the initial size for every appearance.
                if let Some(size) = values.get("discSize").and_then(Value::as_f64) {
                    for key in [
                        "discSizeBasic",
                        "discSizeCd",
                        "discSizeVinyl",
                        "discSizeVinylCover",
                        "discSizeCdCover",
                        "discSizeAlbumCover",
                    ] {
                        if !values.contains_key(key) {
                            object.insert(key.into(), Value::from(size));
                        }
                    }
                }
                // Seed new appearance-specific sizes from the user's previous styles.
                for (new_key, old_key) in [
                    ("discSizeVinylCover", "discSizeVinyl"),
                    ("discSizeCdCover", "discSizeCd"),
                    ("discSizeAlbumCover", "discSizeBasic"),
                ] {
                    if !values.contains_key(new_key) {
                        if let Some(size) = values.get(old_key).or_else(|| values.get("discSize")) {
                            object.insert(new_key.into(), size.clone());
                        }
                    }
                }
                for suffix in [
                    "Basic",
                    "Cd",
                    "Vinyl",
                    "VinylCover",
                    "CdCover",
                    "AlbumCover",
                ] {
                    for key in ["transitionStyle", "transitionDuration", "transitionDelay"] {
                        let name = format!("{key}{suffix}");
                        if !values.contains_key(&name) {
                            if let Some(previous) = values.get(key) {
                                object.insert(name, previous.clone());
                            }
                        }
                    }
                }
                // Sleeve presets exist only for appearances with an actual sleeve.
                // Never emit keys that are absent from DEFAULT_CONFIG: the settings UI
                // submits the entire object and the PUT endpoint rejects unknown keys.
                for suffix in ["VinylCover", "CdCover"] {
                    for key in [
                        "sleeveTransitionStyle",
                        "sleeveAlignment",
                        "sleeveDuration",
                        "sleeveDelayIn",
                        "sleeveDelayOut",
                    ] {
                        let name = format!("{key}{suffix}");
                        if !values.contains_key(&name) {
                            if let Some(previous) = values.get(key) {
                                object.insert(name, previous.clone());
                            }
                        }
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
                for suffix in [
                    "Basic",
                    "Cd",
                    "Vinyl",
                    "VinylCover",
                    "CdCover",
                    "AlbumCover",
                ] {
                    let key = format!("transitionStyle{suffix}");
                    if object.get(&key).and_then(Value::as_str) == Some("instant") {
                        object.insert(key, Value::String("off".into()));
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

fn config_etag(config: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    config.to_string().hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

async fn get_config(State(state): State<WebState>) -> impl IntoResponse {
    let _guard = state.write_lock.lock().await;
    let config = read_settings(&state.path);
    let tag = config_etag(&config);
    (
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::ETAG,
                HeaderValue::from_str(&tag).expect("valid etag"),
            ),
        ],
        Json(config),
    )
}

async fn put_config(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let _guard = state.write_lock.lock().await;
    let current = read_settings(&state.path);
    let expected = config_etag(&current);
    if headers.get(header::IF_MATCH).and_then(|h| h.to_str().ok()) != Some(expected.as_str()) {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            "Settings changed; reload before saving".into(),
        ));
    }
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
    let mut output = current;
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
            "vinylGrooveDensity" => Some((1., 10.)),
            "vinylLabelSize" => Some((10., 70.)),
            "discSize" | "discSizeBasic" | "discSizeCd" | "discSizeVinyl"
            | "discSizeVinylCover" | "discSizeCdCover" | "discSizeAlbumCover" => Some((50., 200.)),
            "centerHoleBasic" | "centerHoleCd" | "centerHoleVinyl" => Some((0., 20.)),
            "discOpacityBasic"
            | "discOpacityCd"
            | "discOpacityVinyl"
            | "discOpacityVinylCover"
            | "discOpacityCdCoverUp"
            | "discOpacityCdCoverDown"
            | "discOpacityAlbumCover"
            | "cdRainbow" => Some((0., 1.)),
            "panelFontSize" => Some((0.6, 1.6)),
            "discRotationSeconds" => Some((1., 120.)),
            "transitionDuration"
            | "sleeveDuration"
            | "coverTransitionDuration"
            | "backdropTransitionDuration"
            | "panelTransitionDuration" => Some((100., 10000.)),
            "idleFadeDuration" => Some((0., 10000.)),
            "sleeveDelay"
            | "sleeveDelayIn"
            | "sleeveDelayOut"
            | "transitionDelay"
            | "backdropTransitionDelay"
            | "panelTransitionDelay"
            | "panelDelayIn"
            | "panelDelayOut" => Some((0., 10000.)),
            "idlePauseDelaySeconds"
            | "panelHideDelaySeconds"
            | "pauseRetractDelaySecondsVinylCover"
            | "pauseRetractDelaySecondsCdCover" => Some((-1., 86400.)),
            "backgroundDim" => Some((0., 1.)),
            "backgroundBlur" => Some((0., 150.)),
            "metadataOpacity" => Some((0., 1.)),
            "metadataScale" => Some((0.5, 2.)),
            _ if key.starts_with("transitionDuration") || key.starts_with("sleeveDuration") => {
                Some((100., 10000.))
            }
            _ if key.starts_with("transitionDelay") || key.starts_with("sleeveDelay") => {
                Some((0., 10000.))
            }
            _ => None,
        } {
            if !value.as_f64().is_some_and(|n| n >= lo && n <= hi) {
                return Err((StatusCode::BAD_REQUEST, format!("Out of range: {key}")));
            }
        }
        if key == "idleAppearance"
            && !["digital-clock", "vinyl-clock", "vinyl", "text", "black"]
                .contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported standby appearance".into(),
            ));
        }
        if (key == "idlePauseDelaySeconds"
            || key == "panelHideDelaySeconds"
            || key == "pauseRetractDelaySecondsVinylCover"
            || key == "pauseRetractDelaySecondsCdCover")
            && !value
                .as_i64()
                .is_some_and(|seconds| seconds == -1 || (0..=86400).contains(&seconds))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Pause delay must be an integer from 0 to 86400, or -1".into(),
            ));
        }
        if key == "idleText" && value.as_str().unwrap_or("").chars().count() > 80 {
            return Err((StatusCode::BAD_REQUEST, "Standby text is too long".into()));
        }
        if key.starts_with("transitionStyle")
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
        if key == "idleTransitionStyle" && !["fade", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported idle transition".into(),
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
        if key == "cdCoverSide" && !["up", "down"].contains(&value.as_str().unwrap_or("")) {
            return Err((StatusCode::BAD_REQUEST, "Unsupported CD side".into()));
        }
        if key.starts_with("sleeveAlignment")
            && !["left", "center"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported sleeve alignment".into(),
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
        if key.starts_with("sleeveTransitionStyle")
            && !["in-out", "off"].contains(&value.as_str().unwrap_or(""))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported sleeve transition".into(),
            ));
        }
        if key == "discAppearance"
            && ![
                "basic",
                "cd",
                "vinyl",
                "vinyl-cover",
                "cd-cover",
                "album-cover",
            ]
            .contains(&value.as_str().unwrap_or(""))
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
    let tag = config_etag(&output);
    Ok((
        [
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::ETAG,
                HeaderValue::from_str(&tag).expect("valid etag"),
            ),
        ],
        Json(output),
    ))
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
        write_lock: Arc::new(Mutex::new(())),
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
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = cli::parse(&args)
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    match command {
        cli::Command::Serve => {}
        cli::Command::Doctor => {
            if cli::doctor().await {
                return Ok(());
            }
            return Err(std::io::Error::other("doctor: checks failed").into());
        }
        cli::Command::Help => {
            cli::help();
            return Ok(());
        }
        cli::Command::Version => {
            println!("Soloist Display {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        cli::Command::DeviceList => {
            cli::list_devices()?;
            return Ok(());
        }
        cli::Command::DeviceSet(id) => {
            cli::set_device(&id)?;
            return Ok(());
        }
        cli::Command::DeviceDefault => {
            cli::default_device()?;
            return Ok(());
        }
    }
    let mut web_task = tokio::spawn(async { web_server().await });
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<String>(64);
    let capture_tx = sender.clone();
    tokio::task::spawn_blocking(move || loop {
        if let Err(error) = capture::run(capture_tx.clone()) {
            eprintln!("Audio capture stopped: {error}; retrying");
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
    eprintln!(
        "Spectrum WebSocket listening on ws://127.0.0.1:{port} (6 bands, {:.1} Hz)",
        RATE as f32 / HOP as f32
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

    #[tokio::test]
    async fn simultaneous_settings_writes_allow_only_one_revision() {
        let dir = tempfile::tempdir().unwrap();
        let state = WebState {
            path: Arc::new(dir.path().join("config.json")),
            write_lock: Arc::new(Mutex::new(())),
        };
        let initial = read_settings(&state.path);
        let etag = config_etag(&initial);
        let mut left = initial.clone();
        let mut right = initial.clone();
        left["backgroundDim"] = Value::from(0.25);
        right["backgroundDim"] = Value::from(0.75);
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_str(&etag).unwrap());
        let (a, b) = tokio::join!(
            put_config(State(state.clone()), headers.clone(), Json(left)),
            put_config(State(state.clone()), headers, Json(right)),
        );
        assert_ne!(a.is_ok(), b.is_ok(), "exactly one writer must succeed");
        let stale = if let Err(error) = a {
            error
        } else {
            b.err().unwrap()
        };
        assert_eq!(stale.0, StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn accepts_custom_pause_timeout_and_rejects_invalid_values() {
        let dir = tempfile::tempdir().unwrap();
        let state = WebState {
            path: Arc::new(dir.path().join("config.json")),
            write_lock: Arc::new(Mutex::new(())),
        };
        assert_eq!(read_settings(&state.path)["idlePauseDelaySeconds"], 300);
        for delay in [-1, 0, 17, 90, 300, 3600, 86400] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["idlePauseDelaySeconds"] = Value::from(delay);
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            assert!(
                put_config(State(state.clone()), headers, Json(config))
                    .await
                    .is_ok(),
                "valid pause timeout {delay}"
            );
        }
        for value in [
            Value::from(-2),
            Value::from(86401),
            Value::from(3.5),
            Value::from("300"),
        ] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["idlePauseDelaySeconds"] = value;
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            let error = put_config(State(state.clone()), headers, Json(config))
                .await
                .err()
                .expect("invalid value must fail");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn validates_idle_fade_duration() {
        let dir = tempfile::tempdir().unwrap();
        let state = WebState {
            path: Arc::new(dir.path().join("config.json")),
            write_lock: Arc::new(Mutex::new(())),
        };
        assert_eq!(read_settings(&state.path)["idleFadeDuration"], 300);
        assert_eq!(read_settings(&state.path)["idleTransitionStyle"], "fade");
        for style in ["fade", "off"] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["idleTransitionStyle"] = Value::from(style);
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            assert!(
                put_config(State(state.clone()), headers, Json(config))
                    .await
                    .is_ok(),
                "valid idle transition {style}"
            );
        }
        let mut config = read_settings(&state.path);
        let tag = config_etag(&config);
        config["idleTransitionStyle"] = Value::from("slide");
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
        assert_eq!(
            put_config(State(state.clone()), headers, Json(config))
                .await
                .err()
                .expect("invalid idle transition must fail")
                .0,
            StatusCode::BAD_REQUEST
        );
        for duration in [0, 50, 300, 10000] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["idleFadeDuration"] = Value::from(duration);
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            assert!(
                put_config(State(state.clone()), headers, Json(config))
                    .await
                    .is_ok(),
                "valid idle fade duration {duration}"
            );
            assert_eq!(read_settings(&state.path)["idleFadeDuration"], duration);
        }
        for value in [Value::from(-1), Value::from(10001), Value::from("300")] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["idleFadeDuration"] = value;
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            let error = put_config(State(state.clone()), headers, Json(config))
                .await
                .err()
                .expect("invalid idle fade duration must fail");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn validates_sleeve_pause_retraction_delay() {
        let dir = tempfile::tempdir().unwrap();
        let state = WebState {
            path: Arc::new(dir.path().join("config.json")),
            write_lock: Arc::new(Mutex::new(())),
        };
        for key in [
            "pauseRetractDelaySecondsVinylCover",
            "pauseRetractDelaySecondsCdCover",
        ] {
            assert_eq!(read_settings(&state.path)[key], -1);
            for delay in [-1, 0, 9, 120, 86400] {
                let mut config = read_settings(&state.path);
                let tag = config_etag(&config);
                config[key] = Value::from(delay);
                let mut headers = HeaderMap::new();
                headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
                assert!(
                    put_config(State(state.clone()), headers, Json(config))
                        .await
                        .is_ok(),
                    "valid {key} = {delay}"
                );
            }
            for bad in [-2, 86401] {
                let mut config = read_settings(&state.path);
                let tag = config_etag(&config);
                config[key] = Value::from(bad);
                let mut headers = HeaderMap::new();
                headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
                let error = put_config(State(state.clone()), headers, Json(config))
                    .await
                    .err()
                    .expect("invalid pause delay must fail");
                assert_eq!(error.0, StatusCode::BAD_REQUEST);
            }
        }
    }

    #[tokio::test]
    async fn validates_panel_hide_delay() {
        let dir = tempfile::tempdir().unwrap();
        let state = WebState {
            path: Arc::new(dir.path().join("config.json")),
            write_lock: Arc::new(Mutex::new(())),
        };
        assert_eq!(read_settings(&state.path)["panelHideDelaySeconds"], -1);
        for seconds in [-1, 0, 9, 300, 86400] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["panelHideDelaySeconds"] = Value::from(seconds);
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            assert!(
                put_config(State(state.clone()), headers, Json(config))
                    .await
                    .is_ok(),
                "valid panel hide delay {seconds}"
            );
        }
        for value in [
            Value::from(-2),
            Value::from(86401),
            Value::from(0.5),
            Value::from("30"),
        ] {
            let mut config = read_settings(&state.path);
            let tag = config_etag(&config);
            config["panelHideDelaySeconds"] = value;
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_MATCH, HeaderValue::from_str(&tag).unwrap());
            let error = put_config(State(state.clone()), headers, Json(config))
                .await
                .err()
                .expect("invalid panel hide delay must fail");
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
        }
    }

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
    fn migrates_global_diameter_to_each_appearance() {
        let path = env::temp_dir().join(format!(
            "soloist-display-diameter-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{ "discSize": 142, "discSizeCd": 88 }"#).unwrap();
        let settings = read_settings(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(settings["discSizeBasic"].as_f64(), Some(142.0));
        assert_eq!(settings["discSizeCd"].as_f64(), Some(88.0));
        assert_eq!(settings["discSizeVinyl"].as_f64(), Some(142.0));
    }

    #[test]
    fn clears_previous_default_standby_text_but_keeps_custom_messages() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        assert_eq!(read_settings(&path)["idleText"], "");
        std::fs::write(&path, r#"{"idleText":"Ready when you are"}"#).unwrap();
        assert_eq!(read_settings(&path)["idleText"], "");
        std::fs::write(&path, r#"{"idleText":"Play some music"}"#).unwrap();
        assert_eq!(read_settings(&path)["idleText"], "Play some music");
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
        assert_eq!(settings["discSizeBasic"], 116);
        assert_eq!(settings["cdCoverSide"], "down");
        assert_eq!(settings["vinylGrooveDensity"], 5);
    }
    #[test]
    fn defaults_and_legacy_presets_are_appearance_specific() {
        let defaults: Value = serde_json::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(defaults["discAppearance"], "vinyl-cover");
        assert_eq!(defaults["discOpacityCdCoverDown"], 0.7);
        assert_eq!(defaults["cdRainbow"], 0.5);
        assert_eq!(defaults["vinylLabelSize"], 34);
        for key in [
            "discSizeVinylCover",
            "discSizeCdCover",
            "discSizeAlbumCover",
        ] {
            assert_eq!(defaults[key], 75, "{key} should default to 75");
        }

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, r#"{"discAppearance":"cd-cover","discSizeCd":91,"transitionStyle":"slide","transitionDuration":3700,"sleeveDuration":1700}"#).unwrap();
        let settings = read_settings(&path);
        assert_eq!(settings["discAppearance"], "cd-cover");
        assert_eq!(settings["discSizeCdCover"], 91);
        assert_eq!(settings["transitionStyleCdCover"], "slide");
        assert_eq!(settings["transitionDurationCdCover"], 3700);
        assert_eq!(settings["sleeveDurationCdCover"], 1700);
        // The response must remain PUT-compatible for every legacy preset.
        let default_keys = defaults.as_object().unwrap();
        for key in settings.as_object().unwrap().keys() {
            assert!(default_keys.contains_key(key), "Unexpected setting: {key}");
        }
        for suffix in ["Basic", "Cd", "Vinyl", "AlbumCover"] {
            for prefix in [
                "sleeveTransitionStyle",
                "sleeveAlignment",
                "sleeveDuration",
                "sleeveDelayIn",
                "sleeveDelayOut",
            ] {
                assert!(settings.get(format!("{prefix}{suffix}")).is_none());
            }
        }

        assert_eq!(
            settings["discSizeAlbumCover"],
            defaults["discSizeAlbumCover"]
        );
    }
}
