//! Lists the Rare devices that answer on this network within 4 s (mDNS `_rare._tcp`).
fn main() {
    for d in rare_stream::nearby::browse(std::time::Duration::from_secs(4)) {
        println!("{} at {} (id {}, streaming {})", d.name, d.address, d.id, if d.stream { "on" } else { "off" });
    }
}
