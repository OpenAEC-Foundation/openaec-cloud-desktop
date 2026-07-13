// User-scoped WebDAV + OCS-Share-client voor Nextcloud. Praat als de INGELOGDE
// gebruiker (basic-auth met app-wachtwoord), niet als service-account — dat is
// het verschil met de server-side openaec-cloud-lib.
use anyhow::{anyhow, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;

#[derive(Clone)]
pub struct Conn {
    base: String,
    user: String,
    pass: String,
    http: reqwest::Client,
}

#[derive(Serialize, Clone, Debug)]
pub struct RemoteEntry {
    pub name: String,
    /// pad relatief aan de files-root van de gebruiker (bv. "Documenten/plan.pdf")
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
}

impl Conn {
    pub fn new(base: &str, user: &str, pass: &str) -> Result<Self> {
        // NB: interne CA (openaec.lan) → certs voorlopig niet gevalideerd.
        // Fase 1.5: de interne root-CA meebundelen + .add_root_certificate().
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            user: user.to_string(),
            pass: pass.to_string(),
            http,
        })
    }

    fn dav_url(&self, path: &str) -> String {
        let p = path.trim_start_matches('/');
        format!("{}/remote.php/dav/files/{}/{}", self.base, self.user, p)
    }

    /// PROPFIND depth 1 → directe kinderen van `path`.
    pub async fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        let body = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:getlastmodified/><d:getcontentlength/><d:getetag/><d:resourcetype/></d:prop></d:propfind>"#;
        let res = self
            .http
            .request(reqwest::Method::from_bytes(b"PROPFIND")?, self.dav_url(path))
            .basic_auth(&self.user, Some(&self.pass))
            .header("Depth", "1")
            .header("Content-Type", "application/xml")
            .body(body)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("PROPFIND {} → {}", path, res.status()));
        }
        parse_propfind(&res.text().await?, &self.user)
    }

    pub async fn download(&self, path: &str) -> Result<Vec<u8>> {
        let res = self
            .http
            .get(self.dav_url(path))
            .basic_auth(&self.user, Some(&self.pass))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("GET {} → {}", path, res.status()));
        }
        Ok(res.bytes().await?.to_vec())
    }

    pub async fn upload(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let res = self
            .http
            .put(self.dav_url(path))
            .basic_auth(&self.user, Some(&self.pass))
            .body(data)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("PUT {} → {}", path, res.status()));
        }
        Ok(())
    }

    /// Map aanmaken. 201 = nieuw, 405 = bestaat al → beide OK.
    pub async fn mkcol(&self, path: &str) -> Result<()> {
        let res = self
            .http
            .request(reqwest::Method::from_bytes(b"MKCOL")?, self.dav_url(path))
            .basic_auth(&self.user, Some(&self.pass))
            .send()
            .await?;
        if res.status().is_success() || res.status().as_u16() == 405 {
            Ok(())
        } else {
            Err(anyhow!("MKCOL {} → {}", path, res.status()))
        }
    }

    /// Publieke deellink via de OCS Share-API (shareType 3 = public link).
    pub async fn create_public_link(&self, path: &str) -> Result<String> {
        let url = format!(
            "{}/ocs/v2.php/apps/files_sharing/api/v1/shares?format=json",
            self.base
        );
        let res = self
            .http
            .post(url)
            .basic_auth(&self.user, Some(&self.pass))
            .header("OCS-APIRequest", "true")
            .form(&[("path", format!("/{}", path.trim_start_matches('/'))), ("shareType", "3".into())])
            .send()
            .await?;
        let status = res.status();
        let txt = res.text().await?;
        if !status.is_success() {
            return Err(anyhow!("share {} → {} {}", path, status, txt));
        }
        let v: serde_json::Value = serde_json::from_str(&txt)?;
        v["ocs"]["data"]["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("geen share-URL in respons"))
    }
}

/// Parse een WebDAV-multistatus naar RemoteEntry's (de self-collectie wordt overgeslagen).
fn parse_propfind(xml: &str, user: &str) -> Result<Vec<RemoteEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let prefix = format!("/remote.php/dav/files/{}/", user);
    let mut out = Vec::new();
    let (mut href, mut size, mut etag, mut lastmod) = (String::new(), 0u64, String::new(), String::new());
    let mut is_dir = false;
    let mut target: Option<&'static str> = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"response" => { href.clear(); size = 0; etag.clear(); lastmod.clear(); is_dir = false; }
                b"href" => target = Some("href"),
                b"getcontentlength" => target = Some("size"),
                b"getetag" => target = Some("etag"),
                b"getlastmodified" => target = Some("mod"),
                b"collection" => is_dir = true,
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if let Some(tg) = target {
                    let s = t.unescape().unwrap_or_default().to_string();
                    match tg {
                        "href" => href = s,
                        "size" => size = s.parse().unwrap_or(0),
                        "etag" => etag = s,
                        "mod" => lastmod = s,
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => {
                target = None;
                if e.local_name().as_ref() == b"response" {
                    let decoded = urlencoding::decode(&href).map(|c| c.to_string()).unwrap_or_else(|_| href.clone());
                    if let Some(rel) = decoded.split(&prefix).nth(1) {
                        let rel = rel.trim_end_matches('/');
                        if !rel.is_empty() {
                            let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
                            out.push(RemoteEntry { name, path: rel.to_string(), is_dir, size, etag: etag.clone(), last_modified: lastmod.clone() });
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("xml-parse: {}", e)),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}
