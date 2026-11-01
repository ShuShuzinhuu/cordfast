//! The release script's signature verifies against the key the app embeds.

#[test]
fn release_signature_verifies() {
    // The private key lives only on the owner's PC (examples/release.rs);
    // elsewhere (CI, Linux, the cloud) there is nothing to sign with.
    let key = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into()).join("CordFast").join("release-signing.key");
    if !key.exists() {
        eprintln!("sem a chave privada em {}: teste pulado", key.display());
        return;
    }
    let dir = std::env::temp_dir().join("cordfast-signing-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("checksums.txt");
    std::fs::write(&file, "abc  cordfast-v0.1.0-x86_64-pc-windows-msvc.zip\n").unwrap();
    let status = std::process::Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--example", "release", "--", "sign"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(status.success());
    let signature: [u8; 64] = std::fs::read(dir.join("checksums.txt.sig")).unwrap().try_into().unwrap();
    let public = include_str!("../assets/update-public-key.hex").trim();
    let key: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&public[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let key = ed25519_dalek::VerifyingKey::from_bytes(&key).unwrap();
    key.verify_strict(&std::fs::read(&file).unwrap(), &ed25519_dalek::Signature::from_bytes(&signature))
        .expect("signature verifies");
}
