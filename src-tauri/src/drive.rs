//! Google Drive as a [`crate::sync::Backend`].
//!
//! No Drive desktop client is involved, which is the point: none exists on iOS,
//! and relying on a folder somebody else keeps in step is what rules that
//! platform out. This talks to the HTTP API, so the same code runs everywhere.
//!
//! # The scope, and the one thing the user has to do
//!
//! `drive.file`, which needs no OAuth verification and no third-party security
//! assessment: the app sees files it created, plus anything the user hands it
//! through the Google Picker. Projects Lazuli made therefore need nothing, and
//! a project somebody shared is added once through the Picker.
//!
//! # Drive has no paths
//!
//! A file has an id and a list of parents, and the same name can appear many
//! times. So the engine's paths are reconstructed: everything the app can see
//! is listed in one paginated query, and each file's path is walked up its
//! parents to the project's folder. [`paths_from`] is that, and is pure.
//!
//! # What has been run
//!
//! All of it, once, against a real account: sign-in, the token exchange,
//! creating a project's folder, listing, uploading, downloading and trashing.
//! Two folders were reconciled through a real Drive until they held the same
//! bytes, including an entry deleted on one reaching the other's trash.
//!
//! What that first run cost, and is worth not re-learning: a `fields` parameter
//! is a promise about what comes back, and asking for a subset means
//! [`DriveFile`] cannot parse the answer — hence [`IdOnly`] and [`Written`].

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::sync::{Backend, RemoteFile};

/// How the app identifies itself to Google, **on desktop**.
///
/// Empty until somebody creates an OAuth client for their own build: a client
/// id belongs to a Google Cloud project, and there is no sensible default. The
/// app says so plainly rather than failing at the redirect.
///
/// Named for the platform because one id does not cover them all. Google ties a
/// client to how the app is identified, and that differs: a desktop client is
/// anonymous and proves itself with PKCE alone, where an Android one is pinned
/// to a package name and the fingerprint of the certificate it is signed with.
/// An Android build therefore needs its own, in the *same* Cloud project — same
/// consent screen, same quota, and a user who approved on their laptop is not
/// asked again on their phone.
pub const DESKTOP_CLIENT_ID: &str =
    "991113913988-hkpcvsdtq4vvvl1jovemgiiceh2o8p0d.apps.googleusercontent.com";

/// The secret Google issues beside it, which is not a secret.
///
/// Google's own documentation says so of installed apps: it is compiled into
/// every copy of the app and anyone may read it out, and it is the reason a
/// desktop client's *security* rests entirely on PKCE, where the verifier never
/// leaves the machine. What this is for is that Google's token endpoint
/// **refuses a desktop exchange without it** — PKCE alone does not stand in for
/// it, least of all when asking for the offline access that yields a refresh
/// token, which is the whole point of connecting once.
///
/// It grants nothing on its own. A sign-in still needs a person to consent, and
/// the tokens that come back belong to their machine.
pub const DESKTOP_CLIENT_SECRET: &str = "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";

/// Asked for at sign-in. `drive.file` and nothing else — the narrowest scope
/// that can do the job, and the one that keeps the app out of Google's
/// restricted-scope review.
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";

/// The mime type Drive uses to mean "folder".
const FOLDER: &str = "application/vnd.google-apps.folder";

/// How close to expiry a token is refreshed rather than used.
///
/// A token that expires during the request it was attached to is a failure the
/// user sees for no reason.
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);

/// What the app keeps between sessions to stay signed in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tokens {
    /// Long-lived, and the only part that matters: everything else can be asked
    /// for again with it.
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    /// When the access token stops working, as seconds since the epoch.
    #[serde(default)]
    pub expires_at: u64,
}

