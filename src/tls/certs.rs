use std::path::Path;

use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;
use rustls_pemfile::certs;
use xx_core::async_std::AsyncIteratorExt;
use xx_core::{debug, trace};
use xx_pulse::*;

use super::*;

#[asynchronous]
async fn try_load_certs(path: impl AsRef<Path>) -> Result<Vec<CertificateDer<'static>>> {
	let data = fs::read(path).await?;

	certs(&mut &data[..])
		.map(|result| result.map_err(Into::into))
		.collect()
}

#[asynchronous]
async fn try_load_ca_path(path: &str, store: &mut RootCertStore) -> Result<()> {
	let mut entries = fs::read_dir(path).await?;
	let mut certs = Vec::new();

	while let Some(entry) = entries.next().await {
		let entry = entry?;
		let path = entry.path();

		if entry.file_type().is_dir() {
			trace!("== Skipping directory {:?}", path);

			continue;
		}

		if let Ok(mut loaded) = try_load_certs(&path).await {
			trace!("++ Loaded {} certs from {:?}", loaded.len(), path);

			certs.append(&mut loaded);
		}
	}

	debug!("++ Loaded {} certificates from {}", certs.len(), path);

	store.add_parsable_certificates(certs);

	Ok(())
}

#[asynchronous]
pub async fn load_system_certs() -> Result<RootCertStore> {
	let mut root_store = RootCertStore::empty();
	let path = root_certs_path();

	try_load_ca_path(path, &mut root_store).await?;

	Ok(root_store)
}
