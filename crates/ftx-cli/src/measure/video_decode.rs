//! Explicit local FFmpeg boundary. Decoding supplies pixels and timestamps, never masks or fit evidence.

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::video::SelectionOptions;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options, required, workspace};

const MAX_MEDIA_BYTES: usize = 256 * 1024 * 1024;
const MAX_PROCESS_BYTES: usize = 8 * 1024 * 1024;
const MAX_FRAME_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_TOTAL_PIXELS: u64 = 1_000_000_000;

// Original source ticks and equivalent microseconds, in decode order.
type FrameTimestamps = Vec<(i64, u64)>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    video: String,
    stream: String,
    camera_epoch: String,
    rotation_cw_deg: u16,
    #[serde(default)]
    video_stream_index: u16,
    #[serde(default)]
    focal_px: Option<[f64; 2]>,
    #[serde(default = "default_max_frames")]
    max_frames: usize,
    #[serde(default)]
    options: SelectionOptions,
}

fn default_max_frames() -> usize {
    600
}

fn budget(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::BudgetExceeded, message)
}

fn name(path: &Path) -> Result<&str, Refusal> {
    path.to_str()
        .ok_or_else(|| bad_input("video paths must be UTF-8"))
}

fn private_dir(path: &Path) -> Result<(), Refusal> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| workspace::io_error(path, e))
}

fn frame_usage(path: &Path) -> Result<(usize, u64), Refusal> {
    let mut count = 0;
    let mut bytes = 0u64;
    for entry in fs::read_dir(path).map_err(|e| workspace::io_error(path, e))? {
        let entry = entry.map_err(|e| workspace::io_error(path, e))?;
        let metadata = entry
            .metadata()
            .map_err(|e| workspace::io_error(&entry.path(), e))?;
        if !metadata.is_file() || entry.file_type().is_ok_and(|t| t.is_symlink()) {
            return Err(bad_input("decoded frames must be regular files"));
        }
        count += 1;
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| budget("decoded byte overflow"))?;
    }
    Ok((count, bytes))
}

fn external(
    program: &str,
    args: &[String],
    out: &Path,
    expected_frames: Option<usize>,
) -> Result<Vec<u8>, Refusal> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            workspace::io_error(Path::new(program), e).repair(
                "install FFmpeg and ffprobe locally, then retry with a new output directory",
            )
        })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    std::thread::scope(|scope| {
        let read = |reader: &mut dyn Read| {
            let mut bytes = Vec::new();
            reader
                .take(MAX_PROCESS_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        };
        let stdout = scope.spawn(move || read(&mut { stdout }));
        let stderr = scope.spawn(move || read(&mut { stderr }));
        let start = Instant::now();
        let status = loop {
            let within_budget = (|| {
                if start.elapsed() > Duration::from_secs(120) {
                    return Err(budget(format!("{program} exceeded 120 seconds")));
                }
                if let Some(expected) = expected_frames {
                    let (count, bytes) = frame_usage(&out.join("frames"))?;
                    if count > expected || bytes > MAX_FRAME_BYTES {
                        return Err(budget("decoded frame count or 1 GiB output limit exceeded"));
                    }
                }
                Ok(())
            })();
            if let Err(error) = within_budget {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(workspace::io_error(Path::new(program), error));
                }
            }
        };
        let stdout = stdout
            .join()
            .map_err(|_| bad_input("decoder stdout worker failed"))?
            .map_err(|e| workspace::io_error(Path::new(program), e))?;
        let stderr = stderr
            .join()
            .map_err(|_| bad_input("decoder stderr worker failed"))?
            .map_err(|e| workspace::io_error(Path::new(program), e))?;
        if stdout.len() > MAX_PROCESS_BYTES || stderr.len() > MAX_PROCESS_BYTES {
            return Err(budget(format!("{program} output exceeds 8 MiB")));
        }
        workspace::write_new(&out.join(format!("{program}.stdout")), &stdout)?;
        workspace::write_new(&out.join(format!("{program}.stderr")), &stderr)?;
        let status = status?;
        if !status.success() || (program == "ffprobe" && !stderr.is_empty()) {
            return Err(bad_input(format!(
                "{program} failed ({status}); inspect {}",
                out.join(format!("{program}.stderr")).display()
            )));
        }
        Ok(stdout)
    })
}

