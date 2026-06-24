mod appinfo;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use appinfo::{Appinfo, AppRecord, Value, set_nested};

const APP_ID: u32 = 2552430;
const APP_ID_STR: &str = "2552430";

struct GameDef {
    description:     &'static str,
    exe:             Option<&'static str>,
    direct_args:     &'static str,
    direct_alt:      Option<(&'static str, &'static str)>,
    launcher_args:   &'static str,
    launcher_always: bool,
    direct_include:  bool,
}

const GAMES: &[GameDef] = &[
    GameDef {
        description:     "Play Kingdom Hearts Final Mix",
        exe:             Some("KINGDOM HEARTS FINAL MIX.exe"),
        direct_args:     "",
        direct_alt:      Some(("Play Kingdom Hearts Final Mix (no copyright screens)", "-reboot=true")),
        launcher_args:   "-kh1",
        launcher_always: false,
        direct_include:  true,
    },
    GameDef {
        description:     "Play Kingdom Hearts Re:Chain of Memories",
        exe:             Some("KINGDOM HEARTS Re_Chain of Memories.exe"),
        direct_args:     "",
        direct_alt:      None,
        launcher_args:   "-recom",
        launcher_always: true,
        direct_include:  true,
    },
    GameDef {
        description:     "Play Kingdom Hearts II Final Mix",
        exe:             Some("KINGDOM HEARTS II FINAL MIX.exe"),
        direct_args:     "",
        direct_alt:      None,
        launcher_args:   "-kh2",
        launcher_always: false,
        direct_include:  true,
    },
    GameDef {
        description:     "Play Kingdom Hearts Birth by Sleep Final Mix",
        exe:             Some("KINGDOM HEARTS Birth by Sleep FINAL MIX.exe"),
        direct_args:     "",
        direct_alt:      None,
        launcher_args:   "-bbs",
        launcher_always: false,
        direct_include:  true,
    },
    GameDef {
        description:     "Play Kingdom Hearts 358/2 Days",
        exe:             None,
        direct_args:     "",
        direct_alt:      None,
        launcher_args:   "-days",
        launcher_always: true,
        direct_include:  false,
    },
    GameDef {
        description:     "Play Kingdom Hearts Re:coded",
        exe:             None,
        direct_args:     "",
        direct_alt:      None,
        launcher_args:   "-recoded",
        launcher_always: true,
        direct_include:  false,
    },
];

enum Mode {
    Direct,
    LauncherConfig,
}

fn parse_mode() -> Mode {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--direct"          => return Mode::Direct,
            "--custom-launcher" => return Mode::LauncherConfig,
            _ => {}
        }
    }
    show_error(
        "kh-launchoptions",
        "Usage:\n  kh-launchoptions --direct\n  kh-launchoptions --custom-launcher",
    );
    std::process::exit(1);
}

#[cfg(windows)]
fn steam_root_from_registry() -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ).ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path))
}

#[cfg(not(windows))]
fn steam_root_from_registry() -> Option<PathBuf> {
    None
}

fn find_steam_root() -> Option<PathBuf> {
    if let Some(p) = steam_root_from_registry() {
        if p.exists() {
            return Some(p);
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let candidates = [
        format!("{}/.local/share/Steam", home),
        format!("{}/.var/app/com.valvesoftware.Steam/data/Steam", home),
        format!("{}/Library/Application Support/Steam", home),
    ];
    candidates.into_iter().map(PathBuf::from).find(|p| p.exists())
}

fn appinfo_path(steam_root: &Path) -> PathBuf {
    steam_root.join("appcache").join("appinfo.vdf")
}

fn find_all_steamapps(steam_root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    let default = steam_root.join("steamapps");
    if default.exists() {
        dirs.push(default);
    }

    let lvdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&lvdf) {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("\"path\"") {
                if let Some(val) = extract_vdf_string_value(line) {
                    let candidate = PathBuf::from(&val).join("steamapps");
                    if candidate.exists() && !dirs.contains(&candidate) {
                        dirs.push(candidate);
                    }
                }
            }
        }
    }

    dirs
}