impl Tokens {
    /// Whether the access token is worth trying.
    pub fn is_fresh(&self, now: SystemTime) -> bool {
        if self.access_token.is_empty() {
            return false;
        }
        let deadline = SystemTime::UNIX_EPOCH + Duration::from_secs(self.expires_at);
        deadline
            .checked_sub(EXPIRY_MARGIN)
            .is_some_and(|safe| now < safe)
    }
}

/// The two halves of a PKCE challenge.
///
/// The verifier stays here and the challenge goes to Google; the code Google
/// hands back is worth nothing without the verifier, which is what makes a
/// client secret unnecessary — and a desktop app cannot keep one anyway.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        // 64 hex characters: inside the 43–128 the spec allows, and made of
        // characters that need no escaping anywhere it is carried.
        let verifier = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let digest = sha2::Sha256::digest(verifier.as_bytes());
        Self {
            challenge: base64_url(&digest),
            verifier,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

use sha2::Digest;

/// Base64 as a URL wants it: no padding, and `-_` for `+/`.
fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = chunk
            .iter()
            .enumerate()
            .fold(0u32, |bits, (index, byte)| {
                bits | (*byte as u32) << (16 - 8 * index)
            });
        for index in 0..=chunk.len() {
            out.push(ALPHABET[(bits >> (18 - 6 * index) & 0b11_1111) as usize] as char);
        }
    }
    out
}

/// The page Google is sent to, for a sign-in that will come back to `redirect`.
pub fn sign_in_url(pkce: &Pkce, redirect: &str) -> String {
    format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}\
         &code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        urlencode(DESKTOP_CLIENT_ID),
        urlencode(redirect),
        urlencode(SCOPE),
        urlencode(&pkce.challenge),
    )
}

/// Percent-encode everything that is not unreserved, which is the only rule a
/// query value here has to follow.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// --- what Drive answers with ----------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
}

/// One file, as `files.list` describes it.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(rename = "mimeType", default)]
    pub mime_type: String,
    /// Present for a file with contents, and the best thing to call a revision:
    /// it changes when the bytes change and not when a label does.
    #[serde(rename = "md5Checksum", default)]
    pub md5: Option<String>,
    /// Bumped by any change at all, including ones that leave the bytes alone.
    /// The fallback when there is no checksum.
    #[serde(default)]
    pub version: Option<String>,
}

impl DriveFile {
    fn is_folder(&self) -> bool {
        self.mime_type == FOLDER
    }

    fn revision(&self) -> String {
        self.md5
            .clone()
            .or_else(|| self.version.clone())
            .unwrap_or_default()
    }
}

/// Drive's answer when only the id was asked for.
///
/// A `fields` parameter is a promise about what comes back, and asking for less
/// means [`DriveFile`] cannot parse it — its `name` is not optional, because a
/// file in a *listing* without one would be a path we could not build.
#[derive(Debug, Deserialize)]
struct IdOnly {
    id: String,
}

#[derive(Debug, Deserialize)]
struct IdList {
    #[serde(default)]
    files: Vec<IdOnly>,
}

