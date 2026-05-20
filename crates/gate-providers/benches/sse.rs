//! Criterion micro-benchmarks for the shared SSE decoder.
//!
//! Covers the P2.2 hot-path shapes that matter for streaming providers:
//! many tiny token frames, a large vendor event, fragmented UTF-8, and a
//! long-lived connection that is cancelled before a blank-line boundary.
//!
//! Run: cargo bench --package gate-providers --bench sse

use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use gate_providers::sse::SseLineDecoder;

fn small_frame(i: usize) -> Bytes {
    Bytes::from(format!("data: {{\"i\":{i},\"delta\":\"x\"}}\n\n"))
}

fn large_frame(bytes: usize) -> Bytes {
    let payload = "x".repeat(bytes);
    Bytes::from(format!(
        "event: chunk\ndata: {{\"blob\":\"{payload}\"}}\n\n"
    ))
}

fn push_frames(frames: &[Bytes]) -> usize {
    let decoder = SseLineDecoder::new();
    let mut events = 0usize;
    for frame in frames {
        events += decoder.push(Ok(frame.clone())).unwrap().len();
    }
    events
}

fn bench_many_small_frames(c: &mut Criterion) {
    let mut group = c.benchmark_group("sse_parser_many_small_frames");
    for count in [128usize, 1_024, 8_192] {
        let frames: Vec<_> = (0..count).map(small_frame).collect();
        let total_bytes: u64 = frames.iter().map(|frame| frame.len() as u64).sum();
        group.throughput(Throughput::Bytes(total_bytes));
        group.bench_with_input(BenchmarkId::from_parameter(count), &frames, |b, frames| {
            b.iter(|| {
                let events = push_frames(frames);
                black_box(events);
            });
        });
    }
    group.finish();
}

fn bench_large_frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("sse_parser_large_frame");
    for size in [64 * 1024usize, 256 * 1024, 1024 * 1024] {
        let frame = large_frame(size);
        group.throughput(Throughput::Bytes(frame.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &frame, |b, frame| {
            b.iter(|| {
                let decoder = SseLineDecoder::new();
                let events = decoder.push(Ok(frame.clone())).unwrap();
                black_box(events);
            });
        });
    }
    group.finish();
}

fn bench_fragmented_utf8(c: &mut Criterion) {
    let frame =
        "data: {\"delta\":\"星辰🚀\",\"repeat\":\"".to_owned() + &"界".repeat(256) + "\"}\n\n";
    let chunks: Vec<_> = frame
        .as_bytes()
        .chunks(3)
        .map(Bytes::copy_from_slice)
        .collect();

    let mut group = c.benchmark_group("sse_parser_fragmented_utf8");
    group.throughput(Throughput::Bytes(frame.len() as u64));
    group.bench_function("three_byte_chunks", |b| {
        b.iter(|| {
            let events = push_frames(&chunks);
            black_box(events);
        });
    });
    group.finish();
}

fn bench_long_connection_cancel(c: &mut Criterion) {
    let partials: Vec<_> = (0..4_096)
        .map(|i| Bytes::from(format!("data: {{\"i\":{i},\"delta\":\"open\"}}\n")))
        .collect();
    let total_bytes: u64 = partials.iter().map(|frame| frame.len() as u64).sum();

    let mut group = c.benchmark_group("sse_parser_long_connection_cancel");
    group.throughput(Throughput::Bytes(total_bytes));
    group.bench_function("incomplete_events_dropped_on_cancel", |b| {
        b.iter(|| {
            let decoder = SseLineDecoder::new();
            let mut events = 0usize;
            for partial in &partials {
                events += decoder.push(Ok(partial.clone())).unwrap().len();
            }
            drop(decoder);
            black_box(events);
        });
    });
    group.finish();
}

criterion_group!(
    sse_benches,
    bench_many_small_frames,
    bench_large_frame,
    bench_fragmented_utf8,
    bench_long_connection_cancel,
);
criterion_main!(sse_benches);