fn extract_vdf_string_value(line: &str) -> Option<String> {
    let mut quotes: Vec<&str> = Vec::new();
    let mut remainder = line;
    loop {
        let start = remainder.find('"')?;
        remainder = &remainder[start + 1..];
        let end = remainder.find('"')?;
        quotes.push(&remainder[..end]);
        remainder = &remainder[end + 1..];
        if quotes.len() == 2 {
            break;
        }
    }
    Some(quotes.get(1)?.replace("\\\\", "\\"))
}

fn find_game_install(steamapps_dirs: &[PathBuf]) -> Option<PathBuf> {
    let manifest_name = format!("appmanifest_{APP_ID_STR}.acf");
    for dir in steamapps_dirs {
        let manifest = dir.join(&manifest_name);
        if !manifest.exists() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            if let Some(install_dir) = parse_installdir(&text) {
                let game_dir = dir.join("common").join(install_dir);
                if game_dir.exists() {
                    return Some(game_dir);
                }
            }
        }
    }
    None
}

fn parse_installdir(acf: &str) -> Option<String> {
    for line in acf.lines() {
        if line.trim().starts_with("\"installdir\"") {
            return extract_vdf_string_value(line.trim());
        }
    }
    None
}

struct WrittenSlot {
    description: String,
    exe:         String,
    args:        String,
}

fn build_direct_slots(install_dir: &Path) -> (Vec<WrittenSlot>, Vec<String>) {
    let mut slots: Vec<WrittenSlot> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // slot 0 is always the launcher as defined by steam
    slots.push(WrittenSlot {
        description: "Play KINGDOM HEARTS -HD 1.5+2.5 ReMIX-".into(),
        exe:         "KINGDOM HEARTS HD 1.5+2.5 ReMIX.exe".into(),
        args:        String::new(),
    });

    slots.push(WrittenSlot {
        description: "Play official Launcher (no copyright screens)".into(),
        exe:         "KINGDOM HEARTS HD 1.5+2.5 Launcher.exe".into(),
        args:        "-reboot=true".to_owned(),
    });

    for game in GAMES {
        if !game.direct_include {
            continue;
        }
        let exe_name = match game.exe {
            Some(e) => e,
            None => continue,
        };

        if install_dir.join(exe_name).exists() {
            slots.push(WrittenSlot {
                description: game.description.to_owned(),
                exe:         exe_name.to_owned(),
                args:        game.direct_args.to_owned(),
            });
            if let Some((alt_desc, alt_args)) = game.direct_alt {
                slots.push(WrittenSlot {
                    description: alt_desc.to_owned(),
                    exe:         exe_name.to_owned(),
                    args:        alt_args.to_owned(),
                });
            }
        } else {
            skipped.push(format!("{} ({})", game.description, exe_name));
        }
    }

    (slots, skipped)
}

fn build_launcher_config_slots(install_dir: &Path) -> (Vec<WrittenSlot>, Vec<String>) {
    let mut slots: Vec<WrittenSlot> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // slot 0 is always the launcher as defined by steam - make it replicate the movies launch for reliability
    slots.push(WrittenSlot {
        description: "Play KINGDOM HEARTS -HD 1.5+2.5 ReMIX-".into(),
        exe:         "LauncherConfig.exe".to_owned(),
        args:        "-recoded".to_owned(),
    });

        slots.push(WrittenSlot {
        description: "Play last launched game".into(),
        exe:         "LauncherConfig.exe".to_owned(),
        args:        "-skipLauncher".to_owned(),
    });

    for game in GAMES {
        if game.launcher_args.is_empty() {
            continue;
        }

        let present = match game.exe {
            Some(exe) => install_dir.join(exe).exists(),
            None      => true,
        };

        if present || game.launcher_always {
            slots.push(WrittenSlot {
                description: game.description.to_owned(),
                exe:         "LauncherConfig.exe".to_owned(),
                args:        game.launcher_args.to_owned(),
            });
        } else {
            skipped.push(game.description.to_owned());
        }
    }

    slots.push(WrittenSlot {
        description: "Open Launcher config".into(),
        exe:         "LauncherConfig.exe".to_owned(),
        args:        String::new(),
    });
    slots.push(WrittenSlot {
        description: "Play official Launcher (no copyright screens)".into(),
        exe:         "KINGDOM HEARTS HD 1.5+2.5 Launcher.exe".into(),
        args:        "-reboot=true".to_owned(),
    });

    (slots, skipped)
}

