use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Condvar};
use std::collections::VecDeque;
use std::thread;
use crate::media::types::get_nearest_keyframe;

#[derive(Debug, Clone)]
pub enum ThumbnailCommand {
    /// visible_frames: currently on-screen slots (decoded first).
    /// padding_frames: ±2 preload zone (decoded after visible GOPs).
    Request {
        visible_frames: Vec<usize>,
        padding_frames: Vec<usize>,
        height: u32,
    },
    Clear,
}

#[derive(Debug)]
pub struct ThumbnailResponse {
    /// Every target_frame satisfied by this single GOP decode.
    pub served_frames: Vec<usize>,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}



/// One GOP's worth of decode work dispatched to a worker thread.
#[derive(Debug)]
struct GopWorkItem {
    keyframe_pts: i64,
    target_frames: Vec<usize>, // sorted ascending
    generation: u64,
    height: u32,
}

/// Groups `frames` into one `GopWorkItem` per GOP, ordered chronologically.
/// Multiple frames that share the same nearest keyframe produce a single item,
/// eliminating redundant seeks.
fn group_frames_by_gop(
    frames: &[usize],
    keyframes: &[usize],
    keyframes_pts: &[i64],
    generation: u64,
    height: u32,
) -> Vec<GopWorkItem> {
    use std::collections::BTreeMap;
    // BTreeMap keyed by keyframe frame-index keeps GOPs in chronological order.
    let mut gops: BTreeMap<usize, (i64, Vec<usize>)> = BTreeMap::new();
    for &target in frames {
        let kf = get_nearest_keyframe(keyframes, target);
        let kf_idx = keyframes.binary_search(&kf).unwrap_or(0);
        let pts = if kf_idx < keyframes_pts.len() { keyframes_pts[kf_idx] } else { 0 };
        gops.entry(kf).or_insert_with(|| (pts, Vec::new())).1.push(target);
    }
    gops.into_values()
        .map(|(keyframe_pts, mut target_frames)| {
            target_frames.sort_unstable();
            GopWorkItem { keyframe_pts, target_frames, generation, height }
        })
        .collect()
}

/// Spawns a coordinator thread plus N independent worker threads.
/// Returns the command sender, response receiver, and all thread handles.
pub fn spawn_thumbnail_decoder_pool(
    path: String,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
    fps: f64,
    thread_count: usize,
    hw_accel: bool,
) -> (Sender<ThumbnailCommand>, Receiver<ThumbnailResponse>, Vec<thread::JoinHandle<()>>) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<ThumbnailCommand>();
    let (resp_tx, resp_rx) = crossbeam_channel::bounded::<ThumbnailResponse>(200);

    // Shared generation counter — incrementing it signals workers to abandon current work.
    let generation: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let shutdown: Arc<std::sync::atomic::AtomicBool> = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Shared FIFO work queue. Coordinator pushes visible GOPs first, padding GOPs after.
    // Workers pull in FIFO order, so visible work is always consumed first.
    let work_queue: Arc<(Mutex<VecDeque<GopWorkItem>>, Condvar)> =
        Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));

    let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(thread_count + 1);

    // Spawn N worker threads first (each owns its own FFmpeg context).
    for _ in 0..thread_count {
        let work_queue = Arc::clone(&work_queue);
        let generation = Arc::clone(&generation);
        let shutdown = Arc::clone(&shutdown);
        let resp_tx = resp_tx.clone();
        let path = path.clone();
        handles.push(thread::spawn(move || {
            worker_thread(path, fps, work_queue, generation, shutdown, resp_tx, hw_accel);
        }));
    }

    // Spawn coordinator last (scheduling logic only — no decoding).
    {
        let work_queue = Arc::clone(&work_queue);
        let generation = Arc::clone(&generation);
        let shutdown = Arc::clone(&shutdown);
        handles.push(thread::spawn(move || {
            coordinator_thread(cmd_rx, work_queue, generation, shutdown, keyframes, keyframes_pts);
        }));
    }

    (cmd_tx, resp_rx, handles)
}

