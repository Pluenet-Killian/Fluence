// SPDX-License-Identifier: Apache-2.0

//! `download-test-assets`: fetch the pinned test models (PLAN 4.3).
//!
//! Model management v0 (D-3.2 minimal). A JSON manifest pins each model by
//! URL + **sha256** + size; the sha256 is the integrity contract — a hash that
//! does not match is a hard, loud failure, never a silently-wrong model. The
//! URL is a hint (it may follow a branch); the content is verified, not
//! trusted. Downloads resume from a partial `.part` file (HTTP `Range`) and the
//! command is idempotent: an already-cached, verified model is left untouched.
//!
//! Scope is *models* only. The `llama-server` binary is infrastructure
//! (provisioned by CI/dev, located via config), not a managed model; minisign
//! signatures arrive in Phase 7.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Exit code for an integrity or download failure (distinct from usage = 1,
/// though both map to a non-zero process exit).
const EXIT_FAILURE: u8 = 1;

/// Read buffer for hashing and the size sanity check.
const CHUNK: usize = 64 * 1024;

/// The pinned test-asset manifest (`models/test-assets.json`).
#[derive(Debug, Deserialize)]
struct Manifest {
    /// Schema version (only `1` is understood).
    version: u32,
    /// The models to fetch.
    models: Vec<Model>,
}

/// One pinned model.
#[derive(Debug, Deserialize)]
struct Model {
    /// Stable identifier for logs.
    id: String,
    /// Cache filename (a plain name, no path separators).
    file: String,
    /// Source URL (HTTPS).
    url: String,
    /// Expected SHA-256, lowercase hex — the integrity contract.
    sha256: String,
    /// Expected size in bytes (a cheap completeness check before hashing).
    bytes: u64,
}

/// What `ensure_model` did.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Already cached and the hash matched: nothing to do.
    AlreadyValid,
    /// Freshly downloaded (or resumed) and verified.
    Downloaded,
}

/// Runs `download-test-assets`. With `check_only`, validates the manifest and
/// reports cache state without touching the network.
pub fn run(repo_root: &Path, check_only: bool) -> ExitCode {
    let manifest_path = repo_root.join("models").join("test-assets.json");
    let manifest = match parse_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("download-test-assets: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    let dir = models_dir(repo_root);

    if check_only {
        println!(
            "download-test-assets --check: manifest v{} OK, {} model(s):",
            manifest.version,
            manifest.models.len()
        );
        for model in &manifest.models {
            let state = if dir.join(&model.file).exists() {
                "cached"
            } else {
                "absent"
            };
            println!(
                "  - {} → {} ({} bytes) [{state}]",
                model.id, model.file, model.bytes
            );
        }
        return ExitCode::SUCCESS;
    }

    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!(
            "download-test-assets: cannot create cache {}: {error}",
            dir.display()
        );
        return ExitCode::from(EXIT_FAILURE);
    }
    let agent = ureq::Agent::new_with_defaults();
    for model in &manifest.models {
        match ensure_model(model, &dir, &agent) {
            Ok(Outcome::AlreadyValid) => {
                println!("download-test-assets: {} — cached, verified", model.id);
            }
            Ok(Outcome::Downloaded) => {
                println!("download-test-assets: {} — downloaded, verified", model.id);
            }
            Err(error) => {
                eprintln!("download-test-assets: {error}");
                return ExitCode::from(EXIT_FAILURE);
            }
        }
    }
    println!(
        "download-test-assets: {} model(s) ready in {}",
        manifest.models.len(),
        dir.display()
    );
    ExitCode::SUCCESS
}

/// Reads, parses and validates the manifest at `path`.
fn parse_manifest(path: &Path) -> Result<Manifest, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("cannot read manifest {}: {error}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid manifest {}: {error}", path.display()))?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Rejects a malformed manifest with a precise reason (loud, never silent).
fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.version != 1 {
        return Err(format!(
            "unsupported manifest version {} (this xtask understands 1)",
            manifest.version
        ));
    }
    if manifest.models.is_empty() {
        return Err("manifest lists no models".to_owned());
    }
    for model in &manifest.models {
        if model.sha256.len() != 64 || !model.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "model {}: sha256 must be 64 hex characters",
                model.id
            ));
        }
        if !model.url.starts_with("https://") {
            return Err(format!("model {}: url must be https", model.id));
        }
        // The filename is joined onto the cache directory: reject anything that
        // could escape it (path traversal) or denote a directory.
        if model.file.is_empty()
            || model.file.contains('/')
            || model.file.contains('\\')
            || model.file.contains("..")
        {
            return Err(format!(
                "model {}: file must be a plain filename, got {:?}",
                model.id, model.file
            ));
        }
    }
    Ok(())
}

