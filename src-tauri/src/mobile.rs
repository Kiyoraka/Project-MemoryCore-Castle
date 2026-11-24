//! A phone in the DEV tab - Mobile setup (S-1124-5, his word Nov 24 2026 4:47 PM: "we need to do for
//! memorycore castle also but we need to ensure if they dont have this so it will prompt the companion
//! to install just like how we do with database right ?"). Lifted from the Alice Keep's mobile.rs (MIT)
//! and made to work on any member's machine: Windows, macOS and Linux, no path of one PC assumed.
//! This module finds the tools - the Android SDK, its emulator and adb, the phones (AVDs), Flutter,
//! Java - says what a project is, and lists what its setup still misses. Tools are found in their
//! install folders as well as on PATH: a Castle started before an install still carries the old PATH.

use serde::Serialize;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What kind of mobile project a folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Flutter,
    Android,
}

/// One phone (an Android Virtual Device) on this PC.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Phone {
    /// The AVD's name, as `emulator -avd` takes it.
    pub name: String,
    /// Its display name, or the name with underscores as spaces.
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The Android level of its image, e.g. 35.
    pub api: Option<u32>,
}

/// One row of Mobile setup.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: &'static str,
    pub label: &'static str,
    pub ok: bool,
    pub detail: String,
    /// "create" (a phone, made here), "install" (asked of the companion), or "" when ok.
    pub action: &'static str,
}

/// What the Phone panel needs to know about a tab.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// None when the project is neither Flutter nor Android: no Phone button.
    pub kind: Option<Kind>,
    pub rows: Vec<Row>,
    pub ready: bool,
    pub phones: Vec<Phone>,
    /// Installed system images a new phone can be made from (`system-images;android-35;...`).
    pub images: Vec<String>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The Android SDK's default install folder on this platform (where Android Studio puts it).
fn default_sdk() -> Option<PathBuf> {
    if cfg!(windows) {
        env_path("LOCALAPPDATA").map(|l| l.join("Android").join("Sdk"))
    } else if cfg!(target_os = "macos") {
        crate::env::home_dir().map(|h| h.join("Library").join("Android").join("sdk"))
    } else {
        crate::env::home_dir().map(|h| h.join("Android").join("Sdk"))
    }
}

/// The Android SDK: ANDROID_HOME, ANDROID_SDK_ROOT, then the platform's default install folder.
pub fn sdk_root() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].iter().filter_map(|n| env_path(n)).collect();
    places.extend(default_sdk());
    places.into_iter().find(|p| p.is_dir())
}

/// A program's file name on this platform: `x.exe` on Windows, `x` elsewhere.
pub fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// A script's file name on this platform: `x.bat` on Windows, `x` elsewhere.
pub fn script(name: &str) -> String {
    if cfg!(windows) { format!("{name}.bat") } else { name.to_string() }
}

pub fn emulator_program(sdk: &Path) -> PathBuf {
    sdk.join("emulator").join(exe("emulator"))
}

pub fn adb_program(sdk: &Path) -> PathBuf {
    sdk.join("platform-tools").join(exe("adb"))
}

pub fn avdmanager_program(sdk: &Path) -> PathBuf {
    sdk.join("cmdline-tools").join("latest").join("bin").join(script("avdmanager"))
}

