use crate::{
    asr::{Transcript, Word},
    media::{self, MAX_BYTES, MAX_SECONDS, Media},
    models::Models,
    sessions::{Sessions, plain_path},
};
use anyhow::{Context, Result};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
#[cfg(feature = "full-media")]
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
#[cfg(feature = "full-media")]
use std::io::Read;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter},
};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

static SCRIPT_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{L}\p{N}][\p{L}\p{M}\p{N}]*(?:['’][\p{L}\p{N}][\p{L}\p{M}\p{N}]*)*").unwrap()
});
static CUE_DATA: LazyLock<Value> = LazyLock::new(|| serde_json::from_slice(include_bytes!("../../public/cue-data.json")).unwrap());
static CUES: LazyLock<HashSet<String>> = LazyLock::new(|| {
    CUE_DATA["cueCards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| card["id"].as_str().unwrap().to_owned())
        .collect()
});
const MAX_UPLOAD_HEADER: usize = 64 * 1024;
const MEDIA_BUFFER: usize = 64 * 1024;
#[cfg(feature = "full-media")]
const WASM: &str = "assets/vision_wasm_internal.wasm";
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; media-src 'self' blob:; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

#[cfg(windows)]
// Keep the WebView profile's existing storage origin. The Windows window
// intercepts requests to this address, so there is no matching listener.
pub(crate) const WEBVIEW_ORIGIN: &str = "http://127.0.0.1:4318";

#[derive(Clone, Copy)]
pub(crate) enum Transport {
    Network(u16),
    #[cfg(windows)]
    WebView,
}

pub struct Studio {
    sessions: Sessions,
    topics: custom_topics::TopicLibrary,
    credentials: crate::credentials::OpenAiCredentials,
    media: Media,
    models: Arc<Models>,
    transport: Transport,
    running: Mutex<Option<String>>,
    client: reqwest::Client,
    sample: Value,
    desktop: bool,
}

pub async fn start(
    root: PathBuf,
    resources: &Path,
    port: u16,
    desktop: bool,
) -> Result<(String, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("Could not listen on 127.0.0.1:{port}. Rehearsal may already be open."))?;
    let address = listener.local_addr()?;
    let studio = open(root, resources, Transport::Network(address.port()), desktop).await?;
    let router = Router::new().fallback(handle).with_state(studio);
    let service = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok((format!("http://{address}"), service))
}

pub(crate) async fn open(
    root: PathBuf,
    resources: &Path,
    transport: Transport,
    desktop: bool,
) -> Result<Arc<Studio>> {
    let override_folder = std::env::var_os("CADENCE_DATA").map(PathBuf::from);
    let sessions = override_folder.clone().unwrap_or_else(|| root.join("sessions"));
    fs::create_dir_all(&sessions).await?;
    let sessions = fs::canonicalize(&sessions).await?;
    let settings = if override_folder.is_some() { sessions.join(".rehearse-storage.json") } else { root.join("storage.json") };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()?;
    let feedback_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut sample: Value = serde_json::from_str(
        include_str!("../../public/assets/example.json").trim_start_matches('\u{feff}'),
    )?;
    sample.as_object_mut().context("Missing example metadata.")?.extend(json!({
        "id": "example", "title": "A short practice talk", "kind": "audio", "mime": "audio/mpeg",
        "media": "/assets/example.mp3", "sample": true
    }).as_object().unwrap().clone());
    Ok(Arc::new(Studio {
        credentials: crate::credentials::OpenAiCredentials::new(&settings),
        topics: custom_topics::TopicLibrary::open(settings.with_file_name("custom-topics.json"))?,
        sessions: Sessions::open(sessions, Some(settings)),
        media: Media::new(resources),
        models: Arc::new(Models::new(root, client.clone())),
        transport,
        running: Default::default(),
        client: feedback_client,
        sample,
        desktop,
    }))
}

#[derive(Debug)]
struct ApiError(StatusCode, String);
impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.1)
    }
}
impl std::error::Error for ApiError {}

fn error(status: u16, message: impl Into<String>) -> anyhow::Error {
    ApiError(StatusCode::from_u16(status).unwrap(), message.into()).into()
}

fn respond(status: StatusCode, value: Value) -> Response {
    (status, axum::Json(value)).into_response()
}

async fn handle(State(studio): State<Arc<Studio>>, req: Request) -> Response {
    serve_request(&studio, req).await
}

pub(crate) async fn serve_request(studio: &Arc<Studio>, req: Request) -> Response {
    let mut response = match dispatch(studio, req).await {
        Ok(response) => response,
        Err(err) => match err.downcast::<ApiError>() {
            Ok(err) => respond(err.0, json!({"error": err.1})),
            Err(err) => {
                eprintln!("{err:#}");
                respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": "Something went wrong. Try again."}),
                )
            }
        },
    };
    for (key, value) in [
        ("x-content-type-options", "nosniff"),
        ("cross-origin-resource-policy", "same-origin"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", CSP),
        ("cache-control", "no-store"),
    ] {
        response.headers_mut().insert(key, value.parse().unwrap());
    }
    response
}

