//! Which game each ROM is and whether its cover loads, read only: scans an Emulation folder,
//! identifies every ROM (header IDs, dump catalogues), and asks rare.market's image front for
//! each cover link in order, printing the one that answered.
//!
//!   emu_ident <Emulation folder> <cache folder>
//!
//! The ROM folder is only read; catalogues and results go in the cache folder.

use std::path::PathBuf;

fn front(url: &str) -> String {
    let enc: String = url.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("https://rare.market/img/w/i.webp?url={enc}&w=400&h=400&fit=cover&output=webp&q=80")
}

fn loads(url: &str) -> bool {
    let a = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(10)).timeout_read(std::time::Duration::from_secs(60)).build();
    match a.get(&front(url)).call() {
        Ok(r) => {
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut r.into_reader(), &mut b).ok();
            b.len() > 500 && &b[..4] == b"RIFF"
        }
        Err(_) => false,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let base = PathBuf::from(a.get(1).expect("Emulation folder"));
    let cache = PathBuf::from(a.get(2).expect("cache folder"));
    let roms = rare_emu::roms::scan_in(&cache, &[rare_emu::roms::Root { tag: "", base }]);
    let t0 = std::time::Instant::now();
    rare_emu::ident::identify_missing(&cache, &roms, &|| {});
    eprintln!("identified {} in {} s", roms.len(), t0.elapsed().as_secs());
    let mut with_art = 0;
    for r in &roms {
        let id = rare_emu::ident::known(&cache, r);
        let urls = rare_emu::art::cover_urls(r, id.as_ref());
        let hit = urls.iter().position(|u| loads(u));
        if hit.is_some() {
            with_art += 1;
        }
        let i = id.unwrap_or_default();
        let src = hit.map(|h| {
            let u = &urls[h];
            let host = u.split('/').nth(2).unwrap_or("");
            let host = if u.contains("xemu-project") { "xemu xdb" } else if host.contains("gametdb") { "GameTDB" } else if host.contains("libretro") { "libretro" } else { host };
            format!("{host} (link {})", h + 1)
        });
        println!(
            "{}\t{}\tmatch: {} {:.2}{}{}\tcover: {}",
            r.system.short,
            r.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            i.how,
            i.confidence,
            i.id.as_deref().map(|x| format!(" id {x}")).unwrap_or_default(),
            i.name.as_deref().map(|x| format!(" as \"{x}\"")).unwrap_or_default(),
            src.unwrap_or_else(|| "none".into())
        );
    }
    println!("{with_art} of {} with a cover", roms.len());
}