/// The local model cache: `FLUENCE_MODELS_DIR` if set, else
/// `<repo>/.fluence-cache/models` (already git-ignored).
fn models_dir(repo_root: &Path) -> PathBuf {
    std::env::var_os("FLUENCE_MODELS_DIR").map_or_else(
        || repo_root.join(".fluence-cache").join("models"),
        PathBuf::from,
    )
}

/// Ensures `model` is present in `dir` and matches its sha256, downloading
/// (or resuming) if needed.
fn ensure_model(model: &Model, dir: &Path, agent: &ureq::Agent) -> Result<Outcome, String> {
    let dest = dir.join(&model.file);
    if dest.exists() {
        match sha256_file(&dest) {
            Ok(hash) if hash == model.sha256 => return Ok(Outcome::AlreadyValid),
            _ => {
                // Present but corrupted or stale: discard and re-fetch.
                fs::remove_file(&dest).map_err(|error| {
                    format!("model {}: cannot remove stale cache: {error}", model.id)
                })?;
            }
        }
    }
    download_with_resume(model, &dest, agent)?;
    Ok(Outcome::Downloaded)
}

/// Downloads `model` to `dest`, resuming from a `<dest>.part` if one exists,
/// then verifies size + sha256 and atomically renames it into place.
fn download_with_resume(model: &Model, dest: &Path, agent: &ureq::Agent) -> Result<(), String> {
    let part = dest.with_file_name(format!("{}.part", model.file));

    let mut have = part.metadata().map(|meta| meta.len()).unwrap_or(0);
    if have > model.bytes {
        // A `.part` larger than expected cannot be a prefix of this file.
        fs::remove_file(&part).ok();
        have = 0;
    }

    let mut request = agent.get(&model.url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request
        .call()
        .map_err(|error| format!("model {}: GET {} failed: {error}", model.id, model.url))?;

    // 206 = the server honoured the Range and is sending the tail; 200 = it
    // ignored it and is resending the whole body, so start the file fresh.
    let resuming = response.status().as_u16() == 206;
    let mut file = if resuming {
        OpenOptions::new()
            .append(true)
            .open(&part)
            .map_err(|error| {
                format!(
                    "model {}: cannot append to {}: {error}",
                    model.id,
                    part.display()
                )
            })?
    } else {
        File::create(&part).map_err(|error| {
            format!(
                "model {}: cannot create {}: {error}",
                model.id,
                part.display()
            )
        })?
    };

    let mut reader = response.into_body().into_reader();
    io::copy(&mut reader, &mut file)
        .map_err(|error| format!("model {}: download write failed: {error}", model.id))?;
    drop(file); // close before re-opening to hash (Windows sharing)

    let size = part
        .metadata()
        .map_err(|error| {
            format!(
                "model {}: cannot stat {}: {error}",
                model.id,
                part.display()
            )
        })?
        .len();
    if size != model.bytes {
        fs::remove_file(&part).ok();
        return Err(format!(
            "model {}: size mismatch (expected {} bytes, got {size}) — incomplete download",
            model.id, model.bytes
        ));
    }

    let hash =
        sha256_file(&part).map_err(|error| format!("model {}: cannot hash: {error}", model.id))?;
    if hash != model.sha256 {
        fs::remove_file(&part).ok();
        return Err(format!(
            "model {}: sha256 mismatch (expected {}, got {hash}) — refusing the file",
            model.id, model.sha256
        ));
    }

    fs::rename(&part, dest).map_err(|error| {
        format!(
            "model {}: cannot finalize {}: {error}",
            model.id,
            dest.display()
        )
    })?;
    Ok(())
}

/// Streams a file through SHA-256, returning lowercase hex.
fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Lowercase-hex encoding of a byte slice.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn sha256_bytes(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn model(url: &str, body: &[u8]) -> Model {
        Model {
            id: "test".to_owned(),
            file: "asset.bin".to_owned(),
            url: url.to_owned(),
            sha256: sha256_bytes(body),
            bytes: body.len() as u64,
        }
    }

    /// Serves one GET, honouring a `Range: bytes=N-` with a 206, else a 200.
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let head = read_head(&mut stream);
                let (line, payload, extra) = match parse_range(&head) {
                    Some(start) if start < body.len() as u64 => {
                        let from = usize::try_from(start).expect("offset fits in usize");
                        let tail = body[from..].to_vec();
                        let range = format!(
                            "Content-Range: bytes {}-{}/{}\r\n",
                            start,
                            body.len() - 1,
                            body.len()
                        );
                        ("206 Partial Content", tail, range)
                    }
                    _ => ("200 OK", body.clone(), String::new()),
                };
                let header = format!(
                    "HTTP/1.1 {line}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&payload);
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    /// Reads request bytes up to the end of the headers (`\r\n\r\n`).
    fn read_head(stream: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 512];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Extracts the start offset of a `Range: bytes=N-` header, if present.
    fn parse_range(head: &str) -> Option<u64> {
        head.lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("range:")
                    .map(str::to_owned)
            })
            .and_then(|value| {
                value
                    .trim()
                    .strip_prefix("bytes=")
                    .and_then(|range| range.split('-').next())
                    .and_then(|start| start.trim().parse().ok())
            })
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::new_with_defaults()
    }

    #[test]
    fn hex_encodes_lowercase_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    #[test]
    fn validate_rejects_bad_sha_url_and_filename() {
        let ok = Model {
            id: "m".to_owned(),
            file: "m.gguf".to_owned(),
            url: "https://example/m".to_owned(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        assert!(
            validate(&Manifest {
                version: 1,
                models: vec![clone_model(&ok)]
            })
            .is_ok()
        );

        let bad_sha = Model {
            sha256: "xyz".to_owned(),
            ..clone_model(&ok)
        };
        let bad_url = Model {
            url: "http://insecure".to_owned(),
            ..clone_model(&ok)
        };
        let traversal = Model {
            file: "../escape".to_owned(),
            ..clone_model(&ok)
        };
        for bad in [bad_sha, bad_url, traversal] {
            assert!(
                validate(&Manifest {
                    version: 1,
                    models: vec![bad]
                })
                .is_err()
            );
        }
        assert!(
            validate(&Manifest {
                version: 2,
                models: vec![clone_model(&ok)]
            })
            .is_err()
        );
        assert!(
            validate(&Manifest {
                version: 1,
                models: vec![]
            })
            .is_err()
        );
    }

    fn clone_model(model: &Model) -> Model {
        Model {
            id: model.id.clone(),
            file: model.file.clone(),
            url: model.url.clone(),
            sha256: model.sha256.clone(),
            bytes: model.bytes,
        }
    }

    #[test]
    fn downloads_and_verifies_a_fresh_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"the keyboard always speaks".to_vec();
        let model = model(&serve_once(body.clone()), &body);
        let outcome = ensure_model(&model, dir.path(), &agent()).expect("download");
        assert_eq!(outcome, Outcome::Downloaded);
        let got = fs::read(dir.path().join("asset.bin")).expect("read");
        assert_eq!(got, body);
    }

    #[test]
    fn resumes_from_a_partial_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body: Vec<u8> = (0..4096_u32)
            .map(|i| u8::try_from(i % 251).expect("modulo 251 fits in u8"))
            .collect();
        // Pre-seed a `.part` with the first half; the server serves the tail.
        let part = dir.path().join("asset.bin.part");
        fs::write(&part, &body[..2000]).expect("seed part");
        let model = model(&serve_once(body.clone()), &body);

        let outcome = ensure_model(&model, dir.path(), &agent()).expect("resume");
        assert_eq!(outcome, Outcome::Downloaded);
        assert_eq!(fs::read(dir.path().join("asset.bin")).expect("read"), body);
        assert!(!part.exists(), "the .part is consumed on success");
    }

    #[test]
    fn a_hash_mismatch_is_refused_and_cleaned_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"served bytes".to_vec();
        let mut model = model(&serve_once(body.clone()), &body);
        model.sha256 = "0".repeat(64); // wrong on purpose
        let error = ensure_model(&model, dir.path(), &agent()).expect_err("must refuse");
        assert!(error.contains("sha256 mismatch"), "got: {error}");
        assert!(
            !dir.path().join("asset.bin").exists(),
            "no file is left behind"
        );
        assert!(
            !dir.path().join("asset.bin.part").exists(),
            "no .part left behind"
        );
    }

    #[test]
    fn a_size_mismatch_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"short".to_vec();
        let mut model = model(&serve_once(body.clone()), &body);
        model.bytes = 9999; // server will deliver fewer bytes
        let error = ensure_model(&model, dir.path(), &agent()).expect_err("must refuse");
        assert!(error.contains("size mismatch"), "got: {error}");
    }

    #[test]
    fn an_already_valid_cache_makes_no_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"cached already".to_vec();
        fs::write(dir.path().join("asset.bin"), &body).expect("seed cache");
        // A dead URL proves no network call happens when the cache is valid.
        let model = model("http://127.0.0.1:1/never", &body);
        assert_eq!(
            ensure_model(&model, dir.path(), &agent()).expect("cached"),
            Outcome::AlreadyValid
        );
    }

    #[test]
    fn a_corrupt_cache_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = b"the real contents".to_vec();
        fs::write(dir.path().join("asset.bin"), b"corrupt").expect("seed corrupt");
        let model = model(&serve_once(body.clone()), &body);
        assert_eq!(
            ensure_model(&model, dir.path(), &agent()).expect("replaced"),
            Outcome::Downloaded
        );
        assert_eq!(fs::read(dir.path().join("asset.bin")).expect("read"), body);
    }
}