/// A `key=value` file's value (Android's .ini and .properties files; keys may have spaces around `=`).
pub fn value_of(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// The emulator's version, from its package file.
pub fn emulator_version(sdk: &Path) -> Option<String> {
    let text = fs::read_to_string(sdk.join("emulator").join("source.properties")).ok()?;
    value_of(&text, "Pkg.Revision")
}

/// The folders Flutter's own install guide uses on this platform, after the PATH.
fn flutter_places(home: Option<&Path>) -> Vec<PathBuf> {
    let mut places = Vec::new();
    if let Some(home) = home {
        places.push(home.join("flutter").join("bin").join(script("flutter")));
        places.push(home.join("development").join("flutter").join("bin").join(script("flutter")));
    }
    if cfg!(windows) {
        places.push(PathBuf::from(r"C:\src\flutter\bin\flutter.bat"));
    } else {
        places.extend(["/opt/homebrew/bin/flutter", "/usr/local/bin/flutter", "/snap/bin/flutter"].map(PathBuf::from));
    }
    places
}

/// Flutter: the PATH, then the folders its install guide names.
pub fn flutter_program() -> Option<PathBuf> {
    crate::env::find_on_path("flutter").or_else(|| flutter_places(crate::env::home_dir().as_deref()).into_iter().find(|p| p.is_file()))
}

/// Android Studio's own Java (its JBR) on this platform.
fn studio_jbr() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from(r"C:\Program Files\Android\Android Studio\jbr")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home")]
    } else {
        let mut v = vec![PathBuf::from("/opt/android-studio/jbr"), PathBuf::from("/usr/local/android-studio/jbr")];
        v.extend(crate::env::home_dir().map(|h| h.join("android-studio").join("jbr")));
        v
    }
}

fn has_java(home: &Path) -> bool {
    home.join("bin").join(exe("java")).is_file()
}

/// A Java home for Gradle and avdmanager: JAVA_HOME, then Android Studio's own JBR, then the PATH.
pub fn java_home() -> Option<PathBuf> {
    if let Some(home) = env_path("JAVA_HOME").filter(|h| has_java(h)) {
        return Some(home);
    }
    if let Some(studio) = studio_jbr().into_iter().find(|s| has_java(s)) {
        return Some(studio);
    }
    // java on PATH: its home is two folders up (`<home>/bin/java`); a stub with no home around it is not one
    crate::env::find_on_path("java")
        .map(|java| fs::canonicalize(&java).unwrap_or(java))
        .and_then(|java| java.parent()?.parent().map(Path::to_path_buf))
        .filter(|home| has_java(home))
}

/// Where the phones live: ANDROID_AVD_HOME, ANDROID_USER_HOME/avd, then `~/.android/avd`.
pub fn avd_home() -> Option<PathBuf> {
    if let Some(p) = env_path("ANDROID_AVD_HOME") {
        return Some(p);
    }
    if let Some(p) = env_path("ANDROID_USER_HOME") {
        return Some(p.join("avd"));
    }
    crate::env::home_dir().map(|h| h.join(".android").join("avd"))
}

/// One phone from its `config.ini`.
pub fn phone_of(name: &str, config: &str) -> Phone {
    let num = |k: &str| value_of(config, k).and_then(|v| v.parse::<u32>().ok());
    let api = value_of(config, "image.sysdir.1").and_then(|dir| {
        dir.replace('\\', "/").split('/').find_map(|part| part.strip_prefix("android-").and_then(|n| n.parse::<u32>().ok()))
    });
    let label = value_of(config, "avd.ini.displayname").filter(|v| !v.is_empty()).unwrap_or_else(|| name.replace('_', " "));
    Phone { name: name.to_string(), label, width: num("hw.lcd.width"), height: num("hw.lcd.height"), api }
}

/// The phones in a folder of AVDs: every `<name>.ini` with its `<name>.avd\config.ini`, by name.
pub fn phones_in(home: &Path) -> Vec<Phone> {
    let Ok(entries) = fs::read_dir(home) else { return Vec::new() };
    let mut phones: Vec<Phone> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()? != "ini" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            let config = fs::read_to_string(home.join(format!("{name}.avd")).join("config.ini")).unwrap_or_default();
            Some(phone_of(&name, &config))
        })
        .collect();
    phones.sort_by(|a, b| a.name.cmp(&b.name));
    phones
}

/// The installed system images as sdkmanager names: `system-images;android-35;google_apis;x86_64`.
pub fn images_in(sdk: &Path) -> Vec<String> {
    let base = sdk.join("system-images");
    let mut out = Vec::new();
    let dirs = |p: &Path| -> Vec<PathBuf> { fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default() };
    for api in dirs(&base) {
        for tag in dirs(&api) {
            for abi in dirs(&tag) {
                let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                out.push(format!("system-images;{};{};{}", name(&api), name(&tag), name(&abi)));
            }
        }
    }
    out.sort();
    out.reverse(); // the newest Android first
    out
}

