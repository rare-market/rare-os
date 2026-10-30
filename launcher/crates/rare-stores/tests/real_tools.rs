//! Against the real legendary and gogdl (`RARE_STORES_BIN=<dir with them> cargo test -p rare-stores -- --ignored`),
//! signed out, in a throwaway home: the tools start, and a job that can't sign in fails with a plain line.

#[test]
#[ignore]
fn real_tools_signed_out() {
    let home = std::env::temp_dir().join(format!("rare-stores-test-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    rare_stores::tools::add_to_path();
    assert!(rare_stores::tools::have("legendary") && rare_stores::tools::have("gogdl"));
    let out = rare_stores::tools::run("legendary", &["status", "--offline", "--json"], &[], std::time::Duration::from_secs(60)).unwrap();
    assert!(out.contains("account"), "{out}");
    assert_eq!(rare_stores::epic::account(), None);
    assert_eq!(rare_stores::gog::account(), None);
    rare_stores::jobs::add("epic:Nope", "Nope", rare_stores::jobs::Kind::Install { base: home.join("Games/Epic") }).unwrap();
    let t0 = std::time::Instant::now();
    loop {
        let j = rare_stores::jobs::get("epic:Nope").unwrap();
        if let rare_stores::jobs::Phase::Failed(e) = &j.phase {
            println!("failed as expected: {e}");
            break;
        }
        assert!(t0.elapsed().as_secs() < 90, "still {:?}", j.phase);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert!(rare_stores::drives::space(&home).is_some_and(|(free, _)| free > 0));
    let _ = std::fs::remove_dir_all(&home);
}