async fn dispatch(studio: &Arc<Studio>, req: Request) -> Result<Response> {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();
    let read = method == Method::GET || method == Method::HEAD;
    let origin = header(req.headers(), "origin");
    let trusted = match studio.transport {
        Transport::Network(port) => {
            let host = header(req.headers(), "host");
            (host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"))
                && (read || origin == format!("http://{host}"))
        }
        #[cfg(windows)]
        Transport::WebView => {
            req.uri().scheme_str() == Some("http")
                && req.uri().authority().map(|authority| authority.as_str()) == WEBVIEW_ORIGIN.strip_prefix("http://")
                && (read || origin == WEBVIEW_ORIGIN)
        }
    };
    if !trusted || (path.starts_with("/api/") && header(req.headers(), "sec-fetch-site") == "cross-site") {
        return Err(error(403, "Open Rehearsal from its local address."));
    }
    let ok = |data| Ok(respond(StatusCode::OK, data));
    match (path.as_str(), &method) {
        ("/api/state", &Method::GET) => {
            // Read this before listing to avoid reporting an idle worker with
            // metadata read just before that worker saved its transcript.
            let running = studio.running.lock().unwrap().clone();
            let sessions = studio.sessions.list().await?;
            let running = studio.running.lock().unwrap().clone().or(running);
            let mut status = studio.models.status();
            status["ffmpegReady"] = (cfg!(feature = "full-media") && studio.media.ready()).into();
            status["mediaReady"] = studio.media.ready().into();
            status["lightMedia"] = (!cfg!(feature = "full-media")).into();
            status["platform"] = std::env::consts::OS.into();
            status["screenCapture"] = (!studio.desktop || desktop_screen_capture()).into();
            status["desktop"] = studio.desktop.into();
            status["sessions"] = Value::Array(sessions);
            status["customTopics"] = json!(studio.topics.list());
            status["recordings"] = studio.recordings().await?;
            status["storage"] = studio.sessions.storage().preferences();
            status["sample"] = studio.sample.clone();
            status["running"] = json!(running);
            status["keyConfigured"] = std::env::var("OPENAI_API_KEY")
                .is_ok_and(|key| !key.is_empty())
                .into();
            return ok(status);
        }
        ("/api/topics/import", &Method::POST) => {
            let body = to_bytes(req.into_body(), 1024 * 1024).await.map_err(|_| error(413, "Choose a smaller topic list."))?;
            let prompts = serde_json::from_slice(&body).map_err(|_| error(400, "Choose a list of topics."))?;
            let studio = Arc::clone(studio);
            return ok(tokio::task::spawn_blocking(move || studio.topics.import(prompts)).await??);
        }
        ("/api/models", &Method::GET) => return ok(studio.models.status()),
        ("/api/transcription", &Method::GET) => {
            return ok(json!({"running": *studio.running.lock().unwrap()}));
        }
        ("/api/models/catalog", &Method::GET) => {
            let current = studio.models.refresh_catalog().await;
            let mut status = studio.models.status();
            status["catalogCurrent"] = current.into();
            return ok(status);
        }
        ("/api/feedback", &Method::POST) => {
            return ok(studio.feedback(read_json_limited(req, 384 * 1024).await?).await?);
        }
        ("/api/credentials/openai", &Method::GET) => {
            let saved = studio.credentials.read().await;
            return ok(credential_status(saved.as_ref().is_ok_and(Option::is_some), saved.is_ok()));
        }
        ("/api/credentials/openai", &Method::PUT) => {
            let mut data = read_json_limited(req, 4096).await?;
            let Value::String(key) = data["key"].take() else { return Err(error(400, "Enter an OpenAI API key.")); };
            if key.is_empty() || key.len() > 1024 || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(error(400, "Enter an API key without spaces or line breaks (up to 1,024 characters)."));
            }
            studio.credentials.save(key).await.map_err(|err| error(503, err.to_string()))?;
            return ok(credential_status(true, true));
        }
        ("/api/credentials/openai", &Method::DELETE) => {
            studio.credentials.remove().await.map_err(|err| error(503, err.to_string()))?;
            return ok(credential_status(false, true));
        }
        ("/api/sessions", &Method::POST) => {
            return Ok(respond(StatusCode::CREATED, studio.create(req).await?));
        }
        ("/api/storage", &Method::PATCH) => {
            let body = read_json(req).await?;
            let folder = body["folder"].as_str().filter(|path| !path.trim().is_empty())
                .ok_or_else(|| error(400, "Enter the full path to an existing folder."))?;
            let keep = if body["keep"].is_null() { None } else {
                Some(body["keep"].as_u64().filter(|keep| (1..=10_000).contains(keep))
                    .ok_or_else(|| error(400, "Choose a limit between 1 and 10,000 sessions."))? as usize)
            };
            let _commit = studio.sessions.commit.lock().await;
            let storage = studio.sessions.configure(Path::new(folder), keep)
                .map_err(|err| error(400, err.to_string()))?;
            return ok(json!({ "storage": storage.preferences(), "sessions": studio.sessions.list().await? }));
        }
        _ => {}
    }
    if method == Method::POST
        && let Some(id) = path
            .strip_prefix("/api/models/")
            .and_then(|path| path.strip_suffix("/install"))
    {
        let model = studio
            .models
            .model(id)
            .ok_or_else(|| error(404, "Model not found."))?;
        if !Models::can_install() && studio.models.engine().is_none() {
            return Err(error(
                409,
                "Automatic setup is unavailable for this system.",
            ));
        }
        studio
            .models
            .start_install(model)
            .map_err(|err| error(409, err.to_string()))?;
        return Ok(respond(StatusCode::ACCEPTED, studio.models.status()));
    }
    if path == "/api/recordings" || path.starts_with("/api/recordings/") {
        return recordings::route(studio, req, &path).await;
    }
    if let Some(suffix) = path.strip_prefix("/api/sessions/") {
        let (id, action) = suffix.split_once('/').unwrap_or((suffix, ""));
        if let Some(id) = Uuid::parse_str(id).ok().map(|id| id.to_string()) {
            return match (action, &method) {
                ("media", _) if read => {
                    let download = req
                        .uri()
                        .query()
                        .unwrap_or_default()
                        .split('&')
                        .any(|part| part == "download" || part.starts_with("download="));
                    studio
                        .playback(&id, req.headers().clone(), method == Method::HEAD, download)
                        .await
                }
                ("transcribe", &Method::POST) => {
                    let body = read_json(req).await?;
                    let studio = Arc::clone(studio);
                    // A page reload drops the response, but the job must finish
                    // and commit before the status endpoint reports it idle.
                    let job = tokio::spawn(async move {
                        studio
                            .transcribe(&id, body["modelId"].as_str().unwrap_or_default())
                            .await
                    });
                    ok(job.await??)
                }
                ("", &Method::PATCH) => ok(studio.update(&id, read_json_limited(req, 384 * 1024).await?).await?),
                ("", &Method::DELETE) => {
                    let guard = studio.sessions.lock(&id).await;
                    studio.read::<Value>(&id).await?;
                    if studio.running.lock().unwrap().as_deref() == Some(&id) {
                        return Err(error(
                            409,
                            "Wait for transcription to finish before deleting this take.",
                        ));
                    }
                    studio.sessions.delete(&id, guard).await?;
                    ok(json!({"deleted": true}))
                }
                _ => Err(error(404, "Not found.")),
            };
        }
    }
    if read {
        return asset(req.headers(), method == Method::HEAD, &path);
    }
    Err(error(404, "Not found."))
}

