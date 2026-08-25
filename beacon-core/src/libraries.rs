use std::path::{Path, PathBuf};

use crate::downloader::DownloadTask;
use crate::manifest::Library;
use crate::rules::{current_arch, native_classifier_for, rules_allow, FeatureFlags};

/// Builds the standard Maven repository-layout path for a library coordinate string
/// (`group:artifact:version[:classifier]`), e.g. `com.mojang:brigadier:1.0.18` becomes
/// `com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar`.
pub fn maven_path(coordinate: &str, classifier: Option<&str>) -> String {
    let mut parts = coordinate.split(':');
    let group = parts.next().unwrap_or_default();
    let artifact = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    let extra_classifier = parts.next();

    let group_path = group.replace('.', "/");
    let classifier = classifier.or(extra_classifier);
    let file_name = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    format!("{group_path}/{artifact}/{version}/{file_name}")
}

/// Modern per-platform native library entries encode their target architecture in the
/// classifier itself (`natives-windows`, `natives-windows-arm64`, `natives-windows-x86`), not in
/// `rules.os.arch` -- Mojang leaves that field null on all three, so `rules_allow` alone cannot
/// tell them apart and would otherwise match, and extract, all of them into the same file names.
enum NativeClassifier {
    /// Not a `natives-*` classifier at all -- an ordinary classpath jar.
    None,
    /// A `natives-*` classifier whose architecture suffix matches this machine.
    Matching,
    /// A `natives-*` classifier for a different architecture; must be skipped entirely.
    OtherArch,
}

fn classify_native(coordinate: &str) -> NativeClassifier {
    let Some(classifier) = coordinate.splitn(4, ':').nth(3) else {
        return NativeClassifier::None;
    };
    let Some(rest) = classifier.strip_prefix("natives-") else {
        return NativeClassifier::None;
    };
    // rest is "<os>" or "<os>-<arch>"; the os part was already filtered by `rules_allow`.
    let arch_suffix = rest.split_once('-').map(|(_, arch)| arch);
    let matches = match arch_suffix {
        None => current_arch() == "x86_64",
        Some(arch) => arch == current_arch(),
    };
    if matches {
        NativeClassifier::Matching
    } else {
        NativeClassifier::OtherArch
    }
}

pub struct ResolvedLibraries {
    /// Main artifact jars, in manifest order, already filtered by platform rules.
    pub classpath: Vec<PathBuf>,
    /// Every file (main artifacts and native archives) that needs to be on disk.
    pub download_tasks: Vec<DownloadTask>,
    /// Native archives (jars containing `.dll`/`.so`/`.dylib`) that must be extracted.
    pub native_archives: Vec<PathBuf>,
}

pub fn resolve(
    libraries: &[Library],
    libraries_dir: &Path,
    features: &FeatureFlags,
) -> ResolvedLibraries {
    let mut classpath = Vec::new();
    let mut download_tasks = Vec::new();
    let mut native_archives = Vec::new();

    for library in libraries {
        if !rules_allow(&library.rules, features) {
            continue;
        }

        // Modern (1.19+) manifests drop the `natives`/`classifiers` maps and instead list one
        // library entry per platform, its coordinate ending in `:natives-<os>[-<arch>]`, gated
        // by the standard `rules` array for the OS but not the architecture.
        let native_classifier = classify_native(&library.name);
        if matches!(native_classifier, NativeClassifier::OtherArch) {
            continue;
        }

        if let Some(downloads) = &library.downloads {
            if let Some(artifact) = &downloads.artifact {
                let rel_path = artifact
                    .path
                    .clone()
                    .unwrap_or_else(|| maven_path(&library.name, None));
                let dest = libraries_dir.join(rel_path);
                if matches!(native_classifier, NativeClassifier::Matching) {
                    native_archives.push(dest.clone());
                } else {
                    classpath.push(dest.clone());
                }
                download_tasks.push(DownloadTask {
                    url: artifact.url.clone(),
                    dest,
                    sha1: Some(artifact.sha1.clone()),
                    size: Some(artifact.size),
                });
            }

            if let (Some(natives_map), Some(classifiers)) =
                (&library.natives, &downloads.classifiers)
            {
                if let Some(classifier_key) = native_classifier_for(natives_map) {
                    if let Some(artifact) = classifiers.get(&classifier_key) {
                        let rel_path = artifact
                            .path
                            .clone()
                            .unwrap_or_else(|| maven_path(&library.name, Some(&classifier_key)));
                        let dest = libraries_dir.join(rel_path);
                        native_archives.push(dest.clone());
                        download_tasks.push(DownloadTask {
                            url: artifact.url.clone(),
                            dest,
                            sha1: Some(artifact.sha1.clone()),
                            size: Some(artifact.size),
                        });
                    }
                }
            }
        } else if let Some(base_url) = &library.url {
            // Pre-1.13 style libraries: no `downloads` block, just a Maven repo base URL.
            let rel_path = maven_path(&library.name, None);
            let dest = libraries_dir.join(&rel_path);
            classpath.push(dest.clone());
            let url = format!("{}/{rel_path}", base_url.trim_end_matches('/'));
            download_tasks.push(DownloadTask {
                url,
                dest,
                sha1: None,
                size: None,
            });
        }
    }

    ResolvedLibraries {
        classpath,
        download_tasks,
        native_archives,
    }
}

/// Extracts every non-`META-INF` entry from each native archive into `natives_dir`.
pub fn extract_natives(native_archives: &[PathBuf], natives_dir: &Path) -> crate::error::Result<()> {
    use crate::error::io_err;

    std::fs::create_dir_all(natives_dir).map_err(io_err(natives_dir))?;

    for archive_path in native_archives {
        let file = std::fs::File::open(archive_path).map_err(io_err(archive_path))?;
        let mut archive = zip::ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if name.starts_with("META-INF/") || entry.is_dir() {
                continue;
            }
            let Some(file_name) = Path::new(&name).file_name() else {
                continue;
            };
            let out_path = natives_dir.join(file_name);
            let mut out_file = std::fs::File::create(&out_path).map_err(io_err(&out_path))?;
            std::io::copy(&mut entry, &mut out_file).map_err(io_err(&out_path))?;
        }
    }
    Ok(())
}
