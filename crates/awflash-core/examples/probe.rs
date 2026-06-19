//! Quick manual probe: real drive enumeration + the live GitHub catalog
//! fallback. `cargo run -p awflash-core --example probe`

#[tokio::main]
async fn main() {
    match awflash_core::enumerate::list_drives() {
        Ok(drives) => {
            println!("DRIVES ({}):", drives.len());
            for d in &drives {
                println!(
                    "  {} | {} | {} bytes | removable={} system={} bus={:?} mounts={:?}",
                    d.device, d.description, d.size_bytes, d.removable, d.system, d.bus_type, d.mountpoints
                );
            }
        }
        Err(e) => println!("DRIVES ERROR: {e}"),
    }

    println!("\nForcing GitHub catalog fallback (bad primary URL)...");
    match awflash_core::catalog::fetch_catalog("https://invalid.invalid/catalog.json").await {
        Ok(c) => println!(
            "CATALOG source={:?} channels={:?} devices={} images={}",
            c.source,
            c.channels,
            c.devices.len(),
            c.images.len()
        ),
        Err(e) => println!("CATALOG ERROR: {e}"),
    }
}
