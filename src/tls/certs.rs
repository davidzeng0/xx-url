use std::{
	fs::{read_dir, File},
	io::{BufReader, Result},
	path::Path
};

use log::debug;
use rustls::RootCertStore;
use rustls_pemfile::certs;

use super::*;

fn try_load_certs(path: impl AsRef<Path>) -> Result<Vec<Vec<u8>>> {
	let file = File::open(path)?;
	let mut reader = BufReader::new(file);

	certs(&mut reader)
}

fn try_load_ca_path(path: &str, store: &mut RootCertStore) -> Result<()> {
	let entries = match read_dir(path) {
		Err(err) => return Err(err),
		Ok(files) => files
	};

	let mut certs = Vec::new();

	for entry in entries {
		let entry = entry?;
		let meta = entry.metadata()?;

		if meta.is_dir() {
			continue;
		}

		if let Ok(mut loaded) = try_load_certs(entry.path()) {
			certs.append(&mut loaded);
		}
	}

	store.add_parsable_certificates(&certs);

	Ok(())
}

pub fn load_system_certs() -> Result<RootCertStore> {
	let mut root_store = RootCertStore::empty();
	let path = root_certs_path();

	try_load_ca_path(path, &mut root_store)?;

	debug!(target: "Root Cert", "== Loaded certificates from {}", path);

	Ok(root_store)
}
