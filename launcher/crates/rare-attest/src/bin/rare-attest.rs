//! `rare-attest --address 0x… [--testnet] [--api https://rare.market] [--aud rare]`: runs one
//! attestation and prints the attestor's answer as JSON (the claim token and the pass offer).
//! For checks over ssh and for other Rare tools; the launcher calls the library directly.

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut address, mut testnet, mut api, mut aud) = (None, false, "https://rare.market".to_string(), None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--address" => address = args.next(),
            "--testnet" => testnet = true,
            "--api" => api = args.next().unwrap_or(api),
            "--aud" => aud = args.next(),
            "--status" => {
                println!("{}", serde_json::to_string(&rare_attest::status()).unwrap());
                return;
            }
            _ => {
                eprintln!("usage: rare-attest --address 0x... [--testnet] [--api URL] [--aud NAME] | --status");
                std::process::exit(2);
            }
        }
    }
    let Some(address) = address else {
        eprintln!("--address is required");
        std::process::exit(2);
    };
    match rare_attest::attest(&rare_attest::Endpoint { base: api, testnet }, &rare_attest::Request { address, aud }) {
        Ok(a) => println!("{}", serde_json::to_string_pretty(&a).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