#[path = "recordings.rs"]
mod recordings;
#[path = "custom_topics.rs"]
mod custom_topics;

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;

impl Studio {
    async fn read<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        self.sessions
            .read(id)
            .await?
            .ok_or_else(|| error(404, "Session not found."))
    }

    async fn create(&self, request: Request) -> Result<Value> {
        if !self.media.ready() {
            return Err(error(503, "FFmpeg and ffprobe are required to save media."));
        }
        if header(request.headers(), "content-type") != "application/octet-stream" {
            return Err(error(400, "Could not read the recording."));
        }
        let mut body = request.into_body().into_data_stream();
        let mut head = Vec::new();
        let first = loop {
            let part = body
                .next()
                .await
                .transpose()?
                .ok_or_else(|| error(400, "Could not read the recording."))?;
            let end = part.iter().position(|byte| *byte == b'\n');
            let length = end.unwrap_or(part.len());
            if head.len() + length > MAX_UPLOAD_HEADER {
                return Err(error(413, "The recording details are too long."));
            }
            head.extend_from_slice(&part[..length]);
            if let Some(end) = end {
                break part.slice(end + 1..);
            }
        };
        let details: Upload = serde_json::from_slice(&head)
            .map_err(|_| error(400, "Could not read the recording."))?;
        let extension = details.validate(&self.topics)?;
        let id = Uuid::new_v4().to_string();
        let folder = self.sessions.new_folder(&id);
        fs::create_dir(&folder).await.map_err(|_| error(409, "Could not save to the recording folder. Check Storage in Settings."))?;
        let name = format!("recording.{extension}");
        let input = folder.join(&name);
        let result = async {
            let mut file = BufWriter::with_capacity(MEDIA_BUFFER, fs::File::create(&input).await?);
            let mut size = 0_u64;
            let mut part = Some(first);
            while let Some(bytes) = part {
                size += bytes.len() as u64;
                if size > MAX_BYTES { return Err(error(413, "Choose a recording smaller than 160 MiB.")); }
                file.write_all(&bytes).await?;
                part = body.next().await.transpose()?;
            }
            file.flush().await?;
            drop(file);
            if size == 0 { return Err(error(400, "Choose a supported audio or video file.")); }
            self.finish_media(&id, &folder, &details, size).await
        }.await;
        if result.is_err() {
            let _ = fs::remove_dir_all(folder).await;
        }
        result
    }

    async fn finish_media(&self, id: &str, folder: &Path, details: &Upload, _size: u64) -> Result<Value> {
        let extension = details.validate(&self.topics)?;
        let name = format!("recording.{extension}");
        let input = folder.join(&name);
        let script = details.script.as_deref().map(str::trim);
        let probe = self.media.probe(&input).await.map_err(|err| {
            let message = err.to_string();
            error(400, if message.contains("no audio track") { "This file has no audio track to transcribe." }
            else if message.contains("Unsupported audio codec") { "This audio codec is unsupported. Try AAC, Opus, MP3, WAV, or FLAC." }
            else { "This file could not be read as audio or video." })
        })?;
        let indexed = extension == "webm" && probe.container_duration > 0.0;
        #[cfg(not(feature = "full-media"))]
        let duration = probe.duration;
        #[cfg(feature = "full-media")]
        let (indexed, duration) = {
            let (mut indexed, mut duration) = (indexed, probe.duration);
            if probe.video && extension == "webm" && !indexed && details.recorded {
                let temp = folder.join("indexed.webm");
                if self.media.index_video(&input, &temp).await.is_ok()
                    && let Ok(probed) = self.media.webm_duration(&temp).await
                    && probed.is_finite() && probed > 0.0
                    && fs::rename(&temp, &input).await.is_ok()
                {
                    duration = duration.max(probed);
                    indexed = true;
                }
                let _ = fs::remove_file(temp).await;
            }
            if duration == 0.0 { duration = self.media.packet_duration(&input).await.unwrap_or(0.0); }
            (indexed, duration)
        };
        if !duration.is_finite() || !(0.1..=MAX_SECONDS + 0.5).contains(&duration) {
            return Err(error(400, "Use a recording between 0.1 seconds and 15 minutes."));
        }
        let compact = "recording-small.webm";
        #[cfg(feature = "full-media")]
        let storage = if details.compress { self.media.compress(&input, &folder.join(compact), probe.video, duration, _size).await } else { None };
        #[cfg(not(feature = "full-media"))]
        let storage: Option<media::Savings> = None;
        let kind = if probe.video { "video" } else { "audio" };
        let mime = match (storage.is_some(), extension.as_str(), probe.video) {
            (true, _, true) => "video/webm",
            (true, _, false) | (false, "webm", false) => "audio/webm",
            (false, "mp4", false) => "audio/mp4",
            _ => media_type(&extension).unwrap(),
        };
        let created = time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
        let mut value = json!({
            "id": id, "title": title(details.title.as_deref(), kind), "kind": kind, "duration": duration,
            "file": if storage.is_some() { compact } else { &name }, "mime": mime, "media": format!("/api/sessions/{id}/media"),
            "createdAt": created, "segments": [],
        });
        if indexed { value["indexed"] = true.into(); }
        if let Some(script) = script { value["script"] = script.into(); }
        if let Some(cue) = details.cue_id.as_deref() { value["cueId"] = cue.into(); }
        if probe.video && let Some(mode @ ("screen" | "screen-camera")) = details.capture_mode.as_deref() {
            value["captureMode"] = mode.into();
        }
        if let Some(storage) = &storage { value["storage"] = json!(storage); }
        // Commit media before session.json, then capture cleanup can safely
        // remove the individual chunks.
        fs::OpenOptions::new().write(true).open(folder.join(if storage.is_some() { compact } else { &name }))
            .await?.sync_all().await?;
        let _commit = self.sessions.commit.lock().await;
        let guard = self.sessions.lock(&id).await;
        let mut value = self.sessions.save(&id, value, guard).await?;
        if storage.is_some() { let _ = fs::remove_file(input).await; }
        let (pruned, pending) = self.prune(&id).await;
        if !pruned.is_empty() { value["pruned"] = json!(pruned); }
        if pending { value["cleanupPending"] = true.into(); }
        Ok(value)
    }

    async fn prune(&self, saved: &str) -> (Vec<String>, bool) {
        let Some(keep) = self.sessions.storage().keep else { return (Vec::new(), false) };
        let Ok((sessions, true)) = self.sessions.scan().await else { return (Vec::new(), true) };
        let mut deleted = Vec::new();
        let mut pending = false;
        for session in sessions.into_iter().skip(keep) {
            let Some(id) = session["id"].as_str() else { continue };
            let guard = self.sessions.lock(id).await;
            if id == saved || self.running.lock().unwrap().as_deref() == Some(id) {
                pending = true;
                continue;
            }
            let Ok(Some(current)) = self.sessions.read::<Value>(id).await else { continue };
            let Ok(name) = recording_file(current["file"].as_str()) else { continue };
            if current["id"] != id || !plain_path(&self.sessions.folder(id).join(name), false) { continue; }
            if self.sessions.delete(id, guard).await.is_ok() { deleted.push(id.to_owned()); }
            else { pending = true; }
        }
        (deleted, pending)
    }

    async fn update(&self, id: &str, request: Value) -> Result<Value> {
        let guard = self.sessions.lock(id).await;
        let mut session: Value = self.read(id).await?;
        if let Some(name) = request["title"].as_str() {
            session["title"] =
                title(Some(name), session["kind"].as_str().unwrap_or_default()).into();
        } else if let Some(feedback) = request["feedback"]
            .as_str()
            .filter(|text| text.encode_utf16().count() <= 50_000)
        {
            let instructions = request.get("feedbackPrompt").map(|value| {
                value.as_str().filter(|text| text.encode_utf16().count() <= 4000)
                    .ok_or_else(|| error(400, "Use up to 4,000 characters in the coaching instructions."))
            }).transpose()?;
            session["feedback"] = feedback.into();
            if let Some(instructions) = instructions {
                session["feedbackPrompt"] = instructions.into();
            } else {
                session.as_object_mut().unwrap().remove("feedbackPrompt");
            }
        } else if request.get("tags").is_some() || request.get("cueId").is_some() {
            if let Some(tags) = request.get("tags") {
                session["tags"] = json!(clean_tags(tags)?);
            }
            if let Some(cue) = request.get("cueId") {
                if !cue.is_null() && !cue.is_string() {
                    return Err(error(400, "Choose a cue card from the list."));
                }
                validate_cue(cue.as_str(), &self.topics)?;
                if cue.is_null() {
                    session
                        .as_object_mut()
                        .context("Invalid session.")?
                        .remove("cueId");
                } else {
                    session["cueId"] = cue.clone();
                }
            }
        } else {
            return Err(error(400, "Nothing to update."));
        }
        self.sessions.save(id, session, guard).await
    }

    async fn playback(
        self: &Arc<Self>,
        id: &str,
        headers: HeaderMap,
        head: bool,
        download: bool,
    ) -> Result<Response> {
        let guard = self.sessions.lock(id).await;
        let session: Playback = self.read(id).await?;
        let name = recording_file(session.file.as_deref())?;
        let folder = self.sessions.folder(id);
        let file = folder.join(name);
        if !plain_path(&file, false) { return Err(error(404, "Recording not found.")); }
        #[cfg(feature = "full-media")]
        let (file, guard) = if !download
            && session.mime.as_deref() == Some("video/webm")
            && session.storage.is_none()
            && session.indexed != Some(true)
        {
            let indexed = folder.join("playback.webm");
            if plain_path(&indexed, false) {
                (indexed, guard)
            } else {
                let studio = Arc::clone(self);
                // A cancelled seek must not release the lock while this job is
                // still writing or renaming the prepared playback file.
                tokio::spawn(async move {
                    let file = studio.prepare_playback(file, indexed).await;
                    (file, guard)
                })
                .await?
            }
        } else {
            (file, guard)
        };
        let file = fs::File::open(file).await?;
        drop(guard);
        let mut response = file_body(
            file,
            &headers,
            head,
            session.mime.as_deref().unwrap_or("application/octet-stream"),
            self.transport,
        )
        .await?;
        if download {
            response.headers_mut().insert(
                "content-disposition",
                format!("attachment; filename=\"{name}\"").parse()?,
            );
        }
        Ok(response)
    }

    #[cfg(feature = "full-media")]
    async fn prepare_playback(&self, original: PathBuf, indexed: PathBuf) -> PathBuf {
        if !self.media.webm_duration(&original).await.is_ok_and(|duration| duration == 0.0) {
            return original;
        }
        let temporary = indexed.with_extension(format!("{}.partial.webm", Uuid::new_v4()));
        if self.media.index_video(&original, &temporary).await.is_ok()
            && fs::rename(&temporary, &indexed).await.is_ok()
        {
            return indexed;
        }
        let _ = fs::remove_file(temporary).await;
        original
    }

    async fn transcribe(&self, id: &str, model_id: &str) -> Result<Value> {
        let model = self
            .models
            .model(model_id)
            .filter(|model| self.models.installed(model));
        let (Some(model), Some(library)) = (model, self.models.engine()) else {
            return Err(error(
                409,
                "Install your selected transcription model in Settings first.",
            ));
        };
        if !self.media.ready() {
            return Err(error(503, "Install FFmpeg and restart Rehearsal."));
        }
        let guard = self.sessions.lock(id).await;
        let session: Value = self.read(id).await?;
        {
            let mut running = self.running.lock().unwrap();
            if running.is_some() {
                return Err(error(
                    409,
                    "Rehearsal is already transcribing a take. Try again when it finishes.",
                ));
            }
            *running = Some(id.to_string());
        }
        drop(guard);
        let _running = Running(&self.running);
        let folder = self.sessions.folder(id);
        let input = folder.join(recording_file(session["file"].as_str())?);
        if !plain_path(&input, false) { return Err(error(404, "Recording not found.")); }
        let pcm = folder.join(format!("transcribe-{}.pcm", Uuid::new_v4()));
        let result = async {
            self.media.decode_audio(&input, &pcm).await?;
            let mut child = media::command(&media::executable()?);
            child
                .arg("--asr-worker")
                .arg(&library)
                .arg(self.models.file(&model))
                .arg(&pcm)
                .env(media::LIBRARY_SEARCH_PATH, media::library_path(&library)?);
            let transcript: Transcript = serde_json::from_str(&media::output(child, 600).await?)?;
            let duration = session["duration"].as_f64().unwrap_or_default().max(0.0);
            let rows = if transcript.segments.is_empty() {
                segments(transcript.words, duration, session["script"].is_string())
            } else {
                transcript.segments.into_iter().filter_map(|row| {
                    let start = row.start.clamp(0.0, duration);
                    let end = row.end.clamp(start, duration);
                    (end > start).then(|| json!({"start": start, "end": end, "text": row.text}))
                }).collect()
            };
            if rows.is_empty() && !transcript.text.trim().is_empty() {
                anyhow::bail!("The engine returned text without usable timestamps.");
            }
            let guard = self.sessions.lock(id).await;
            let mut current: Value = self.read(id).await?;
            current["segments"] = json!(rows);
            current["modelId"] = model_id.into();
            current["transcriptDone"] = true.into();
            current["feedback"] = Value::Null;
            current.as_object_mut().unwrap().remove("feedbackPrompt");
            self.sessions.save(id, current, guard).await
        }
        .await;
        let _ = fs::remove_file(pcm).await;
        result.map_err(|err| {
            eprintln!("Transcription failed: {err:#}");
            error(502, "Transcription did not finish. The recording is still saved. Check Settings and try again.")
        })
    }

    async fn feedback_request(&self, body: &Value) -> Result<reqwest::RequestBuilder> {
        let provider = body["provider"].as_str().unwrap_or_default();
        let mut request = match provider {
            "openai" => self
                .client
                .post("https://api.openai.com/v1/chat/completions"),
            "ollama" => self.client.post("http://127.0.0.1:11434/api/chat"),
            _ => return Err(error(400, "Choose a coaching provider in Settings.")),
        };
        let prompt = body["prompt"]
            .as_str()
            .filter(|text| !text.trim().is_empty() && text.encode_utf16().count() <= 48_000)
            .ok_or_else(|| error(400, "The coaching prompt is missing or too long."))?;
        let model = body["model"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.encode_utf16().count() <= 100)
            .ok_or_else(|| error(400, "Enter a model name in Settings."))?;
        let mut payload = json!({"model": model, "messages": [{"role": "user", "content": prompt}], "stream": false});
        if provider == "openai" {
            // Older clients can still supply a key for one request.
            let key = if let Some(key) = body["key"].as_str().map(str::trim).filter(|key| !key.is_empty()) {
                Some(key.to_owned())
            } else {
                match self.credentials.read().await {
                    Ok(saved) => saved.or_else(environment_key),
                    Err(err) => Some(environment_key().ok_or_else(|| error(503, err.to_string()))?),
                }
            }
                .ok_or_else(|| error(400, "Add an OpenAI API key in Settings."))?;
            request = request.bearer_auth(key);
            payload["store"] = false.into();
        }
        Ok(request.json(&payload))
    }

    async fn feedback(&self, body: Value) -> Result<Value> {
        let provider = body["provider"].as_str().unwrap_or_default();
        let request = self.feedback_request(&body).await?;
        let response = request
            .timeout(Duration::from_secs(150))
            .send()
            .await
            .map_err(|_| {
                error(
                    502,
                    if provider == "ollama" {
                        "Could not reach Ollama. Start Ollama on this device and try again."
                    } else {
                        "Could not reach OpenAI. Check your connection and try again."
                    },
                )
            })?;
        if !response.status().is_success() {
            return Err(error(
                502,
                if response.status() == StatusCode::UNAUTHORIZED {
                    "OpenAI rejected the API key.".into()
                } else {
                    format!(
                        "The provider could not answer (HTTP {}). Check the model and account.",
                        response.status().as_u16()
                    )
                },
            ));
        }
        let data: Value = response
            .json()
            .await
            .map_err(|_| error(502, "The provider returned no feedback."))?;
        let result = if provider == "openai" {
            &data["choices"][0]["message"]["content"]
        } else {
            &data["message"]["content"]
        };
        let mut units = 0;
        let text: String = result
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| error(502, "The provider returned no feedback."))?
            .chars()
            .take_while(|ch| { units += ch.len_utf16(); units <= 50_000 })
            .collect();
        Ok(json!({"text": text}))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Upload {
    filename: String,
    title: Option<String>,
    script: Option<String>,
    cue_id: Option<String>,
    capture_mode: Option<String>,
    #[cfg(feature = "full-media")]
    #[serde(default)]
    recorded: bool,
    #[cfg(feature = "full-media")]
    #[serde(default)]
    compress: bool,
}

