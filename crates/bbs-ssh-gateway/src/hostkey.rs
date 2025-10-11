use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rand_core::OsRng;
use ssh_key::private::{Ed25519Keypair, KeypairData, PrivateKey};
use ssh_key::{HashAlg, LineEnding};
use tracing::warn;

pub fn load_or_generate(path: &Path) -> Result<PrivateKey> {
    match PrivateKey::read_openssh_file(path) {
        Ok(key) => {
            // Ensure the key decodes and fingerprint is computable.
            let _ = key.public_key().fingerprint(HashAlg::Sha256);
            return Ok(key);
        }
        Err(err) => {
            if path.exists() {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to parse host key, regenerating"
                );
            }
        }
    }

    generate_ed25519(path).context("failed to generate new host key")
}

fn generate_ed25519(path: &Path) -> Result<PrivateKey> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o700);
            fs::set_permissions(parent, perms).ok();
        }
    }

    let mut rng = OsRng;
    let keypair = Ed25519Keypair::random(&mut rng);
    let key_data = KeypairData::Ed25519(keypair);
    let private_key = PrivateKey::new(key_data, "bbs-ssh-gateway generated key")?;
    private_key.write_openssh_file(path, LineEnding::LF)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).ok();
    }
    Ok(private_key)
}
