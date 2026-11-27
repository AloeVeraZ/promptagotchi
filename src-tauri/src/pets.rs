//! Pet discovery + loading.
//!
//! A pet is a folder with a pet.json. Two flavours are understood:
//!   1. Promptagotchi pets – { "animations": { "idle": …, "working": … } } with text-pixel frames,
//!      image frames, sprite-sheet rows or an animated GIF/APNG/WebP per state.
//!   2. Codex / Petdex pets – { "spritesheetPath": "spritesheet.webp" } on the 8×9 192×208 atlas.
//!
//! Pets are looked up in (later wins on id clashes): app → ~/.codex/pets → ~/.petdex/pets → ~/.promptagotchi/pets
use crate::paths;
use crate::util::{dec, enc};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const STATES: [&str; 14] = ["idle", "working", "question", "done", "error", "sleep", "drag", "dragLeft", "dragRight", "wave", "jump", "look", "stretch", "hit"];
const ONCE: [&str; 5] = ["wave", "jump", "look", "stretch", "hit"];

// Codex atlas rows (8 columns × 9 rows, 192×208 cells). Durations in ms.
const CODEX_ROWS: [(&str, u32, &[f64], bool); 9] = [
    ("idle", 0, &[280.0, 110.0, 110.0, 140.0, 140.0, 320.0], true),
    ("dragRight", 1, &[120.0, 120.0, 120.0, 120.0, 120.0, 120.0, 120.0, 220.0], true),
    ("dragLeft", 2, &[120.0, 120.0, 120.0, 120.0, 120.0, 120.0, 120.0, 220.0], true),
    ("wave", 3, &[140.0, 140.0, 140.0, 280.0], false),
    ("jump", 4, &[140.0, 140.0, 140.0, 140.0, 280.0], false),
    ("error", 5, &[140.0, 140.0, 140.0, 140.0, 140.0, 140.0, 140.0, 240.0], true),
    ("question", 6, &[150.0, 150.0, 150.0, 150.0, 150.0, 260.0], true),
    ("working", 7, &[120.0, 120.0, 120.0, 120.0, 120.0, 220.0], true),
    ("done", 8, &[150.0, 150.0, 150.0, 150.0, 150.0, 280.0], true),
];

pub fn sources() -> Vec<(&'static str, PathBuf)> {
    let p = paths::get();
    vec![("builtin", p.builtin_pets.clone()), ("codex", p.codex_pets()), ("petdex", p.petdex_pets()), ("user", p.user_pets())]
}

fn read_json(file: &Path) -> Result<Value, String> {
    let raw = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty())
}