fn coordinator_thread(
    cmd_rx: Receiver<ThumbnailCommand>,
    work_queue: Arc<(Mutex<VecDeque<GopWorkItem>>, Condvar)>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
) {
    loop {
        let cmd = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => break, // sender dropped — shut down
        };
        match cmd {
            ThumbnailCommand::Clear => {
                // Preempt workers and drain the queue.
                generation.fetch_add(1, Ordering::SeqCst);
                {
                    let mut queue = work_queue.0.lock().unwrap();
                    queue.clear();
                }
                work_queue.1.notify_all();
                while cmd_rx.try_recv().is_ok() {} // drain further clears
            }
            ThumbnailCommand::Request { visible_frames, padding_frames, height } => {
                // Advance generation — workers observing the old value will abort.
                let new_gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                // Drain the stale work queue before pushing new work.
                {
                    let mut queue = work_queue.0.lock().unwrap();
                    queue.clear();
                    // Visible GOPs pushed first → consumed first by workers (FIFO priority).
                    for item in group_frames_by_gop(&visible_frames, &keyframes, &keyframes_pts, new_gen, height) {
                        queue.push_back(item);
                    }
                    // Padding GOPs pushed after → consumed once visible GOPs are done.
                    for item in group_frames_by_gop(&padding_frames, &keyframes, &keyframes_pts, new_gen, height) {
                        queue.push_back(item);
                    }
                }
                work_queue.1.notify_all();
            }
        }
    }
    // Signal workers to exit once the coordinator's command channel closes.
    shutdown.store(true, Ordering::SeqCst);
    work_queue.1.notify_all();
}

