//! Settings ▸ Playback ▸ Hardware decoding through the decoder registry and the media stack:
//! Off gives the software decoder (and no hardware frames), Auto the hardware one where the
//! system has it. Its own test binary: the switch is process-wide.

mod common;

use common::*;
use mortiflix_media::FrameRequest;

#[test]
fn off_uses_the_software_decoder() {
    let ff = mortiflix_testkit::require_ffmpeg!();
    let Some(path) = named(&ff, "hevc_main.mp4") else { return };
    let s = read_stream(&path);
    let available = mortiflix_platform::register();
    assert_eq!(mortiflix_platform::register(), available, "registering twice is harmless");

    mortiflix_codecs::hw::set_hardware_decoding(false);
    let d = mortiflix_codecs::make_video_decoder(&s.entry).unwrap();
    assert_eq!(d.name(), "Mortiflix Studio HEVC");
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&path).unwrap().into();
    let (g0, h0) = (mortiflix_codecs::gop_stats(), mortiflix_codecs::hw::hw_stats());
    let src = mortiflix_codecs::open_bytes("hevc_main.mp4", bytes.clone()).unwrap();
    let rate = src.info().frame_rate();
    for i in 0..30 {
        src.video_frame(FrameRequest::full(rate.tick_of(i))).unwrap();
    }
    assert!(mortiflix_codecs::gop_stats().frames > g0.frames, "frames counted");
    assert_eq!(mortiflix_codecs::hw::hw_stats().frames, h0.frames, "Off: no hardware frames");

    mortiflix_codecs::hw::set_hardware_decoding(true);
    let d = mortiflix_codecs::make_video_decoder(&s.entry).unwrap();
    let hw = mortiflix_platform::hardware_decoder_for(&s.entry);
    if !hw {
        eprintln!("SKIPPED (Auto half): no hardware decoder ({available:?})");
        assert_eq!(d.name(), "Mortiflix Studio HEVC");
        return;
    }
    // Linux has two backends (NVDEC in front of VA-API): `register` reports the front one, but
    // a stream NVDEC declines (an older GPU without HEVC) can still go to VA-API on a second GPU
    let backend = match available {
        mortiflix_platform::Availability::Available(name) => name,
        mortiflix_platform::Availability::Unavailable(why) => panic!("a hardware decoder, but register() said {why}"),
    };
    let decoders: &[&str] = if backend == "NVDEC" { &["NVDEC", "VA-API"] } else { &[backend] };
    assert!(decoders.iter().any(|b| d.name().starts_with(b)) && d.name().contains("HEVC"), "{} from {backend}", d.name());
    assert_eq!(mortiflix_codecs::hw::hw_backend(), Some(backend), "perf.stats reports the backend");
    assert!(mortiflix_platform::registered(), "register() put the factory in the registry");
    let h1 = mortiflix_codecs::hw::hw_stats();
    let src = mortiflix_codecs::open_bytes("hevc_main.mp4", bytes).unwrap();
    for i in 0..30 {
        src.video_frame(FrameRequest::full(rate.tick_of(i))).unwrap();
    }
    let h2 = mortiflix_codecs::hw::hw_stats();
    assert!(h2.frames > h1.frames && h2.sessions > h1.sessions, "Auto: hardware frames {h1:?} → {h2:?}");
}
