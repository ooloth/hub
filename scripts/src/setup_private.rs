//! Links hub-private's sources and a device's config into this checkout.
//! See docs/architecture/private-workflows.md
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};

use crate::repo_root::repo_root;

/// A device hub-private has a config for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Device {
    pub(crate) name: String,
    pub(crate) config: PathBuf,
}

/// One symlink the setup wants: `at` inside hub, pointing to `target` inside hub-private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) target: PathBuf,
    pub(crate) at: PathBuf,
}

/// What applying a link found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkOutcome {
    Created,
    AlreadyLinked,
    /// A symlink is already there but points somewhere else. Left as it is.
    LinkedElsewhere {
        points_to: PathBuf,
    },
    /// Something that is not a symlink is in the way. Left as it is, and the setup stops.
    Blocked,
}

/// The only device whose checkout also links the media investigation module.
const MEDIA_DEVICE: &str = "home-laptop";

/// `hub_private` made absolute against `root` and resolved, or an error naming where it looked.
///
/// # Errors
/// Returns an error when no hub-private checkout exists there.
pub(crate) fn resolve_hub_private(root: &Path, hub_private: &Path) -> Result<PathBuf> {
    let joined = root.join(hub_private);
    if !joined.is_dir() {
        bail!(
            "hub-private not found at {}\nclone it first, then run: just setup-private <device>",
            joined.display()
        );
    }
    joined
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", joined.display()))
}

/// The devices hub-private has configs for, sorted, for error messages.
fn available_devices(hub_private: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(hub_private.join("devices"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    (path
                        .extension()
                        .is_some_and(|extension| extension == "toml"))
                    .then(|| {
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                    })
                    .flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        "  (none yet)".to_string()
    } else {
        names
            .iter()
            .map(|name| format!("  {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Device {
    /// The device called `name`, or an error listing the devices hub-private has configs for.
    ///
    /// # Errors
    /// Returns an error when no name is given or hub-private has no config for it.
    pub(crate) fn find(hub_private: &Path, name: Option<&str>) -> Result<Self> {
        let Some(name) = name else {
            bail!(
                "device name required\nusage: just setup-private <device>\n\navailable devices:\n{}",
                available_devices(hub_private)
            );
        };
        let config = hub_private.join(format!("devices/{name}.toml"));
        if !config.is_file() {
            bail!(
                "no config found for device '{name}'\nexpected: {}\n\navailable devices:\n{}",
                config.display(),
                available_devices(hub_private)
            );
        }
        Ok(Self {
            name: name.to_string(),
            config,
        })
    }
}

/// Every link the setup wants for `device`, with absolute targets.
pub(crate) fn plan(hub_root: &Path, hub_private: &Path, device: &Device) -> Vec<Link> {
    let source = |dir: &str| Link {
        target: hub_private.join(dir),
        at: hub_root.join(dir).join("private"),
    };
    let mut links = vec![
        source("clients/src"),
        source("workflows/src"),
        source("ui/cli/src"),
        source("ui/tui/src"),
        Link {
            target: device.config.clone(),
            at: hub_root.join("hub.toml"),
        },
    ];
    if device.name == MEDIA_DEVICE {
        let media = "ui/tui/src/investigations/media.rs";
        links.push(Link {
            target: hub_private.join(media),
            at: hub_root.join(media),
        });
    }
    links
}

/// Creates `link` unless something is already at its path.
///
/// # Errors
/// Returns an error when the path cannot be inspected or the symlink cannot be created.
pub(crate) fn apply(link: &Link) -> Result<LinkOutcome> {
    match std::fs::symlink_metadata(&link.at) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let points_to = std::fs::read_link(&link.at)
                .with_context(|| format!("failed to read the link at {}", link.at.display()))?;
            Ok(
                if points_to == link.target || same_file(&link.at, &link.target) {
                    LinkOutcome::AlreadyLinked
                } else {
                    LinkOutcome::LinkedElsewhere { points_to }
                },
            )
        }
        Ok(_) => Ok(LinkOutcome::Blocked),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&link.target, &link.at).with_context(|| {
                format!(
                    "failed to link {} -> {}",
                    link.at.display(),
                    link.target.display()
                )
            })?;
            Ok(LinkOutcome::Created)
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect {}", link.at.display()))
        }
    }
}

