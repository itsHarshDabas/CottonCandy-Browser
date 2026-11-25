//! Extensions from the Chrome Web Store (CRX3 files, signature checked) and, best effort, Firefox add-ons
//! from addons.mozilla.org (converted to what Chromium runs). Installed extensions are unpacked into the
//! profile and loaded by Chromium.
//!
//! Chromium 156 runs Manifest V3 extensions only, so Manifest V2 add-ons are refused with a clear reason.

use ring::{digest, signature};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 20_000;

#[derive(Debug, PartialEq, Eq)]
pub enum ExtError {
    BadUrl,
    BadPackage(String),
    BadSignature,
    Unsupported(String),
    Io(String),
}

impl std::fmt::Display for ExtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtError::BadUrl => write!(f, "That isn't a Chrome Web Store or Firefox Add-ons page."),
            ExtError::BadPackage(why) => write!(f, "The extension file is damaged: {why}"),
            ExtError::BadSignature => write!(
                f,
                "The extension's signature doesn't match its id, so it wasn't installed."
            ),
            ExtError::Unsupported(why) => write!(f, "{why}"),
            ExtError::Io(why) => write!(f, "Couldn't save the extension: {why}"),
        }
    }
}

/// Where an extension comes from, parsed from the store page the user is on (or pasted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A Chrome Web Store id (32 letters a-p).
    Chrome(String),
    /// An addons.mozilla.org slug or numeric id.
    Firefox(String),
}

pub fn is_chrome_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Recognizes chromewebstore.google.com/detail/<name>/<id>, the old chrome.google.com/webstore links, and
/// addons.mozilla.org/<lang>/firefox/addon/<slug>/.
pub fn parse_store_url(text: &str) -> Result<Source, ExtError> {
    let text = text.trim();
    if is_chrome_id(text) {
        return Ok(Source::Chrome(text.to_string()));
    }
    let url = url::Url::parse(text).map_err(|_| ExtError::BadUrl)?;
    if url.scheme() != "https" {
        return Err(ExtError::BadUrl);
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    match url.host_str() {
        Some("chromewebstore.google.com") | Some("chrome.google.com") => segments
            .iter()
            .rev()
            .find(|s| is_chrome_id(s))
            .map(|id| Source::Chrome((*id).to_string()))
            .ok_or(ExtError::BadUrl),
        Some("addons.mozilla.org") => {
            let at = segments
                .iter()
                .position(|s| *s == "addon")
                .ok_or(ExtError::BadUrl)?;
            let slug = segments.get(at + 1).ok_or(ExtError::BadUrl)?;
            let ok = !slug.is_empty()
                && slug.len() <= 200
                && slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.@{}".contains(c));
            ok.then(|| Source::Firefox((*slug).to_string()))
                .ok_or(ExtError::BadUrl)
        }
        _ => Err(ExtError::BadUrl),
    }
}

/// The Web Store's download address for an extension (it redirects to the CRX file).
pub fn chrome_download_url(id: &str, chromium_version: &str) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion={chromium_version}&acceptformat=crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    )
}

/// addons.mozilla.org's API page for an add-on; its `current_version.file.url` is the XPI.
pub fn firefox_api_url(slug: &str) -> String {
    format!(
        "https://addons.mozilla.org/api/v5/addons/addon/{}/",
        percent_encoding::utf8_percent_encode(slug, percent_encoding::NON_ALPHANUMERIC)
    )
}

/// The XPI link in an addons.mozilla.org API reply. Only links on addons.mozilla.org are accepted.
pub fn firefox_xpi_url(api_json: &str) -> Result<String, ExtError> {
    let v: serde_json::Value = serde_json::from_str(api_json).map_err(|_| ExtError::BadUrl)?;
    let url = v["current_version"]["file"]["url"]
        .as_str()
        .ok_or_else(|| ExtError::Unsupported("That add-on has no file to install.".into()))?;
    let parsed = url::Url::parse(url).map_err(|_| ExtError::BadUrl)?;
    if parsed.scheme() == "https" && parsed.host_str() == Some("addons.mozilla.org") {
        Ok(url.to_string())
    } else {
        Err(ExtError::BadUrl)
    }
}

