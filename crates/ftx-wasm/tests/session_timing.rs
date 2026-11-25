//! Times the showcase's posing session natively: the published baked body, the drape room's bolero and the
//! page's settings, then three 10° moves of the left arm. Ignored by default (it reads the site's files and
//! takes minutes); run it to measure or profile:
//!
//!   cargo test --release -p ftx-wasm --test session_timing -- --ignored --nocapture
//!
//! It asserts nothing about speed; it prints what it measured.
use std::path::Path;
use std::time::Instant;

use ftx_wasm::session::{PosableBody, Session, SessionSettings};
use serde_json::Value;

#[test]
#[ignore = "reads the showcase's published body and takes minutes; run explicitly to measure"]
fn posing_session_timing() {
    let site = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../showcase/public");
    let read = |p: &str| std::fs::read(site.join(p)).expect("a published file");
    let header = String::from_utf8(read("exhibits/pose/child-520.json")).expect("utf-8");
    let packed = read("exhibits/pose/child-520.bin.gz");
    let data =
        miniz_oxide::inflate::decompress_to_vec(&packed[10..packed.len() - 8]).expect("gzip body");
    let pattern: Value =
        serde_json::from_slice(&read("exhibits/poses/bolero-start.json")).expect("pattern");
    let fit: Value = serde_json::from_slice(&read("exhibits/poses/fit-spec.json")).expect("fit");
    let settings: SessionSettings = serde_json::from_str(
        &std::env::var("FTX_SESSION_SETTINGS").unwrap_or_else(|_| {
            r#"{"spacing_mm":12,"sew_steps":12,"drape_steps":36,"time_subdivision":true,"motion_step_s":0.0333}"#.into()
        }),
    )
    .expect("settings");
    let body = PosableBody::parse(&header, &data).expect("baked body");
    let base = body.base_arm_deg();
    let began = Instant::now();
    let mut session =
        Session::start(body, &pattern, &fit, settings, |_| {}).expect("opening drape");
    println!(
        "opening drape: {:.1} s, {} cloth vertices",
        began.elapsed().as_secs_f64(),
        session.summary()["cloth_vertices"]
    );
    let stride = 1 + 3 * session.summary()["cloth_vertices"].as_u64().unwrap() as usize;
    let mut left = base;
    for _ in 0..3 {
        let began = Instant::now();
        match session.advance(left + 10.0, base, 10.0 / 60.0) {
            Ok(samples) => {
                println!(
                    "move {left}° -> {}°: {} accepted steps in {:.1} s",
                    left + 10.0,
                    samples.len() / stride,
                    began.elapsed().as_secs_f64()
                );
                left += 10.0;
            }
            Err(refusal) => {
                println!(
                    "move refused after {:.1} s: {refusal}",
                    began.elapsed().as_secs_f64()
                );
                break;
            }
        }
    }
}