impl Upload {
    fn validate(&self, topics: &custom_topics::TopicLibrary) -> Result<String> {
        let extension = Path::new(&self.filename)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if media_type(&extension).is_none() {
            return Err(error(400, "Choose a supported audio or video file."));
        }
        validate_cue(self.cue_id.as_deref(), topics)?;
        let script = self.script.as_deref().map(str::trim);
        if let Some(script) = script {
            let words = SCRIPT_WORDS.find_iter(script).take(1601).count();
            if self.script.as_deref().unwrap().encode_utf16().count() > 12000
                || !(1..=1600).contains(&words)
            {
                return Err(error(
                    400,
                    "Use a script with 1–1600 words and at most 12000 characters.",
                ));
            }
        }
        Ok(extension)
    }
}

#[derive(Deserialize)]
struct Playback {
    file: Option<String>,
    mime: Option<String>,
    #[cfg(feature = "full-media")]
    indexed: Option<bool>,
    #[cfg(feature = "full-media")]
    storage: Option<serde::de::IgnoredAny>,
}

struct Running<'a>(&'a Mutex<Option<String>>);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = None;
    }
}

fn header<'a>(headers: &'a HeaderMap, key: &str) -> &'a str {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

async fn read_json(req: Request) -> Result<Value> {
    read_json_limited(req, 60_000).await
}