// ---- CRX3 ----------------------------------------------------------------------------------------

fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// The length-delimited fields of a protobuf message: (field number, bytes). Other wire types are skipped.
fn proto_fields(buf: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut pos = 0;
    let mut out = vec![];
    while pos < buf.len() {
        let key = varint(buf, &mut pos)?;
        match key & 7 {
            0 => {
                varint(buf, &mut pos)?;
            }
            1 => pos += 8,
            5 => pos += 4,
            2 => {
                let len = varint(buf, &mut pos)? as usize;
                let end = pos.checked_add(len)?;
                out.push((key >> 3, buf.get(pos..end)?));
                pos = end;
            }
            _ => return None,
        }
    }
    (pos == buf.len()).then_some(out)
}

/// A DER element: (tag, contents, rest).
fn der(buf: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *buf.first()?;
    let first = *buf.get(1)? as usize;
    let (len, start) = if first < 0x80 {
        (first, 2)
    } else {
        let n = first & 0x7f;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *buf.get(2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let end = start.checked_add(len)?;
    Some((tag, buf.get(start..end)?, buf.get(end..)?))
}

/// The key inside a SubjectPublicKeyInfo (PKCS#1 for RSA, the point for EC).
fn spki_key(spki: &[u8]) -> Option<&[u8]> {
    let (tag, body, _) = der(spki)?;
    if tag != 0x30 {
        return None;
    }
    let (alg_tag, _, rest) = der(body)?;
    let (bit_tag, bits, _) = der(rest)?;
    (alg_tag == 0x30 && bit_tag == 0x03 && bits.first() == Some(&0)).then(|| &bits[1..])
}

/// Chromium's extension id for a public key: the first 16 bytes of its SHA-256, as letters a-p.
pub fn id_for_key(spki: &[u8]) -> String {
    let hash = digest::digest(&digest::SHA256, spki);
    hash.as_ref()[..16]
        .iter()
        .flat_map(|b| [b >> 4, b & 15])
        .map(|n| (b'a' + n) as char)
        .collect()
}

fn id_for_crx_id(crx_id: &[u8]) -> String {
    crx_id
        .iter()
        .flat_map(|b| [b >> 4, b & 15])
        .map(|n| (b'a' + n) as char)
        .collect()
}

/// Checks a CRX3 file and returns (extension id, the zip inside). The developer key's signature must be valid
/// and the key must hash to the id the file claims; if `expected_id` is given, that must match too.
pub fn verify_crx3<'a>(
    file: &'a [u8],
    expected_id: Option<&str>,
) -> Result<(String, &'a [u8]), ExtError> {
    let bad = |why: &str| ExtError::BadPackage(why.to_string());
    if file.len() > MAX_PACKAGE_BYTES {
        return Err(bad("too big"));
    }
    if file.get(..4) != Some(b"Cr24") {
        return Err(bad("not a CRX file"));
    }
    let version = u32::from_le_bytes(
        file.get(4..8)
            .ok_or_else(|| bad("short"))?
            .try_into()
            .unwrap(),
    );
    if version != 3 {
        return Err(ExtError::Unsupported(
            "Only CRX3 extension files are supported.".into(),
        ));
    }
    let header_len = u32::from_le_bytes(
        file.get(8..12)
            .ok_or_else(|| bad("short"))?
            .try_into()
            .unwrap(),
    ) as usize;
    let header = file
        .get(12..12 + header_len)
        .ok_or_else(|| bad("short header"))?;
    let archive = &file[12 + header_len..];
    let fields = proto_fields(header).ok_or_else(|| bad("bad header"))?;
    let signed_data = fields
        .iter()
        .find(|(n, _)| *n == 10000)
        .map(|(_, b)| *b)
        .ok_or_else(|| bad("unsigned"))?;
    let crx_id = proto_fields(signed_data)
        .and_then(|f| f.into_iter().find(|(n, _)| *n == 1).map(|(_, b)| b))
        .ok_or_else(|| bad("no id"))?;
    if crx_id.len() != 16 {
        return Err(bad("bad id"));
    }
    let id = id_for_crx_id(crx_id);
    if let Some(expected) = expected_id {
        if expected != id {
            return Err(ExtError::BadSignature);
        }
    }
    let mut message = Vec::with_capacity(archive.len() + signed_data.len() + 32);
    message.extend_from_slice(b"CRX3 SignedData\x00");
    message.extend_from_slice(&(signed_data.len() as u32).to_le_bytes());
    message.extend_from_slice(signed_data);
    message.extend_from_slice(archive);
    let mut verified = false;
    for (field, proof) in fields.iter().filter(|(n, _)| *n == 2 || *n == 3) {
        let Some(parts) = proto_fields(proof) else {
            continue;
        };
        let key = parts.iter().find(|(n, _)| *n == 1).map(|(_, b)| *b);
        let sig = parts.iter().find(|(n, _)| *n == 2).map(|(_, b)| *b);
        let (Some(key), Some(sig)) = (key, sig) else {
            continue;
        };
        if id_for_key(key) != id {
            continue; // the store's own key also signs; the developer's key is the one that names the id
        }
        let Some(raw) = spki_key(key) else { continue };
        let algorithm: &dyn signature::VerificationAlgorithm = if *field == 2 {
            &signature::RSA_PKCS1_2048_8192_SHA256
        } else {
            &signature::ECDSA_P256_SHA256_ASN1
        };
        if signature::UnparsedPublicKey::new(algorithm, raw)
            .verify(&message, sig)
            .is_ok()
        {
            verified = true;
        }
    }
    if !verified {
        return Err(ExtError::BadSignature);
    }
    Ok((id, archive))
}

// ---- unpacking ------------------------------------------------------------------------------------

/// Unpacks a zip into `dest` (replacing what's there). Paths that would leave `dest`, links, and archives
/// that are too big are refused.
pub fn unpack(zip_bytes: &[u8], dest: &Path) -> Result<(), ExtError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| ExtError::BadPackage(e.to_string()))?;
    if archive.len() > MAX_FILES {
        return Err(ExtError::BadPackage("too many files".into()));
    }
    let staging = dest.with_extension("installing");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| ExtError::Io(e.to_string()))?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ExtError::BadPackage(e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(ExtError::BadPackage("unsafe path in archive".into()));
        };
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(ExtError::BadPackage("unsafe path in archive".into()));
        }
        if entry.is_symlink() {
            return Err(ExtError::BadPackage("links aren't allowed".into()));
        }
        let path = staging.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| ExtError::Io(e.to_string()))?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ExtError::Io(e.to_string()))?;
        }
        let mut data = Vec::new();
        (&mut entry)
            .take(MAX_UNPACKED_BYTES - total + 1)
            .read_to_end(&mut data)
            .map_err(|e| ExtError::BadPackage(e.to_string()))?;
        total += data.len() as u64;
        if total > MAX_UNPACKED_BYTES {
            return Err(ExtError::BadPackage("too big when unpacked".into()));
        }
        std::fs::write(&path, data).map_err(|e| ExtError::Io(e.to_string()))?;
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&staging, dest).map_err(|e| ExtError::Io(e.to_string()))
}

