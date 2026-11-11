//! Game covers from the internet, for the games the launcher recognizes.
//!
//! Two sources, neither needing a key:
//! - **Nlib** (`api.nlib.cc/nx/<title ID>/icon`, data from blawar/titledb):
//!   by title ID, so an exact match, but Switch games only.
//! - **Nintendo of Europe's eShop search** (`searching.nintendo-europe.com`):
//!   by name, Switch 2 games too. A result counts only when it is a game of
//!   the same console and its name matches, so a wrong cover is never taken.
//!
//! Requests go through `/usr/bin/curl`, as the launcher runs `sips` and `git`:
//! no HTTP library to build in. They send the title ID (Nlib) or the game's
//! name (Nintendo), nothing else; Preferences can turn them off.

use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const USER_AGENT: &str = "NX-Launcher (game covers)";
const NLIB: &str = "https://api.nlib.cc/nx";
const EU_SEARCH: &str = "https://searching.nintendo-europe.com/en/select";
/// No cover is bigger than this; a larger answer is not an image we want.
const MAX_BYTES: &str = "5000000";

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Nlib,
    NintendoEu,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Console {
    Switch,
    Switch2,
}

/// A base game's title ID starts with 01 on the Switch, 04 on the Switch 2.
pub fn console(title_id: &str) -> Console {
    if title_id.starts_with("04") { Console::Switch2 } else { Console::Switch }
}

/// A name reduced to lower-case words: no ™ ® ©, punctuation as spaces, so
/// "The Legend of Zelda™: Breath of the Wild" and the shop's spelling meet.
pub fn normalize(name: &str) -> String {
    let words: String = name
        .chars()
        .filter(|c| !matches!(c, '™' | '®' | '©'))
        .map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { ' ' })
        .collect();
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The shop lists a Switch 2 Edition under its own name; the game's own name
/// lacks the suffix.
fn without_edition(normalized: &str) -> &str {
    ["nintendo switch 2 edition", "switch 2 edition"]
        .iter()
        .find_map(|s| normalized.strip_suffix(s).map(str::trim_end))
        .unwrap_or(normalized)
}

fn same_console(doc: &Value, console: Console) -> bool {
    let systems = doc["system_type"].as_array().into_iter().flatten().filter_map(Value::as_str);
    // One entry may hold several types joined by commas.
    systems.flat_map(|s| s.split(',')).any(|s| match console {
        Console::Switch2 => s.starts_with("nintendoswitch2"),
        Console::Switch => s.starts_with("nintendoswitch") && !s.starts_with("nintendoswitch2"),
    })
}

/// Only images from Nintendo's own sites are fetched, whatever the answer says.
fn nintendo_image(url: &str) -> Option<String> {
    let url = if let Some(rest) = url.strip_prefix("//") { format!("https://{rest}") } else { url.to_string() };
    let host = url.strip_prefix("https://")?.split('/').next()?;
    (host == "nintendo.com" || host.ends_with(".nintendo.com") || host.ends_with(".nintendo.net")).then_some(url)
}

/// The square image of the first search result that is this game: same
/// console, same name (or the name plus "Nintendo Switch 2 Edition").
pub fn pick_eu(answer: &Value, name: &str, console: Console) -> Option<String> {
    let want = normalize(name);
    if want.is_empty() {
        return None;
    }
    answer["response"]["docs"].as_array()?.iter().find_map(|doc| {
        let title = normalize(doc["title"].as_str()?);
        let matches = title == want || without_edition(&title) == want;
        if !matches || !same_console(doc, console) {
            return None;
        }
        nintendo_image(doc["image_url_sq_s"].as_str()?)
    })
}

/// What a request brought: a body, or nothing (the source does not know the
/// game). A failure worth trying again later (offline, timeout) is an `Err`.
enum Got {
    Body(Vec<u8>),
    Missing,
}

