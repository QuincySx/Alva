// INPUT: Explicit scoped artifact IDs, installed decoder, root ALVA Plugin authority and model route declaration
// OUTPUT: HubMediaTool and request-only selected image delivery middleware
// POS: Optional product media processing; originals/reference policy unchanged, no automatic raw media input.
use crate::{
    store::{MediaArtifact, MediaSource},
    HubStore,
};
use alva_engine_adapter_alva::{
    AgentState, LlmCallFn, Middleware, MiddlewareError, MiddlewarePriority,
};
use alva_kernel_abi::{
    AgentError, CancellationToken, ContentBlock, Message, Tool, ToolContent, ToolExecutionContext,
    ToolOutput, ToolSchemaContext,
};
use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{process::Stdio, sync::OnceLock, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Clone)]
pub(crate) struct HubMedia {
    pub store: HubStore,
    pub product: String,
    pub native: String,
    pub image_capability: bool,
    pub route: String,
}
pub(crate) struct HubMediaTool(pub HubMedia);
pub(crate) struct MediaDeliveryMiddleware(pub HubMedia);

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Probe {
        attachment_id: String,
    },
    ProcessImage {
        attachment_id: String,
    },
    ExtractFrames {
        attachment_id: String,
        #[serde(default)]
        timestamps: Option<Vec<f64>>,
        #[serde(default)]
        frame_count: Option<usize>,
        reason: String,
    },
    TranscodeVideo {
        attachment_id: String,
    },
    ListArtifacts {
        attachment_id: String,
        #[serde(default)]
        after: i64,
    },
    ReadManifest {
        artifact_id: String,
    },
    Select {
        artifact_ids: Vec<String>,
        reason: String,
    },
    Receipt {
        delivery_id: String,
    },
    CancelSelection {
        delivery_id: String,
    },
}
#[async_trait]
impl Tool for HubMediaTool {
    fn name(&self) -> &str {
        "hub_media"
    }
    fn description(&self) -> &str {
        "Explicitly probe/process uploaded media by attachment ID, never paths. Create <=512px lossy JPEG, 1–4 video frames (uniform or chosen timestamps), or a silent <=60s 720px/5fps compressed video. Originals are preserved. Read a derivative manifest before select; select 1–2 derived JPEGs for ONE model request only if the pinned route declares images/vision. Receipt records actual request outcome, not understanding. Native decoder access follows host approval; no automatic processing or upload-body delivery."
    }
    fn parameters_schema(&self, _: &ToolSchemaContext) -> Value {
        json!({"type":"object","required":["action"],"additionalProperties":false,"properties":{
            "action":{"type":"string","enum":["probe","process_image","extract_frames","transcode_video","list_artifacts","read_manifest","select","receipt","cancel_selection"]},
            "attachment_id":{"type":"string","minLength":1,"maxLength":128},"artifact_id":{"type":"string","maxLength":128},"delivery_id":{"type":"string","maxLength":128},
            "after":{"type":"integer","minimum":0},"timestamps":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"number","minimum":0}},
            "frame_count":{"type":"integer","minimum":1,"maximum":4},"reason":{"type":"string","minLength":1,"maxLength":200},
            "artifact_ids":{"type":"array","minItems":1,"maxItems":2,"uniqueItems":true,"items":{"type":"string","minLength":1,"maxLength":128}}
        }})
    }
    fn is_read_only(&self, input: &Value) -> bool {
        matches!(
            input["action"].as_str(),
            Some("list_artifacts" | "read_manifest" | "receipt")
        )
    }
    fn describe_effect(&self, input: &Value) -> alva_kernel_abi::EffectDescription {
        alva_kernel_abi::EffectDescription::of(match input["action"].as_str() {
            Some("select")=>"Send only the selected bounded derived JPEGs to the pinned model in one request; original images/video/audio are not attached. Provider access/cost and existing permission policy apply.",
            Some("process_image"|"extract_frames"|"transcode_video"|"probe")=>"Run the installed native decoder on the session-owned uploaded byte snapshot and save bounded derivatives. Originals are preserved. This decoder is not an OS sandbox. Processing itself does not send media to the model.",
            _=>"Read only this session's media index, processing manifest or delivery receipt. No decoding or automatic media delivery.",
        })
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let failure = |message: String| AgentError::ToolError {
            tool_name: "hub_media".into(),
            message,
        };
        if ctx.session_id() != self.0.native {
            return Err(failure("Media session binding mismatch".into()));
        }
        if ctx.cancel_token().is_cancelled() {
            return Err(failure("Media operation cancelled".into()));
        }
        if input.to_string().len() > 16 * 1024 {
            return Err(failure("Media request exceeds 16 KiB input budget".into()));
        }
        let request: Request =
            serde_json::from_value(input).map_err(|_| failure("Invalid media request".into()))?;
        let value=match request {
            Request::ListArtifacts{attachment_id,after}=>self.0.store.media_index(&self.0.product,&attachment_id,after),
            Request::ReadManifest{artifact_id}=>self.0.store.media_artifact(&self.0.product,&artifact_id).map(|a|a.manifest),
            Request::Receipt{delivery_id}=>self.0.store.media_receipt(&self.0.product,&self.0.native,&delivery_id).map(|r|r.0),
            Request::CancelSelection{delivery_id}=>self.0.store.transition_media_delivery(&self.0.product,&self.0.native,&delivery_id,"prepared","not_submitted_cancelled").map(|_|json!({"delivery_id":delivery_id,"status":"not_submitted_cancelled"})),
            Request::Select{artifact_ids,reason}=>{
                if !self.0.image_capability {return Err(failure("Pinned model route does not explicitly declare image/vision support; media was not sent".into()));}
                let call=ctx.tool_call_id().ok_or_else(||failure("Media selection requires a runtime tool-call identity".into()))?;
                self.0.store.prepare_media_delivery(&self.0.product,&self.0.native,call,&artifact_ids,&reason,&self.0.route)
            }
            processing=>self.process(processing,ctx.cancel_token().clone()).await,
        }.map_err(failure)?;
        // Large processing manifests use the ordinary durable reference policy.
        Ok(ToolOutput::text(value.to_string()))
    }
}

