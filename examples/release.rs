//! Release signing for CordFast's updater.
//!
//!     cargo run --example release -- keygen
//!         Makes the publisher key pair: the private key in the config
//!         folder (never in the repository), the public key in
//!         assets/update-public-key.hex, which the app embeds.
//!
//!     cargo run --example release -- sign dist/checksums.txt
//!         Writes checksums.txt.sig: a raw 64-byte Ed25519 signature over
//!         the file's exact bytes, as fastframe-update expects.
//!
//!     cargo run --example release -- checksums dist/<files>...
//!         Prints `sha256sum`-style lines for the files.

use std::path::PathBuf;

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

fn private_key_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("CordFast").join("release-signing.key")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let path = private_key_path();
            if path.exists() {
                eprintln!("A chave já existe em {}; não sobrescrevo.", path.display());
                std::process::exit(1);
            }
            let key = SigningKey::generate(&mut rand::rngs::OsRng);
            std::fs::create_dir_all(path.parent().expect("folder")).expect("config folder");
            std::fs::write(&path, hex(&key.to_bytes())).expect("write private key");
            let public = hex(key.verifying_key().as_bytes());
            std::fs::create_dir_all("assets").expect("assets");
            std::fs::write("assets/update-public-key.hex", &public).expect("write public key");
            println!("chave privada: {}\nchave pública: {public}", path.display());
        }
        Some("sign") => {
            let file = args.get(1).expect("arquivo para assinar");
            let secret = std::fs::read_to_string(private_key_path()).expect("chave privada (rode keygen)");
            let bytes: [u8; 32] = unhex(secret.trim()).try_into().expect("32 bytes");
            let key = SigningKey::from_bytes(&bytes);
            let data = std::fs::read(file).expect("ler arquivo");
            let signature = key.sign(&data);
            std::fs::write(format!("{file}.sig"), signature.to_bytes()).expect("write signature");
            println!("assinado: {file}.sig");
        }
        Some("checksums") => {
            for file in &args[1..] {
                let data = std::fs::read(file).expect("ler arquivo");
                let name = std::path::Path::new(file).file_name().expect("nome").to_string_lossy();
                println!("{}  {name}", hex(&Sha256::digest(&data)));
            }
        }
        _ => {
            eprintln!("uso: release keygen | sign <arquivo> | checksums <arquivos...>");
            std::process::exit(2);
        }
    }
}