fn timestamps(probe: &Value, request: &Request) -> Result<(FrameTimestamps, [usize; 2]), Refusal> {
    let streams = probe["streams"]
        .as_array()
        .ok_or_else(|| bad_input("probe has no video stream"))?;
    if streams.len() != 1 {
        return Err(bad_input("choose exactly one existing video stream"));
    }
    let stream = &streams[0];
    let dimensions = |value: &Value| -> Result<[usize; 2], Refusal> {
        let width = value["width"]
            .as_u64()
            .ok_or_else(|| bad_input("missing frame width"))?;
        let height = value["height"]
            .as_u64()
            .ok_or_else(|| bad_input("missing frame height"))?;
        if width < 3 || height < 3 || width.checked_mul(height).is_none_or(|n| n > 40_000_000) {
            return Err(budget(
                "decoded frames need at least 3 x 3 and at most 40 million pixels",
            ));
        }
        Ok([width as usize, height as usize])
    };
    let size = dimensions(stream)?;
    let (numerator, denominator) = stream["time_base"]
        .as_str()
        .and_then(|s| s.split_once('/'))
        .and_then(|(n, d)| Some((n.parse::<u64>().ok()?, d.parse::<u64>().ok()?)))
        .filter(|&(n, d)| n > 0 && d > 0)
        .ok_or_else(|| bad_input("video needs a positive rational time base"))?;
    let frames = probe["frames"]
        .as_array()
        .ok_or_else(|| bad_input("probe has no decoded frames"))?;
    if frames.is_empty() {
        return Err(bad_input("video has no decoded frames"));
    }
    if frames.len() > request.max_frames
        || frames.len() as u64 > MAX_TOTAL_PIXELS / (size[0] as u64 * size[1] as u64)
    {
        return Err(budget(
            "video exceeds the declared frame count or one billion decoded pixels; trim the source explicitly",
        ));
    }
    let mut result = Vec::with_capacity(frames.len());
    for frame in frames {
        if dimensions(frame)? != size {
            return Err(bad_input(
                "resolution changes require separately declared captures and calibration",
            ));
        }
        let pts = frame["pts"].as_i64().filter(|&n| n >= 0)
            .ok_or_else(|| bad_input("every decoded frame needs an original nonnegative integer PTS; frame rate is not a substitute"))?;
        // FFmpeg rescales nonnegative PTS with AV_ROUND_NEAR_INF. Keep the exact source ticks too.
        let us = i128::from(pts)
            .checked_mul(i128::from(numerator))
            .and_then(|n| n.checked_mul(1_000_000))
            .and_then(|n| n.checked_add(i128::from(denominator / 2)))
            .map(|n| n / i128::from(denominator))
            .and_then(|n| u64::try_from(n).ok())
            .filter(|&n| n <= i64::MAX as u64)
            .ok_or_else(|| {
                bad_input("presentation timestamp cannot be represented in microseconds")
            })?;
        if result
            .last()
            .is_some_and(|&(last_pts, last_us)| last_pts >= pts || last_us >= us)
        {
            return Err(bad_input(
                "decoded timestamps must increase strictly before and after microsecond rescaling",
            ));
        }
        result.push((pts, us));
    }
    Ok((result, size))
}

