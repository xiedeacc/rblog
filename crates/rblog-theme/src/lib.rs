//! Theme system for the public SSR blog.
//!
//! A "theme" is a directory tree under `<work_dir>/themes/<name>/` with:
//!
//! ```text
//! themes/<name>/
//!   theme.yaml         # ThemeManifest — display name, author, templates
//!   templates/         # MiniJinja templates (.html / .j2)
//!     base.html
//!     index.html
//!     post.html
//!     ...
//!   assets/            # static files served at /themes/<name>/assets/...
//!   settings.yaml      # optional Setting+ConfigMap pair the admin edits
//! ```
//!
//! [`ThemeRegistry`] discovers themes on disk and exposes them to the HTTP
//! layer. [`ThemeRenderer`] wraps a [`minijinja::Environment`] preloaded
//! with rblog-specific filters (markdown rendering, date formatting,
//! pluralization).
//!
//! The default theme ships in `crates/rblog-theme/default/`. The HTTP layer
//! copies it into `<work_dir>/themes/default/` at first boot, exactly the
//! way Halo bootstraps its own default theme.

pub mod default_theme;
pub mod filters;
pub mod manifest;
pub mod registry;
pub mod renderer;

pub use default_theme::install_default_theme;
pub use manifest::{ThemeManifest, ThemeManifestError};
pub use registry::{LoadedTheme, ThemeRegistry, ThemeRegistryError};
pub use renderer::{ThemeRenderer, ThemeRendererError};
