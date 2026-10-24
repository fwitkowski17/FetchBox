//! `fetchbox plugin link`: try a plugin project on this machine without publishing a release.
//!
//! A plugin whose program is a dependency pointing at the author's GitHub releases cannot be run
//! before the first release exists. Linking puts the project's `plugin.json` into
//! `<data dir>/plugins/<id>/` and the program the author just built (`target/release/<id>`) where
//! the installed dependency would be, recorded as installed, so enabling the plugin fetches
//! nothing. A rebuild is picked up by restarting the plugin; a changed `plugin.json` is copied
//! again by running this once more (and approved again, like any change to a manifest).
//!
//! The plugin folder itself is not a link to the project: the plugin scan only looks at real
//! folders. Files other than `plugin.json` are not copied.

use std::path::{Path, PathBuf};

use anyhow::Context;
use fetchbox_plugins::install::{self, Layout};

use crate::scaffold::print_out;

/// What `plugin link` was asked for.
#[derive(Debug, Clone)]
pub struct LinkRequest {
    /// The project: a folder with a `plugin.json`.
    pub project: PathBuf,
    /// The data directory of the FetchBox that will run the plugin.
    pub data_dir: PathBuf,
    /// The dependency the program stands in for; the one named like the plugin, else the only one.
    pub dependency: Option<String>,
    /// The program; `target/release/<dependency>` in the project when absent.
    pub bin: Option<PathBuf>,
}

/// What was linked.
#[derive(Debug)]
pub struct Linked {
    pub id: String,
    pub plugin_dir: PathBuf,
    /// The dependency and the program that stands in for it.
    pub program: Option<(String, PathBuf)>,
}

pub fn link(request: &LinkRequest) -> anyhow::Result<Linked> {
    // Not created here: a wrong --data-dir would "succeed" in a folder nobody runs FetchBox from.
    anyhow::ensure!(
        request.data_dir.is_dir(),
        "no data directory at {}: is --data-dir the one FetchBox uses? (start it once to create it)",
        request.data_dir.display()
    );
    let manifest_path = request.project.join("plugin.json");
    let bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest = install::parse_manifest(&bytes)
        .map_err(|e| anyhow::anyhow!("{} is not a valid plugin: {e}", manifest_path.display()))?;

    let layout = Layout::new(&request.data_dir);
    let dep = match &request.dependency {
        Some(wanted) => Some(
            manifest
                .dependencies
                .iter()
                .find(|d| &d.id == wanted)
                .with_context(|| format!("the plugin has no dependency \"{wanted}\""))?,
        ),
        None => match manifest.dependencies.as_slice() {
            [] => None,
            [only] => Some(only),
            many => Some(many.iter().find(|d| d.id == manifest.id).with_context(|| {
                format!(
                    "the plugin has several dependencies ({}): name one with --dependency",
                    many.iter()
                        .map(|d| d.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?),
        },
    };

    // The program first: if it is missing, nothing has been changed yet.
    let program = match dep {
        Some(dep) => {
            let path = request
                .bin
                .clone()
                .unwrap_or_else(|| default_program(&request.project, &dep.id));
            anyhow::ensure!(
                path.is_file(),
                "no program at {}: build it first (cargo build --release) or point at it with --bin",
                path.display()
            );
            fetchbox_plugins::deps::link_local(&layout, &manifest.id, dep, &path)
                .map_err(|e| anyhow::anyhow!("link the program: {e}"))?;
            Some((dep.id.clone(), path))
        }
        None => None,
    };

    let plugin_dir = layout.plugin_dir(&manifest.id);
    std::fs::create_dir_all(&plugin_dir)
        .with_context(|| format!("create {}", plugin_dir.display()))?;
    std::fs::write(plugin_dir.join("plugin.json"), &bytes)
        .with_context(|| format!("write {}", plugin_dir.join("plugin.json").display()))?;

    Ok(Linked {
        id: manifest.id,
        plugin_dir,
        program,
    })
}

fn default_program(project: &Path, dependency: &str) -> PathBuf {
    let name = format!("{dependency}{}", std::env::consts::EXE_SUFFIX);
    project.join("target").join("release").join(name)
}

/// `fetchbox plugin link`: do it and say what to do next.
pub fn run_link(request: &LinkRequest) -> anyhow::Result<()> {
    let linked = link(request)?;
    let mut text = format!(
        "Linked {} into {}\n",
        linked.id,
        linked.plugin_dir.display()
    );
    if let Some((dependency, path)) = &linked.program {
        text.push_str(&format!(
            "The program {dependency} is {} (nothing will be downloaded for it)\n",
            path.display()
        ));
    }
    text.push_str(
        "\nNext: Extensions -> Rescan, review the permissions, approve and switch it on.\n\
         After a rebuild press Restart on its card; after editing plugin.json run this again.\n",
    );
    print_out(&text)
}
