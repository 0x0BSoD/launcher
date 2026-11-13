use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use std::{
    collections::HashSet,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    pub path: PathBuf,
}

pub fn read_plist(root: &Path) -> Option<PathBuf> {
    let output = Command::new("/usr/bin/plutil")
        .args([
            "-extract",
            "CFBundleIconFile",
            "raw",
            "-expect",
            "string",
            "-n",
            "-o",
            "-",
            "--",
        ])
        .arg(root.join("Contents/Info.plist"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?;
    if name.is_empty() {
        return None;
    }
    let resources = root.join("Contents/Resources").canonicalize().ok()?;
    let mut path = resources.join(name);
    if path.extension().is_none() {
        path.set_extension("icns");
    }
    let path = path.canonicalize().ok()?;
    (path.starts_with(resources) && path.is_file()).then_some(path)
}

fn render_icon(source: &Path, output: &Path) -> Option<()> {
    let status = Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "64"])
        .arg(source)
        .arg("--out")
        .arg(output)
        .output()
        .ok()?;
    status.status.success().then_some(())
}

fn decode_icon(path: &Path) -> Option<iced::widget::image::Handle> {
    let pixels = image::open(path).ok()?.into_rgba8();
    Some(iced::widget::image::Handle::from_rgba(
        pixels.width(),
        pixels.height(),
        pixels.into_raw(),
    ))
}

// Rendered icons live in `cache` and stay valid while newer than the bundle's Info.plist,
// so a warm start skips both plutil and sips.
pub fn load_icon(app: &Path, cache: &Path) -> Option<iced::widget::image::Handle> {
    // ponytail: DefaultHasher may change between Rust releases; that only costs one re-render.
    let mut hasher = std::hash::DefaultHasher::new();
    app.hash(&mut hasher);
    let cached = cache.join(format!("{:016x}.png", hasher.finish()));
    let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
    let plist = modified(&app.join("Contents/Info.plist"));
    if plist.is_some() && modified(&cached) >= plist {
        if let Some(icon) = decode_icon(&cached) {
            return Some(icon);
        }
    }
    let source = read_plist(app)?;
    fs::create_dir_all(cache).ok()?;
    render_icon(&source, &cached)?;
    decode_icon(&cached)
}

pub fn discover(roots: Vec<PathBuf>) -> (Vec<App>, Vec<String>) {
    let mut pending = roots;
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    let mut errors = Vec::new();
    while let Some(path) = pending.pop() {
        let canonical = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        if !canonical.is_dir() || !seen.insert(canonical) {
            continue;
        }
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        {
            apps.push(App {
                name: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
            });
            continue;
        }
        match fs::read_dir(&path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => pending.push(entry.path()),
                        Err(e) => errors.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.path.clone()));
    (apps, errors)
}

pub fn search(apps: &[App], query: &str) -> Vec<usize> {
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut matches: Vec<_> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, app)| {
            if query.trim().is_empty() {
                Some((i, 0))
            } else {
                matcher
                    .fuzzy_match(&app.name, query.trim())
                    .map(|score| (i, score))
            }
        })
        .collect();
    matches.sort_by(|a, b| b.1.cmp(&a.1));
    matches.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_xml_and_binary_plists_and_rejects_missing_or_external_icons() {
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("Contents/Resources");
        fs::create_dir_all(&resources).unwrap();
        let plist = root.path().join("Contents/Info.plist");
        let icon = resources.join("Test Icon.icns");
        fs::write(&icon, b"invalid image").unwrap();
        for name in ["Test Icon", "Test Icon.icns"] {
            fs::write(&plist, format!(r#"<?xml version="1.0" encoding="UTF-8"?>
                <plist version="1.0"><dict><key>CFBundleIconFile</key><string>{name}</string></dict></plist>"#)).unwrap();
            assert_eq!(read_plist(root.path()), Some(icon.canonicalize().unwrap()));
            assert!(
                Command::new("/usr/bin/plutil")
                    .args(["-convert", "binary1"])
                    .arg(&plist)
                    .status()
                    .unwrap()
                    .success()
            );
            assert_eq!(read_plist(root.path()), Some(icon.canonicalize().unwrap()));
        }
        let out = root.path().join("out.png");
        assert!(render_icon(&icon, &out).is_none());
        fs::remove_file(&icon).unwrap();
        assert!(read_plist(root.path()).is_none());
        fs::write(&icon, b"outside").unwrap();
        fs::write(&plist, format!(r#"<plist version="1.0"><dict><key>CFBundleIconFile</key><string>{}</string></dict></plist>"#, plist.display())).unwrap();
        assert!(read_plist(root.path()).is_none());
        fs::write(&plist, b"broken plist").unwrap();
        assert!(read_plist(root.path()).is_none());
        let png = resources.join("test.png");
        image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 0, 0, 255]))
            .save(&png)
            .unwrap();
        assert!(render_icon(&png, &out).is_some());
        assert!(decode_icon(&out).is_some());
    }

    #[test]
    fn caches_icons_until_info_plist_changes() {
        let app = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let resources = app.path().join("Contents/Resources");
        fs::create_dir_all(&resources).unwrap();
        let plist = app.path().join("Contents/Info.plist");
        let write_plist = || {
            fs::write(&plist, r#"<plist version="1.0"><dict><key>CFBundleIconFile</key><string>icon.png</string></dict></plist>"#).unwrap()
        };
        write_plist();
        let icon = resources.join("icon.png");
        image::RgbaImage::from_pixel(128, 128, image::Rgba([0, 0, 255, 255]))
            .save(&icon)
            .unwrap();
        assert!(load_icon(app.path(), &cache.path().join("icons")).is_some());
        fs::remove_file(&icon).unwrap();
        assert!(load_icon(app.path(), &cache.path().join("icons")).is_some());
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_plist();
        assert!(load_icon(app.path(), &cache.path().join("icons")).is_none());
    }

    #[test]
    fn scans_nested_apps_without_bundle_contents_or_symlink_duplicates() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            root.path()
                .join("Tools/Visual Studio Code.app/Contents/Helper.app"),
        )
        .unwrap();
        fs::create_dir(root.path().join("Safari.app")).unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
        let (apps, errors) = discover(vec![root.path().into(), root.path().join("missing")]);
        assert!(errors.is_empty());
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[search(&apps, "vsc")[0]].name, "Visual Studio Code");
        assert_eq!(search(&apps, "SAFARI"), vec![0]);
        assert!(search(&apps, "zzzzz").is_empty());
        assert_eq!(search(&apps, "  "), vec![0, 1]);
    }
}
