use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::{Envelope, Transport};

/// A WebDAV folder the user runs (Nextcloud, `rclone serve webdav`, …) as the
/// chain's mailbox (S9): one file per envelope,
/// `<base>/<chain_id>/<sync_id>.json`. The server only ever holds ciphertext;
/// the credentials only decide who may read and write the folder.
///
/// Use `https://` outside a LAN or WireGuard: over plain HTTP the password
/// travels in the clear (the clips don't, they are sealed).
pub struct WebDav {
    folder: String,
    auth: Option<String>,
    agent: ureq::Agent,
}

impl WebDav {
    /// `base` is the folder URL; the chain's mailbox is created inside it.
    pub fn new(base: &str, chain_id: &str, user: Option<&str>, password: Option<&str>) -> Self {
        Self {
            folder: format!("{}/{}/", base.trim_end_matches('/'), chain_id),
            auth: user.map(|u| {
                format!("Basic {}", STANDARD.encode(format!("{u}:{}", password.unwrap_or(""))))
            }),
            agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).build(),
        }
    }

    fn request(&self, method: &str, url: &str) -> ureq::Request {
        let r = self.agent.request(method, url);
        match &self.auth {
            Some(a) => r.set("Authorization", a),
            None => r,
        }
    }
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => format!("WebDAV {code} on {}", r.get_url()),
        ureq::Error::Transport(t) => format!("WebDAV unreachable: {t}"),
    }
}

impl Transport for WebDav {
    fn push(&self, env: &Envelope) -> Result<(), String> {
        let body = serde_json::to_string(env).map_err(|e| e.to_string())?;
        let url = format!("{}{}.json", self.folder, env.sync_id);
        match self.request("PUT", &url).send_string(&body) {
            Ok(_) => Ok(()),
            // 404/409: the mailbox doesn't exist yet. Create it and retry once.
            Err(ureq::Error::Status(404 | 409, _)) => {
                match self.request("MKCOL", &self.folder).call() {
                    Ok(_) | Err(ureq::Error::Status(405, _)) => {} // 405: already there
                    Err(e) => return Err(err(e)),
                }
                self.request("PUT", &url).send_string(&body).map(|_| ()).map_err(err)
            }
            Err(e) => Err(err(e)),
        }
    }

    fn pull(&self) -> Result<Vec<Envelope>, String> {
        let listing = match self.request("PROPFIND", &self.folder).set("Depth", "1").call() {
            Ok(r) => r.into_string().map_err(|e| e.to_string())?,
            Err(ureq::Error::Status(404, _)) => return Ok(vec![]), // nothing pushed yet
            Err(e) => return Err(err(e)),
        };
        let mut out = Vec::new();
        for name in envelope_names(&listing) {
            let body = self
                .request("GET", &format!("{}{name}", self.folder))
                .call()
                .map_err(err)?
                .into_string()
                .map_err(|e| e.to_string())?;
            // A file that isn't an envelope is skipped, not fatal: anyone with
            // the folder's password can drop junk in it.
            if let Ok(env) = serde_json::from_str(&body) {
                out.push(env);
            }
        }
        Ok(out)
    }
}

/// The `<sync_id>.json` names in a PROPFIND answer. Servers differ in the
/// namespace prefix (`<D:href>`, `<d:href>`, `<href>`) and in whether hrefs
/// are absolute, so this reads every `href` element's last path segment and
/// keeps those shaped like ours (hex, so never percent-encoded).
fn envelope_names(xml: &str) -> Vec<String> {
    xml.split("href>")
        .skip(1)
        .filter_map(|s| s.split('<').next())
        .filter_map(|href| href.trim().trim_end_matches('/').rsplit('/').next())
        .filter(|n| {
            n.strip_suffix(".json")
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_different_servers() {
        let rclone = r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href>/mbox/abc/</D:href></D:response>
            <D:response><D:href>/mbox/abc/00ff.json</D:href></D:response></D:multistatus>"#;
        let nextcloud = r#"<d:multistatus xmlns:d="DAV:"><d:response>
            <d:href>https://cloud.example/remote.php/dav/files/leo/mbox/abc/a1b2.json</d:href>
            </d:response><d:response><d:href>/x/notes.txt</d:href></d:response>
            <d:response><d:href>/x/zz.json</d:href></d:response></d:multistatus>"#;
        assert_eq!(envelope_names(rclone), ["00ff.json"]);
        assert_eq!(envelope_names(nextcloud), ["a1b2.json"]);
    }
}