/// What an upload answers with: enough to know what it landed as.
#[derive(Debug, Deserialize)]
struct Written {
    #[serde(rename = "md5Checksum", default)]
    md5: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

impl Written {
    fn revision(self) -> String {
        self.md5.or(self.version).unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
struct FileList {
    #[serde(default)]
    files: Vec<DriveFile>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

/// Turn Drive's flat list of files-with-parents into the engine's paths.
///
/// Pure, and the part of this worth testing: a file's path is its name behind
/// its parents' names, up to the project's own folder. Anything that does not
/// lead back to `root` belongs to another project and is left out, and so are
/// the folders themselves — the engine syncs files, and a folder is implied by
/// the files in it.
pub fn paths_from(files: &[DriveFile], root: &str) -> HashMap<String, DriveFile> {
    let by_id: HashMap<&str, &DriveFile> =
        files.iter().map(|file| (file.id.as_str(), file)).collect();

    let mut paths = HashMap::new();
    for file in files.iter().filter(|file| !file.is_folder()) {
        let mut parts = vec![file.name.as_str()];
        let mut at = file;
        // Bounded by the number of files, so a parent loop — which Drive should
        // never produce — cannot hang the sync.
        let mut steps = 0;
        let reached_root = loop {
            let Some(parent) = at.parents.first() else {
                break false;
            };
            if parent == root {
                break true;
            }
            let Some(next) = by_id.get(parent.as_str()) else {
                break false;
            };
            parts.push(next.name.as_str());
            at = next;
            steps += 1;
            if steps > files.len() {
                break false;
            }
        };
        if reached_root {
            parts.reverse();
            paths.insert(parts.join("/"), file.clone());
        }
    }
    paths
}

// --- the backend -----------------------------------------------------------

/// A project's folder on Drive, and the signed-in account that reaches it.
pub struct Drive {
    client: reqwest::blocking::Client,
    access_token: String,
    /// The id of the folder this project lives in.
    folder: String,
    /// Path to Drive id, as of the last `list`. Rebuilt each time rather than
    /// kept: Drive is the authority on what is there, and a stale id is a write
    /// to the wrong file.
    known: std::sync::Mutex<HashMap<String, DriveFile>>,
}

impl Drive {
    pub fn new(access_token: String, folder: String) -> Result<Self> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .context("building the HTTP client")?,
            access_token,
            folder,
            known: std::sync::Mutex::new(HashMap::new()),
        })
    }

    fn authorised(&self, request: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
        request.bearer_auth(&self.access_token)
    }

    /// Every file the app can see, in one paginated query.
    fn all_files(&self) -> Result<Vec<DriveFile>> {
        let mut found = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut request = self
                .client
                .get(format!("{API}/files"))
                .query(&[
                    ("q", "trashed = false"),
                    ("pageSize", "1000"),
                    (
                        "fields",
                        "nextPageToken, files(id, name, parents, mimeType, md5Checksum, version)",
                    ),
                ]);
            if let Some(token) = &page {
                request = request.query(&[("pageToken", token)]);
            }
            let response = self.authorised(request).send().context("listing Drive")?;
            let list: FileList = json(response)?;
            found.extend(list.files);
            match list.next_page_token {
                Some(token) => page = Some(token),
                None => break,
            }
        }
        Ok(found)
    }

    /// The id of the folder `path` should sit in, creating any that are missing.
    fn folder_for(&self, path: &str) -> Result<String> {
        let mut parent = self.folder.clone();
        let parts: Vec<&str> = path.split('/').collect();
        for name in &parts[..parts.len().saturating_sub(1)] {
            parent = self.folder_named(name, &parent)?;
        }
        Ok(parent)
    }

    fn folder_named(&self, name: &str, parent: &str) -> Result<String> {
        let query = format!(
            "name = '{}' and '{parent}' in parents and mimeType = '{FOLDER}' and trashed = false",
            name.replace('\'', "\\'")
        );
        let response = self
            .authorised(
                self.client
                    .get(format!("{API}/files"))
                    .query(&[("q", query.as_str()), ("fields", "files(id)")]),
            )
            .send()
            .context("looking for a folder")?;
        let list: IdList = json(response)?;
        if let Some(found) = list.files.first() {
            return Ok(found.id.clone());
        }

        let created: IdOnly = json(
            self.authorised(
                self.client
                    .post(format!("{API}/files"))
                    .query(&[("fields", "id")])
                    .json(&serde_json::json!({
                        "name": name,
                        "mimeType": FOLDER,
                        "parents": [parent],
                    })),
            )
            .send()
            .context("creating a folder")?,
        )?;
        Ok(created.id)
    }
}