async fn read_json_limited(req: Request, limit: usize) -> Result<Value> {
    let bytes = to_bytes(req.into_body(), limit)
        .await
        .map_err(|_| error(413, "The request is too long."))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| error(400, "The request could not be read."))?;
    if !value.is_object() {
        return Err(error(400, "The request could not be read."));
    }
    Ok(value)
}

fn title(value: Option<&str>, kind: &str) -> String {
    match value.map(str::trim).filter(|text| !text.is_empty()) {
        Some(name) => name.chars().take(110).collect(),
        None if kind == "video" => "Video practice".into(),
        None => "Voice practice".into(),
    }
}

fn environment_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}

fn credential_status(saved: bool, available: bool) -> Value {
    json!({ "saved": saved, "available": available, "environment": environment_key().is_some() })
}

fn validate_cue(value: Option<&str>, topics: &custom_topics::TopicLibrary) -> Result<()> {
    if value.is_some_and(|value| !CUES.contains(value) && !topics.contains(value)) {
        return Err(error(400, "Choose a cue card from the list."));
    }
    Ok(())
}

fn clean_tags(values: &Value) -> Result<Vec<String>> {
    let invalid = || error(400, "Use up to 10 tags, each between 1 and 32 characters.");
    let values = values
        .as_array()
        .filter(|values| values.len() <= 10)
        .ok_or_else(invalid)?;
    let mut result = Vec::new();
    let mut unique = HashSet::new();
    for value in values {
        let tag = value
            .as_str()
            .ok_or_else(invalid)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if tag.is_empty()
            || tag.encode_utf16().count() > 32
            || tag.chars().any(|ch| ch == ',' || ch.is_control())
        {
            return Err(invalid());
        }
        if unique.insert(tag.to_lowercase()) {
            result.push(tag);
        }
    }
    Ok(result)
}

