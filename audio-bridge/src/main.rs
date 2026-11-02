use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::SinkExt;
use rustfft::{num_complex::Complex32, FftPlanner};
use serde_json::Value;
use std::{env, path::PathBuf, process::Stdio, sync::Arc, time::Instant};
use tokio::{
    io::AsyncReadExt, net::TcpListener, process::Command, sync::broadcast, time::Duration,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

const RATE: usize = 22050;
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
        if key == "discAppearance"
            && !["basic", "cd", "vinyl"].contains(&value.as_str().unwrap_or(""))
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

const WINDOW: usize = 2048;
const HOP: usize = 512;
const BANDS: [f32; 7] = [55., 150., 350., 800., 1800., 4000., 9000.];
// Typical music loses energy at higher frequencies; use progressively
// lower reference levels so sensitivity can meaningfully affect all bars.
const REFERENCE_DB: [f32; 6] = [-19., -23., -29., -34., -39., -44.];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut web_task = tokio::spawn(async { web_server().await });
    let port = env::var("SOLOIST_DISPLAY_AUDIO_PORT").unwrap_or_else(|_| "9091".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let (sender, _) = broadcast::channel::<String>(64);
    let capture_tx = sender.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = capture(capture_tx.clone()).await {
                eprintln!("Audio capture stopped: {error}; retrying");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
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
                    if ws.send(Message::Text(frame)).await.is_err() {
                        break;
                    }
                }
            }
        });
    }
}

async fn capture(
    sender: broadcast::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("parec");
    cmd.args([
        "--raw",
        "--format=s16le",
        "--rate",
        "22050",
        "--channels=1",
        "--latency-msec=25",
        "--process-time-msec=10",
    ]);
    let source = env::var("SOLOIST_DISPLAY_MONITOR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let output = std::process::Command::new("pactl")
                .arg("get-default-sink")
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            Some(format!(
                "{}.monitor",
                String::from_utf8(output.stdout).ok()?.trim()
            ))
        });
    if let Some(source) = &source {
        cmd.arg("--device").arg(source);
        eprintln!("Capturing monitor source: {source}");
    } else {
        eprintln!("Warning: no output monitor found; set SOLOIST_DISPLAY_MONITOR explicitly");
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().ok_or("No audio stream")?;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos())
        .collect();
    let mut ring = [0f32; WINDOW];
    let mut cursor = 0usize;
    let mut filled = 0usize;
    let mut frame = vec![Complex32::new(0., 0.); WINDOW];
    let mut raw = [0u8; HOP * 2];
    let mut smoothed = [0f32; 6];
    let mut peak = 0f32;
    let mut frames = 0u64;
    let mut report = Instant::now();
    let mut previous_frame = Instant::now();
    let mut max_gap = Duration::ZERO;
    loop {
        stdout.read_exact(&mut raw).await?;
        for offset in (0..raw.len()).step_by(2) {
            let sample = i16::from_le_bytes([raw[offset], raw[offset + 1]]) as f32 / 32768.;
            peak = peak.max(sample.abs());
            ring[cursor] = sample;
            cursor = (cursor + 1) % WINDOW;
            filled = (filled + 1).min(WINDOW);
        }
        if filled < WINDOW {
            continue;
        }
        let now = Instant::now();
        max_gap = max_gap.max(now.duration_since(previous_frame));
        previous_frame = now;
        for i in 0..WINDOW {
            frame[i] = Complex32::new(ring[(cursor + i) % WINDOW] * hann[i], 0.);
        }
        fft.process(&mut frame);
        let mut bands = [0f32; 6];
        for band in 0..6 {
            let lo = ((BANDS[band] * WINDOW as f32 / RATE as f32).floor() as usize).max(1);
            let hi =
                ((BANDS[band + 1] * WINDOW as f32 / RATE as f32).ceil() as usize).min(WINDOW / 2);
            let mut power = 0f32;
            let mut count = 0usize;
            for value in frame.iter().take(hi).skip(lo) {
                power += value.norm_sqr();
                count += 1;
            }
            let rms = if count > 0 {
                (power / count as f32).sqrt() / (WINDOW as f32 * 0.25)
            } else {
                0.
            };
            // Per-band reference levels compensate for the natural spectral
            // roll-off of music: a shared dB threshold hides the upper bands.
            // These are fixed calibrations, not automatic normalization, so
            // silence stays silent and each bar still follows its own energy.
            let db = 20. * rms.max(1e-8).log10();
            let level = ((db - REFERENCE_DB[band] + 49.) / 49.)
                .clamp(0., 1.)
                .powf(1.25);
            // ~12 ms attack / ~180 ms release at 43 Hz.
            let coefficient = if level > smoothed[band] { 0.85 } else { 0.12 };
            smoothed[band] += (level - smoothed[band]) * coefficient;
            bands[band] = smoothed[band];
        }
        frames += 1;
        if sender.receiver_count() > 0 {
            let payload = format!(
                "{{\"bands\":[{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}]}}",
                bands[0], bands[1], bands[2], bands[3], bands[4], bands[5]
            );
            let _ = sender.send(payload);
        }
        if report.elapsed() >= Duration::from_secs(5) {
            eprintln!(
                "Audio: {:.1} frames/s | max gap: {:.0}ms | peak: {:.3} | bands: {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} | clients: {}",
                frames as f32 / report.elapsed().as_secs_f32(), max_gap.as_secs_f64() * 1000., peak,
                bands[0], bands[1], bands[2], bands[3], bands[4], bands[5],
                sender.receiver_count()
            );
            report = Instant::now();
            frames = 0;
            peak = 0.;
            max_gap = Duration::ZERO;
        }
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