impl Drive {
    /// A folder for a project, made once when syncing is turned on.
    ///
    /// Under Drive's root rather than in a folder of the app's own: with the
    /// `drive.file` scope the app sees only what it created, so a "Lazuli"
    /// folder would be one more thing to keep track of for no gain, and a
    /// project the user drags somewhere tidier still works.
    pub fn make_project_folder(access_token: &str, name: &str) -> Result<String> {
        let client = reqwest::blocking::Client::new();
        let created: IdOnly = json(
            client
                .post(format!("{API}/files"))
                .bearer_auth(access_token)
                .query(&[("fields", "id")])
                .json(&serde_json::json!({ "name": name, "mimeType": FOLDER }))
                .send()
                .context("making the project's folder on Drive")?,
        )?;
        Ok(created.id)
    }
}

impl Backend for Drive {
    fn list(&self) -> Result<Vec<RemoteFile>> {
        let paths = paths_from(&self.all_files()?, &self.folder);
        let files = paths
            .iter()
            .map(|(path, file)| RemoteFile {
                path: path.clone(),
                revision: file.revision(),
            })
            .collect();
        *self.known.lock().expect("drive index was poisoned") = paths;
        Ok(files)
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        let id = self
            .known
            .lock()
            .expect("drive index was poisoned")
            .get(path)
            .map(|file| file.id.clone())
            .ok_or_else(|| anyhow!("{path} is not on this Drive"))?;
        let response = self
            .authorised(
                self.client
                    .get(format!("{API}/files/{id}"))
                    .query(&[("alt", "media")]),
            )
            .send()
            .with_context(|| format!("downloading {path}"))?;
        let response = check(response)?;
        Ok(response.bytes().context("reading the download")?.to_vec())
    }

    fn put(&self, path: &str, bytes: &[u8], _expected: Option<&str>) -> Result<String> {
        let existing = self
            .known
            .lock()
            .expect("drive index was poisoned")
            .get(path)
            .map(|file| file.id.clone());

        let uploaded: Written = match existing {
            // Replacing what is there: the contents alone, in one request.
            Some(id) => json(
                self.authorised(
                    self.client
                        .patch(format!("{UPLOAD_API}/files/{id}"))
                        .query(&[("uploadType", "media"), ("fields", "id, md5Checksum, version")])
                        .body(bytes.to_vec()),
                )
                .send()
                .with_context(|| format!("uploading {path}"))?,
            )?,
            // New: the metadata says where it goes, so this is a two-part body.
            None => {
                let name = path.rsplit('/').next().unwrap_or(path);
                let parent = self.folder_for(path)?;
                let metadata = serde_json::json!({ "name": name, "parents": [parent] });
                let form = reqwest::blocking::multipart::Form::new()
                    .part(
                        "metadata",
                        reqwest::blocking::multipart::Part::text(metadata.to_string())
                            .mime_str("application/json")?,
                    )
                    .part(
                        "file",
                        reqwest::blocking::multipart::Part::bytes(bytes.to_vec()),
                    );
                json(
                    self.authorised(
                        self.client
                            .post(format!("{UPLOAD_API}/files"))
                            .query(&[
                                ("uploadType", "multipart"),
                                ("fields", "id, md5Checksum, version"),
                            ])
                            .multipart(form),
                    )
                    .send()
                    .with_context(|| format!("uploading {path}"))?,
                )?
            }
        };
        Ok(uploaded.revision())
    }

    fn delete(&self, path: &str) -> Result<()> {
        let id = self
            .known
            .lock()
            .expect("drive index was poisoned")
            .get(path)
            .map(|file| file.id.clone());
        let Some(id) = id else {
            // Already gone, which is the outcome asked for.
            return Ok(());
        };
        // Into Drive's own trash rather than erased, which is the same promise
        // the app makes about its own: nothing is destroyed, only passed on.
        check(
            self.authorised(self.client.patch(format!("{API}/files/{id}")).json(
                &serde_json::json!({ "trashed": true }),
            ))
            .send()
            .with_context(|| format!("deleting {path}"))?,
        )?;
        Ok(())
    }
}

