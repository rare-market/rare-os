use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;

/// Serves `routes[path]` with its content type; counts requests per path.
fn server(routes: Vec<(&'static str, Vec<u8>, &'static str)>) -> (u16, Arc<Mutex<HashMap<String, usize>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(HashMap::new()));
    let h = hits.clone();
    let routes = Arc::new(routes);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (h, routes) = (h.clone(), routes.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                r.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                *h.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
                let mut s = s;
                match routes.iter().find(|(p, _, _)| *p == path) {
                    Some((_, body, ct)) => {
                        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            });
        }
    });
    (port, hits)
}

fn encode(w: u32, h: u32, fmt: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, fmt).unwrap();
    out.into_inner()
}

fn dims(img: &Image) -> (u32, u32) {
    let d = image::load_from_memory(&img.bytes).unwrap();
    (d.width(), d.height())
}

fn open(dir: &Path) -> Images {
    Images::open(Config { workers: 2, ..Config::new(dir) }).unwrap()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..300 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn downloads_once_resizes_to_webp_and_serves_from_memory_then_disk() {
    let (port, hits) = server(vec![("/cover.png", encode(1200, 1800, image::ImageFormat::Png), "image/png")]);
    let dir = tempfile::tempdir().unwrap();
    let src = Source::Url(format!("http://127.0.0.1:{port}/cover.png"));
    let imgs = open(dir.path());
    let a = imgs.get(&src, Size::Tile).unwrap();
    assert_eq!(a.mime, "image/webp");
    assert_eq!(dims(&a), (400, 600));
    let b = imgs.get(&src, Size::Tile).unwrap();
    assert!(Arc::ptr_eq(&a.bytes, &b.bytes), "second read is the same bytes from memory");
    let thumb = imgs.get(&src, Size::Thumb).unwrap();
    assert_eq!(dims(&thumb), (256, 384));
    assert_eq!(hits.lock().unwrap()["/cover.png"], 2, "one download per size");
    imgs.save_hot();
    // A new process: from disk, no network.
    let again = open(dir.path());
    let c = again.get(&src, Size::Tile).unwrap();
    assert_eq!(&*c.bytes, &*a.bytes);
    assert_eq!(hits.lock().unwrap()["/cover.png"], 2);
    assert_eq!(again.stats().disk_hits, 1);
}

#[test]
fn concurrent_gets_share_one_download() {
    let (port, hits) = server(vec![("/h.jpg", encode(2400, 1000, image::ImageFormat::Jpeg), "image/jpeg")]);
    let dir = tempfile::tempdir().unwrap();
    let imgs = open(dir.path());
    let src = Source::Url(format!("http://127.0.0.1:{port}/h.jpg"));
    let ts: Vec<_> = (0..6).map(|_| {
        let (imgs, src) = (imgs.clone(), src.clone());
        std::thread::spawn(move || dims(&imgs.get(&src, Size::Hero).unwrap()))
    }).collect();
    for t in ts {
        assert_eq!(t.join().unwrap(), (1920, 800));
    }
    assert_eq!(hits.lock().unwrap()["/h.jpg"], 1);
}

#[test]
fn small_compact_images_gifs_and_unknown_formats_pass_through() {
    let small = encode(120, 180, image::ImageFormat::Jpeg);
    let gif = {
        let mut out = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
            for i in 0..2u8 {
                let f = image::RgbaImage::from_pixel(8, 8, image::Rgba([i * 100, 0, 0, 255]));
                enc.encode_frame(image::Frame::new(f)).unwrap();
            }
        }
        out
    };
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>".to_vec();
    let (port, _) = server(vec![("/s.jpg", small.clone(), "image/jpeg"), ("/a.gif", gif.clone(), "image/gif"), ("/i.svg", svg.clone(), "image/svg+xml")]);
    let dir = tempfile::tempdir().unwrap();
    let imgs = open(dir.path());
    let u = |p: &str| Source::Url(format!("http://127.0.0.1:{port}{p}"));
    let s = imgs.get(&u("/s.jpg"), Size::Tile).unwrap();
    assert_eq!((&*s.bytes, s.mime), (&small[..], "image/jpeg"));
    let g = imgs.get(&u("/a.gif"), Size::Thumb).unwrap();
    assert_eq!((&*g.bytes, g.mime), (&gif[..], "image/gif"));
    let v = imgs.get(&u("/i.svg"), Size::Thumb).unwrap();
    assert_eq!(v.mime, "image/svg+xml");
    let o = imgs.get(&u("/s.jpg"), Size::Orig).unwrap();
    assert_eq!(&*o.bytes, &small[..]);
}

