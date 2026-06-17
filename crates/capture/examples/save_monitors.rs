//! Capture every monitor and the stitched virtual desktop, save them as PNGs.
//!
//! Run with `cargo run -p freally-capture --example save_monitors`. This is the
//! P1.1 acceptance demo: the saved PNGs should match what is on screen.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::temp_dir().join("freally-capture-demo");
    std::fs::create_dir_all(&out_dir)?;

    let monitors = freally_capture::capture_all()?;
    println!("Captured {} monitor(s):", monitors.len());
    for (i, m) in monitors.iter().enumerate() {
        let path = out_dir.join(format!("monitor-{i}.png"));
        m.image.save(&path)?;
        println!(
            "  [{i}] {:<24} bounds={:?} scale={} primary={} -> {}",
            m.name,
            m.bounds,
            m.scale,
            m.is_primary,
            path.display()
        );
    }

    if let Some(comp) = freally_capture::composite(&monitors) {
        let path = out_dir.join("virtual-desktop.png");
        comp.image.save(&path)?;
        println!(
            "Virtual desktop {}x{} at {:?} -> {}",
            comp.image.width(),
            comp.image.height(),
            comp.origin(),
            path.display()
        );
    }

    Ok(())
}