/// Fail on an error status, with the body in the message.
///
/// Google says what is wrong in the body and nothing useful in the status, so a
/// bare `error_for_status` would report "400 Bad Request" for a fixable
/// mistake.
fn check(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().unwrap_or_default();
    bail!("Google Drive said {status}: {body}")
}

fn json<T: serde::de::DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    check(response)?.json().context("reading Drive's answer")
}

// --- signing in ------------------------------------------------------------

/// Whether this build carries what Google needs to talk to it at all.
fn check_configured() -> Result<()> {
    if DESKTOP_CLIENT_ID.is_empty() || DESKTOP_CLIENT_SECRET.is_empty() {
        bail!(
            "This build has no Google client id and secret, so it cannot sign in              to Drive. See the README."
        );
    }
    Ok(())
}

/// Swap the code Google handed back for tokens.
pub fn exchange(code: &str, pkce: &Pkce, redirect: &str) -> Result<Tokens> {
    if DESKTOP_CLIENT_ID.is_empty() {
        bail!(
            "This build has no Google client id, so it cannot sign in to Drive. \
             See the README for how to make one."
        );
    }
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", DESKTOP_CLIENT_ID),
            ("client_secret", DESKTOP_CLIENT_SECRET),
            ("code", code),
            ("code_verifier", &pkce.verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect),
        ])
        .send()
        .context("asking Google for a token")?;
    let token: TokenResponse = json(response)?;
    Ok(Tokens {
        refresh_token: token
            .refresh_token
            .ok_or_else(|| anyhow!("Google did not send a refresh token"))?,
        access_token: token.access_token,
        expires_at: expires_at(token.expires_in),
    })
}