#[test]
fn failures_are_remembered_and_not_retried_at_once() {
    let (port, hits) = server(vec![("/page", b"<html>no</html>".to_vec(), "text/html")]);
    let dir = tempfile::tempdir().unwrap();
    let imgs = open(dir.path());
    let page = Source::Url(format!("http://127.0.0.1:{port}/page"));
    assert_eq!(imgs.get(&page, Size::Tile).unwrap_err(), Error::NotImage);
    assert!(imgs.get(&page, Size::Tile).is_err());
    assert_eq!(hits.lock().unwrap()["/page"], 1);
    let gone = Source::Url(format!("http://127.0.0.1:{port}/gone.png"));
    assert_eq!(imgs.get(&gone, Size::Tile).unwrap_err(), Error::NotFound);
    assert_eq!(imgs.get(&gone, Size::Tile).unwrap_err().status(), 404);
}

#[test]
fn local_files_follow_the_filter_and_their_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let steam = tempfile::tempdir().unwrap();
    let cover = steam.path().join("library_600x900.jpg");
    std::fs::write(&cover, encode(600, 900, image::ImageFormat::Jpeg)).unwrap();
    let root = steam.path().to_path_buf();
    let imgs = Images::open(Config { allow_file: Some(Arc::new(move |p: &Path| p.starts_with(&root))), ..Config::new(dir.path()) }).unwrap();
    let a = imgs.get(&Source::File(cover.clone()), Size::Tile).unwrap();
    assert_eq!(dims(&a), (400, 600));
    assert_eq!(imgs.get(&Source::File("/etc/passwd".into()), Size::Tile).unwrap_err(), Error::NotAllowed);
    assert_eq!(imgs.get(&Source::File(steam.path().join("../x.jpg")), Size::Tile).unwrap_err(), Error::NotAllowed);
    // Steam replaced the cover: a new picture, not the cached one.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&cover, encode(300, 450, image::ImageFormat::Png)).unwrap();
    let b = imgs.get(&Source::File(cover), Size::Tile).unwrap();
    assert_eq!(dims(&b), (300, 450));
}

