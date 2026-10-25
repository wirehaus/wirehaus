#![cfg(target_os = "linux")]

use std::process::{Command, Stdio};

use wirehaus_audio::{Encoder, FRAME_VALUES, SAMPLE_RATE, open_default};

fn server_available() -> bool {
    Command::new("pactl")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wav(seconds: u32, hertz: f64) -> Vec<u8> {
    let samples = SAMPLE_RATE * seconds;
    let data_len = samples * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = f64::from(i) / f64::from(SAMPLE_RATE);
        #[allow(clippy::cast_possible_truncation)]
        let value = (12000.0 * (2.0 * std::f64::consts::PI * hertz * t).sin()) as i16;
        out.extend_from_slice(&value.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[test]
fn a_played_tone_is_captured_and_encoded() {
    if !server_available() {
        eprintln!("skipped: no PulseAudio or PipeWire server");
        return;
    }
    let path = std::env::temp_dir().join(format!("wirehaus-tone-{}.wav", std::process::id()));
    std::fs::write(&path, wav(3, 1000.0)).unwrap();
    let mut source = open_default().unwrap();
    let mut player = Command::new("paplay")
        .arg(&path)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut encoder = Encoder::new().unwrap();
    let mut frame = vec![0f32; FRAME_VALUES];
    let mut loud = 0;
    let mut packets = 0;
    for _ in 0..100 {
        source.read_frame(&mut frame).unwrap();
        let peak = frame.iter().fold(0f32, |peak, value| peak.max(value.abs()));
        if peak > 0.2 {
            loud += 1;
        }
        assert!(encoder.encode(&frame).unwrap().len() > 1);
        packets += 1;
    }
    let _ = player.kill();
    let _ = player.wait();
    let _ = std::fs::remove_file(&path);
    assert_eq!(packets, 100);
    assert!(loud > 30, "only {loud} of 100 frames carried the tone");
}