/// Ask for a new access token with the refresh token.
pub fn refresh(tokens: &Tokens) -> Result<Tokens> {
    check_configured()?;
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", DESKTOP_CLIENT_ID),
            ("client_secret", DESKTOP_CLIENT_SECRET),
            ("refresh_token", tokens.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .context("refreshing the Google token")?;
    let token: TokenResponse = json(response)?;
    Ok(Tokens {
        // Google sends one only the first time; the one we have stays good.
        refresh_token: token
            .refresh_token
            .unwrap_or_else(|| tokens.refresh_token.clone()),
        access_token: token.access_token,
        expires_at: expires_at(token.expires_in),
    })
}

fn expires_at(seconds: u64) -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.as_secs() + seconds)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, parent: &str) -> DriveFile {
        DriveFile {
            id: id.into(),
            name: name.into(),
            parents: vec![parent.into()],
            mime_type: String::new(),
            md5: Some(format!("{id}-md5")),
            version: None,
        }
    }

    fn folder(id: &str, name: &str, parent: &str) -> DriveFile {
        DriveFile {
            mime_type: FOLDER.into(),
            md5: None,
            ..file(id, name, parent)
        }
    }

    #[test]
    fn a_file_in_the_project_root_is_its_own_name() {
        let files = vec![file("f1", "lazuli.yaml", "ROOT")];
        let paths = paths_from(&files, "ROOT");
        assert_eq!(paths.keys().collect::<Vec<_>>(), vec!["lazuli.yaml"]);
    }

    #[test]
    fn a_nested_file_is_its_name_behind_its_folders() {
        let files = vec![
            folder("d1", "entries", "ROOT"),
            folder("d2", "an-id", "d1"),
            file("f1", "entry.md", "d2"),
        ];
        let paths = paths_from(&files, "ROOT");
        assert_eq!(
            paths.keys().collect::<Vec<_>>(),
            vec!["entries/an-id/entry.md"]
        );
    }

    #[test]
    fn folders_are_not_files_to_sync() {
        // The engine syncs files; a folder is implied by what is in it.
        let files = vec![folder("d1", "entries", "ROOT"), file("f1", "a.md", "d1")];
        assert_eq!(paths_from(&files, "ROOT").len(), 1);
    }

    #[test]
    fn another_projects_files_are_left_out() {
        // `drive.file` shows the app everything it ever created, which is every
        // project the user has, not just this one.
        let files = vec![
            file("f1", "lazuli.yaml", "ROOT"),
            file("f2", "lazuli.yaml", "SOMEWHERE-ELSE"),
        ];
        let paths = paths_from(&files, "ROOT");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths["lazuli.yaml"].id, "f1");
    }

    #[test]
    fn a_file_whose_parents_lead_nowhere_is_left_out() {
        // A folder the app can no longer see, which `drive.file` makes possible
        // whenever a parent was not created by us.
        let files = vec![file("f1", "orphan.md", "MISSING")];
        assert!(paths_from(&files, "ROOT").is_empty());
    }

    #[test]
    fn a_loop_in_the_parents_does_not_hang() {
        let mut one = file("a", "a.md", "b");
        let two = folder("b", "b", "a");
        one.parents = vec!["b".into()];
        assert!(paths_from(&[one, two], "ROOT").is_empty());
    }

    #[test]
    fn the_checksum_is_preferred_to_the_version() {
        // A version changes when a label does; a checksum changes when the
        // bytes do, which is the only change a sync should react to.
        let mut file = file("f1", "a.md", "ROOT");
        file.version = Some("17".into());
        assert_eq!(file.revision(), "f1-md5");
        file.md5 = None;
        assert_eq!(file.revision(), "17");
    }

    #[test]
    fn a_token_near_its_expiry_is_not_fresh() {
        let now = SystemTime::now();
        let in_seconds = |seconds: u64| Tokens {
            refresh_token: "r".into(),
            access_token: "a".into(),
            expires_at: now
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("after the epoch")
                .as_secs()
                + seconds,
        };
        assert!(in_seconds(3600).is_fresh(now));
        // Inside the margin: it could expire during the request it is attached
        // to, which is a failure the user would see for no reason.
        assert!(!in_seconds(30).is_fresh(now));
        assert!(!in_seconds(0).is_fresh(now));
    }

    #[test]
    fn a_token_we_do_not_have_is_not_fresh() {
        assert!(!Tokens::default().is_fresh(SystemTime::now()));
    }

    #[test]
    fn the_pkce_challenge_is_the_verifiers_digest() {
        let pkce = Pkce::new();
        assert!(pkce.verifier.len() >= 43 && pkce.verifier.len() <= 128);
        assert_eq!(
            pkce.challenge,
            base64_url(&sha2::Sha256::digest(pkce.verifier.as_bytes()))
        );
        // URL-safe and unpadded, which is what the spec asks for.
        assert!(!pkce.challenge.contains('+'));
        assert!(!pkce.challenge.contains('/'));
        assert!(!pkce.challenge.contains('='));
    }

    #[test]
    fn base64_url_matches_the_standard_alphabet() {
        // RFC 4648 §5's vectors, with `-_` in place of `+/` and no padding.
        assert_eq!(base64_url(&[0xfb, 0xff, 0xbf]), "-_-_");
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"fo"), "Zm8");
        assert_eq!(base64_url(b"foo"), "Zm9v");
    }

    #[test]
    fn the_sign_in_url_carries_the_challenge_and_asks_to_stay_signed_in() {
        let pkce = Pkce::new();
        let url = sign_in_url(&pkce, "http://127.0.0.1:1421/");
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&urlencode(&pkce.challenge)));
        // Without this Google sends no refresh token, and the user would be
        // asked to sign in again within the hour.
        assert!(url.contains("access_type=offline"));
        assert!(url.contains(&urlencode(SCOPE)));
        assert!(url.contains(&urlencode("http://127.0.0.1:1421/")));
    }

    #[test]
    fn urlencoding_leaves_the_unreserved_alone_and_escapes_the_rest() {
        assert_eq!(urlencode("abcXYZ019-._~"), "abcXYZ019-._~");
        assert_eq!(urlencode("a/b:c?d=e&f"), "a%2Fb%3Ac%3Fd%3De%26f");
    }
}

