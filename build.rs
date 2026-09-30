extern crate winres;

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;

/// Reads a `const NAME: &str = "..."` literal straight out of src/canary.rs, so
/// the source stays the single source of truth for the build script, the binary
/// and tools/canary_check.py alike (same pattern as LICENSE_BASE_SECRET).
fn const_from_canary_rs(src: &str, name: &str) -> String {
    let needle = format!("pub const {}: &str = \"", name);
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("{} not found in src/canary.rs", name))
        + needle.len();
    let rest = &src[start..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated {} literal in src/canary.rs", name));
    rest[..end].to_string()
}

/// Bakes `plain` into `OUT_DIR/<out_name>` as an AES-256-GCM blob under the
/// given const prefix, with a key and nonce drawn fresh for this build.
///
/// Two private things need exactly this treatment - the relay credential and the
/// built-in exit list - and a second copy of the cipher setup is a second place
/// to get it wrong. Neither the key nor the plaintext is in any committed source;
/// the honest ceiling is unchanged (D8), this keeps them off a `strings` dump.
fn bake_secret(out_name: &str, prefix: &str, plain: &str) {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut key).expect("random key");
    getrandom::getrandom(&mut nonce).expect("random nonce");
    let ct = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
        .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
        .expect("encrypt build secret");
    let bytes = |b: &[u8]| {
        b.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR unset")).join(out_name);
    fs::write(
        &out,
        format!(
            "pub const {p}_KEY: [u8; 32] = [{}];\n\
             pub const {p}_NONCE: [u8; 12] = [{}];\n\
             pub const {p}_CT: &[u8] = &[{}];\n",
            bytes(&key),
            bytes(&nonce),
            bytes(&ct),
            p = prefix
        ),
    )
    .unwrap_or_else(|e| panic!("failed to write {}: {}", out_name, e));
}

/// Must stay identical to canary::token_for() and to tools/canary_check.py.
fn token_for(seed: &str, sep: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.update(sep.as_bytes());
    hasher.update(version.as_bytes());
    let hex = hex::encode(hasher.finalize()).to_uppercase();
    format!("OAG-{}-{}-{}", &hex[0..5], &hex[5..10], &hex[10..15])
}