pub fn list_pets_in(sources: &[(&'static str, PathBuf)]) -> Vec<Value> {
    let mut by_key: Vec<(String, Value)> = Vec::new();
    for (source, dir) in sources {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut names: Vec<_> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            let pet_dir = dir.join(&name);
            let file = pet_dir.join("pet.json");
            if !file.exists() {
                continue;
            }
            let v = match read_json(&file) {
                Ok(j) => {
                    let id = str_of(&j, "id").unwrap_or(&name).to_string();
                    json!({
                        "key": format!("{source}:{id}"), "id": id, "source": source, "dir": pet_dir.display().to_string(), "folder": name,
                        "name": str_of(&j, "name").or(str_of(&j, "displayName")).unwrap_or(&id),
                        "description": str_of(&j, "description").unwrap_or(""), "author": str_of(&j, "author").unwrap_or(""),
                        "format": if j.get("spritesheetPath").is_some() { "codex" } else { "promptagotchi" },
                    })
                }
                Err(e) => json!({ "key": format!("{source}:{name}"), "id": name, "source": source, "dir": pet_dir.display().to_string(), "folder": name, "name": name, "error": format!("pet.json: {e}") }),
            };
            let key = v["key"].as_str().unwrap_or_default().to_string();
            by_key.retain(|(k, _)| *k != key);
            by_key.push((key, v));
        }
    }
    by_key.into_iter().map(|(_, v)| v).collect()
}

pub fn list_pets() -> Vec<Value> {
    list_pets_in(&sources())
}

pub fn find_pet(key_or_id: &str) -> Option<Value> {
    let all = list_pets();
    all.iter().find(|p| p["key"] == key_or_id).cloned().or_else(|| all.iter().filter(|p| p["id"] == key_or_id).last().cloned())
}

/// URL the UI uses for a file inside a pet folder (served by the pet:// protocol).
pub fn asset_url(source: &str, folder: &str, rel: &str) -> String {
    format!("pet://local/{}/{}/{}", enc(source), enc(folder), rel.split('/').map(enc).collect::<Vec<_>>().join("/"))
}

/// pet://local/<source>/<folder>/<file…> → a file, refusing anything outside the pet folder.
pub fn resolve_asset_url(url: &str) -> Option<PathBuf> {
    let rest = url.split("://").nth(1)?;
    let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    let parts: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(dec).collect();
    if parts.len() < 3 || parts.iter().any(|p| p == ".." || p == "." || p.contains('/')) {
        return None;
    }
    let (_, dir) = sources().into_iter().find(|(s, _)| *s == parts[0])?;
    let base = dir.join(&parts[1]);
    let file = parts[2..].iter().fold(base.clone(), |p, seg| p.join(seg));
    file.starts_with(&base).then_some(file)
}

fn timing(anim: &Value, fallback_fps: Option<f64>, count: usize) -> Vec<f64> {
    if let Some(d) = anim["durations"].as_array().filter(|d| !d.is_empty()) {
        let mut out: Vec<f64> = d.iter().map(|x| x.as_f64().unwrap_or(100.0).max(16.0)).collect();
        while out.len() < count {
            out.push(*out.last().unwrap());
        }
        out.truncate(count);
        return out;
    }
    let fps = anim["fps"].as_f64().or(fallback_fps).filter(|f| *f > 0.0).unwrap_or(6.0);
    vec![(1000.0 / fps).round(); count]
}

/// Rows where the pet actually has pixels across all idle frames: (top, bottom).
pub fn pixel_content(anim: Option<&Value>, palette: &Map<String, Value>, fh: usize) -> Option<(usize, usize)> {
    let frames = anim?.get("frames")?.as_array()?;
    let (mut top, mut bottom) = (usize::MAX, None);
    for f in frames {
        for (y, line) in f.as_array()?.iter().enumerate().take(fh) {
            let painted = line.as_str().unwrap_or("").chars().any(|ch| {
                ch != '.' && ch != ' ' && palette.get(&ch.to_string()).and_then(|c| c.as_str()).map(|c| c != "transparent").unwrap_or(false)
            });
            if painted {
                top = top.min(y);
                bottom = Some(bottom.map_or(y, |b: usize| b.max(y)));
            }
        }
    }
    bottom.map(|b| (top, b))
}

/// Load a pet into one normalized shape the UI can play.
pub fn load_pet(key_or_id: &str) -> Result<Value, String> {
    let pet = find_pet(key_or_id).ok_or_else(|| format!("No pet called \"{key_or_id}\""))?;
    if let Some(e) = pet["error"].as_str() {
        return Err(e.to_string());
    }
    let dir = PathBuf::from(pet["dir"].as_str().unwrap_or_default());
    let j = read_json(&dir.join("pet.json"))?;
    let source = pet["source"].as_str().unwrap_or("user").to_string();
    let folder = pet["folder"].as_str().unwrap_or_default().to_string();
    let url = |rel: &str| asset_url(&source, &folder, rel);
    let mut out = pet.clone();
    let mut warnings: Vec<String> = Vec::new();

    if let Some(sheet) = str_of(&j, "spritesheetPath") {
        let mut animations = Map::new();
        for (state, row, durations, looped) in CODEX_ROWS {
            animations.insert(state.into(), json!({ "type": "sheet", "url": url(sheet), "row": row, "col": 0, "count": durations.len(), "durations": durations, "loop": looped }));
        }
        out["format"] = json!("codex");
        out["frameWidth"] = json!(192);
        out["frameHeight"] = json!(208);
        out["pixelArt"] = json!(j["pixelArt"].as_bool().unwrap_or(false));
        out["palette"] = json!({});
        out["animations"] = Value::Object(animations);
        out["content"] = Value::Null;
        out["warnings"] = json!(warnings);
        return Ok(out);
    }

    let fw = j["frameWidth"].as_u64().unwrap_or(32) as usize;
    let fh = j["frameHeight"].as_u64().unwrap_or(32) as usize;
    let palette = j["palette"].as_object().cloned().unwrap_or_default();
    let fps = j["fps"].as_f64();
    let empty = Map::new();
    let anims = j["animations"].as_object().unwrap_or(&empty);
    if !anims.contains_key("idle") {
        warnings.push("Every pet needs an \"idle\" animation.".into());
    }
    let mut animations = Map::new();
    for (state, a) in anims {
        if !STATES.contains(&state.as_str()) {
            warnings.push(format!("Unknown state \"{state}\" (known: {}). It will be ignored.", STATES.join(", ")));
        }
        let looped = a["loop"].as_bool().unwrap_or(!ONCE.contains(&state.as_str()));
        let v = if let Some(img) = a["image"].as_str() {
            json!({ "type": "gif", "url": url(img), "loop": looped, "durations": [a["duration"].as_f64().unwrap_or(1200.0)] })
        } else if !a["sheet"].is_null() {
            let sh = if a["sheet"].is_string() { json!({ "image": a["sheet"] }) } else { a["sheet"].clone() };
            let count = sh["count"].as_u64().or(a["count"].as_u64()).unwrap_or(1) as usize;
            json!({ "type": "sheet", "url": url(sh["image"].as_str().unwrap_or("")), "row": sh["row"].as_u64().unwrap_or(0), "col": sh["col"].as_u64().unwrap_or(0), "count": count, "durations": timing(a, fps, count), "loop": looped })
        } else if let Some(frames) = a["frames"].as_array().filter(|f| !f.is_empty()) {
            if frames[0].is_string() {
                json!({ "type": "images", "urls": frames.iter().map(|f| url(f.as_str().unwrap_or(""))).collect::<Vec<_>>(), "durations": timing(a, fps, frames.len()), "loop": looped })
            } else {
                for (i, rows) in frames.iter().enumerate() {
                    let Some(rows) = rows.as_array() else {
                        warnings.push(format!("{state} frame {} should be a list of text rows.", i + 1));
                        continue;
                    };
                    if rows.len() != fh {
                        warnings.push(format!("{state} frame {} has {} rows (frameHeight is {fh}).", i + 1, rows.len()));
                    }
                    for (y, r) in rows.iter().enumerate() {
                        let r = r.as_str().unwrap_or("");
                        if r.chars().count() != fw {
                            warnings.push(format!("{state} frame {} row {} is {} wide (frameWidth is {fw}).", i + 1, y + 1, r.chars().count()));
                        }
                        if let Some(ch) = r.chars().find(|ch| *ch != '.' && *ch != ' ' && !palette.contains_key(&ch.to_string())) {
                            warnings.push(format!("{state} frame {} uses \"{ch}\", which isn't in the palette.", i + 1));
                        }
                    }
                }
                json!({ "type": "pixels", "frames": frames, "durations": timing(a, fps, frames.len()), "loop": looped })
            }
        } else {
            warnings.push(format!("\"{state}\" needs \"frames\", \"sheet\" or \"image\"."));
            continue;
        };
        animations.insert(state.clone(), v);
    }
    warnings.dedup();
    warnings.truncate(30);
    out["format"] = json!("promptagotchi");
    out["frameWidth"] = json!(fw);
    out["frameHeight"] = json!(fh);
    out["pixelArt"] = json!(j["pixelArt"].as_bool() != Some(false));
    out["content"] = match pixel_content(anims.get("idle"), &palette, fh) {
        Some((t, b)) => json!({ "top": t, "bottom": b }),
        None => Value::Null,
    };
    out["palette"] = Value::Object(palette);
    out["animations"] = Value::Object(animations);
    out["warnings"] = json!(warnings);
    Ok(out)
}

/// Raw pet.json (for the Editor).
pub fn read_pet_json(key: &str) -> Result<(Value, Value), String> {
    let pet = find_pet(key).ok_or_else(|| format!("No pet called \"{key}\""))?;
    let j = read_json(&Path::new(pet["dir"].as_str().unwrap_or_default()).join("pet.json"))?;
    Ok((pet, j))
}

/// Save a pet.json into ~/.promptagotchi/pets/<id>. Returns the pet key.
pub fn save_pet_json(mut j: Value) -> Result<String, String> {
    let raw = j["id"].as_str().unwrap_or("").to_lowercase();
    let id: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect::<String>().trim_matches('-').to_string();
    if id.is_empty() {
        return Err("Give your pet an id (letters, numbers, dashes).".into());
    }
    j["id"] = json!(id);
    let dir = paths::get().user_pets().join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("pet.json"), serde_json::to_string_pretty(&j).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    Ok(format!("user:{id}"))
}

/// Copy a pet folder into ~/.promptagotchi/pets.
pub fn install_pet_folder(src: &Path) -> Result<String, String> {
    let j = read_json(&src.join("pet.json")).map_err(|_| "That folder has no pet.json.".to_string())?;
    let fallback = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "pet".into());
    let id: String = str_of(&j, "id").unwrap_or(&fallback).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    crate::util::copy_dir(src, &paths::get().user_pets().join(&id)).map_err(|e| e.to_string())?;
    Ok(format!("user:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pets_load_without_warnings() {
        let all = list_pets_in(&[("builtin", paths::get().builtin_pets.clone())]);
        assert!(all.len() >= 3, "pip, mochi, volt");
        for p in all {
            let n = load_pet(p["key"].as_str().unwrap()).unwrap();
            assert_eq!(n["warnings"], json!([]), "{}", p["key"]);
            assert!(n["animations"]["idle"].is_object());
            assert!(n["content"].is_object(), "text-pixel pets know where their body is");
        }
    }

    #[test]
    fn pet_urls_cannot_escape_the_pet_folder() {
        assert!(resolve_asset_url("pet://local/builtin/pip/../../package.json").is_none());
        assert!(resolve_asset_url("pet://local/builtin/pip/%2E%2E/x").is_none());
        assert!(resolve_asset_url("pet://local/nope/pip/a.png").is_none());
        assert!(resolve_asset_url("pet://local/builtin/pip/pet.json").unwrap().ends_with("pets/pip/pet.json"));
        assert_eq!(resolve_asset_url(&asset_url("builtin", "pip", "pet.json")), resolve_asset_url("pet://local/builtin/pip/pet.json"));
    }
}