// --- catching the redirect -------------------------------------------------

/// How long the app waits on the sign-in page before giving up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// A loopback listener, and the redirect URL that reaches it.
///
/// Google will not redirect to an app, only to a URL, so a desktop sign-in ends
/// at a web server the app runs for the length of it. Port 0 so the OS picks a
/// free one: a fixed port is one another program may already hold, and one the
/// user would have to register in the Cloud console.
///
/// **Desktop only.** A phone has no loopback to redirect to and no business
/// running a web server; a mobile sign-in comes back through a custom URI
/// scheme or an app link the OS routes to the app. That is a second way in
/// rather than a change to this one, and it is not built — see
/// `ideas/syncing-projects.md`.
pub struct Redirect {
    listener: std::net::TcpListener,
    pub url: String,
}

impl Redirect {
    pub fn new() -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("opening a port for Google to come back to")?;
        let port = listener.local_addr().context("reading the port")?.port();
        Ok(Self {
            listener,
            url: format!("http://127.0.0.1:{port}"),
        })
    }

    /// Wait for Google to come back, and hand over the code it brought.
    ///
    /// Answers the browser either way, because a sign-in that ends on a
    /// connection error looks like a broken app rather than a finished job.
    pub fn wait_for_code(self) -> Result<String> {
        use std::io::{BufRead, BufReader, Write};

        self.listener
            .set_nonblocking(false)
            .context("waiting for the browser")?;
        let deadline = SystemTime::now() + SIGN_IN_TIMEOUT;

        for stream in self.listener.incoming() {
            if SystemTime::now() > deadline {
                bail!("the Google sign-in was not finished in time");
            }
            let mut stream = stream.context("accepting the browser's request")?;
            let mut line = String::new();
            BufReader::new(&stream)
                .read_line(&mut line)
                .context("reading the browser's request")?;

            let outcome = code_in(&line);
            let body = match &outcome {
                Some(_) => "<h1>Signed in</h1><p>You can close this tab and go back to Lazuli.</p>",
                None => "<h1>That did not work</h1><p>Go back to Lazuli and try again.</p>",
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.flush();

            // A browser asks for a favicon too; only a request carrying an
            // answer ends the wait.
            if let Some(code) = outcome {
                return Ok(code);
            }
            if line.contains("error=") {
                bail!("Google refused the sign-in");
            }
        }
        bail!("the browser never came back")
    }
}

/// The `code` out of a request line like `GET /?code=abc&scope=… HTTP/1.1`.
pub fn code_in(request_line: &str) -> Option<String> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "code").then(|| urldecode(value))
    })
}

/// The inverse of [`urlencode`], for the one value that comes back.
fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod redirect_tests {
    use super::*;

    #[test]
    fn the_code_is_read_out_of_the_request_line() {
        assert_eq!(
            code_in("GET /?code=4%2F0AbCd&scope=drive.file HTTP/1.1"),
            Some("4/0AbCd".to_owned())
        );
    }

    #[test]
    fn a_request_carrying_no_code_is_not_an_answer() {
        // A browser asks for the favicon on the same connection, and Google
        // sends `error=` when the user says no.
        assert_eq!(code_in("GET /favicon.ico HTTP/1.1"), None);
        assert_eq!(code_in("GET /?error=access_denied HTTP/1.1"), None);
        assert_eq!(code_in("nonsense"), None);
    }

    #[test]
    fn urldecoding_undoes_urlencoding() {
        for value in ["4/0AbCd", "a b+c", "plain", "%%%", "ünïcode"] {
            assert_eq!(urldecode(&urlencode(value)), value);
        }
    }

    #[test]
    fn a_redirect_names_a_port_that_is_actually_open() {
        let redirect = Redirect::new().expect("should open a port");
        assert!(redirect.url.starts_with("http://127.0.0.1:"));
        // Reachable, which is the whole requirement Google places on it.
        let address = redirect.url.trim_start_matches("http://");
        std::net::TcpStream::connect(address).expect("should connect");
    }
}