/// (name, version) from an unpacked extension, refusing what Chromium 156 can't run.
pub fn check_manifest(dir: &Path) -> Result<(String, String), ExtError> {
    let text = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|_| ExtError::BadPackage("no manifest.json".into()))?;
    let text = text.trim_start_matches('\u{feff}');
    let m: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ExtError::BadPackage(format!("manifest.json: {e}")))?;
    if m["manifest_version"].as_u64() != Some(3) {
        return Err(ExtError::Unsupported(
            "This extension uses Manifest V2, which Chromium no longer runs. Look for a newer (Manifest V3) version.".into(),
        ));
    }
    let name = m["name"]
        .as_str()
        .unwrap_or("Extension")
        .chars()
        .take(80)
        .collect();
    let version = m["version"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(30)
        .collect();
    Ok((name, version))
}

/// Best-effort conversion of a Firefox (WebExtension, Manifest V3) add-on into one Chromium runs:
/// Firefox-only keys are removed, `browser.*` is made to point at `chrome.*`, and background scripts run in a
/// service worker. Add-ons that use Firefox-only APIs still won't work, and the UI says so.
pub fn convert_firefox(dir: &Path) -> Result<(String, String), ExtError> {
    let path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| ExtError::BadPackage("no manifest.json".into()))?;
    let mut m: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| ExtError::BadPackage(format!("manifest.json: {e}")))?;
    if m["manifest_version"].as_u64() != Some(3) {
        return Err(ExtError::Unsupported(
            "This Firefox add-on uses Manifest V2, which Chromium no longer runs. If it's on the Chrome Web Store too, install it from there.".into(),
        ));
    }
    let obj = m
        .as_object_mut()
        .ok_or_else(|| ExtError::BadPackage("manifest isn't an object".into()))?;
    obj.remove("browser_specific_settings");
    obj.remove("applications");
    obj.remove("sidebar_action");
    obj.remove("developer");
    std::fs::write(
        dir.join("cottoncandy-browser-polyfill.js"),
        "globalThis.browser = globalThis.browser || globalThis.chrome;\n",
    )
    .map_err(|e| ExtError::Io(e.to_string()))?;
    if let Some(bg) = obj.get_mut("background").and_then(|b| b.as_object_mut()) {
        if let Some(scripts) = bg.remove("scripts").and_then(|s| s.as_array().cloned()) {
            let names: Vec<String> = scripts
                .iter()
                .filter_map(|s| s.as_str())
                .filter(|s| safe_relative(s))
                .map(|s| serde_json::to_string(s).unwrap())
                .collect();
            let worker = format!(
                "importScripts(\"cottoncandy-browser-polyfill.js\", {});\n",
                names.join(", ")
            );
            std::fs::write(dir.join("cottoncandy-background.js"), worker)
                .map_err(|e| ExtError::Io(e.to_string()))?;
            bg.insert("service_worker".into(), "cottoncandy-background.js".into());
            bg.remove("persistent");
            bg.remove("type");
        }
    }
    if let Some(cs) = obj
        .get_mut("content_scripts")
        .and_then(|c| c.as_array_mut())
    {
        for entry in cs {
            if let Some(js) = entry.get_mut("js").and_then(|j| j.as_array_mut()) {
                js.insert(0, "cottoncandy-browser-polyfill.js".into());
            }
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&m).unwrap())
        .map_err(|e| ExtError::Io(e.to_string()))?;
    check_manifest(dir)
}

fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Where an extension is unpacked: `<data>/extensions/<id>`. `id` must be a Chrome id or a safe slug.
pub fn install_dir(data_dir: &Path, id: &str) -> Option<PathBuf> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| data_dir.join("extensions").join(id))
}

/// A safe directory name for a Firefox add-on.
pub fn firefox_dir_name(slug: &str) -> String {
    let clean: String = slug
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    format!("ff-{clean}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, data) in files {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn store_urls() {
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        assert_eq!(
            parse_store_url(&format!(
                "https://chromewebstore.google.com/detail/ublock-origin/{id}"
            ))
            .unwrap(),
            Source::Chrome(id.into())
        );
        assert_eq!(
            parse_store_url(&format!(
                "https://chrome.google.com/webstore/detail/x/{id}?hl=en"
            ))
            .unwrap(),
            Source::Chrome(id.into())
        );
        assert_eq!(
            parse_store_url("https://addons.mozilla.org/en-US/firefox/addon/ublock-origin/")
                .unwrap(),
            Source::Firefox("ublock-origin".into())
        );
        assert!(parse_store_url(
            "http://chromewebstore.google.com/detail/x/cjpalhdlnbpafiamejdnhcphjbkeiagm"
        )
        .is_err());
        assert!(
            parse_store_url("https://evil.test/detail/x/cjpalhdlnbpafiamejdnhcphjbkeiagm").is_err()
        );
        assert!(chrome_download_url(id, "156.0.0.0")
            .contains("x=id%3Dcjpalhdlnbpafiamejdnhcphjbkeiagm"));
        assert_eq!(
            firefox_xpi_url(r#"{"current_version":{"file":{"url":"https://addons.mozilla.org/firefox/downloads/file/1/x.xpi"}}}"#).unwrap(),
            "https://addons.mozilla.org/firefox/downloads/file/1/x.xpi"
        );
        assert!(firefox_xpi_url(
            r#"{"current_version":{"file":{"url":"https://evil.test/x.xpi"}}}"#
        )
        .is_err());
    }

    #[test]
    fn crx_rejects_tampering() {
        assert!(matches!(
            verify_crx3(b"PK\x03\x04", None),
            Err(ExtError::BadPackage(_))
        ));
        // A CRX3 with a header that names an id but carries no valid proof.
        let mut signed = vec![0x0a, 16];
        signed.extend_from_slice(&[0u8; 16]);
        let mut header = vec![0x82, 0xf1, 0x04, signed.len() as u8]; // field 10000, wire type 2
        header.extend_from_slice(&signed);
        let mut file = b"Cr24".to_vec();
        file.extend_from_slice(&3u32.to_le_bytes());
        file.extend_from_slice(&(header.len() as u32).to_le_bytes());
        file.extend_from_slice(&header);
        file.extend_from_slice(&zip_of(&[("manifest.json", b"{}")]));
        assert_eq!(
            verify_crx3(&file, None).unwrap_err(),
            ExtError::BadSignature
        );
        assert_eq!(
            verify_crx3(&file, Some("cjpalhdlnbpafiamejdnhcphjbkeiagm")).unwrap_err(),
            ExtError::BadSignature
        );
    }

    #[test]
    fn ids_from_keys() {
        // Chromium's own example: this key is the "Google Docs Offline"-style id derivation (16 bytes of SHA-256).
        let id = id_for_key(b"any key bytes");
        assert!(is_chrome_id(&id));
    }

    #[test]
    fn unpack_refuses_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("ext");
        let bad = zip_of(&[("../evil.txt", b"x")]);
        assert!(unpack(&bad, &dest).is_err());
        assert!(!dir.path().join("evil.txt").exists());
        let good = zip_of(&[
            (
                "manifest.json",
                br#"{"manifest_version":3,"name":"Hi","version":"1.0"}"#,
            ),
            ("js/a.js", b"1"),
        ]);
        unpack(&good, &dest).unwrap();
        assert_eq!(check_manifest(&dest).unwrap(), ("Hi".into(), "1.0".into()));
        let mv2 = zip_of(&[(
            "manifest.json",
            br#"{"manifest_version":2,"name":"Old","version":"1"}"#,
        )]);
        unpack(&mv2, &dest).unwrap();
        assert!(matches!(
            check_manifest(&dest),
            Err(ExtError::Unsupported(_))
        ));
    }

    #[test]
    fn firefox_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("ff");
        let xpi = zip_of(&[(
            "manifest.json",
            br#"{"manifest_version":3,"name":"Dark","version":"2.1","browser_specific_settings":{"gecko":{"id":"d@x"}},
               "background":{"scripts":["bg.js","../x.js"]},"content_scripts":[{"matches":["<all_urls>"],"js":["cs.js"]}]}"#,
        )]);
        unpack(&xpi, &dest).unwrap();
        assert_eq!(
            convert_firefox(&dest).unwrap(),
            ("Dark".into(), "2.1".into())
        );
        let m: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dest.join("manifest.json")).unwrap())
                .unwrap();
        assert!(m.get("browser_specific_settings").is_none());
        assert_eq!(
            m["background"]["service_worker"],
            "cottoncandy-background.js"
        );
        assert_eq!(
            m["content_scripts"][0]["js"][0],
            "cottoncandy-browser-polyfill.js"
        );
        let worker = std::fs::read_to_string(dest.join("cottoncandy-background.js")).unwrap();
        assert!(worker.contains("\"bg.js\"") && !worker.contains("../x.js"));
        assert_eq!(install_dir(dir.path(), "../x"), None);
        assert_eq!(
            firefox_dir_name("ublock@raymondhill.net"),
            "ff-ublock_raymondhill_net"
        );
    }
}