/// Automatically generates `assets/icon_64.rgba` and `icon.ico` from a single `icon.png` (or `assets/icon.png`).
/// If the user drops a new PNG file, cargo automatically re-runs this step and regenerates all required formats.
fn process_icon_assets() {
    println!("cargo:rerun-if-changed=icon.png");
    println!("cargo:rerun-if-changed=assets/icon.png");

    let root_icon = Path::new("icon.png");
    let asset_icon = Path::new("assets/icon.png");
    let target_rgba = Path::new("assets/icon_64.rgba");
    let target_ico = Path::new("icon.ico");
    let docs_icon = Path::new("docs/icon.png");

    // Whichever file exists or is newer takes priority
    let src_png = if root_icon.exists() {
        if let Ok(root_bytes) = fs::read(root_icon) {
            if !asset_icon.exists() || fs::read(asset_icon).map_or(true, |b| b != root_bytes) {
                let _ = fs::write(asset_icon, &root_bytes);
            }
            if docs_icon.parent().map_or(false, |p| p.exists()) {
                if !docs_icon.exists() || fs::read(docs_icon).map_or(true, |b| b != root_bytes) {
                    let _ = fs::write(docs_icon, &root_bytes);
                }
            }
        }
        root_icon
    } else if asset_icon.exists() {
        if let Ok(asset_bytes) = fs::read(asset_icon) {
            if !root_icon.exists() || fs::read(root_icon).map_or(true, |b| b != asset_bytes) {
                let _ = fs::write(root_icon, &asset_bytes);
            }
            if docs_icon.parent().map_or(false, |p| p.exists()) {
                if !docs_icon.exists() || fs::read(docs_icon).map_or(true, |b| b != asset_bytes) {
                    let _ = fs::write(docs_icon, &asset_bytes);
                }
            }
        }
        asset_icon
    } else {
        return;
    };

    let src_metadata = match fs::metadata(src_png) {
        Ok(m) => m,
        Err(_) => return,
    };
    let src_mtime = src_metadata.modified().ok();

    let rgba_needs_update = !target_rgba.exists()
        || src_mtime
            .and_then(|sm| {
                fs::metadata(target_rgba)
                    .ok()
                    .and_then(|tm| tm.modified().ok())
                    .map(|tm| sm > tm)
            })
            .unwrap_or(true);

    let ico_needs_update = !target_ico.exists()
        || src_mtime
            .and_then(|sm| {
                fs::metadata(target_ico)
                    .ok()
                    .and_then(|tm| tm.modified().ok())
                    .map(|tm| sm > tm)
            })
            .unwrap_or(true);

    if !rgba_needs_update && !ico_needs_update {
        return;
    }

    let png_bytes = match fs::read(src_png) {
        Ok(b) => b,
        Err(e) => {
            println!("cargo:warning=failed to read {}: {}", src_png.display(), e);
            return;
        }
    };

    let dyn_img = match image::load_from_memory(&png_bytes) {
        Ok(img) => img,
        Err(e) => {
            println!("cargo:warning=failed to decode icon PNG: {}", e);
            return;
        }
    };

    // 1. Generate assets/icon_64.rgba (64x64 raw RGBA bytes)
    if rgba_needs_update {
        let img64 = dyn_img.resize_exact(64, 64, image::imageops::FilterType::Lanczos3).to_rgba8();
        let bytes = img64.as_raw();
        if let Err(e) = fs::write(target_rgba, bytes) {
            println!("cargo:warning=failed to write {}: {}", target_rgba.display(), e);
        } else {
            println!("cargo:warning=generated {} from {}", target_rgba.display(), src_png.display());
        }
    }

    // 2. Generate multi-layer icon.ico (16, 32, 48, 64, 128, 256)
    if ico_needs_update {
        use image::ImageEncoder;
        let sizes: &[u32] = &[16, 32, 48, 64, 128, 256];
        let mut png_blobs: Vec<(u32, Vec<u8>)> = Vec::new();

        for &sz in sizes {
            let resized = dyn_img.resize_exact(sz, sz, image::imageops::FilterType::Lanczos3);
            let mut buf = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut buf);
            if encoder.write_image(resized.to_rgba8().as_raw(), sz, sz, image::ExtendedColorType::Rgba8).is_ok() {
                png_blobs.push((sz, buf));
            }
        }

        if !png_blobs.is_empty() {
            let mut ico_data = Vec::new();
            let count = png_blobs.len() as u16;

            // ICO Header: 6 bytes
            ico_data.extend_from_slice(&0u16.to_le_bytes()); // Reserved
            ico_data.extend_from_slice(&1u16.to_le_bytes()); // Type = 1 (Icon)
            ico_data.extend_from_slice(&count.to_le_bytes()); // Count

            let mut offset = 6 + (count as usize) * 16;
            for (sz, blob) in &png_blobs {
                let w_byte = if *sz >= 256 { 0u8 } else { *sz as u8 };
                let h_byte = if *sz >= 256 { 0u8 } else { *sz as u8 };
                ico_data.push(w_byte);
                ico_data.push(h_byte);
                ico_data.push(0); // Colors
                ico_data.push(0); // Reserved
                ico_data.extend_from_slice(&1u16.to_le_bytes()); // Planes
                ico_data.extend_from_slice(&32u16.to_le_bytes()); // BPP
                ico_data.extend_from_slice(&(blob.len() as u32).to_le_bytes()); // Size
                ico_data.extend_from_slice(&(offset as u32).to_le_bytes()); // Offset
                offset += blob.len();
            }

            for (_, blob) in &png_blobs {
                ico_data.extend_from_slice(blob);
            }

            if let Err(e) = fs::write(target_ico, &ico_data) {
                println!("cargo:warning=failed to write {}: {}", target_ico.display(), e);
            } else {
                println!("cargo:warning=generated {} from {}", target_ico.display(), src_png.display());
            }
        }
    }
}