// --- who is signed in ------------------------------------------------------

#[derive(Debug, Deserialize)]
struct About {
    user: AboutUser,
}

#[derive(Debug, Deserialize)]
struct AboutUser {
    #[serde(rename = "permissionId", default)]
    permission_id: String,
    #[serde(rename = "emailAddress", default)]
    email: String,
    #[serde(rename = "displayName", default)]
    name: String,
}

/// Who a token belongs to: the account id, and what to call them.
///
/// The id is Drive's `permissionId`, which is stable for an account and is also
/// what a permission on a shared folder is keyed by — so the same value both
/// recognises this user's own author record and matches them against the people
/// a project is shared with.
pub fn who_am_i(access_token: &str) -> Result<(String, String, String)> {
    let client = reqwest::blocking::Client::new();
    let about: About = json(
        client
            .get(format!("{API}/about"))
            .bearer_auth(access_token)
            .query(&[("fields", "user(permissionId, emailAddress, displayName)")])
            .send()
            .context("asking Google who is signed in")?,
    )?;
    Ok((
        format!("google:{}", about.user.permission_id),
        about.user.name,
        about.user.email,
    ))
}

// --- who a project is shared with ------------------------------------------

/// One person a project's folder is shared with.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Member {
    pub id: String,
    /// `owner`, `writer`, `commenter` or `reader`, as Drive enforces them.
    pub role: String,
    #[serde(rename = "emailAddress", default)]
    pub email: String,
    #[serde(rename = "displayName", default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct Permissions {
    #[serde(default)]
    permissions: Vec<Member>,
}

/// Everyone the project's folder is shared with, owner included.
///
/// The roles are Drive's and are enforced by Drive, which is the whole reason
/// there is no access control of our own to write: reader, writer and owner are
/// real because Google says no, not because the app declines to draw a button.
pub fn members(access_token: &str, folder: &str) -> Result<Vec<Member>> {
    let client = reqwest::blocking::Client::new();
    let listed: Permissions = json(
        client
            .get(format!("{API}/files/{folder}/permissions"))
            .bearer_auth(access_token)
            .query(&[(
                "fields",
                "permissions(id, role, emailAddress, displayName)",
            )])
            .send()
            .context("listing who this project is shared with")?,
    )?;
    Ok(listed.permissions)
}

/// Share the project's folder with somebody, as a reader or a writer.
///
/// Google sends the invitation and enforces the outcome, so this is the whole
/// of "sharing" — there is no Lazuli account for them to make and no server of
/// ours for them to reach.
pub fn share_with(access_token: &str, folder: &str, email: &str, role: &str) -> Result<Member> {
    if !matches!(role, "reader" | "writer") {
        bail!("{role} is not a role this offers");
    }
    let client = reqwest::blocking::Client::new();
    json(
        client
            .post(format!("{API}/files/{folder}/permissions"))
            .bearer_auth(access_token)
            .query(&[
                ("sendNotificationEmail", "true"),
                ("fields", "id, role, emailAddress, displayName"),
            ])
            .json(&serde_json::json!({
                "type": "user",
                "role": role,
                "emailAddress": email,
            }))
            .send()
            .context("sharing the project")?,
    )
}

/// Stop sharing with somebody.
pub fn unshare(access_token: &str, folder: &str, permission: &str) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    check(
        client
            .delete(format!("{API}/files/{folder}/permissions/{permission}"))
            .bearer_auth(access_token)
            .send()
            .context("removing someone from the project")?,
    )?;
    Ok(())
}