#[derive(Clone)]
struct Probe {
    value: Value,
    width: u64,
    height: u64,
    duration: Option<f64>,
}
fn input_format(kind: &str) -> Result<&'static str, String> {
    match kind {"image/png"|"image/jpeg"|"image/webp"=>Ok("image2pipe"),"image/gif"=>Ok("gif"),"video/mp4"|"video/quicktime"=>Ok("mov"),"video/webm"=>Ok("matroska"),_=>Err("This decoder supports PNG/JPEG/WebP/GIF and MP4/MOV/WebM only; source was not analyzed".into())}
}
fn decoder(name: &str) -> Result<String, String> {
    ["/opt/homebrew/bin","/usr/local/bin","/usr/bin"].iter().map(|p|format!("{p}/{name}")).find(|p|std::path::Path::new(p).is_file()).ok_or_else(||format!("Installed {name} is unavailable; no program was installed and no media was processed"))
}
async fn capture(
    mut reader: impl AsyncRead + Unpin,
    maximum: usize,
) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(read) > maximum {
            return Err(std::io::Error::other("decoder output budget exceeded"));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}
async fn run_decoder(
    program: &str,
    args: &[String],
    source: Option<&std::fs::File>,
    mut cancel: CancellationToken,
    maximum: usize,
) -> Result<(Vec<u8>, String), String> {
    if cancel.is_cancelled() {
        return Err("Media operation cancelled".into());
    }
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let descriptor = source.map(|s| s.as_raw_fd());
        unsafe {
            command.pre_exec(move || {
                // Fixed native decoder only, no shell, network protocol or nested
                // media command. CPU limits (and Linux address space) fail closed.
                let cpu = libc::rlimit {
                    rlim_cur: 30,
                    rlim_max: 30,
                };
                #[cfg(target_os = "linux")]
                let memory = libc::rlimit {
                    rlim_cur: 2 * 1024 * 1024 * 1024,
                    rlim_max: 2 * 1024 * 1024 * 1024,
                };
                if libc::setrlimit(libc::RLIMIT_CPU, &cpu) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(fd) = descriptor {
                    if libc::dup2(fd, 3) < 0
                        || libc::fcntl(3, libc::F_SETFD, 0) < 0
                        || libc::lseek(3, 0, libc::SEEK_SET) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = source;
        return Err("Native media processing is currently supported on macOS/Linux only".into());
    }
    let mut child = command.spawn().map_err(|error| {
        format!("Decoder unavailable or resource limits could not be enforced: {error}")
    })?;
    let stdout = child.stdout.take().ok_or("Decoder output unavailable")?;
    let stderr = child.stderr.take().ok_or("Decoder output unavailable")?;
    let process_id = child.id().ok_or("Decoder process unavailable")?;
    let reaped = std::sync::atomic::AtomicBool::new(false);
    let outcome = tokio::select! {
        biased;
        _=cancel.cancelled()=>Err("Media operation cancelled".to_string()),
        _=tokio::time::sleep(Duration::from_secs(35))=>Err("Media operation timed out".to_string()),
        result=async {let wait=async {let result=child.wait().await;reaped.store(result.is_ok(),std::sync::atomic::Ordering::Release);result};tokio::try_join!(wait,capture(stdout,maximum),capture(stderr,128*1024))}=>result.map_err(|_|"Decoder output budget exceeded or capture failed".to_string()),
        result=watch_memory(process_id,&reaped)=>Err(result.err().unwrap_or_else(||"Decoder memory guard stopped".into())),
    };
    match outcome {
        Ok((status, output, error)) if status.success() => {
            Ok((output, String::from_utf8_lossy(&error).into_owned()))
        }
        Ok((_status, _output, _error)) => {
            #[cfg(test)]
            eprintln!("Synthetic decoder fixture failure: {_status}: {_error:?}");
            Err("Media decoder failed; no complete derived artifact was saved".into())
        }
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(error)
        }
    }
}
async fn watch_memory(
    process_id: u32,
    reaped: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if reaped.load(std::sync::atomic::Ordering::Acquire) {
            std::future::pending::<()>().await;
        }
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
        let result = unsafe {
            libc::proc_pid_rusage(
                process_id as libc::c_int,
                libc::RUSAGE_INFO_V2,
                info.as_mut_ptr().cast::<libc::rusage_info_t>(),
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            // Only the unreaped owned child is inspected. It may already be
            // exiting; wait/capture owns its terminal state, never another PID.
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err("Decoder memory supervision unavailable".into());
        }
        if unsafe { info.assume_init() }.ri_resident_size > 512 * 1024 * 1024 {
            return Err("Decoder exceeded sampled 512 MiB resident-memory guard".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (process_id, reaped);
        std::future::pending().await
    }
}
fn input_args(format: &str) -> Vec<String> {
    let mut args = vec![
        "-max_alloc".into(),
        "67108864".into(),
        "-threads".into(),
        "1".into(),
        "-protocol_whitelist".into(),
        "file,pipe".into(),
        "-f".into(),
        format.into(),
    ];
    if format == "mov" {
        args.extend([
            "-enable_drefs".into(),
            "0".into(),
            "-use_absolute_path".into(),
            "0".into(),
        ]);
    }
    args.extend(["-i".into(), "/dev/fd/3".into()]);
    args
}
async fn probe(
    program: &str,
    file: &std::fs::File,
    format: &str,
    cancel: CancellationToken,
) -> Result<Probe, String> {
    let mut args = input_args(format);
    args.splice(0..0, ["-v".into(), "error".into()]);
    args.extend([
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=codec_name,width,height,duration,r_frame_rate:format=duration,format_name".into(),
        "-of".into(),
        "json".into(),
    ]);
    let (bytes, _) = run_decoder(program, &args, Some(file), cancel, 128 * 1024).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Media probe corrupt")?;
    let stream = &value["streams"][0];
    let width = stream["width"]
        .as_u64()
        .ok_or("No decoded video/image dimensions")?;
    let height = stream["height"]
        .as_u64()
        .ok_or("No decoded video/image dimensions")?;
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || width.saturating_mul(height) > 16 * 1024 * 1024
    {
        return Err("Media exceeds 16 megapixel/8192 edge decoding budget".into());
    }
    let duration = value["format"]["duration"]
        .as_str()
        .or_else(|| stream["duration"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 3600.0);
    Ok(Probe {
        value,
        width,
        height,
        duration,
    })
}
impl HubMediaTool {
    async fn process(&self, request: Request, cancel: CancellationToken) -> Result<Value, String> {
        static SLOTS: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
        let _permit = SLOTS
            .get_or_init(|| tokio::sync::Semaphore::new(2))
            .try_acquire()
            .map_err(|_| "Media processing busy; try again")?;
        let id = match &request {
            Request::Probe { attachment_id }
            | Request::ProcessImage { attachment_id }
            | Request::ExtractFrames { attachment_id, .. }
            | Request::TranscodeVideo { attachment_id } => attachment_id,
            _ => return Err("Invalid processing action".into()),
        };
        let source = self.0.store.media_source(&self.0.product, id)?;
        let format = input_format(&source.content_type)?;
        let file = self.0.store.media_snapshot(&self.0.product, &source)?;
        let ffprobe = decoder("ffprobe")?;
        let original = probe(&ffprobe, &file, format, cancel.clone()).await?;
        if matches!(request, Request::Probe { .. }) {
            return Ok(
                json!({"attachment_id":source.id,"source_sha256":source.hash,"probe":original.value,"method":"ffprobe_metadata_only","semantic_analysis":false,"model_delivery":false}),
            );
        }
        let ffmpeg = decoder("ffmpeg")?;
        let (version, _) =
            run_decoder(&ffmpeg, &["-version".into()], None, cancel.clone(), 8192).await?;
        let version = String::from_utf8_lossy(&version)
            .lines()
            .next()
            .unwrap_or("unknown")
            .chars()
            .take(200)
            .collect::<String>();
        let mut outputs = Vec::new();
        let is_video = source.content_type.starts_with("video/");
        match request {
            Request::ProcessImage { .. } => {
                if is_video {
                    return Err(
                        "Use extract_frames for video; image processing accepts image uploads only"
                            .into(),
                    );
                }
                outputs.push(
                    self.jpeg(
                        &source,
                        &file,
                        &original,
                        &ffmpeg,
                        &ffprobe,
                        &version,
                        None,
                        "first_image_frame",
                        "Explicit image compression; animation beyond the first frame is omitted",
                        cancel.clone(),
                    )
                    .await?,
                );
            }
            Request::ExtractFrames {
                timestamps,
                frame_count,
                reason,
                ..
            } => {
                if !is_video || reason.is_empty() || reason.chars().count() > 200 {
                    return Err("Video frame extraction requires a video and a 1–200 character selection reason".into());
                }
                let duration = original
                    .duration
                    .ok_or("Video duration unavailable or exceeds 3600 seconds")?;
                let (times, strategy) =
                    match (timestamps, frame_count) {
                        (Some(times), None)
                            if !times.is_empty()
                                && times.len() <= 4
                                && times
                                    .iter()
                                    .all(|t| t.is_finite() && *t >= 0.0 && *t < duration) =>
                        {
                            (times, "requested_timestamps")
                        }
                        (None, count) => {
                            let count = count.unwrap_or(3);
                            if !(1..=4).contains(&count) {
                                return Err("Frame count must be 1–4".into());
                            }
                            (
                                (1..=count)
                                    .map(|n| duration * n as f64 / (count + 1) as f64)
                                    .collect(),
                                "uniform_time",
                            )
                        }
                        _ => return Err(
                            "Use either 1–4 timestamps inside the video or frame_count, not both"
                                .into(),
                        ),
                    };
                for time in times {
                    outputs.push(
                        self.jpeg(
                            &source,
                            &file,
                            &original,
                            &ffmpeg,
                            &ffprobe,
                            &version,
                            Some(time),
                            strategy,
                            &reason,
                            cancel.clone(),
                        )
                        .await?,
                    );
                }
            }
            Request::TranscodeVideo { .. } => {
                if !is_video {
                    return Err("Video transcoding requires a video upload".into());
                }
                let duration = original
                    .duration
                    .ok_or("Video duration unavailable or exceeds 3600 seconds")?
                    .min(60.0);
                let mut args = input_args(format);
                args.splice(0..0, ["-v".into(), "error".into()]);
                args.extend(["-filter_threads".into(),"1".into(),"-map".into(),"0:v:0".into(),"-an".into(),"-t".into(),duration.to_string(),"-vf".into(),"scale=w='min(720,iw)':h='min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,fps=5".into(),"-c:v".into(),"libx264".into(),"-threads".into(),"1".into(),"-pix_fmt".into(),"yuv420p".into(),"-b:v".into(),"400k".into(),"-maxrate".into(),"500k".into(),"-bufsize".into(),"1000k".into(),"-movflags".into(),"frag_keyframe+empty_moov".into(),"-f".into(),"mp4".into(),"pipe:1".into()]);
                let (bytes, _) =
                    run_decoder(&ffmpeg, &args, Some(&file), cancel.clone(), 8 * 1024 * 1024)
                        .await?;
                let derived = MediaSource {
                    id: source.id.clone(),
                    bytes: bytes.clone(),
                    content_type: "video/mp4".into(),
                    hash: crate::store::media_sha(&bytes),
                    workspace: source.workspace.clone(),
                };
                let output = self.0.store.media_snapshot(&self.0.product, &derived)?;
                let actual = probe(&ffprobe, &output, "mov", cancel.clone()).await?;
                outputs.push((bytes,"video/mp4".into(),json!({"version":1,"method":"bounded_video_transcode","backend":version,"source_probe":original.value,"output_probe":actual.value,"requested_range_seconds":[0,duration],"audio_policy":"drop_any_audio","audio_removed":true,"cropped":false,"lossy":true,"max_edge":720,"target_fps":5,"frame_rate_policy":"resample to 5fps; frames can be dropped or duplicated","target_bitrate":400000,"truncated":original.duration.is_some_and(|total|total>duration),"metadata_policy":"decoder defaults; not a privacy scrub","model_delivery":false})));
            }
            _ => return Err("Invalid processing action".into()),
        }
        if cancel.is_cancelled() {
            return Err("Media operation cancelled; derived artifacts were not committed".into());
        }
        let artifacts = self
            .0
            .store
            .save_media_artifacts(&self.0.product, &source, outputs)?;
        Ok(
            json!({"artifacts":artifacts,"original_preserved":true,"model_delivery":false,"next":"Read each relevant manifest; explicitly select only relevant derived images. Compressed videos remain files, not model input."}),
        )
    }
    #[allow(clippy::too_many_arguments)]
    async fn jpeg(
        &self,
        source: &MediaSource,
        file: &std::fs::File,
        original: &Probe,
        ffmpeg: &str,
        ffprobe: &str,
        version: &str,
        time: Option<f64>,
        strategy: &str,
        reason: &str,
        cancel: CancellationToken,
    ) -> Result<(Vec<u8>, String, Value), String> {
        let mut args = input_args(input_format(&source.content_type)?);
        args.splice(0..0, ["-v".into(), "info".into()]);
        let select = time
            .map(|t| format!("select='gte(t,{t})',showinfo,"))
            .unwrap_or_default();
        args.extend(["-filter_threads".into(),"1".into(),"-map".into(),"0:v:0".into(),"-an".into(),"-vf".into(),format!("{select}scale=w='min(512,iw)':h='min(512,ih)':force_original_aspect_ratio=decrease"),"-frames:v".into(),"1".into(),"-fps_mode".into(),"passthrough".into(),"-c:v".into(),"mjpeg".into(),"-threads".into(),"1".into(),"-q:v".into(),"4".into(),"-f".into(),"image2pipe".into(),"pipe:1".into()]);
        let (bytes, log) =
            run_decoder(ffmpeg, &args, Some(file), cancel.clone(), 256 * 1024).await?;
        if !bytes.starts_with(b"\xff\xd8\xff") {
            return Err("Decoder did not produce a complete JPEG".into());
        }
        let actual_time = if let Some(requested) = time {
            let actual = log
                .lines()
                .filter(|line| line.contains("showinfo") && line.contains("pts_time:"))
                .find_map(|line| {
                    line.split("pts_time:")
                        .nth(1)?
                        .split_whitespace()
                        .next()?
                        .parse::<f64>()
                        .ok()
                })
                .filter(|t| t.is_finite() && *t >= requested)
                .ok_or("Decoded frame timestamp unavailable; no frame was saved")?;
            Some(actual)
        } else {
            None
        };
        let derived = MediaSource {
            id: source.id.clone(),
            bytes: bytes.clone(),
            content_type: "image/jpeg".into(),
            hash: crate::store::media_sha(&bytes),
            workspace: source.workspace.clone(),
        };
        let snapshot = self.0.store.media_snapshot(&self.0.product, &derived)?;
        let output = probe(ffprobe, &snapshot, "image2pipe", cancel).await?;
        if output.width > 512 || output.height > 512 {
            return Err("Derived image exceeds 512px edge budget".into());
        }
        Ok((
            bytes,
            "image/jpeg".into(),
            json!({"version":1,"method":strategy,"backend":version,"source_probe":original.value,"output_probe":output.value,"requested_timestamp_seconds":time,"actual_timestamp_seconds":actual_time,"timestamp_origin":"decoder-normalized stream timeline; first decoded frame at or after requested time","selection_reason":reason,"lossy":true,"jpeg_quality_parameter":4,"max_edge":512,"cropped":false,"audio_removed_from_image":true,"animation_policy":"Only first decoded image frame retained; source animation detection not claimed","color_policy":"decoder/scaler defaults; color-profile fidelity not guaranteed","metadata_policy":"decoder defaults; not a privacy scrub","semantic_analysis":false,"model_delivery":false}),
        ))
    }
}

#[async_trait]
impl Middleware for MediaDeliveryMiddleware {
    fn name(&self) -> &str {
        "HubSelectedMediaDelivery"
    }
    fn priority(&self) -> i32 {
        MiddlewarePriority::CONTEXT + 1
    }
    async fn on_agent_end(
        &self,
        _state: &AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        self.0
            .store
            .settle_media_deliveries(&self.0.product, &self.0.native)
            .map_err(MiddlewareError::Other)
    }
    async fn wrap_llm_call(
        &self,
        state: &AgentState,
        mut messages: Vec<Message>,
        next: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        let fail = |message: String| MiddlewareError::Other(message);
        if state.session.session_id() != self.0.native {
            return Err(fail("Media model-request session mismatch".into()));
        }
        let mut deliveries = Vec::new();
        let mut selected = 0;
        for message in &mut messages {
            for block in &mut message.content {
                let ContentBlock::ToolResult { id, content, .. } = block else {
                    continue;
                };
                let plan = content.iter().filter_map(|c| c.as_text()).find_map(|s| {
                    serde_json::from_str::<Value>(s)
                        .ok()
                        .and_then(|v| v["delivery_id"].as_str().map(str::to_string))
                });
                let Some(delivery) = plan else { continue };
                let Ok((mut plan, status, call)) =
                    self.0
                        .store
                        .media_receipt(&self.0.product, &self.0.native, &delivery)
                else {
                    continue;
                };
                if status != "prepared" || call != *id {
                    continue;
                }
                plan["actual_model_id"] = json!(state.model.model_id());
                plan["provider_backend"] = json!(state.model.provider_id());
                if !self.0.image_capability {
                    return Err(fail(
                        "Selected media route no longer declares image support".into(),
                    ));
                }
                let ids = plan["artifact_ids"]
                    .as_array()
                    .ok_or_else(|| fail("Media selection corrupt".into()))?;
                selected += ids.len();
                if selected > 2 {
                    return Err(fail(
                        "Only 2 derived images may enter one model request".into(),
                    ));
                }
                let mut provenance = Vec::new();
                let mut images = Vec::new();
                for (index, id) in ids.iter().enumerate() {
                    let artifact: MediaArtifact = self
                        .0
                        .store
                        .media_artifact(
                            &self.0.product,
                            id.as_str()
                                .ok_or_else(|| fail("Media selection corrupt".into()))?,
                        )
                        .map_err(fail)?;
                    if artifact.content_type != "image/jpeg" || artifact.bytes.len() > 256 * 1024 {
                        return Err(fail("Selected media exceeds image delivery budget".into()));
                    }
                    if plan["artifacts"][index]["artifact_id"] != *id
                        || plan["artifacts"][index]["output_sha256"]
                            != artifact.manifest["output_sha256"]
                    {
                        return Err(fail("Selected media changed; model request stopped".into()));
                    }
                    provenance.push(artifact.manifest);
                    images.push(ToolContent::image(
                        base64::engine::general_purpose::STANDARD.encode(&artifact.bytes),
                        "image/jpeg",
                    ));
                }
                let provenance=json!({"delivery_id":delivery,"selection":plan,"processed_images":provenance,"source_notice":"Only these selected derivatives are attached to this request. Original files, remaining frames/audio/video are not attached. Processing is not semantic understanding."}).to_string();
                if provenance.len() > 16 * 1024 {
                    return Err(fail(
                        "Selected media provenance exceeds 16 KiB budget".into(),
                    ));
                }
                content.push(ToolContent::text(provenance));
                content.extend(images);
                deliveries.push(delivery);
            }
        }
        for delivery in &deliveries {
            self.0
                .store
                .submit_media_delivery(
                    &self.0.product,
                    &self.0.native,
                    delivery,
                    state.model.model_id(),
                    state.model.provider_id(),
                )
                .map_err(fail)?;
        }
        let result = next.call(state, messages).await;
        for delivery in deliveries {
            self.0
                .store
                .transition_media_delivery(
                    &self.0.product,
                    &self.0.native,
                    &delivery,
                    "submitted",
                    if result.is_ok() {
                        "response_received"
                    } else {
                        "provider_error_acceptance_unknown"
                    },
                )
                .map_err(fail)?;
        }
        result.map_err(MiddlewareError::from)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::store::PreparedTurn;
    use crate::{EngineTarget, InputAttachment, PermissionProfile};
    use alva_engine_adapter_alva::{
        BaseAgentFactory, CatalogBaseAgentFactory, SessionRepository, COMPONENTS,
    };
    use alva_test::{
        fixtures::{make_assistant_message, tool_use_message},
        mock_provider::MockLanguageModel,
    };
    use std::sync::Arc;

    async fn generated(video: bool) -> Vec<u8> {
        let program =
            decoder("ffmpeg").expect("this native-media integration requires installed ffmpeg");
        let mut args = vec![
            "-v".into(),
            "error".into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "testsrc2=size=640x360:rate=10".into(),
            "-an".into(),
            "-threads".into(),
            "1".into(),
        ];
        if video {
            args.extend([
                "-t".into(),
                "2".into(),
                "-c:v".into(),
                "libx264".into(),
                "-movflags".into(),
                "frag_keyframe+empty_moov".into(),
                "-f".into(),
                "mp4".into(),
                "pipe:1".into(),
            ]);
        } else {
            args.extend([
                "-frames:v".into(),
                "1".into(),
                "-c:v".into(),
                "mjpeg".into(),
                "-f".into(),
                "image2pipe".into(),
                "pipe:1".into(),
            ]);
        }
        run_decoder(&program, &args, None, CancellationToken::new(), 1024 * 1024)
            .await
            .unwrap()
            .0
    }
    fn fixture(bytes: Vec<u8>) -> (tempfile::TempDir, HubMediaTool, PreparedTurn, String) {
        let temp = tempfile::tempdir().unwrap();
        let store = HubStore::open(temp.path().join("data/hub.db")).unwrap();
        let turn = store
            .prepare_new_turn(
                "media",
                temp.path(),
                "look at selected media",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::Interactive,
            )
            .unwrap();
        store
            .save_input_attachments(
                &turn,
                &[InputAttachment {
                    name: "original.bin".into(),
                    text: String::new(),
                    data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                }],
            )
            .unwrap();
        let id = store.input_attachment_refs("media", &turn.turn_id).unwrap()[0]
            .attachment_id
            .clone();
        let tool = HubMediaTool(HubMedia {
            store,
            product: "media".into(),
            native: "native-root".into(),
            image_capability: true,
            route: "synthetic/image-capable".into(),
        });
        (temp, tool, turn, id)
    }
    fn available() -> bool {
        let okay = decoder("ffmpeg").is_ok() && decoder("ffprobe").is_ok();
        if !okay {
            eprintln!("SKIP native decoder exercise: ffmpeg/ffprobe not installed; no installation attempted");
        }
        okay
    }
    #[tokio::test]
    async fn actual_frames_transcode_and_image_compression_have_truthful_provenance() {
        if !available() {
            return;
        }
        let original = generated(true).await;
        let (_temp, tool, _turn, id) = fixture(original.clone());
        let result = tool
            .process(
                Request::ExtractFrames {
                    attachment_id: id.clone(),
                    timestamps: Some(vec![0.55, 1.25]),
                    frame_count: None,
                    reason: "Compare two task-relevant points".into(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result["artifacts"].as_array().unwrap().len(), 2);
        for (index, item) in result["artifacts"].as_array().unwrap().iter().enumerate() {
            let artifact = tool
                .0
                .store
                .media_artifact("media", item["artifact_id"].as_str().unwrap())
                .unwrap();
            let manifest = artifact.manifest;
            let requested = [0.55, 1.25][index];
            let actual = manifest["actual_timestamp_seconds"].as_f64().unwrap();
            assert!(
                actual >= requested && actual < requested + 0.11,
                "requested={requested} actual={actual}"
            );
            assert_eq!(
                manifest["source_sha256"],
                crate::store::media_sha(&original)
            );
            assert_eq!(
                manifest["output_sha256"],
                crate::store::media_sha(&artifact.bytes)
            );
            assert_eq!(manifest["lossy"], true);
            assert_eq!(manifest["original_preserved"], true);
            assert_eq!(manifest["output_probe"]["streams"][0]["width"], 512);
            assert!(manifest["backend"]
                .as_str()
                .unwrap()
                .starts_with("ffmpeg version"));
            assert!(artifact.bytes.len() <= 256 * 1024);
        }
        let uniform = tool
            .process(
                Request::ExtractFrames {
                    attachment_id: id.clone(),
                    timestamps: None,
                    frame_count: Some(2),
                    reason: "Uniform overview".into(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(uniform["artifacts"][0]["method"], "uniform_time");
        let compressed = tool
            .process(
                Request::TranscodeVideo {
                    attachment_id: id.clone(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let artifact = tool
            .0
            .store
            .media_artifact(
                "media",
                compressed["artifacts"][0]["artifact_id"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(artifact.manifest["audio_removed"], true);
        assert_eq!(artifact.manifest["target_fps"], 5);
        assert_eq!(
            artifact.manifest["output_probe"]["streams"][0]["codec_name"],
            "h264"
        );
        assert_eq!(
            tool.0.store.media_source("media", &id).unwrap().bytes,
            original
        );
        assert!(tool
            .0
            .store
            .media_artifact(
                "other",
                compressed["artifacts"][0]["artifact_id"].as_str().unwrap()
            )
            .is_err());
        assert!(tool
            .process(
                Request::ExtractFrames {
                    attachment_id: id,
                    timestamps: Some(vec![99.0]),
                    frame_count: None,
                    reason: "bad".into()
                },
                CancellationToken::new()
            )
            .await
            .is_err());
        let image = generated(false).await;
        let (_temp, tool, _turn, id) = fixture(image.clone());
        let compressed = tool
            .process(
                Request::ProcessImage {
                    attachment_id: id.clone(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let artifact = tool
            .0
            .store
            .media_artifact(
                "media",
                compressed["artifacts"][0]["artifact_id"].as_str().unwrap(),
            )
            .unwrap();
        assert!(artifact.bytes.len() < image.len());
        assert_eq!(artifact.manifest["method"], "first_image_frame");
        assert_eq!(
            tool.0.store.media_source("media", &id).unwrap().bytes,
            image
        );
    }
    struct Context {
        id: String,
        call: String,
        cancel: CancellationToken,
    }
    impl ToolExecutionContext for Context {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            &self.id
        }
        fn tool_call_id(&self) -> Option<&str> {
            Some(&self.call)
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    #[tokio::test]
    async fn cancel_corruption_scope_and_undeclared_models_fail_without_delivery() {
        let (_temp, mut tool, _turn, id) = fixture(b"\xff\xd8\xff broken".to_vec());
        let cancel = CancellationToken::new();
        cancel.cancel();
        let root = Context {
            id: "native-root".into(),
            call: "call".into(),
            cancel,
        };
        assert!(tool
            .execute(json!({"action":"process_image","attachment_id":id}), &root)
            .await
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        let child = Context {
            id: "child".into(),
            call: "call".into(),
            cancel: CancellationToken::new(),
        };
        assert!(tool
            .execute(json!({"action":"probe","attachment_id":id}), &child)
            .await
            .unwrap_err()
            .to_string()
            .contains("binding mismatch"));
        tool.0.image_capability = false;
        let root = Context {
            id: "native-root".into(),
            call: "call".into(),
            cancel: CancellationToken::new(),
        };
        assert!(tool
            .execute(
                json!({"action":"select","artifact_ids":["unknown"],"reason":"task"}),
                &root
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("does not explicitly declare"));
        if available() {
            assert!(tool
                .process(
                    Request::ProcessImage { attachment_id: id },
                    CancellationToken::new()
                )
                .await
                .is_err());
        }
        assert_eq!(
            tool.0.store.media_index("media", "unknown", 0).unwrap()["artifacts"],
            json!([])
        );
        assert!(serde_json::from_value::<Request>(
            json!({"action":"probe","attachment_id":"id","path":"/etc/hosts"})
        )
        .is_err());
    }
    #[tokio::test]
    async fn real_factory_approval_selected_request_receipt_and_no_replay() {
        if !available() {
            return;
        }
        let (temp, tool, _turn, id) = fixture(generated(false).await);
        let result = tool
            .process(
                Request::ProcessImage { attachment_id: id },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let artifact = result["artifacts"][0]["artifact_id"]
            .as_str()
            .unwrap()
            .to_string();
        let model=Arc::new(MockLanguageModel::new()
            .with_response(tool_use_message("selection","hub_media",json!({"action":"select","artifact_ids":[artifact],"reason":"Inspect the task-relevant compressed image"})))
            .with_response(make_assistant_message("seen selected derivative"))
            .with_response(make_assistant_message("no replay")));
        let repository = SessionRepository::open(temp.path().join("native.db")).unwrap();
        let session = repository.open_session("native-root", None).await.unwrap();
        let factory = CatalogBaseAgentFactory::new(model.clone(), temp.path())
            .with_toggles(
                COMPONENTS
                    .iter()
                    .map(|c| (c.id.to_string(), false))
                    .collect(),
            )
            .with_managed_mcp(vec![])
            .with_context_repository(repository)
            .with_session_plugins(Arc::new(
                crate::HubContextPluginFactory::new(tool.0.store.clone(), "media".into())
                    .with_media_route(true, "synthetic/image-capable".into()),
            ));
        let mut built = factory
            .build(
                &alva_engine_runtime::RuntimeRequest::new("inspect selected derivative")
                    .with_cwd(temp.path()),
                session,
            )
            .await
            .unwrap();
        let mut approvals = built.approval_rx.take().unwrap();
        let mut events = built.agent.prompt_text("inspect selected derivative").await;
        let mut approved = 0;
        loop {
            tokio::select! {
                event=events.recv()=>if event.is_none() {break;},
                approval=approvals.recv()=>{let request=approval.unwrap();assert_eq!(request.tool_name,"hub_media");assert!(request.effect_summary.unwrap().contains("only the selected"));approved+=1;assert!(built.agent.resolve_permission(&request.request_id,&request.tool_name,alva_agent_security::PermissionDecision::AllowOnce).await);},
                _=tokio::time::sleep(Duration::from_secs(10))=>panic!("media approval/delivery stalled"),
            }
        }
        assert_eq!(approved, 1);
        assert_eq!(model.calls().len(), 2);
        fn images(messages: &[Message]) -> usize {
            messages
                .iter()
                .flat_map(|m| &m.content)
                .map(|block| {
                    if let ContentBlock::ToolResult { content, .. } = block {
                        content
                            .iter()
                            .filter(|c| matches!(c, ToolContent::Image { .. }))
                            .count()
                    } else {
                        0
                    }
                })
                .sum()
        }
        assert_eq!(images(&model.calls()[0]), 0);
        assert_eq!(images(&model.calls()[1]), 1);
        assert!(serde_json::to_string(&model.calls()[1])
            .unwrap()
            .contains("jpeg_quality_parameter"));
        let call = model.calls()[1]
            .iter()
            .flat_map(|m| &m.content)
            .filter_map(|b| {
                if let ContentBlock::ToolResult { content, .. } = b {
                    content.first()?.as_text()
                } else {
                    None
                }
            })
            .find_map(|s| {
                serde_json::from_str::<Value>(s)
                    .ok()
                    .and_then(|v| v["delivery_id"].as_str().map(str::to_string))
            })
            .unwrap();
        assert_eq!(
            tool.0
                .store
                .media_receipt("media", "native-root", &call)
                .unwrap()
                .1,
            "response_received"
        );
        assert!(tool.0.store.media_receipt("media", "child", &call).is_err());
        let receipt = tool
            .0
            .store
            .media_receipt("media", "native-root", &call)
            .unwrap()
            .0;
        assert_eq!(receipt["actual_model_id"], "mock-language-model");
        assert_eq!(receipt["provider_backend"], "unknown");
        assert!(serde_json::to_string(&model.calls()[1])
            .unwrap()
            .contains("mock-language-model"));
        let mut events = built.agent.prompt_text("another request").await;
        while events.recv().await.is_some() {}
        assert_eq!(images(&model.calls()[2]), 0);
        model.as_ref().clone().with_response(tool_use_message("failed-selection","hub_media",json!({"action":"select","artifact_ids":[artifact],"reason":"Explicit retry with the same relevant image"})))
            .with_error(AgentError::LlmError("synthetic provider rejection".into()));
        let mut events = built.agent.prompt_text("explicit new selection").await;
        loop {
            tokio::select! {
                event=events.recv()=>if event.is_none() {break;},
                approval=approvals.recv()=>{let request=approval.unwrap();assert!(built.agent.resolve_permission(&request.request_id,&request.tool_name,alva_agent_security::PermissionDecision::AllowOnce).await);},
                _=tokio::time::sleep(Duration::from_secs(10))=>panic!("provider error receipt stalled"),
            }
        }
        assert_eq!(images(&model.calls()[4]), 1);
        let failed = model.calls()[4]
            .iter()
            .flat_map(|m| &m.content)
            .find_map(|b| {
                if let ContentBlock::ToolResult { id, content, .. } = b {
                    if id == "call-failed-selection" {
                        let text = content.first()?.as_text()?;
                        return serde_json::from_str::<Value>(text)
                            .ok()
                            .and_then(|v| v["delivery_id"].as_str().map(str::to_string));
                    }
                    None
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            tool.0
                .store
                .media_receipt("media", "native-root", &failed)
                .unwrap()
                .1,
            "provider_error_acceptance_unknown"
        );
        drop(built);
        drop(factory);
        let database = tool.0.store.path().to_path_buf();
        drop(tool);
        let store = HubStore::open(database).unwrap();
        assert_eq!(
            store.media_artifact("media", &artifact).unwrap().manifest["artifact_id"],
            artifact
        );
        assert_eq!(
            store
                .media_receipt("media", "native-root", &call)
                .unwrap()
                .1,
            "response_received"
        );
    }
    #[tokio::test]
    async fn owned_decoder_is_cancelled_and_bounded_output_is_not_success() {
        if !available() {
            return;
        }
        let program = decoder("ffmpeg").unwrap();
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let canceller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            signal.cancel();
        });
        let args = [
            "-v",
            "error",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "color=size=64x64:rate=10",
            "-threads",
            "1",
            "-f",
            "null",
            "-",
        ]
        .map(str::to_string);
        let start = std::time::Instant::now();
        assert!(run_decoder(&program, &args, None, cancel, 1024)
            .await
            .unwrap_err()
            .contains("cancelled"));
        canceller.await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(run_decoder(
            &program,
            &["-version".into()],
            None,
            CancellationToken::new(),
            1
        )
        .await
        .unwrap_err()
        .contains("budget"));
        assert!(decoder("alva_hub_missing_decoder_fixture")
            .unwrap_err()
            .contains("no program was installed"));
    }
}