fn main() {
    process_icon_assets();

    println!("cargo:rerun-if-changed=src/canary.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=AG_FULL_VERSION");
    println!("cargo:rerun-if-env-changed=AG_BUILTIN_PROXY");
    println!("cargo:rerun-if-env-changed=AG_PORTABLE");
    println!("cargo:rerun-if-env-changed=AG_UPDATE_URL");

    // URL проверки обновлений (AG_UPDATE_URL=https://host/version.json).
    // По умолчанию используется GitHub Pages репозитория OpenAntigravity:
    // https://keelbismark.github.io/OpenAntigravity/version.json
    // Вшивается в бинарник через rustc-env и считывается через option_env! в update.rs.
    // Может быть переопределён переменной AG_UPDATE_URL ("none" / "off" полностью отключает).
    const DEFAULT_UPDATE_URL: &str = "https://keelbismark.github.io/OpenAntigravity/version.json";
    let effective_update_url = match env::var("AG_UPDATE_URL") {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("off") {
                None
            } else if trimmed.is_empty() {
                Some(DEFAULT_UPDATE_URL.to_string())
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => Some(DEFAULT_UPDATE_URL.to_string()),
    };

    if let Some(url) = effective_update_url {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            println!(
                "cargo:warning=AG_UPDATE_URL не начинается с http:// или https:// — \n                 такой адрес парсер отвергнет: {}",
                url.split('/').next().unwrap_or("")
            );
        } else {
            println!("cargo:rustc-env=AG_UPDATE_URL={}", url);
            println!("cargo:warning=update feed -> {}", url);
        }
    } else {
        println!("cargo:warning=проверка обновлений выключена (AG_UPDATE_URL=none)");
    }

    // Вшитый прокси (AG_BUILTIN_PROXY=логин:пароль@хост:порт). Строка не должна
    // лежать в exe открытым текстом — пароль от своего прокси в чужих руках это
    // чужой трафик через ваш счёт. Ниже: проверка формата (лог сборки содержит
    // только хост:порт — не пароль) и XOR-обфускация в OUT_DIR (ключ рядом:
    // от целенаправленного реверса это не защита, только от strings-дампа;
    // тот же уровень честности, что и у остальной сборки).

    // Вшитый прокси (AG_BUILTIN_PROXY=логин:пароль@хост:порт): проверяем
    // формат уже на сборке, чтобы опечатка не превратилась в молчаливое
    // «маршрута нет». В лог сборки попадает только хост:порт — не пароль.
    if let Some(raw) = env::var_os("AG_BUILTIN_PROXY") {
        let raw = raw.to_string_lossy().trim().to_string();
        if raw.is_empty() {
            println!("cargo:warning=AG_BUILTIN_PROXY пуст — сборка без вшитого прокси (пользователь укажет свой в настройках)");
        } else {
            let proxies: Vec<&str> = raw
                .split(';')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            for (idx, p) in proxies.iter().enumerate() {
                let rest = p
                    .trim_start_matches("http://")
                    .trim_start_matches("https://");
                let (auth, hostport) = match rest.rsplit_once('@') {
                    Some((a, hp)) => (Some(a), hp),
                    None => (None, rest),
                };
                let port_ok = hostport
                    .rsplit_once(':')
                    .and_then(|(_, port)| port.parse::<u16>().ok())
                    .is_some_and(|p| p > 0);
                let auth_ok = auth.is_none_or(|a| a.contains(':'));
                let label = if proxies.len() > 1 {
                    format!("builtin own-proxy [{}/{}]", idx + 1, proxies.len())
                } else {
                    "builtin own-proxy".to_string()
                };
                if port_ok && auth_ok {
                    println!("cargo:warning={} -> {} (порт и логин:пароль в порядке)", label, hostport);
                } else {
                    println!(
                        "cargo:warning={} «{}» не похож на 'логин:пароль@хост:порт' — релей такой адрес отвергнет (нужен HTTP-прокси с CONNECT)",
                        label, p
                    );
                }
            }
        }
    }

    // XOR-обфускация вшитого прокси в OUT_DIR/builtin_proxy_gen.rs. Файл пишется
    // всегда (пустая сборка — пустой массив), чтобы include! в upstream.rs был
    // валиден без переменной тоже.
    let proxy_raw = env::var("AG_BUILTIN_PROXY")
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut xor_key = [0u8; 8];
    getrandom::getrandom(&mut xor_key).expect("random xor key");
    let enc: Vec<u8> = proxy_raw
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ xor_key[i % xor_key.len()])
        .collect();
    let bytes_lit = |b: &[u8]| {
        b.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let proxy_out =
        Path::new(&env::var("OUT_DIR").expect("OUT_DIR unset")).join("builtin_proxy_gen.rs");
    fs::write(
        &proxy_out,
        format!(
            "pub const BUILTIN_PROXY_ENC: [u8; {}] = [{}];\n\
             pub const BUILTIN_PROXY_KEY: [u8; 8] = [{}];\n",
            enc.len(),
            bytes_lit(&enc),
            bytes_lit(&xor_key)
        ),
    )
    .expect("failed to write builtin_proxy_gen.rs");

    println!("cargo:rerun-if-changed=VERSION");
    let file_ver = fs::read_to_string("VERSION")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "1.1.0".to_string());
    let full_ver = env::var("AG_FULL_VERSION").unwrap_or(file_ver);
    let trimmed = full_ver.trim();
    if !trimmed.is_empty() {
        println!("cargo:rustc-env=AG_FULL_VERSION={}", trimmed);
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let canary_src = fs::read_to_string("src/canary.rs").expect("src/canary.rs is missing");
    let seed = const_from_canary_rs(&canary_src, "CANARY_SEED");
    let sep = const_from_canary_rs(&canary_src, "CANARY_SEP");
    let static_canary = const_from_canary_rs(&canary_src, "STATIC_CANARY");
    let release_token = token_for(&seed, &sep, &version);

    // Emitted for canary.rs to include!(), so the token itself is a literal in
    // .rdata rather than something computed at runtime - a plain `strings` dump
    // of an unpacked binary shows it.
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR unset")).join("canary_gen.rs");
    fs::write(
        &out,
        format!(
            "/// Canary for this exact release, derived from CANARY_SEED + version.\n\
             pub const RELEASE_TOKEN: &str = \"{}\";\n",
            release_token
        ),
    )
    .expect("failed to write canary_gen.rs");

    // Visible to anyone running the build, and to build_rust.py's release log.
    println!(
        "cargo:warning=release canary {} (v{})",
        release_token, version
    );

    // The fast relay route's whole method lives in the gitignored `src/relay.rs`
    // and is compiled in only when it AND the credential file are present (owner
    // build) - the `relay` cfg. A public clone lacks both, builds under
    // `cfg(not(relay))`, and runs the DNS route. The credential is AES-256-GCM
    // encrypted here with a random per-build key; neither the key nor the method
    // is in any committed source. This is not un-reversible (agy's AES-GCM vault
    // was reversed, and so can an official binary): it keeps the method off the
    // public repo and off a `strings` dump of the packed binary.
    println!("cargo::rustc-check-cfg=cfg(relay)");
    println!("cargo:rerun-if-changed=.relay_key");
    println!("cargo:rerun-if-changed=src/relay.rs");
    let relay = Path::new("src/relay.rs").exists() && Path::new(".relay_key").exists();
    if relay {
        println!("cargo:rustc-cfg=relay");
        let cred = fs::read_to_string(".relay_key")
            .expect("read .relay_key")
            .trim()
            .to_string();
        bake_secret("relay_gen.rs", "RELAY", &cred);
    }

    // The built-in exits - third-party CONNECT proxies that already come out in a
    // permitted region, so they lift the gate with no DNS trickery at all. Same
    // arrangement as the relay for the same reason: the *method* is unremarkable
    // (a plain CONNECT, which upstream.rs already speaks in public source), so
    // what is private is the address list. `.exits` holds it, `src/exits.rs` uses
    // it, both are gitignored, and a public clone compiles `cfg(not(exits))` and
    // takes the routes below.
    println!("cargo::rustc-check-cfg=cfg(exits)");
    println!("cargo:rerun-if-changed=.exits");
    println!("cargo:rerun-if-changed=src/exits.rs");
    let exits = Path::new("src/exits.rs").exists() && Path::new(".exits").exists();
    if exits {
        println!("cargo:rustc-cfg=exits");
        let list = fs::read_to_string(".exits").expect("read .exits");
        // Comments and blank lines are stripped here rather than at runtime, so
        // nothing but addresses is ever inside the binary.
        let cleaned = list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        if cleaned.is_empty() {
            panic!(".exits exists but lists no addresses");
        }
        bake_secret("exits_gen.rs", "EXITS", &cleaned);
    }

    // Which private routes this build actually got. Printed because the failure
    // mode is silent: a release built with one of the files missing works fine
    // and is simply slower, which is exactly the kind of thing that ships.
    println!(
        "cargo:warning=private routes: relay={} exits={}",
        relay, exits
    );

    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        res.set("FileDescription", "Open Antigravity");
        res.set("ProductName", "Open Antigravity");
        res.set("LegalCopyright", "keelbismark");
        // Copyright management information in the version resource: survives
        // UPX (the resource directory stays readable on the packed file) and is
        // visible in the file's Properties dialog without any tooling.
        res.set(
            "LegalTrademarks",
            "Open Antigravity (c) 2026 keelbismark - github.com/keelbismark/OpenAntigravity",
        );
        res.set(
            "Comments",
            &format!(
                "Open Antigravity (c) 2026 keelbismark. \
                 Origin: github.com/keelbismark/OpenAntigravity. \
                 mark {} build {}",
                static_canary, release_token
            ),
        );
        // Kept in step with Cargo.toml, which build_rust.py rewrites per release.
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        // AG_NO_RES: кросс-проверка типов с Linux (`cargo check --target
        // x86_64-pc-windows-gnu`) — компилятор ресурсов требует rc.exe/windres,
        // которых на другой ОС нет; на обычной сборке под Windows ничего не меняет.
        if env::var_os("AG_NO_RES").is_none() {
            res.compile().unwrap();
        }
    }
}
