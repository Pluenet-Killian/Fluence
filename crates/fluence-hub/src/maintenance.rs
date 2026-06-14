// SPDX-License-Identifier: Apache-2.0

//! Offline data-maintenance subcommands of the hub binary (PLAN 7.3): encrypted
//! backup (with a printable recovery kit), restore, and content purge.
//!
//! These run with the hub **stopped** and reuse its config/path/key resolution
//! ([`crate::store_paths`]) — no duplication. `fluencectl` stays the HTTP client
//! of a *running* hub; data at rest is managed here, where the store key already
//! lives. A backup is keyed by a freshly generated **recovery secret**, printed
//! once as the kit (QR + phrase): that kit, not the machine keystore, is what
//! restores the data on another machine when this one dies (SPEC §9.A).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fluence_store::{RecoverySecret, Store, StoreConfig, back_up, restore};
use qrcode::QrCode;
use qrcode::render::{svg, unicode};

use crate::config::HubConfig;
use crate::store_paths;

/// Backs the store up to `archive`, emitting a fresh recovery kit. Exit code for
/// the `backup` subcommand.
#[must_use]
pub fn backup(config: &HubConfig, archive: &Path) -> ExitCode {
    let (store_path, key) = store_paths(config);
    if !store_path.exists() {
        eprintln!(
            "backup: no store at {} — nothing to back up",
            store_path.display()
        );
        return ExitCode::FAILURE;
    }
    let secret = match do_backup(&store_path, &key, archive) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("backup: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("backup: encrypted archive written to {}", archive.display());
    emit_recovery_kit(&secret, archive);
    ExitCode::SUCCESS
}

/// Restores `archive` (decrypted with `phrase`) into the store. Exit code for
/// the `restore` subcommand.
#[must_use]
pub fn restore_cmd(config: &HubConfig, archive: &Path, phrase: &str) -> ExitCode {
    let (store_path, key) = store_paths(config);
    let secret = match RecoverySecret::from_phrase(phrase) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("restore: invalid recovery phrase: {error}");
            return ExitCode::FAILURE;
        }
    };
    match do_restore(archive, &secret, &store_path, &key) {
        Ok(()) => {
            println!("restore: store restored at {}", store_path.display());
            println!(
                "restore: the previous store (if any) was moved aside as \
                 *.pre-restore — delete it once you have confirmed the restore."
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("restore: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Erases all personal content (SPEC §9.A « oubli »). Exit code for the `wipe`
/// subcommand; requires explicit `confirmed` (the caller's `--yes`).
pub async fn wipe(config: &HubConfig, confirmed: bool) -> ExitCode {
    if !confirmed {
        eprintln!(
            "wipe: this erases all drafts and profiles (SPEC §9.A). \
             Re-run with --yes to confirm."
        );
        return ExitCode::FAILURE;
    }
    let (store_path, key) = store_paths(config);
    if !store_path.exists() {
        println!(
            "wipe: no store at {} — nothing to erase",
            store_path.display()
        );
        return ExitCode::SUCCESS;
    }
    let store = match Store::open(StoreConfig {
        path: store_path,
        key,
    })
    .await
    {
        Ok(store) => store,
        Err(error) => {
            eprintln!("wipe: cannot open store: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = store.purge_content().await;
    let _ = store.close().await;
    match result {
        Ok(removed) => {
            println!("wipe: erased {removed} content row(s) and reclaimed the pages.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("wipe: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Core backup: generates a recovery secret and writes the re-encrypted archive,
/// returning the secret so the caller (or a test) can render/keep the kit.
fn do_backup(
    store_path: &Path,
    key: &fluence_store::KeySource,
    archive: &Path,
) -> Result<RecoverySecret, String> {
    let secret = RecoverySecret::generate();
    back_up(store_path, key, archive, &secret).map_err(|e| format!("{e}"))?;
    Ok(secret)
}

/// Core restore: moves any existing store aside (rolled back on failure, so a
/// botched restore never destroys live data) and re-encrypts the archive into a
/// fresh store under this machine's key.
fn do_restore(
    archive: &Path,
    secret: &RecoverySecret,
    store_path: &Path,
    key: &fluence_store::KeySource,
) -> Result<(), String> {
    let moved = move_aside(store_path)?;
    match restore(archive, secret, store_path, key) {
        Ok(()) => Ok(()),
        Err(error) => {
            roll_back(&moved);
            Err(format!("{error}"))
        }
    }
}

/// Renames the store and its WAL sidecars to `*.pre-restore`, returning the
/// moves so they can be rolled back. Refuses to clobber a previous aside.
fn move_aside(store_path: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let bases = [
        store_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", store_path.display())),
        PathBuf::from(format!("{}-shm", store_path.display())),
    ];
    for base in &bases {
        let aside = aside_path(base);
        if aside.exists() {
            return Err(format!(
                "a previous restore aside already exists at {} — remove it first",
                aside.display()
            ));
        }
    }
    let mut moved = Vec::new();
    for base in &bases {
        if base.exists() {
            let aside = aside_path(base);
            std::fs::rename(base, &aside)
                .map_err(|e| format!("cannot move {} aside: {e}", base.display()))?;
            moved.push((base.clone(), aside));
        }
    }
    Ok(moved)
}

/// Best-effort rollback of [`move_aside`] when a restore fails.
fn roll_back(moved: &[(PathBuf, PathBuf)]) {
    for (original, aside) in moved {
        let _ = std::fs::rename(aside, original);
    }
}

fn aside_path(base: &Path) -> PathBuf {
    PathBuf::from(format!("{}.pre-restore", base.display()))
}

/// Writes the QR to `<archive>.kit.svg` and prints the kit (phrase + a terminal
/// QR + instructions). The phrase always prints; a QR-render failure degrades to
/// phrase-only rather than failing the backup.
fn emit_recovery_kit(secret: &RecoverySecret, archive: &Path) {
    let phrase = secret.phrase();
    let svg_path = aside_with_extension(archive, "kit.svg");
    let svg_written = match recovery_kit_svg(&phrase) {
        Ok(svg) => std::fs::write(&svg_path, svg).is_ok(),
        Err(_) => false,
    };

    println!("\n──────────── KIT DE SECOURS — À IMPRIMER ET GARDER EN SÛRETÉ ────────────");
    println!("Cette phrase EST la clé de déchiffrement de la sauvegarde. Sans elle, la");
    println!("sauvegarde est irrécupérable ; avec elle, n'importe qui peut la lire.\n");
    if let Ok(qr) = recovery_kit_unicode(&phrase) {
        println!("{qr}");
    }
    println!("Phrase de récupération :\n\n    {phrase}\n");
    if svg_written {
        println!("QR imprimable : {}", svg_path.display());
    }
    println!("Restaurer : fluence-hub restore --in <archive> --recovery \"<phrase>\"");
    println!("─────────────────────────────────────────────────────────────────────────\n");
}

/// `<path>` with `.<ext>` appended (e.g. `fluence.backup` → `fluence.backup.kit.svg`).
fn aside_with_extension(path: &Path, ext: &str) -> PathBuf {
    PathBuf::from(format!("{}.{ext}", path.display()))
}

/// The recovery phrase as a printable SVG QR code.
fn recovery_kit_svg(phrase: &str) -> Result<String, qrcode::types::QrError> {
    let code = QrCode::new(phrase.as_bytes())?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

/// The recovery phrase as a terminal (unicode) QR code.
fn recovery_kit_unicode(phrase: &str) -> Result<String, qrcode::types::QrError> {
    let code = QrCode::new(phrase.as_bytes())?;
    Ok(code.render::<unicode::Dense1x2>().quiet_zone(true).build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::{ExposeSecret, SecretString};

    fn test_config(dir: &Path) -> HubConfig {
        HubConfig {
            data_dir: dir.to_path_buf(),
            store_key_file: Some(dir.join("store.key")),
            ..HubConfig::default()
        }
    }

    #[test]
    fn recovery_kit_renders_svg_and_unicode() {
        let phrase = RecoverySecret::generate().phrase();
        let svg = recovery_kit_svg(&phrase).expect("svg");
        assert!(svg.contains("<svg"), "an SVG document");
        assert!(svg.contains("</svg>"));
        let term = recovery_kit_unicode(&phrase).expect("unicode");
        assert!(!term.trim().is_empty(), "a non-empty terminal QR");
    }

    #[test]
    fn move_aside_then_roll_back_restores_the_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = dir.path().join("store.db");
        std::fs::write(&store, b"db").expect("db");
        std::fs::write(dir.path().join("store.db-wal"), b"wal").expect("wal");

        let moved = move_aside(&store).expect("move aside");
        assert!(!store.exists(), "the live store was moved");
        assert!(dir.path().join("store.db.pre-restore").exists());
        // A second aside refuses to clobber the first.
        std::fs::write(&store, b"db2").expect("db2");
        assert!(
            move_aside(&store).is_err(),
            "must not clobber a prior aside"
        );
        std::fs::remove_file(&store).expect("rm");

        roll_back(&moved);
        assert_eq!(std::fs::read(&store).expect("read"), b"db", "rolled back");
        assert!(!dir.path().join("store.db.pre-restore").exists());
    }

    #[tokio::test]
    async fn backup_then_restore_round_trips_through_maintenance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config(dir.path());
        let (store_path, key) = store_paths(&config);

        // Seed a store with a P0 draft, then close it cleanly.
        let store = Store::open(StoreConfig {
            path: store_path.clone(),
            key: key.clone(),
        })
        .await
        .expect("open");
        store
            .upsert_draft("s1".into(), SecretString::from("contenu a sauver"), 5, 1)
            .await
            .expect("draft");
        store.close().await.expect("close");

        // Back up (keeping the returned secret for the restore), then restore.
        let archive = dir.path().join("fluence.backup");
        let secret = do_backup(&store_path, &key, &archive).expect("backup");
        do_restore(&archive, &secret, &store_path, &key).expect("restore");
        assert!(
            dir.path().join("store.db.pre-restore").exists(),
            "the prior store is kept aside, not destroyed"
        );

        // The restored store still has the P0 draft.
        let restored = Store::open(StoreConfig {
            path: store_path,
            key,
        })
        .await
        .expect("reopen");
        let draft = restored
            .draft("s1".into())
            .await
            .expect("read")
            .expect("present");
        assert_eq!(draft.text.expose_secret(), "contenu a sauver");
    }

    #[tokio::test]
    async fn wipe_requires_confirmation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config(dir.path());
        // Unconfirmed: refuses and touches nothing (no store created either).
        assert_eq!(
            format!("{:?}", wipe(&config, false).await),
            format!("{:?}", ExitCode::FAILURE)
        );
    }
}