/// Whether `a` and `b` resolve to the same file. A relative link that reaches the target through
/// another link is the same link in every way that matters.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Runs the setup for this checkout, printing what each link found or did.
pub(crate) fn run(device: Option<&str>, hub_private: &Path) -> ExitCode {
    match setup(device, hub_private) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn setup(device: Option<&str>, hub_private: &Path) -> Result<ExitCode> {
    let hub_root = repo_root();
    let hub_private = resolve_hub_private(&hub_root, hub_private)?;
    let device = Device::find(&hub_private, device)?;

    for link in plan(&hub_root, &hub_private, &device) {
        match apply(&link)? {
            LinkOutcome::Created => {
                println!("linked: {} -> {}", link.at.display(), link.target.display());
            }
            LinkOutcome::AlreadyLinked => println!("already linked: {}", link.at.display()),
            LinkOutcome::LinkedElsewhere { points_to } => println!(
                "linked elsewhere, left as it is: {} -> {} (this setup links {})",
                link.at.display(),
                points_to.display(),
                link.target.display()
            ),
            LinkOutcome::Blocked => {
                eprintln!(
                    "error: {} exists but is not a symlink. Remove it and run this again.",
                    link.at.display()
                );
                return Ok(ExitCode::FAILURE);
            }
        }
    }

    println!("\ndone. device: {}", device.name);
    println!(
        "edit hub-private/devices/{}.toml to configure projects and credentials.",
        device.name
    );
    println!("run 'just check' to verify compilation.");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parent holding `hub/` (with the source directories links go into) and `hub-private/`
    /// (with the directories they point at and the named device configs).
    fn checkouts(devices: &[&str]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().canonicalize().unwrap();
        let hub = root.join("hub");
        for dir in [
            "clients/src",
            "workflows/src",
            "ui/cli/src",
            "ui/tui/src/investigations",
        ] {
            std::fs::create_dir_all(hub.join(dir)).unwrap();
        }
        let private = root.join("hub-private");
        for dir in [
            "clients/src",
            "workflows/src",
            "ui/cli/src",
            "ui/tui/src/investigations",
        ] {
            std::fs::create_dir_all(private.join(dir)).unwrap();
        }
        std::fs::create_dir_all(private.join("devices")).unwrap();
        for device in devices {
            std::fs::write(private.join(format!("devices/{device}.toml")), "").unwrap();
        }
        (parent, hub, private)
    }

    fn device(private: &Path, name: &str) -> Device {
        Device {
            name: name.to_string(),
            config: private.join(format!("devices/{name}.toml")),
        }
    }

    fn link(target: &Path, at: &Path) -> Link {
        Link {
            target: target.to_path_buf(),
            at: at.to_path_buf(),
        }
    }

    #[test]
    fn a_device_gets_the_source_links_and_its_config() {
        let (_dir, hub, private) = checkouts(&["work-laptop"]);

        let links = plan(&hub, &private, &device(&private, "work-laptop"));

        assert_eq!(
            links,
            vec![
                link(
                    &private.join("clients/src"),
                    &hub.join("clients/src/private")
                ),
                link(
                    &private.join("workflows/src"),
                    &hub.join("workflows/src/private")
                ),
                link(&private.join("ui/cli/src"), &hub.join("ui/cli/src/private")),
                link(&private.join("ui/tui/src"), &hub.join("ui/tui/src/private")),
                link(
                    &private.join("devices/work-laptop.toml"),
                    &hub.join("hub.toml")
                ),
            ]
        );
    }

    #[test]
    fn the_media_device_also_gets_the_media_module() {
        let (_dir, hub, private) = checkouts(&["home-laptop"]);

        let links = plan(&hub, &private, &device(&private, "home-laptop"));

        assert_eq!(
            links.last(),
            Some(&link(
                &private.join("ui/tui/src/investigations/media.rs"),
                &hub.join("ui/tui/src/investigations/media.rs"),
            ))
        );
        assert_eq!(links.len(), 6);
    }

    #[test]
    fn a_relative_hub_private_path_resolves_against_the_checkout() {
        let (_dir, hub, private) = checkouts(&[]);

        let resolved = resolve_hub_private(&hub, Path::new("../hub-private")).unwrap();

        assert_eq!(resolved, private);
    }

    #[test]
    fn a_missing_hub_private_is_named() {
        let (_dir, hub, _private) = checkouts(&[]);

        let error = resolve_hub_private(&hub, Path::new("../nowhere"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("nowhere"), "{error}");
    }

    #[test]
    fn no_device_name_lists_the_devices() {
        let (_dir, _hub, private) = checkouts(&["home-laptop", "work-laptop"]);

        let error = format!("{:#}", Device::find(&private, None).unwrap_err());

        assert!(
            error.contains("home-laptop") && error.contains("work-laptop"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_device_lists_the_devices() {
        let (_dir, _hub, private) = checkouts(&["work-laptop"]);

        let error = format!("{:#}", Device::find(&private, Some("desk")).unwrap_err());

        assert!(
            error.contains("desk") && error.contains("work-laptop"),
            "{error}"
        );
    }

    #[test]
    fn a_known_device_is_found() {
        let (_dir, _hub, private) = checkouts(&["work-laptop"]);

        assert_eq!(
            Device::find(&private, Some("work-laptop")).unwrap(),
            device(&private, "work-laptop")
        );
    }

    #[test]
    fn applying_twice_creates_then_leaves_the_link() {
        let (_dir, hub, private) = checkouts(&[]);
        let wanted = link(
            &private.join("clients/src"),
            &hub.join("clients/src/private"),
        );

        assert_eq!(apply(&wanted).unwrap(), LinkOutcome::Created);
        assert_eq!(apply(&wanted).unwrap(), LinkOutcome::AlreadyLinked);
        assert_eq!(std::fs::read_link(&wanted.at).unwrap(), wanted.target);
    }

    #[test]
    fn a_real_file_in_the_way_is_left_untouched() {
        let (_dir, hub, private) = checkouts(&[]);
        std::fs::write(hub.join("hub.toml"), "mine").unwrap();
        let wanted = link(&private.join("devices/x.toml"), &hub.join("hub.toml"));

        assert_eq!(apply(&wanted).unwrap(), LinkOutcome::Blocked);
        assert_eq!(
            std::fs::read_to_string(hub.join("hub.toml")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn a_relative_link_to_the_same_file_is_already_linked() {
        let (_dir, hub, private) = checkouts(&[]);
        std::fs::write(private.join("ui/tui/src/investigations/media.rs"), "").unwrap();
        std::os::unix::fs::symlink(&private.join("ui/tui/src"), hub.join("ui/tui/src/private"))
            .unwrap();
        std::os::unix::fs::symlink(
            "../private/investigations/media.rs",
            hub.join("ui/tui/src/investigations/media.rs"),
        )
        .unwrap();
        let wanted = link(
            &private.join("ui/tui/src/investigations/media.rs"),
            &hub.join("ui/tui/src/investigations/media.rs"),
        );

        assert_eq!(apply(&wanted).unwrap(), LinkOutcome::AlreadyLinked);
    }

    #[test]
    fn a_link_pointing_elsewhere_is_reported_and_left() {
        let (_dir, hub, private) = checkouts(&[]);
        let elsewhere = hub.join("other-src");
        std::os::unix::fs::symlink(&elsewhere, hub.join("clients/src/private")).unwrap();
        let wanted = link(
            &private.join("clients/src"),
            &hub.join("clients/src/private"),
        );

        assert_eq!(
            apply(&wanted).unwrap(),
            LinkOutcome::LinkedElsewhere {
                points_to: elsewhere.clone()
            }
        );
        assert_eq!(std::fs::read_link(&wanted.at).unwrap(), elsewhere);
    }
}
