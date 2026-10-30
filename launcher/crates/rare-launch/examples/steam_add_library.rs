//! Adds a games folder to Steam the way the launcher's "Add to Steam" does (closes Steam around
//! the edit, starts it again). For checking on a device without a second drive:
//! `steam_add_library <folder holding steamapps>`.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: steam_add_library <folder>");
        std::process::exit(2);
    };
    let runner = rare_launch::launch::Runner::default();
    match rare_launch::api::steam_add_library(&runner, &path, true) {
        Ok(()) => println!("added {path}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