/// A Flutter project (`pubspec.yaml` with a `flutter:` section) or an Android app (a Gradle file
/// that applies `com.android.application`, at the root or in `app/`); None for anything else.
pub fn kind_of(folder: &Path) -> Option<Kind> {
    if let Ok(pubspec) = fs::read_to_string(folder.join("pubspec.yaml")) {
        if pubspec.lines().any(|l| l.trim_end() == "flutter:" || l.starts_with("flutter:")) {
            return Some(Kind::Flutter);
        }
    }
    for dir in [folder.to_path_buf(), folder.join("app")] {
        for file in ["build.gradle.kts", "build.gradle"] {
            if let Ok(text) = fs::read_to_string(dir.join(file)) {
                if text.contains("com.android.application") {
                    return Some(Kind::Android);
                }
            }
        }
    }
    None
}

/// The Android app's id, from `app/build.gradle[.kts]` (`applicationId = "x"` or `applicationId "x"`).
pub fn application_id(folder: &Path) -> Option<String> {
    for file in ["build.gradle.kts", "build.gradle"] {
        let Ok(text) = fs::read_to_string(folder.join("app").join(file)) else { continue };
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("applicationId") {
                let id: String = rest.trim().trim_start_matches('=').trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// What a machine has, gathered once, so the rows can be built (and tested) without touching it.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub sdk: Option<PathBuf>,
    pub emulator: Option<String>,
    pub phones: usize,
    pub images: usize,
    pub flutter: Option<PathBuf>,
    pub java: Option<PathBuf>,
}

/// The setup rows for a kind of project: Flutter checks Flutter, Android (Kotlin) checks Java.
pub fn rows_for(kind: Kind, f: &Found) -> Vec<Row> {
    let mut rows = vec![
        Row {
            id: "sdk",
            label: "Android SDK",
            ok: f.sdk.is_some(),
            detail: f.sdk.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()),
            action: if f.sdk.is_some() { "" } else { "install" },
        },
        Row {
            id: "emulator",
            label: "Emulator",
            ok: f.emulator.is_some(),
            detail: f.emulator.clone().unwrap_or_else(|| "not found".into()),
            action: if f.emulator.is_some() { "" } else { "install" },
        },
        Row {
            id: "phone",
            label: "A phone",
            ok: f.phones > 0,
            detail: match f.phones {
                0 if f.images > 0 => "none yet".into(),
                0 => "none yet, and no Android image to make one from".into(),
                1 => "1 phone".into(),
                n => format!("{n} phones"),
            },
            // a phone is made here from an installed image; without one the companion installs an image
            action: match (f.phones, f.images) {
                (0, 0) => "install",
                (0, _) => "create",
                _ => "",
            },
        },
    ];
    rows.push(match kind {
        Kind::Flutter => Row {
            id: "flutter",
            label: "Flutter",
            ok: f.flutter.is_some(),
            detail: f.flutter.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()),
            action: if f.flutter.is_some() { "" } else { "install" },
        },
        Kind::Android => Row {
            id: "java",
            label: "Java",
            ok: f.java.is_some(),
            detail: f.java.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not found".into()),
            action: if f.java.is_some() { "" } else { "install" },
        },
    });
    rows
}

/// Mobile setup for one project folder, read from this PC.
pub fn check(folder: &Path) -> Check {
    let Some(kind) = kind_of(folder) else {
        return Check { kind: None, rows: Vec::new(), ready: false, phones: Vec::new(), images: Vec::new() };
    };
    let sdk = sdk_root();
    let phones = avd_home().map(|h| phones_in(&h)).unwrap_or_default();
    let images = sdk.as_deref().map(images_in).unwrap_or_default();
    let found = Found {
        emulator: sdk.as_deref().filter(|s| emulator_program(s).is_file()).map(|s| emulator_version(s).unwrap_or_else(|| "installed".into())),
        sdk,
        phones: phones.len(),
        images: images.len(),
        flutter: if kind == Kind::Flutter { flutter_program() } else { None },
        java: if kind == Kind::Android { java_home() } else { None },
    };
    let rows = rows_for(kind, &found);
    let ready = rows.iter().all(|r| r.ok);
    Check { kind: Some(kind), rows, ready, phones, images }
}