async fn curl(args: &[&str]) -> Result<Got, String> {
    // The status is printed after the body, not read from curl's exit code:
    // with -f, an HTTP/2 404 ends with 56 ("receive error") as well as 22.
    let out = Command::new("/usr/bin/curl")
        .args(["-sSL", "-m", "20", "--max-filesize", MAX_BYTES, "-A", USER_AGENT, "-w", "\n%{http_code}"])
        .args(args)
        .output()
        .await
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let (body, status) = split_status(out.stdout);
    match status {
        200 => Ok(Got::Body(body)),
        // The source does not know the game (Nlib: 404 for an unknown title ID).
        400..=499 => Ok(Got::Missing),
        s => Err(format!("HTTP {s}")),
    }
}

/// Splits curl's output into the body and the status `-w "\n%{http_code}"` appended.
fn split_status(mut out: Vec<u8>) -> (Vec<u8>, u16) {
    let Some(nl) = out.iter().rposition(|b| *b == b'\n') else { return (out, 0) };
    let status = std::str::from_utf8(&out[nl + 1..]).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    out.truncate(nl);
    (out, status)
}

/// JPEG or PNG, told by the first bytes: the file name the cover is saved as.
fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else {
        None
    }
}

async fn image(url: &str) -> Result<Option<Vec<u8>>, String> {
    Ok(match curl(&[url]).await? {
        Got::Body(b) if image_ext(&b).is_some() => Some(b),
        _ => None,
    })
}

/// Finds a cover: Nlib by title ID for a Switch game, then the eShop search
/// by name. `Ok(None)` when no source knows the game.
pub async fn find(title_id: &str, name: &str) -> Result<Option<(Vec<u8>, Source)>, String> {
    let console = console(title_id);
    if console == Console::Switch {
        if let Some(bytes) = image(&format!("{NLIB}/{title_id}/icon/512/512")).await? {
            return Ok(Some((bytes, Source::Nlib)));
        }
    }
    let q = format!("q={name}");
    let answer = match curl(&[
        "--get", "--data-urlencode", &q, "--data-urlencode", "fq=type:GAME",
        "--data-urlencode", "rows=30", "--data-urlencode", "wt=json", EU_SEARCH,
    ])
    .await?
    {
        Got::Body(body) => serde_json::from_slice::<Value>(&body).map_err(|e| format!("eShop search: {e}"))?,
        Got::Missing => return Ok(None),
    };
    let Some(url) = pick_eu(&answer, name, console) else { return Ok(None) };
    Ok(image(&url).await?.map(|b| (b, Source::NintendoEu)))
}