fn recording_file(file: Option<&str>) -> Result<&str> {
    file.filter(|name| name.rsplit_once('.').is_some_and(|(stem, ext)| {
        matches!(stem, "recording" | "recording-small") && media_type(ext).is_some()
    })).ok_or_else(|| error(404, "Recording not found."))
}

fn segments(words: Vec<Word>, duration: f64, include_word_times: bool) -> Vec<Value> {
    fn flush(
        current: &mut Vec<Word>,
        output: &mut Vec<Value>,
        duration: f64,
        include_word_times: bool,
    ) {
        if current.is_empty() {
            return;
        }
        let first = current[0].start;
        let end = duration.min((first + 0.08).max(current.last().unwrap().end));
        let mut text = String::new();
        for word in current.iter() {
            if !text.is_empty() && !word.text.starts_with([',', '.', ';', ':', '!', '?']) {
                text.push(' ');
            }
            text.push_str(&word.text);
        }
        if end > first && !text.is_empty() {
            let mut segment = json!({"start": first, "end": end, "text": text});
            if include_word_times {
                segment["words"] = json!(
                    current
                        .iter()
                        .map(|word| json!({"text": word.text, "start": word.start}))
                        .collect::<Vec<_>>()
                );
            }
            output.push(segment);
        }
        current.clear();
    }
    let mut output = Vec::new();
    let mut current: Vec<Word> = Vec::new();
    for mut word in words {
        if !word.start.is_finite()
            || !word.end.is_finite()
            || word.start >= duration
            || word.end < word.start
            || word.text.trim().is_empty()
        {
            continue;
        }
        word.start = word.start.max(0.0);
        word.end = word.end.max(0.0);
        word.text = word.text.trim().to_owned();
        if current.len() >= 3 && word.start - current.last().unwrap().end >= 0.72 {
            flush(&mut current, &mut output, duration, include_word_times);
        }
        let ending = word
            .text
            .trim_end_matches(['"', '\'', ')'])
            .ends_with(['.', '!', '?']);
        let last = word.end;
        current.push(word);
        if ending || current.len() >= 19 || last - current[0].start >= 11.0 {
            flush(&mut current, &mut output, duration, include_word_times);
        }
    }
    flush(&mut current, &mut output, duration, include_word_times);
    output
}