/// A phone's name as avdmanager and the emulator take it.
pub fn is_phone_name(name: &str) -> bool {
    (1..=40).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

/// An installed image's sdkmanager name, as `images_in` writes it.
pub fn is_image_name(image: &str) -> bool {
    let parts: Vec<&str> = image.split(';').collect();
    parts.len() == 4 && parts[0] == "system-images" && parts[1..].iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
}

/// Makes a phone from an installed image with avdmanager (a Pixel 7 shape), answering its one
/// question ("a custom hardware profile?") with no. Its own words come back when it refuses.
pub fn create_phone(name: &str, image: &str) -> Result<(), String> {
    if !is_phone_name(name) {
        return Err("A phone's name is letters, digits, _ . or -, up to 40.".into());
    }
    if !is_image_name(image) {
        return Err("That Android image is not one installed here.".into());
    }
    let sdk = sdk_root().ok_or("The Android SDK is not on this computer.")?;
    if !images_in(&sdk).iter().any(|i| i == image) {
        return Err("That Android image is not one installed here.".into());
    }
    let tool = avdmanager_program(&sdk);
    if !tool.is_file() {
        return Err("avdmanager is missing - the SDK's command-line tools are not installed.".into());
    }
    match make_avd(&tool, name, image, Some("pixel_7")) {
        // an older SDK without the Pixel 7 profile: the tool's default shape is a phone too
        Err(said) if said.to_ascii_lowercase().contains("device") => make_avd(&tool, name, image, None),
        other => other,
    }
}

fn make_avd(tool: &Path, name: &str, image: &str, device: Option<&str>) -> Result<(), String> {
    let mut command = Command::new(tool);
    command.args(["create", "avd", "-n", name, "-k", image]);
    if let Some(device) = device {
        command.args(["-d", device]);
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(java) = java_home() {
        command.env("JAVA_HOME", java);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // no console window
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err("avdmanager is missing.".into()),
        Err(_) => return Err("avdmanager could not run.".into()),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"no\n");
    }
    let out = child.wait_with_output().map_err(|_| "avdmanager could not run.".to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let said = if said.is_empty() { String::from_utf8_lossy(&out.stdout).trim().to_string() } else { said };
    Err(said.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string).unwrap_or_else(|| "avdmanager refused.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("castle-mobile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_folder_says_what_kind_of_mobile_project_it_is() {
        let flutter = temp("flutter");
        fs::write(flutter.join("pubspec.yaml"), "name: demo\ndependencies:\n  flutter:\n    sdk: flutter\nflutter:\n  uses-material-design: true\n").unwrap();
        assert_eq!(kind_of(&flutter), Some(Kind::Flutter));

        let dart = temp("dart");
        fs::write(dart.join("pubspec.yaml"), "name: tool\ndependencies:\n  args: ^2.0.0\n").unwrap();
        assert_eq!(kind_of(&dart), None);

        let android = temp("android");
        fs::create_dir_all(android.join("app")).unwrap();
        fs::write(android.join("app").join("build.gradle.kts"), "plugins {\n    id(\"com.android.application\")\n}\nandroid {\n    defaultConfig {\n        applicationId = \"com.kiyo.demo\"\n    }\n}\n").unwrap();
        assert_eq!(kind_of(&android), Some(Kind::Android));
        assert_eq!(application_id(&android), Some("com.kiyo.demo".into()));

        let web = temp("web");
        fs::write(web.join("package.json"), "{}").unwrap();
        assert_eq!(kind_of(&web), None);
        for d in [flutter, dart, android, web] {
            let _ = fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_phone_is_read_from_its_config() {
        let config = "avd.ini.displayname=Pixel 7a API 35\nhw.lcd.height = 2400\nhw.lcd.width = 1080\nimage.sysdir.1 = system-images\\android-35\\google_apis_playstore_ps16k\\x86_64\\\n";
        let p = phone_of("Pixel_7a_API_35", config);
        assert_eq!(p.label, "Pixel 7a API 35");
        assert_eq!((p.width, p.height, p.api), (Some(1080), Some(2400), Some(35)));
        assert_eq!(phone_of("Small_Phone", "").label, "Small Phone");

        let home = temp("avd");
        fs::write(home.join("Small_Phone.ini"), "path=x").unwrap();
        fs::create_dir_all(home.join("Small_Phone.avd")).unwrap();
        fs::write(home.join("Small_Phone.avd").join("config.ini"), config).unwrap();
        fs::write(home.join("notes.txt"), "not a phone").unwrap();
        let phones = phones_in(&home);
        assert_eq!(phones.len(), 1);
        assert_eq!(phones[0].name, "Small_Phone");
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn the_images_are_named_as_sdkmanager_names_them() {
        let sdk = temp("sdk");
        fs::create_dir_all(sdk.join("system-images").join("android-34").join("google_apis").join("x86_64")).unwrap();
        fs::create_dir_all(sdk.join("system-images").join("android-35").join("google_apis_playstore_ps16k").join("x86_64")).unwrap();
        let images = images_in(&sdk);
        assert_eq!(images[0], "system-images;android-35;google_apis_playstore_ps16k;x86_64");
        assert!(images.iter().all(|i| is_image_name(i)));
        assert!(!is_image_name("system-images;android-35;../x;x86_64"));
        assert!(!is_image_name("platforms;android-35"));
        let _ = fs::remove_dir_all(sdk);
    }

    #[test]
    fn the_rows_say_what_is_missing_and_how_it_gets_done() {
        let all = Found { sdk: Some("C:/sdk".into()), emulator: Some("35.5.10".into()), phones: 2, images: 2, flutter: Some("C:/flutter/bin/flutter.bat".into()), java: Some("C:/jbr".into()) };
        let rows = rows_for(Kind::Flutter, &all);
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec!["sdk", "emulator", "phone", "flutter"]);
        assert!(rows.iter().all(|r| r.ok && r.action.is_empty()));
        assert_eq!(rows_for(Kind::Android, &all)[3].id, "java");

        let bare = Found { sdk: Some("C:/sdk".into()), emulator: Some("35.5.10".into()), images: 1, ..Found::default() };
        let rows = rows_for(Kind::Flutter, &bare);
        assert_eq!((rows[2].ok, rows[2].action), (false, "create"));
        assert_eq!((rows[3].ok, rows[3].action), (false, "install"));
        let none = rows_for(Kind::Flutter, &Found::default());
        assert_eq!(none[2].action, "install");
        assert!(none.iter().all(|r| !r.ok));
    }

    #[test]
    fn the_tools_are_named_for_this_platform() {
        let sdk = Path::new("sdk");
        let (adb, emulator, avd) = (adb_program(sdk), emulator_program(sdk), avdmanager_program(sdk));
        if cfg!(windows) {
            assert!(adb.ends_with("platform-tools/adb.exe") && emulator.ends_with("emulator/emulator.exe") && avd.ends_with("bin/avdmanager.bat"));
        } else {
            assert!(adb.ends_with("platform-tools/adb") && emulator.ends_with("emulator/emulator") && avd.ends_with("bin/avdmanager"));
        }
        let places = flutter_places(Some(Path::new("home")));
        assert_eq!(places[0], Path::new("home").join("flutter").join("bin").join(script("flutter")));
        assert!(!studio_jbr().is_empty());
    }

    #[test]
    fn names_and_values_are_read_carefully() {
        assert!(is_phone_name("Pixel_7a_API_35"));
        assert!(!is_phone_name("a b"));
        assert!(!is_phone_name(""));
        assert!(!is_phone_name("x&calc"));
        assert_eq!(value_of("Pkg.Revision=35.5.10\n", "Pkg.Revision"), Some("35.5.10".into()));
        assert_eq!(value_of("a = 1\n", "a"), Some("1".into()));
        assert_eq!(value_of("ab=1\n", "a"), None);
    }
}