fn worker_thread(
    path: String,
    fps: f64,
    work_queue: Arc<(Mutex<VecDeque<GopWorkItem>>, Condvar)>,
    generation: Arc<AtomicU64>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    resp_tx: Sender<ThumbnailResponse>,
    hw_accel: bool,
) {
    // Each worker owns its own independent FFmpeg input context and decoder.
    let mut input = match ffmpeg_next::format::input(&path) {
        Ok(i) => i,
        Err(_) => return,
    };
    let stream = match input.streams().best(ffmpeg_next::media::Type::Video) {
        Some(s) => s,
        None => return,
    };
    let video_stream_index = stream.index();
    let time_base = stream.time_base();
    let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
    let start_time = if stream.start_time() > -1_000_000_000_000 {
        stream.start_time() as f64 * time_base_f64
    } else {
        0.0
    };

    // Peek aspect ratio before the first codec_ctx is consumed by decoder().video().
    let aspect_ratio = {
        let ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
            Ok(c) => c,
            Err(_) => return,
        };
        match ctx.decoder().video() {
            Ok(d) if d.width() > 0 && d.height() > 0 => d.width() as f32 / d.height() as f32,
            _ => 16.0 / 9.0,
        }
    };

    // Build the real codec context with HW acceleration.
    let mut codec_ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
        Ok(c) => c,
        Err(_) => return,
    };
    if hw_accel {
        let _ = crate::media::hw_context::probe_and_attach_hw_context(&mut codec_ctx);
    }
    let mut decoder = match codec_ctx.decoder().video() {
        Ok(d) => d,
        Err(_) => return,
    };
    let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

    loop {
        // Wait for a work item using Mutex + Condvar.
        let item = {
            let mut queue = work_queue.0.lock().unwrap();
            loop {
                if shutdown.load(Ordering::SeqCst) { return; }
                if let Some(i) = queue.pop_front() {
                    break i;
                }
                queue = work_queue.1.wait(queue).unwrap();
            }
        };

        // Stale generation — a newer batch arrived; skip without decoding.
        if generation.load(Ordering::SeqCst) != item.generation { continue; }

        let pts_us = (item.keyframe_pts as f64 * time_base_f64 * 1_000_000.0).round() as i64;
        if input.seek(pts_us, ..pts_us).is_err() { continue; }
        decoder.flush();

        let height = item.height;
        let thumb_width = (height as f32 * aspect_ratio).round() as u32;
        let mut remaining = item.target_frames.clone(); // already sorted ascending
        let mut served_frames: Vec<usize> = Vec::new();
        let mut collected_rgba: Option<(Vec<u8>, u32, u32)> = None;
        let mut fallback_frame_idx = 0usize;

        'packets: for (stream_ref, packet) in input.packets() {
            // Preemption check — abort without emitting a partial response.
            // The next batch will re-request any uncovered frames.
            if generation.load(Ordering::SeqCst) != item.generation { break 'packets; }
            if stream_ref.index() != video_stream_index { continue; }
            if decoder.send_packet(&packet).is_err() { continue; }

            let mut raw_frame = ffmpeg_next::util::frame::Video::empty();
            while decoder.receive_frame(&mut raw_frame).is_ok() {
                let pts_sec = if let Some(ts) = raw_frame.timestamp() {
                    ts as f64 * time_base_f64 - start_time
                } else if let Some(pts) = raw_frame.pts() {
                    pts as f64 * time_base_f64 - start_time
                } else {
                    let pkt_dts = unsafe { (*raw_frame.as_ptr()).pkt_dts };
                    if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                        pkt_dts as f64 * time_base_f64 - start_time
                    } else {
                        fallback_frame_idx as f64 / fps
                    }
                };
                let frame_idx = (pts_sec * fps).round() as usize;
                fallback_frame_idx = frame_idx + 1;

                // Drain all targets we have now reached or passed.
                let mut i = 0;
                while i < remaining.len() {
                    if remaining[i] <= frame_idx {
                        let target = remaining.remove(i);

                        // Scale once per GOP group — all targets share the same RGBA pixels.
                        if collected_rgba.is_none() {
                            let actual = if crate::media::hw_context::is_hw_pixel_format(raw_frame.format()) {
                                let mut sw = ffmpeg_next::util::frame::Video::empty();
                                let ok = unsafe {
                                    ffmpeg_next::sys::av_hwframe_transfer_data(
                                        sw.as_mut_ptr(), raw_frame.as_ptr(), 0,
                                    )
                                };
                                if ok < 0 { raw_frame.clone() } else { sw }
                            } else {
                                raw_frame.clone()
                            };

                            let sc = match scaler.as_mut() {
                                Some(s) if s.input().format == actual.format()
                                    && s.input().width == actual.width()
                                    && s.input().height == actual.height()
                                    && s.output().height == height => s,
                                _ => {
                                    match ffmpeg_next::software::scaling::context::Context::get(
                                        actual.format(), actual.width(), actual.height(),
                                        ffmpeg_next::format::Pixel::RGBA,
                                        thumb_width, height,
                                        ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                                    ) {
                                        Ok(s) => { scaler = Some(s); scaler.as_mut().unwrap() }
                                        Err(_) => break,
                                    }
                                }
                            };

                            let mut scaled = ffmpeg_next::util::frame::Video::empty();
                            if sc.run(&actual, &mut scaled).is_ok() {
                                let w = scaled.width() as usize;
                                let h = scaled.height() as usize;
                                let stride = scaled.stride(0) as usize;
                                let data = scaled.data(0);
                                let mut rgba = Vec::with_capacity(w * h * 4);
                                for y in 0..h {
                                    let s = y * stride;
                                    if let Some(row) = data.get(s..s + w * 4) {
                                        rgba.extend_from_slice(row);
                                    }
                                }
                                collected_rgba = Some((rgba, w as u32, h as u32));
                            }
                        }

                        served_frames.push(target);
                    } else {
                        i += 1;
                    }
                }

                if remaining.is_empty() { break 'packets; }
            }
        }

        // Emit only if not preempted and we have pixel data.
        if generation.load(Ordering::SeqCst) == item.generation {
            if let Some((rgba_data, w, h)) = collected_rgba {
                if !served_frames.is_empty() {
                    let _ = resp_tx.send(ThumbnailResponse {
                        served_frames,
                        width: w,
                        height: h,
                        rgba_data,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_group_frames_by_gop_deduplicates_seeks() {
        let keyframes = vec![0usize, 60, 120];
        let keyframes_pts = vec![0i64, 60_000, 120_000];

        // Five frames all in GOP-0 → one work item.
        let items = group_frames_by_gop(&[0, 10, 20, 30, 40], &keyframes, &keyframes_pts, 1, 64);
        assert_eq!(items.len(), 1, "all frames in same GOP → one work item");
        assert_eq!(items[0].keyframe_pts, 0);
        assert_eq!(items[0].target_frames.len(), 5);

        // Frames spanning two GOPs → two work items.
        let items2 = group_frames_by_gop(&[0, 60], &keyframes, &keyframes_pts, 1, 64);
        assert_eq!(items2.len(), 2, "frames in two GOPs → two work items");
        // Items are chronologically ordered (BTreeMap guarantee).
        assert!(items2[0].keyframe_pts <= items2[1].keyframe_pts);
    }

    #[test]
    fn test_thumbnail_decoder_lifecycle() {
        let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
        if !std::path::Path::new(&test_file).exists() {
            eprintln!("Skipping thumbnail test: test-files/bbb_sunflower_2160p_60fps_normal.mp4 not found");
            return;
        }
        let keyframes = vec![0, 60, 120];
        let keyframes_pts = vec![0, 60 * 1000, 120 * 1000];

        let (cmd_tx, resp_rx, _handles) = spawn_thumbnail_decoder_pool(
            test_file, keyframes, keyframes_pts, 60.0, 1, false,
        );

        cmd_tx.send(ThumbnailCommand::Request {
            visible_frames: vec![0, 60],
            padding_frames: vec![],
            height: 31,
        }).unwrap();

        let mut got_frames = std::collections::HashSet::new();
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(resp) = resp_rx.recv_timeout(Duration::from_millis(500)) {
                assert_eq!(resp.height, 31);
                assert_eq!(resp.rgba_data.len(), (resp.width * resp.height * 4) as usize);
                for f in resp.served_frames { got_frames.insert(f); }
            }
            if got_frames.contains(&0) && got_frames.contains(&60) { break; }
        }
        assert!(got_frames.contains(&0), "frame 0 not received");
        assert!(got_frames.contains(&60), "frame 60 not received");

        drop(cmd_tx);
    }

    #[test]
    fn test_thumbnail_decoder_preemption() {
        let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
        if !std::path::Path::new(&test_file).exists() {
            eprintln!("Skipping thumbnail test: test-files/bbb_sunflower_2160p_60fps_normal.mp4 not found");
            return;
        }
        let keyframes = vec![0, 60, 120];
        let keyframes_pts = vec![0, 60 * 1000, 120 * 1000];

        let (cmd_tx, resp_rx, _handles) = spawn_thumbnail_decoder_pool(
            test_file, keyframes, keyframes_pts, 60.0, 1, false,
        );

        // Large request immediately preempted by a focused one.
        cmd_tx.send(ThumbnailCommand::Request {
            visible_frames: vec![0, 60, 120],
            padding_frames: vec![],
            height: 31,
        }).unwrap();
        cmd_tx.send(ThumbnailCommand::Request {
            visible_frames: vec![120],
            padding_frames: vec![],
            height: 31,
        }).unwrap();

        let mut got_120 = false;
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(resp) = resp_rx.recv_timeout(Duration::from_millis(100)) {
                if resp.served_frames.contains(&120) {
                    got_120 = true;
                    break;
                }
            }
        }
        assert!(got_120, "frame 120 not received after preemption");
        drop(cmd_tx);
    }
}