fn verify_decoded_times(log: &[u8], times: &[(i64, u64)]) -> Result<(), Refusal> {
    let text = std::str::from_utf8(log).map_err(|_| bad_input("decoder log is not UTF-8"))?;
    let mut observed = Vec::new();
    for line in text
        .lines()
        .filter(|s| s.contains("Parsed_showinfo_") && s.contains(" n:"))
    {
        let field = |key: &str| -> Result<u64, Refusal> {
            line.split_once(key)
                .and_then(|(_, s)| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad_input("decoder did not retain an integer frame identity"))
        };
        let index = field(" n:")?;
        let us = field(" pts:")?;
        if index != observed.len() as u64 {
            return Err(bad_input(
                "decoder frame sequence is incomplete or duplicated",
            ));
        }
        observed.push(us);
    }
    if observed.iter().copied().ne(times.iter().map(|&(_, us)| us)) {
        return Err(bad_input(
            "decoded presentation timestamps disagree with the independently probed source",
        ));
    }
    Ok(())
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure video-decode", &["out"])?;
    let request_path = fs::canonicalize(super::single(args, "video-decode")?)
        .map_err(|e| bad_input(format!("video decode request: {e}")))?;
    let bytes = super::video::read_limited(&request_path, 1024 * 1024)?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|e| bad_input(e.to_string()))?;
    if request.schema != "ftx-video-decode/1"
        || [&request.video, &request.stream, &request.camera_epoch]
            .iter()
            .any(|s| s.trim().is_empty())
        || request.video.len() > 4096
        || request.stream.len() > 256
        || request.camera_epoch.len() > 256
        || !matches!(request.rotation_cw_deg, 0 | 90 | 180 | 270)
        || !(1..=20_000).contains(&request.max_frames)
        || request
            .focal_px
            .is_some_and(|v| v.iter().any(|n| !n.is_finite() || *n <= 0.0))
    {
        return Err(bad_input(
            "video decode needs schema ftx-video-decode/1, video/stream/epoch, explicit quarter-turn rotation, 1..20000 max_frames and positive optional calibrated focal lengths",
        ));
    }
    request.options.validate()?;
    let out = Path::new(required(args, "out")?);
    if fs::symlink_metadata(out).is_ok() {
        return Err(bad_input(
            "video decode output must be a new directory, not an existing path or link",
        ));
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if out.file_name().is_none() || !parent.is_dir() {
        return Err(bad_input(
            "video decode output needs an existing parent directory",
        ));
    }
    let source = fs::canonicalize(request_path.parent().unwrap().join(&request.video))
        .map_err(|e| bad_input(format!("local video: {e}")))?;
    let source_bytes = super::video::read_limited(&source, MAX_MEDIA_BYTES)?;
    let source_hash = sha256_hex(&source_bytes);
    private_dir(out)?;
    let out = fs::canonicalize(out).map_err(|e| workspace::io_error(out, e))?;
    private_dir(&out.join("frames"))?;
    private_dir(&out.join("masks"))?;
    workspace::write_new(&out.join("request.json"), &bytes)?;
    // Both external passes read the same private snapshot, even if the original clip is edited.
    workspace::write_new(&out.join("source.media"), &source_bytes)?;
    drop(source_bytes);
    let source_name = name(&out)?.to_string() + "/source.media";
    let mut probe_args: Vec<String> = [
        "-v",
        "error",
        "-err_detect",
        "explode",
        "-threads",
        "1",
        "-max_alloc",
        "268435456",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mov,matroska,webm,avi",
        "-select_streams",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    probe_args.push(format!("v:{}", request.video_stream_index));
    probe_args.extend(["-show_frames", "-show_streams", "-show_program_version", "-show_entries",
        "stream=index,codec_type,width,height,time_base:stream_tags=rotate:stream_side_data=rotation,displaymatrix:frame=pts,best_effort_timestamp,width,height",
        "-of", "json"].map(str::to_string));
    probe_args.push(source_name.clone());
    let probe_bytes = external("ffprobe", &probe_args, &out, None)?;
    let probe: Value = serde_json::from_slice(&probe_bytes)
        .map_err(|e| bad_input(format!("ffprobe JSON: {e}")))?;
    let (times, size) = timestamps(&probe, &request)?;
    let turn = match request.rotation_cw_deg {
        0 => "",
        90 => "transpose=clock,",
        180 => "hflip,vflip,",
        270 => "transpose=cclock,",
        _ => unreachable!(),
    };
    let oriented = if request.rotation_cw_deg.is_multiple_of(180) {
        size
    } else {
        [size[1], size[0]]
    };
    let mut decode_args: Vec<String> = [
        "-nostdin",
        "-nostats",
        "-xerror",
        "-err_detect",
        "explode",
        "-n",
        "-noautorotate",
        "-copyts",
        "-max_alloc",
        "268435456",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mov,matroska,webm,avi",
        "-threads",
        "1",
        "-i",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    decode_args.push(source_name);
    decode_args.extend([
        "-map".into(),
        format!("0:v:{}", request.video_stream_index),
        "-an".into(),
        "-sn".into(),
        "-dn".into(),
        "-vf".into(),
        format!("{turn}settb=1/1000000,showinfo"),
        "-fps_mode".into(),
        "passthrough".into(),
        "-enc_time_base".into(),
        "1:1000000".into(),
        "-avoid_negative_ts".into(),
        "disabled".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
        "-threads".into(),
        "1".into(),
        "-frames:v".into(),
        (times.len() + 1).to_string(),
        "-f".into(),
        "image2".into(),
        "-start_number".into(),
        "0".into(),
        "frames/frame-%08d.png".into(),
    ]);
    external("ffmpeg", &decode_args, &out, Some(times.len()))?;
    verify_decoded_times(
        &super::video::read_limited(&out.join("ffmpeg.stderr"), MAX_PROCESS_BYTES)?,
        &times,
    )?;
    // image2's frame_pts filenames wrap at signed 32-bit on supported FFmpeg versions. Ordinals
    // are transport names only; independently checked PTS, never frame rate, supply the identity.
    let expected: BTreeSet<_> = (0..times.len())
        .map(|i| format!("frame-{i:08}.png"))
        .collect();
    let actual: BTreeSet<_> = fs::read_dir(out.join("frames"))
        .map_err(|e| workspace::io_error(&out, e))?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()
        .map_err(|e| workspace::io_error(&out, e))?;
    if actual != expected || frame_usage(&out.join("frames"))?.1 > MAX_FRAME_BYTES {
        return Err(bad_input(
            "decoder did not produce the complete expected frame set",
        ));
    }
    let mut frames = Vec::with_capacity(times.len());
    let mut evidence = Vec::with_capacity(times.len());
    for (index, (pts, us)) in times.into_iter().enumerate() {
        let image = format!("frames/frame-{index:08}.png");
        let (pixels, image_sha256) = super::video::load_hashed(&out, &image, false)?;
        if [pixels.w, pixels.h] != oriented {
            return Err(bad_input(
                "decoded image dimensions disagree with the declared rotation",
            ));
        }
        let path = out.join(&image);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|e| workspace::io_error(&path, e))?;
        }
        evidence.push(json!({"source_pts":pts,"timestamp_us":us,"image":image,
            "image_sha256":image_sha256,"decoded_gray_sha256":sha256_hex(&pixels.data),"image_size_px":oriented}));
        frames.push(json!({"metadata":{"frame":{"stream":request.stream,"timestamp_us":us},
            "camera_epoch":request.camera_epoch,"rotation_cw_deg":request.rotation_cw_deg,"focal_px":request.focal_px},
            "image":image,"person_mask":format!("masks/person-{us}.png"),"sheet_mask":format!("masks/sheet-{us}.png")}));
    }
    let manifest = json!({"schema":"ftx-video-frames/1","options":request.options,"frames":frames,
        "decode_receipt":format!("ftx measure video-decode; source SHA-256 {source_hash}; original PTS rescaled to microseconds without frame-rate inference; explicit {} degree clockwise turn. See decode.json, ffprobe.stdout and ffmpeg.stderr. Person/sheet masks require independent review.", request.rotation_cw_deg)});
    let manifest_bytes = workspace::json_bytes(&manifest)?;
    let report = json!({"schema":"ftx-video-decode-report/1","status":"DECODED_REQUIRES_MASKS",
        "source":{"original_path":source,"snapshot":"source.media","sha256":source_hash},
        "request_sha256":sha256_hex(&bytes),"source_time_base":probe["streams"][0]["time_base"],
        "probe":probe,"working_directory":out,"commands":{"ffprobe":probe_args,"ffmpeg":decode_args},"frames":evidence,
        "manifest":"frames.json","manifest_sha256":sha256_hex(&manifest_bytes),
        "no_claim":"Decoding does not infer masks, lens changes, stillness, anatomy, calibration accuracy or fit. Review the explicit orientation and supply matching binary person/sheet masks before video-select."});
    workspace::write_new(&out.join("frames.json"), &manifest_bytes)?;
    workspace::write_json(&out.join("decode.json"), &report)?;
    let text = format!(
        "Decoded {} frames into {}. Supply reviewed person/sheet masks named in frames.json, then run ftx measure video-select. No frames were selected or body measurements inferred.",
        evidence.len(),
        out.display()
    );
    Ok((report, 0, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        serde_json::from_value(json!({"schema":"ftx-video-decode/1","video":"clip.mov",
            "stream":"synthetic","camera_epoch":"one-lens","rotation_cw_deg":90}))
        .unwrap()
    }

    fn probe(pts: &[i64], time_base: &str) -> Value {
        json!({"streams":[{"width":64,"height":48,"time_base":time_base}],
            "frames":pts.iter().map(|pts| json!({"pts":pts,"width":64,"height":48})).collect::<Vec<_>>()})
    }

    #[test]
    fn timestamp_conversion_preserves_epochs_and_ffmpeg_half_rounding() {
        let request = request();
        assert_eq!(
            timestamps(&probe(&[0, 1, 2], "1/128"), &request).unwrap().0,
            vec![(0, 0), (1, 7813), (2, 15625)]
        );
        assert_eq!(
            timestamps(
                &probe(&[3_600_000, 3_600_017, 3_600_070], "1/1000"),
                &request
            )
            .unwrap()
            .0,
            vec![
                (3_600_000, 3_600_000_000),
                (3_600_017, 3_600_017_000),
                (3_600_070, 3_600_070_000)
            ]
        );
        let log = b"[Parsed_showinfo_1] n:   0 pts:3600000000 pts_time:3600\n[Parsed_showinfo_1] n:   1 pts:3600017000 pts_time:3600.017\n[Parsed_showinfo_1] color_range:pc\n";
        verify_decoded_times(
            log,
            &[(3_600_000, 3_600_000_000), (3_600_017, 3_600_017_000)],
        )
        .unwrap();
        assert!(verify_decoded_times(log, &[(0, 0), (1, 17_000)]).is_err());
        assert!(verify_decoded_times(b"", &[(0, 0)]).is_err());
        assert!(
            verify_decoded_times(b"[Parsed_showinfo_1] n: 1 pts:0 pts_time:0", &[(0, 0)]).is_err()
        );
    }

    #[test]
    fn incomplete_ambiguous_or_over_budget_decodes_refuse() {
        let mut request = request();
        for (pts, base) in [
            (vec![], "1/1000"),
            (vec![-1, 0], "1/1000"),
            (vec![0, 0], "1/1000"),
            (vec![1, 0], "1/1000"),
            (vec![0, 1], "1/10000000"),
            (vec![0], "0/1"),
            (vec![i64::MAX], "18446744073709551615/1"),
        ] {
            assert!(
                timestamps(&probe(&pts, base), &request).is_err(),
                "{pts:?} {base}"
            );
        }
        let mut missing = probe(&[0], "1/1000");
        missing["frames"][0].as_object_mut().unwrap().remove("pts");
        missing["frames"][0]["best_effort_timestamp"] = json!(0);
        assert!(timestamps(&missing, &request).is_err());
        let mut changed = probe(&[0, 1], "1/1000");
        changed["frames"][1]["width"] = json!(32);
        assert!(timestamps(&changed, &request).is_err());
        request.max_frames = 1;
        assert_eq!(
            timestamps(&probe(&[0, 1], "1/1000"), &request)
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }
}
