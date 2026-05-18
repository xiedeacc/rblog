//! Static asset serving for themes. Currently a stub — `super::mod`
//! mounts a [`tower_http::services::ServeDir`] under `/themes/` so this
//! module exists purely to host future asset transforms (cache-busting
//! hashes, image variant negotiation).
