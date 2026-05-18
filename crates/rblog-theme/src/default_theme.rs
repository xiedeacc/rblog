//! Default theme bootstrap.
//!
//! Ships the markup needed to render every public route. The HTTP layer
//! copies these files into `<work_dir>/themes/default/` at first boot, then
//! treats them like any other theme.
//!
//! Keeping the assets as compile-time strings (not `include_dir!`) keeps the
//! dependency graph small and lets us iterate on the theme without
//! rebuilding the crate.

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("create directory `{path}`: {source}")]
    Mkdir {
        path: String,
        source: std::io::Error,
    },
    #[error("write file `{path}`: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
}

/// Files shipped by the default theme. The leading component is always
/// relative to the theme directory.
const FILES: &[(&str, &str)] = &[
    ("theme.yaml", include_str!("../default/theme.yaml")),
    (
        "templates/base.html",
        include_str!("../default/templates/base.html"),
    ),
    (
        "templates/index.html",
        include_str!("../default/templates/index.html"),
    ),
    (
        "templates/post.html",
        include_str!("../default/templates/post.html"),
    ),
    (
        "templates/page.html",
        include_str!("../default/templates/page.html"),
    ),
    (
        "templates/tag.html",
        include_str!("../default/templates/tag.html"),
    ),
    (
        "templates/category.html",
        include_str!("../default/templates/category.html"),
    ),
    (
        "templates/archive.html",
        include_str!("../default/templates/archive.html"),
    ),
    (
        "templates/search.html",
        include_str!("../default/templates/search.html"),
    ),
    (
        "templates/404.html",
        include_str!("../default/templates/404.html"),
    ),
    (
        "templates/_post_card.html",
        include_str!("../default/templates/_post_card.html"),
    ),
    (
        "templates/_pagination.html",
        include_str!("../default/templates/_pagination.html"),
    ),
    (
        "assets/style.css",
        include_str!("../default/assets/style.css"),
    ),
];

/// Install (or overwrite) the default theme inside `theme_root/default/`.
///
/// `force` controls overwrite policy: when `false`, existing files are left
/// alone so the operator can hand-edit. When `true`, every file is rewritten
/// from the bundled copy (the "reset to defaults" admin button).
pub fn install_default_theme(theme_root: &Path, force: bool) -> Result<(), InstallError> {
    let target = theme_root.join("default");
    std::fs::create_dir_all(&target).map_err(|e| InstallError::Mkdir {
        path: target.display().to_string(),
        source: e,
    })?;
    for (rel, body) in FILES {
        let path = target.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| InstallError::Mkdir {
                path: parent.display().to_string(),
                source: e,
            })?;
        }
        if path.exists() && !force {
            continue;
        }
        std::fs::write(&path, body).map_err(|e| InstallError::Write {
            path: path.display().to_string(),
            source: e,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn install_writes_every_default_file() {
        let tmp = tempfile::tempdir().unwrap();
        install_default_theme(tmp.path(), false).unwrap();
        for (rel, _) in FILES {
            let p = tmp.path().join("default").join(rel);
            assert!(p.exists(), "missing {p:?}");
        }
    }

    #[test]
    fn install_is_idempotent_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        install_default_theme(tmp.path(), false).unwrap();
        // Touch a file to see it survives a second non-force install.
        let custom = tmp.path().join("default/templates/post.html");
        std::fs::write(&custom, "<!-- custom -->").unwrap();
        install_default_theme(tmp.path(), false).unwrap();
        let body = std::fs::read_to_string(&custom).unwrap();
        assert_eq!(body, "<!-- custom -->", "must not overwrite");
    }

    #[test]
    fn install_force_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        install_default_theme(tmp.path(), false).unwrap();
        let custom = tmp.path().join("default/templates/post.html");
        std::fs::write(&custom, "<!-- custom -->").unwrap();
        install_default_theme(tmp.path(), true).unwrap();
        let body = std::fs::read_to_string(&custom).unwrap();
        assert!(body.contains("post.title"), "expected default theme body");
    }
}