fn media_type(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        _ => return None,
    })
}

fn asset(headers: &HeaderMap, head: bool, path: &str) -> Result<Response> {
    let path = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    #[cfg(feature = "full-media")]
    if path == WASM {
        let file = include_bytes!("../../public/assets/vision_wasm_internal.wasm.gz");
        let gzip = header(headers, "accept-encoding").split(',').any(|entry| {
            let (name, quality) = entry
                .trim()
                .split_once(';')
                .unwrap_or((entry.trim(), "q=1"));
            name.eq_ignore_ascii_case("gzip")
                && quality
                    .trim()
                    .strip_prefix("q=")
                    .and_then(|q| q.parse::<f64>().ok())
                    .is_some_and(|q| q > 0.0)
        }) && !headers.contains_key("range");
        let mut response = if gzip {
            bytes_body(Bytes::from_static(file), headers, head, "application/wasm")
        } else {
            let mut bytes = Vec::new();
            GzDecoder::new(&file[..]).read_to_end(&mut bytes)?;
            bytes_body(bytes.into(), headers, head, "application/wasm")
        };
        response
            .headers_mut()
            .insert("vary", "Accept-Encoding".parse().unwrap());
        if gzip {
            response
                .headers_mut()
                .insert("content-encoding", "gzip".parse().unwrap());
        }
        return Ok(response);
    }
    let file = embedded(path).ok_or_else(|| error(404, "Not found."))?;
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    let mime = match extension {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "tflite" => "application/octet-stream",
        "txt" => "text/plain; charset=utf-8",
        extension => media_type(extension).ok_or_else(|| error(404, "Not found."))?,
    };
    Ok(bytes_body(Bytes::from_static(file), headers, head, mime))
}

fn embedded(path: &str) -> Option<&'static [u8]> {
    macro_rules! file {
        ($name:literal) => {
            include_bytes!(concat!("../../public/", $name))
        };
    }
    Some(match path {
        "index.html" => file!("index.html"),
        "style.css" => file!("style.css"),
        "app.js" => file!("app.js"),
        "api.js" => file!("api.js"),
        "appearance.js" => file!("appearance.js"),
        "camera.js" => file!("camera.js"),
        "capture-options.js" => file!("capture-options.js"),
        "events.js" => file!("events.js"),
        "feedback.js" => file!("feedback.js"),
        "library.js" => file!("library.js"),
        "models.js" => file!("models.js"),
        "models-view.js" => file!("models-view.js"),
        "practice.js" => file!("practice.js"),
        "preferences.js" => file!("preferences.js"),
        "preferences-backup.js" => file!("preferences-backup.js"),
        "prompter-page.js" => file!("prompter-page.js"),
        "recorder-view.js" => file!("recorder-view.js"),
        "recording-upload.js" => file!("recording-upload.js"),
        "recording.js" => file!("recording.js"),
        "review.js" => file!("review.js"),
        "settings.js" => file!("settings.js"),
        "state.js" => file!("state.js"),
        "topics.js" => file!("topics.js"),
        "ui.js" => file!("ui.js"),
        "cue-cards.js" => file!("cue-cards.js"),
        "cue-data.json" => file!("cue-data.json"),
        "recorder-display.js" => file!("recorder-display.js"),
        "screen-camera.js" => file!("screen-camera.js"),
        "script-review.js" => file!("script-review.js"),
        "teleprompter.js" => file!("teleprompter.js"),
        "transcription-status.js" => file!("transcription-status.js"),
        "assets/bitcount.woff2" => file!("assets/bitcount.woff2"),
        "assets/bitcount-ofl.txt" => file!("assets/bitcount-ofl.txt"),
        "assets/example.json" => file!("assets/example.json"),
        "assets/example.mp3" => file!("assets/example.mp3"),
        "assets/geist.woff2" => file!("assets/geist.woff2"),
        "assets/geist-ofl.txt" => file!("assets/geist-ofl.txt"),
        "assets/mark.svg" => file!("assets/mark.svg"),
        #[cfg(feature = "full-media")]
        "background.js" => file!("background.js"),
        #[cfg(feature = "full-media")]
        "background-worker.js" => file!("background-worker.js"),
        #[cfg(feature = "full-media")]
        "assets/mediapipe-license.txt" => file!("assets/mediapipe-license.txt"),
        #[cfg(feature = "full-media")]
        "assets/selfie-segmenter.tflite" => file!("assets/selfie-segmenter.tflite"),
        #[cfg(feature = "full-media")]
        "assets/vision_bundle.mjs" => file!("assets/vision_bundle.mjs"),
        #[cfg(feature = "full-media")]
        "assets/vision_wasm_internal.js" => file!("assets/vision_wasm_internal.js"),
        _ => return None,
    })
}