fn apply_slots(record: &mut AppRecord, slots: &[WrittenSlot]) {
    let mut launch: BTreeMap<String, Value> = BTreeMap::new();

    for (i, slot) in slots.iter().enumerate() {
        let mut entry: BTreeMap<String, Value> = BTreeMap::new();
        entry.insert("description".into(), Value::String(slot.description.clone()));
        entry.insert("executable".into(),  Value::String(slot.exe.clone()));

        if !slot.args.is_empty() {
            entry.insert("arguments".into(), Value::String(slot.args.clone()));
        }

        let mut cfg: BTreeMap<String, Value> = BTreeMap::new();
        cfg.insert("oslist".into(), Value::String("windows".into()));
        entry.insert("config".into(), Value::Dict(cfg));

        launch.insert(i.to_string(), Value::Dict(entry));
    }

    set_nested(
        &mut record.sections,
        &["appinfo", "config", "launch"],
        Value::Dict(launch),
    );
}

fn show_message(title: &str, msg: &str) {
    if msgbox::create(title, msg, msgbox::IconType::Info).is_err() {
        eprintln!("[{title}] {msg}");
    }
}

fn show_error(title: &str, msg: &str) {
    if msgbox::create(title, msg, msgbox::IconType::Error).is_err() {
        eprintln!("[{title}] {msg}");
    }
}

fn main() {
    let mode = parse_mode();

    let steam_root = find_steam_root().unwrap_or_else(|| {
        show_error("kh-launchoptions", "Could not locate Steam installation.");
        std::process::exit(1);
    });

    let steamapps_dirs = find_all_steamapps(&steam_root);
    if steamapps_dirs.is_empty() {
        show_error("kh-launchoptions", "No steamapps directories found.");
        std::process::exit(1);
    }

    let install_dir = find_game_install(&steamapps_dirs).unwrap_or_else(|| {
        show_error(
            "kh-launchoptions",
            &format!(
                "Could not find appmanifest_{APP_ID_STR}.acf in any Steam library.\nIs Kingdom Hearts HD 1.5+2.5 ReMIX installed?"
            ),
        );
        std::process::exit(1);
    });

    let (slots, skipped) = match mode {
        Mode::Direct         => build_direct_slots(&install_dir),
        Mode::LauncherConfig => build_launcher_config_slots(&install_dir),
    };

    let vdf_path = appinfo_path(&steam_root);
    let data = std::fs::read(&vdf_path).unwrap_or_else(|e| {
        show_error("kh-launchoptions", &format!("Failed to read appinfo.vdf:\n{e}"));
        std::process::exit(1);
    });

    let mut appinfo = Appinfo::from_bytes(data).unwrap_or_else(|e| {
        show_error("kh-launchoptions", &format!("Failed to parse appinfo.vdf:\n{e}"));
        std::process::exit(1);
    });

    let mut record = appinfo.read_app(APP_ID).unwrap_or_else(|e| {
        show_error("kh-launchoptions", &format!("App {APP_ID} not found in appinfo.vdf:\n{e}"));
        std::process::exit(1);
    });

    apply_slots(&mut record, &slots);

    appinfo.write_app(&record).unwrap_or_else(|e| {
        show_error("kh-launchoptions", &format!("Failed to encode record:\n{e}"));
        std::process::exit(1);
    });

    std::fs::write(&vdf_path, appinfo.data()).unwrap_or_else(|e| {
        show_error("kh-launchoptions", &format!("Failed to write appinfo.vdf:\n{e}"));
        std::process::exit(1);
    });

    // Build result message
    let written_list = slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if s.args.is_empty() {
                format!("  [{i}] {}", s.description)
            } else {
                format!("  [{i}] {} ({})", s.description, s.args)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let skipped_section = if skipped.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nSkipped (exe not found):\n{}",
            skipped.iter().map(|s| format!("  - {s}")).collect::<Vec<_>>().join("\n")
        )
    };

    show_message(
        "kh-launchoptions",
        &format!(
            "Wrote {} launch option{} for app {APP_ID}.\nInstall dir: {}\n\n{written_list}{skipped_section}",
            slots.len(),
            if slots.len() == 1 { "" } else { "s" },
            install_dir.display(),
        ),
    );
}