use std::fs::read_dir;
use std::path::Path;

use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;
use rustls_pemfile::certs;
use xx_core::debug;
use xx_pulse::*;

use super::*;

#[asynchronous]
async fn try_load_certs(path: impl AsRef<Path>) -> Result<Vec<CertificateDer<'static>>> {
	let data = File::load(path).await?;

	certs(&mut &data[..])
		.map(|result| result.map_err(Into::into))
		.collect()
}

#[asynchronous]
async fn try_load_ca_path(path: &str, store: &mut RootCertStore) -> Result<()> {
	let entries = match read_dir(path) {
		Err(err) => return Err(err.into()),
		Ok(files) => files
	};

	let mut certs = Vec::new();

	for entry in entries {
		let entry = entry?;
		let meta = entry.metadata()?;

		if meta.is_dir() {
			continue;
		}

		if let Ok(mut loaded) = try_load_certs(entry.path()).await {
			certs.append(&mut loaded);
		}
	}

	store.add_parsable_certificates(certs);

	Ok(())
}

#[asynchronous]
pub async fn load_system_certs() -> Result<RootCertStore> {
	let mut root_store = RootCertStore::empty();
	let path = root_certs_path();

	try_load_ca_path(path, &mut root_store).await?;

	debug!(target: &root_store, "== Loaded certificates from {}", path);

	Ok(root_store)
}