/// Saves a cover as `cover-<title ID>.jpg|png` in `dir` (the icons folder the
/// web view may read), replacing an older one of the other kind.
pub fn save(dir: &Path, title_id: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let ext = image_ext(bytes).ok_or_else(|| std::io::Error::other("not a JPEG or PNG"))?;
    let path = dir.join(format!("cover-{title_id}.{ext}"));
    let tmp = path.with_extension(format!("{ext}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    for other in ["jpg", "png"].iter().filter(|e| **e != ext) {
        let _ = std::fs::remove_file(dir.join(format!("cover-{title_id}.{other}")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape of `searching.nintendo-europe.com/en/select` answers (trimmed).
    fn answer() -> Value {
        json!({"response": {"numFound": 4, "docs": [
            {"title": "The Legend of Zelda: Breath of the Wild", "system_type": ["wiiu_disc,wiiu_digitaldistribution"],
             "image_url_sq_s": "https://www.nintendo.com/eu/media/images/wiiu.png"},
            {"title": "The Legend of Zelda: Breath of the Wild",
             "system_type": ["nintendoswitch_digitaldistribution,nintendoswitch_gamecard"],
             "image_url_sq_s": "https://www.nintendo.com/eu/media/images/switch.jpg"},
            {"title": "The Legend of Zelda: Breath of the Wild – Nintendo Switch 2 Edition",
             "system_type": ["nintendoswitch2,nintendoswitch2"],
             "image_url_sq_s": "//www.nintendo.com/eu/media/images/switch2.png"},
            {"title": "Pokémon Pokopia", "system_type": ["nintendoswitch2"],
             "image_url_sq_s": "https://evil.example/pokopia.png"}
        ]}})
    }

    #[test]
    fn names_meet_without_marks_or_punctuation() {
        assert_eq!(normalize("The Legend of Zelda™: Breath of the Wild"), "the legend of zelda breath of the wild");
        assert_eq!(normalize("  Pokémon   Pokopia® "), "pokémon pokopia");
        assert_eq!(normalize("™"), "");
    }

    #[test]
    fn the_console_comes_from_the_title_id() {
        assert_eq!(console("01007EF00011E000"), Console::Switch);
        assert_eq!(console("0400486024DEC000"), Console::Switch2);
    }

    #[test]
    fn picks_the_same_game_on_the_same_console() {
        let a = answer();
        let name = "The Legend of Zelda™: Breath of the Wild";
        assert_eq!(pick_eu(&a, name, Console::Switch).as_deref(), Some("https://www.nintendo.com/eu/media/images/switch.jpg"));
        // The Switch 2 Edition, listed under its longer name; a //-URL becomes https.
        assert_eq!(pick_eu(&a, name, Console::Switch2).as_deref(), Some("https://www.nintendo.com/eu/media/images/switch2.png"));
    }

    #[test]
    fn takes_nothing_rather_than_a_wrong_cover() {
        let a = answer();
        assert_eq!(pick_eu(&a, "Breath of the Wild", Console::Switch), None, "a partial name is another game");
        assert_eq!(pick_eu(&a, "Pokémon Pokopia", Console::Switch2), None, "an image outside Nintendo's sites");
        assert_eq!(pick_eu(&a, "™", Console::Switch), None);
        assert_eq!(pick_eu(&json!({"error": "x"}), "Zelda", Console::Switch), None);
    }

    #[test]
    fn the_status_comes_off_the_end_of_the_body() {
        let (body, status) = split_status(b"\x89PNG\n\x00binary\n200".to_vec());
        assert_eq!((body.as_slice(), status), (&b"\x89PNG\n\x00binary"[..], 200));
        assert_eq!(split_status(b"\n404".to_vec()), (Vec::new(), 404));
        assert_eq!(split_status(b"garbage".to_vec()).1, 0);
    }

    #[test]
    fn only_nintendo_hosts() {
        assert!(nintendo_image("https://img-eshop.cdn.nintendo.net/i/x.jpg").is_some());
        assert!(nintendo_image("https://www.nintendo.com/eu/x.png").is_some());
        assert!(nintendo_image("http://www.nintendo.com/x.png").is_none());
        assert!(nintendo_image("https://nintendo.com.evil.example/x.png").is_none());
        assert!(nintendo_image("https://evilnintendo.com/x.png").is_none());
    }

    #[test]
    fn saves_by_image_kind_and_replaces_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let png = b"\x89PNG\r\n\x1a\nrest";
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2];
        let p = save(dir.path(), "0400486024DEC000", png).unwrap();
        assert!(p.ends_with("cover-0400486024DEC000.png"));
        let j = save(dir.path(), "0400486024DEC000", &jpg).unwrap();
        assert!(j.ends_with("cover-0400486024DEC000.jpg") && j.is_file() && !p.exists());
        assert!(save(dir.path(), "0400486024DEC000", b"<html>").is_err());
    }

    /// Asks the real services (network): `cargo test covers -- --ignored`.
    #[test]
    #[ignore]
    fn live_sources() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(live());
    }

    async fn live() {
        let (b, s) = find("01007EF00011E000", "The Legend of Zelda™: Breath of the Wild").await.unwrap().unwrap();
        assert_eq!((s, image_ext(&b)), (Source::Nlib, Some("jpg")));
        let (b, s) = find("0400A3A01EE4C000", "Pokémon Pokopia").await.unwrap().unwrap();
        assert_eq!((s, image_ext(&b)), (Source::NintendoEu, Some("png")));
        assert!(find("0100FA4E00001000", "Avventura di Prova").await.unwrap().is_none());
    }
}