enum Range {
    Full,
    Partial(u64, u64),
    Invalid,
}
fn range(headers: &HeaderMap, size: u64) -> Range {
    let Some(header) = headers.get("range") else {
        return Range::Full;
    };
    let get = || -> Option<(u64, u64)> {
        let (first, last) = header
            .to_str()
            .ok()?
            .strip_prefix("bytes=")?
            .split_once('-')?;
        let end = size.checked_sub(1)?;
        let pair = if first.is_empty() {
            let suffix: u64 = last.parse().ok()?;
            if suffix == 0 {
                return None;
            }
            (size.saturating_sub(suffix), end)
        } else {
            (
                first.parse().ok()?,
                if last.is_empty() {
                    end
                } else {
                    end.min(last.parse().ok()?)
                },
            )
        };
        (pair.0 <= pair.1).then_some(pair)
    };
    get()
        .map(|(start, end)| Range::Partial(start, end))
        .unwrap_or(Range::Invalid)
}

fn stream_response(range: Range, size: u64, mime: &str, body: Body) -> Response {
    let builder = Response::builder()
        .header("content-type", mime)
        .header("accept-ranges", "bytes");
    match range {
        Range::Full => builder.header("content-length", size).body(body),
        Range::Partial(start, end) => builder
            .status(206)
            .header("content-range", format!("bytes {start}-{end}/{size}"))
            .header("content-length", end - start + 1)
            .body(body),
        Range::Invalid => builder
            .status(416)
            .header("content-range", format!("bytes */{size}"))
            .body(Body::empty()),
    }
    .unwrap()
}

fn bytes_body(bytes: Bytes, headers: &HeaderMap, head: bool, mime: &str) -> Response {
    let size = bytes.len() as u64;
    let range = range(headers, size);
    let body = if head {
        Body::empty()
    } else {
        match range {
            Range::Full => Body::from(bytes),
            Range::Partial(start, end) => Body::from(bytes.slice(start as usize..=end as usize)),
            Range::Invalid => Body::empty(),
        }
    };
    stream_response(range, size, mime, body)
}

async fn file_body(
    mut file: fs::File,
    headers: &HeaderMap,
    head: bool,
    mime: &str,
    transport: Transport,
) -> Result<Response> {
    let size = file.metadata().await?.len();
    let range = range(headers, size);
    #[cfg(windows)]
    if matches!(transport, Transport::WebView) && !head {
        let (start, len) = match range {
            Range::Full => (0, size),
            Range::Partial(start, end) => (start, end - start + 1),
            Range::Invalid => return Ok(stream_response(range, size, mime, Body::empty())),
        };
        let mut response = stream_response(range, size, mime, Body::empty());
        response.extensions_mut().insert(FileSlice {
            file: Arc::new(file.into_std().await),
            start,
            len,
        });
        return Ok(response);
    }
    #[cfg(not(windows))]
    let _ = transport;
    let body = if head {
        Body::empty()
    } else {
        match range {
            Range::Full => Body::from_stream(ReaderStream::with_capacity(file, MEDIA_BUFFER)),
            Range::Partial(start, end) => {
                file.seek(std::io::SeekFrom::Start(start)).await?;
                Body::from_stream(ReaderStream::with_capacity(
                    file.take(end - start + 1),
                    (MEDIA_BUFFER as u64).min(end - start + 1) as usize,
                ))
            }
            Range::Invalid => Body::empty(),
        }
    };
    Ok(stream_response(range, size, mime, body))
}

#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct FileSlice {
    pub file: Arc<std::fs::File>,
    pub start: u64,
    pub len: u64,
}

/// Safari 17.6 (WebKit 618.3) fixed `getDisplayMedia` in apps whose web view
/// answers media capture permission requests, as wry does
/// (https://bugs.webkit.org/show_bug.cgi?id=274896). It shipped for macOS 12
/// to 14, so the loaded WebKit decides this, not the macOS version.
#[cfg(target_os = "macos")]
fn desktop_screen_capture() -> bool {
    use std::sync::LazyLock;
    static SUPPORTED: LazyLock<bool> =
        LazyLock::new(|| loaded_webkit().is_some_and(|version| version >= (618, 3)));
    *SUPPORTED
}

#[cfg(not(target_os = "macos"))]
fn desktop_screen_capture() -> bool {
    true
}

/// The oldest WebKit the page runs on, as (build, Safari release). Safari 16.2
/// added `color-mix()`, used by the theme colors; it also covers container
/// queries, `AbortSignal.timeout()` (16.0) and `Object.hasOwn()`, `:has()` and
/// `<dialog>` (15.4).
#[cfg(target_os = "macos")]
pub(crate) const MINIMUM_WEBKIT: ((u32, u32), &str) = ((614, 3), "16.2");

#[cfg(target_os = "macos")]
pub(crate) fn loaded_webkit() -> Option<(u32, u32)> {
    crate::macos::webkit_version()
        .as_deref()
        .and_then(parse_webkit_version)
}

/// Reads a WebKit `CFBundleVersion` such as "19618.3.11.11.5" as (618, 3). The
/// digits before the last three of the first part only name the macOS release.
#[cfg(any(target_os = "macos", test))]
fn parse_webkit_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().split('.');
    let build: u32 = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    Some((build % 1000, minor))
}