#[test]
fn preload_and_warm_fill_memory_in_the_background() {
    let (port, _) = server(vec![
        ("/1.png", encode(800, 1200, image::ImageFormat::Png), "image/png"),
        ("/2.png", encode(800, 1200, image::ImageFormat::Png), "image/png"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let u = |p: &str| Source::Url(format!("http://127.0.0.1:{port}{p}"));
    {
        let imgs = open(dir.path());
        imgs.preload([(u("/1.png"), Size::Tile), (u("/2.png"), Size::Card)]);
        assert!(wait_until(|| imgs.in_memory(&u("/1.png"), Size::Tile) && imgs.in_memory(&u("/2.png"), Size::Card)));
        // Served pictures are what the next start warms.
        imgs.get(&u("/1.png"), Size::Tile).unwrap();
        imgs.get(&u("/2.png"), Size::Card).unwrap();
        imgs.shutdown();
    }
    let imgs = open(dir.path());
    assert!(!imgs.in_memory(&u("/1.png"), Size::Tile));
    assert_eq!(imgs.warm(10), 2);
    assert!(wait_until(|| imgs.in_memory(&u("/1.png"), Size::Tile) && imgs.in_memory(&u("/2.png"), Size::Card)));
    assert_eq!(imgs.stats().fetches, 0, "warming reads the disk, not the network");
}

#[test]
fn memory_lru_keeps_to_its_budget() {
    let mut lru = Lru { map: HashMap::new(), order: BTreeMap::new(), tick: 0, bytes: 0, cap: 100 };
    let img = |n: usize| Image { bytes: vec![0u8; n].into(), mime: "image/webp" };
    lru.put(1, img(20));
    lru.put(2, img(20));
    lru.put(3, img(20));
    assert!(lru.get(1).is_some()); // 1 is now the newest
    lru.put(4, img(20));
    lru.put(5, img(20));
    lru.put(6, img(20)); // over 100: the oldest (2) goes
    assert!(lru.get(2).is_none());
    assert!(lru.get(1).is_some());
    assert!(lru.bytes <= 100);
    lru.put(7, img(60)); // bigger than a quarter of the budget: not kept
    assert!(lru.get(7).is_none());
}

#[test]
fn uri_paths_round_trip_and_sizes_snap() {
    let p = uri_path("https://rare.market/img/w/i.webp?url=x&w=400", Size::W(300));
    assert!(p.starts_with("w400/"));
    let (src, size) = parse_uri_path(&format!("/{p}?v=1")).unwrap();
    assert_eq!(src, Source::Url("https://rare.market/img/w/i.webp?url=x&w=400".into()));
    assert_eq!(size, Size::W(400));
    let (src, size) = parse_uri_path(&uri_path("/home/deck/.steam/steam/appcache/librarycache/570/library_600x900.jpg", Size::Tile)).unwrap();
    assert_eq!(size, Size::Tile);
    assert!(matches!(src, Source::File(_)));
    assert!(parse_uri_path("tile/!!!").is_none());
    assert!(parse_uri_path(&uri_path("relative/x.png", Size::Tile)).is_none());
    assert_eq!(Size::parse("w5000"), Some(Size::W(2560)));
    assert_eq!(Size::W(1).name(), "w64");
}

#[test]
fn sniffing() {
    assert_eq!(sniff(&encode(4, 4, image::ImageFormat::Png)), Some("image/png"));
    assert_eq!(sniff(&encode(4, 4, image::ImageFormat::Jpeg)), Some("image/jpeg"));
    assert_eq!(sniff(b"\x00\x00\x00\x1cftypavif\x00\x00"), Some("image/avif"));
    assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg/>"), Some("image/svg+xml"));
    assert_eq!(sniff(b"{\"json\":1}"), None);
    let _ = AtomicUsize::new(0);
}

/// `cargo test --release -p rare-images -- --ignored --nocapture`: what one picture costs to make
/// and to serve again.
#[test]
#[ignore]
fn bench_pipeline() {
    let photo = |w: u32, h: u32| {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let v = ((x as f32 * 0.013).sin() * 60.0 + (y as f32 * 0.021).cos() * 60.0 + 128.0) as u8;
            image::Rgb([v, v.wrapping_add((x / 7) as u8), v.wrapping_sub((y / 5) as u8)])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    };
    let hero = photo(3840, 2160);
    let cover = photo(600, 900);
    let capsule = encode(1232, 706, image::ImageFormat::Png);
    let (port, _) = server(vec![("/hero.jpg", hero.clone(), "image/jpeg"), ("/cover.jpg", cover.clone(), "image/jpeg"), ("/cap.png", capsule.clone(), "image/png")]);
    let dir = tempfile::tempdir().unwrap();
    let imgs = open(dir.path());
    let u = |p: &str| Source::Url(format!("http://127.0.0.1:{port}{p}"));
    for (name, path, size, raw) in [("hero 3840x2160 jpg -> hero", "/hero.jpg", Size::Hero, hero.len()), ("cover 600x900 jpg -> tile", "/cover.jpg", Size::Tile, cover.len()), ("capsule 1232x706 png -> card", "/cap.png", Size::Card, capsule.len())] {
        let t = Instant::now();
        let a = imgs.get(&u(path), size).unwrap();
        let make = t.elapsed();
        let t = Instant::now();
        for _ in 0..1000 {
            imgs.get(&u(path), size).unwrap();
        }
        let mem = t.elapsed() / 1000;
        let fresh = open(dir.path());
        let t = Instant::now();
        fresh.get(&u(path), size).unwrap();
        let disk = t.elapsed();
        println!("{name:30} first {:>6.1} ms  disk {:>6.2} ms  memory {:>5.1} us  {} KB -> {} KB", make.as_secs_f64() * 1e3, disk.as_secs_f64() * 1e3, mem.as_secs_f64() * 1e6, raw / 1024, a.bytes.len() / 1024);
    }
}
