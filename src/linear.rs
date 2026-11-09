//! Linear: assigned issues become tasks (`ext = linear:<identifier>`), and completing such a
//! task in Notchpad moves the issue to its team's completed state.
//!
//! OAuth credentials and the legacy personal API key live only in the macOS Keychain. HTTP goes
//! through `curl`, which reads URLs, headers and bodies from a config on stdin, so credentials
//! never appear in a process listing. OAuth authorization and every request run on background
//! threads; the main thread is only told to pick up results.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use objc2::msg_send;
use objc2::runtime::{AnyObject, Sel};
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde_json::{Value, json};

use crate::store::Task;

const ENDPOINT: &str = "https://api.linear.app/graphql";
const TOKEN_ENDPOINT: &str = "https://api.linear.app/oauth/token";
const AUTHORIZE_ENDPOINT: &str = "https://linear.app/oauth/authorize";
const SERVICE: &str = "notchpad.linear";
const API_KEY_ACCOUNT: &str = "api-key";
const OAUTH_ACCOUNT: &str = "oauth-token";
const CALLBACK_ADDR: &str = "127.0.0.1:53682";
const REDIRECT_URI: &str = "http://127.0.0.1:53682/oauth/callback";
const CALLBACK_PATH: &str = "/oauth/callback";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SYNC_EVERY: Duration = Duration::from_secs(10 * 60);
const EXT_PREFIX: &str = "linear:";

const VIEWER: &str = "query { viewer { id } }";
const ASSIGNED: &str = r#"query { viewer { assignedIssues(filter: {state: {type: {nin: ["completed", "canceled"]}}}) { nodes { id identifier title dueDate } } } }"#;
const COMPLETED_STATE: &str = r#"query($id: String!) { issue(id: $id) { id team { states(filter: {type: {eq: "completed"}}) { nodes { id } } } } }"#;
const SET_STATE: &str = "mutation($id: String!, $state: String!) { issueUpdate(id: $id, input: {stateId: $state}) { success } }";

static AUTH_GENERATION: AtomicU64 = AtomicU64::new(0);
static AUTH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static AUTH_TRANSITION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static DISCONNECT_PENDING: AtomicBool = AtomicBool::new(false);

/// An open issue assigned to the user.
#[derive(Debug, PartialEq)]
pub struct Issue {
    pub identifier: String,
    pub title: String,
    /// `YYYY-MM-DD`, or empty.
    pub due: String,
}

// ---- OAuth --------------------------------------------------------------------------------

/// Linear's client ID is public, but must be supplied by the app distributor after registering
/// the exact loopback redirect URI. A shell launch can use the runtime variable; app builds can
/// set `NOTCHPAD_LINEAR_CLIENT_ID` when compiling.
pub fn oauth_client_id() -> Option<String> {
    let configured = option_env!("NOTCHPAD_LINEAR_CLIENT_ID")
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| std::env::var("NOTCHPAD_LINEAR_CLIENT_ID").ok());
    configured.filter(|id| !id.trim().is_empty())
}

pub fn oauth_configuration_error() -> String {
    format!(
        "Linear OAuth is not configured. Set NOTCHPAD_LINEAR_CLIENT_ID when building or launching Notchpad, and register {REDIRECT_URI} as the redirect URI."
    )
}

/// A pending browser authorization. The UI polls the receiver on the main thread, so the worker
/// never retains or messages an AppKit object after the window has gone away.
pub struct OAuthAttempt {
    receiver: Receiver<Result<(), String>>,
    cancel: Arc<AtomicBool>,
}

impl OAuthAttempt {
    pub fn try_result(&self) -> Option<Result<(), String>> {
        self.receiver.try_recv().ok()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        AUTH_GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Starts the browser authorization flow without doing any network or waiting on the UI thread.
pub fn start_oauth() -> Result<OAuthAttempt, String> {
    let client_id = oauth_client_id().ok_or_else(oauth_configuration_error)?;
    let generation = reserve_oauth_start()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = oauth_flow(&client_id, generation, &worker_cancel);
        let _ = sender.send(result);
    });
    Ok(OAuthAttempt { receiver, cancel })
}

fn oauth_flow(client_id: &str, generation: u64, cancel: &AtomicBool) -> Result<(), String> {
    let listener = TcpListener::bind(CALLBACK_ADDR)
        .map_err(|_| format!("Could not listen for Linear's callback on {CALLBACK_ADDR}; close another OAuth attempt and try again"))?;
    listener.set_nonblocking(true).map_err(|err| format!("Linear callback listener: {err}"))?;
    let verifier = random_url_token()?;
    let state = random_url_token()?;
    let challenge = url_token(&sha256(verifier.as_bytes()));
    let url = format!(
        "{AUTHORIZE_ENDPOINT}?client_id={}&redirect_uri={}&response_type=code&scope=read%2Cwrite&state={}&code_challenge={}&code_challenge_method=S256",
        form_component(client_id),
        form_component(REDIRECT_URI),
        form_component(&state),
        form_component(&challenge),
    );
    Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|err| format!("Could not open Linear in the default browser: {err}"))?;

    let code = receive_callback(listener, &state, cancel)?;
    if cancel.load(Ordering::Acquire) {
        return Err("Linear authorization canceled".to_owned());
    }
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", &verifier),
    ]);
    let reply = post_form(TOKEN_ENDPOINT, &body)?;
    let token = parse_token_response(&reply, None)?;
    save_oauth(&token, generation)
}

#[derive(Debug, Clone, PartialEq)]
struct TokenSet {
    access: String,
    refresh: String,
    expires_at: u64,
}

fn auth_lock() -> &'static Mutex<()> {
    AUTH_LOCK.get_or_init(|| Mutex::new(()))
}

fn auth_transition_lock() -> &'static Mutex<()> {
    AUTH_TRANSITION_LOCK.get_or_init(|| Mutex::new(()))
}

fn reserve_oauth_start() -> Result<u64, String> {
    let _gate = auth_transition_lock().try_lock().map_err(|_| "Linear is finishing a disconnect; please try Connect again shortly".to_owned())?;
    if DISCONNECT_PENDING.load(Ordering::Acquire) {
        return Err("Linear is finishing a disconnect; please try Connect again shortly".to_owned());
    }
    Ok(AUTH_GENERATION.fetch_add(1, Ordering::AcqRel) + 1)
}

fn save_oauth(token: &TokenSet, generation: u64) -> Result<(), String> {
    let _guard = auth_lock().lock().map_err(|_| "Linear credential lock failed".to_owned())?;
    save_oauth_locked(token, generation)
}

fn save_oauth_locked(token: &TokenSet, generation: u64) -> Result<(), String> {
    if AUTH_GENERATION.load(Ordering::Acquire) != generation {
        return Err("Linear credentials were disconnected during authorization".to_owned());
    }
    let value = json!({"access_token": token.access, "refresh_token": token.refresh, "expires_at": token.expires_at});
    set_generic_password(SERVICE, OAUTH_ACCOUNT, value.to_string().as_bytes()).map_err(|err| format!("Keychain: {err}"))
}

fn oauth_tokens() -> Result<Option<TokenSet>, String> {
    let bytes = match get_generic_password(SERVICE, OAUTH_ACCOUNT) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "The Linear OAuth token in the Keychain is not valid".to_owned())?;
    let access = value["access_token"].as_str().filter(|s| !s.is_empty()).ok_or("The Linear OAuth token has no access token")?;
    let refresh = value["refresh_token"].as_str().filter(|s| !s.is_empty()).ok_or("The Linear OAuth token has no refresh token")?;
    let expires_at = value["expires_at"].as_u64().ok_or("The Linear OAuth token has no expiry")?;
    Ok(Some(TokenSet { access: access.to_owned(), refresh: refresh.to_owned(), expires_at }))
}

fn access_token() -> Result<String, String> {
    let generation = AUTH_GENERATION.load(Ordering::Acquire);
    let Some(token) = oauth_tokens()? else { return legacy_key() };
    if token.expires_at > now().saturating_add(30) {
        return Ok(format!("Bearer {}", token.access));
    }
    Ok(format!("Bearer {}", refresh_oauth(generation)?))
}

/// Refresh is a single-flight operation. The Keychain is re-read after acquiring the lock so
/// callers that observed the same expired token reuse a rotated result instead of racing it.
fn refresh_oauth(expected_generation: u64) -> Result<String, String> {
    let _guard = auth_lock().lock().map_err(|_| "Linear credential lock failed".to_owned())?;
    if AUTH_GENERATION.load(Ordering::Acquire) != expected_generation {
        return Err("Linear credentials were disconnected during refresh".to_owned());
    }
    let old = oauth_tokens()?.ok_or("Linear OAuth credentials disappeared")?;
    if old.expires_at > now().saturating_add(30) {
        return Ok(old.access);
    }
    let generation = expected_generation;
    let client_id = oauth_client_id().ok_or_else(oauth_configuration_error)?;
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &old.refresh),
        ("client_id", &client_id),
    ]);
    let reply = post_form(TOKEN_ENDPOINT, &body)?;
    let token = parse_token_response(&reply, Some(&old))?;
    let access = token.access.clone();
    save_oauth_locked(&token, generation)?;
    Ok(access)
}

fn parse_token_response(text: &str, old: Option<&TokenSet>) -> Result<TokenSet, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "Linear's token response is not JSON".to_owned())?;
    if let Some(error) = value["error_description"].as_str().or_else(|| value["error"].as_str()) {
        return Err(format!("Linear OAuth failed: {error}"));
    }
    let access = value["access_token"].as_str().filter(|s| !s.is_empty()).ok_or("Linear's token response has no access token")?;
    let refresh = value["refresh_token"].as_str().filter(|s| !s.is_empty()).or_else(|| old.map(|token| token.refresh.as_str())).ok_or("Linear's token response has no refresh token")?;
    let expires_in = value["expires_in"].as_u64().ok_or("Linear's token response has no expiry")?;
    Ok(TokenSet { access: access.to_owned(), refresh: refresh.to_owned(), expires_at: now().saturating_add(expires_in) })
}

fn receive_callback(listener: TcpListener, expected_state: &str, cancel: &AtomicBool) -> Result<String, String> {
    receive_callback_until(listener, expected_state, cancel, CALLBACK_TIMEOUT)
}

fn receive_callback_until(listener: TcpListener, expected_state: &str, cancel: &AtomicBool, timeout: Duration) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|err| format!("Linear callback listener: {err}"))?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Acquire) {
            return Err("Linear authorization canceled".to_owned());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if stream.set_read_timeout(Some(Duration::from_millis(100))).is_err() {
                    continue;
                }
                let request = match read_callback_request(&mut stream) {
                    Ok(request) => request,
                    Err(_) => continue,
                };
                let Some(target) = request.strip_prefix("GET ").and_then(|request| request.split_once(' ').map(|(target, _)| target)) else {
                    respond(&mut stream, "Notchpad is waiting for Linear's callback.");
                    continue;
                };
                let Some((path, query)) = target.split_once('?') else {
                    respond(&mut stream, "Notchpad is waiting for Linear's callback.");
                    continue;
                };
                if path != CALLBACK_PATH {
                    respond(&mut stream, "This callback is not for Notchpad.");
                    continue;
                }
                let params = match parse_query(query) {
                    Ok(params) => params,
                    Err(error) => {
                        respond(&mut stream, "The Linear callback was malformed; return to Notchpad and try again.");
                        eprintln!("notchpad: malformed Linear callback: {error}");
                        continue;
                    }
                };
                if params.get("state").map(String::as_str) != Some(expected_state) {
                    respond(&mut stream, "The Linear authorization state was invalid; return to Notchpad and try again.");
                    continue;
                }
                if let Some(error) = params.get("error") {
                    let description = params.get("error_description").unwrap_or(error);
                    respond(&mut stream, "Linear authorization was denied. You can close this tab.");
                    return Err(format!("Linear authorization was denied: {description}"));
                }
                let Some(code) = params.get("code").filter(|code| !code.is_empty()) else {
                    respond(&mut stream, "Linear did not return an authorization code.");
                    return Err("Linear did not return an authorization code".to_owned());
                };
                respond(&mut stream, "Linear is connected. You can close this tab.");
                return Ok(code.clone());
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(40)),
            Err(err) => return Err(format!("Linear callback listener: {err}")),
        }
    }
    if cancel.load(Ordering::Acquire) {
        Err("Linear authorization canceled".to_owned())
    } else {
        Err("Timed out waiting for Linear authorization".to_owned())
    }
}

fn read_callback_request(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 2048];
    while bytes.len() < 64 * 1024 {
        let count = stream.read(&mut chunk).map_err(|err| err.to_string())?;
        if count == 0 { break; }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") { break; }
    }
    let line = String::from_utf8(bytes).map_err(|_| "Linear callback was not valid HTTP".to_owned())?;
    line.lines().next().ok_or("Linear callback was empty".to_owned()).map(str::to_owned)
}

fn respond(stream: &mut TcpStream, message: &str) {
    let body = format!("<html><body>{message}</body></html>");
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.shutdown(Shutdown::Both);
}

fn parse_query(query: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut params = std::collections::HashMap::new();
    for part in query.split('&') {
        let (key, value) = part.split_once('=').ok_or("query parameter has no '='")?;
        let key = percent_decode(key).ok_or("query parameter has invalid percent encoding")?;
        let value = percent_decode(value).ok_or("query parameter has invalid percent encoding")?;
        if key.is_empty() {
            return Err("query parameter has an empty name".to_owned());
        }
        if params.insert(key, value).is_some() {
            return Err("query parameter is duplicated".to_owned());
        }
    }
    Ok(params)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut chars = text.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        match byte {
            b'+' => bytes.push(b' '),
            b'%' => {
                let hi = hex_digit(chars.next()?)?;
                let lo = hex_digit(chars.next()?)?;
                bytes.push((hi << 4) | lo);
            }
            byte => bytes.push(byte),
        }
    }
    String::from_utf8(bytes).ok()
}

fn random_url_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    let mut random = std::fs::File::open("/dev/urandom").map_err(|err| format!("Secure random source: {err}"))?;
    random.read_exact(&mut bytes).map_err(|err| format!("Secure random source: {err}"))?;
    Ok(url_token(&bytes))
}

fn url_token(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for chunk in bytes.chunks(3) {
        let value = ((chunk[0] as u32) << 16) | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8) | chunk.get(2).copied().unwrap_or(0) as u32;
        result.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { result.push(ALPHABET[((value >> 6) & 63) as usize] as char); }
        if chunk.len() > 2 { result.push(ALPHABET[(value & 63) as usize] as char); }
    }
    result
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(key, value)| format!("{}={}", form_component(key), form_component(value))).collect::<Vec<_>>().join("&")
}

fn form_component(text: &str) -> String {
    text.bytes().flat_map(|byte| match byte {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => vec![byte as char],
        byte => format!("%{byte:02X}").chars().collect(),
    }).collect()
}

// FIPS 180-4 SHA-256, kept here to avoid adding a cryptography dependency for one PKCE digest.
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70,
        0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee,
        0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 { padded.push(0); }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, bytes) in block.chunks_exact(4).take(16).enumerate() { w[i] = u32::from_be_bytes(bytes.try_into().unwrap()); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(temp1), c, b, a, temp1.wrapping_add(temp2));
        }
        for (value, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) { *value = (*value).wrapping_add(add); }
    }
    let mut result = [0u8; 32];
    for (bytes, value) in result.chunks_exact_mut(4).zip(h) { bytes.copy_from_slice(&value.to_be_bytes()); }
    result
}

// ---- Keychain ----------------------------------------------------------------------------

/// Saves a legacy personal key after checking that Linear accepts it, preserving compatibility
/// with installations that connected before OAuth was available.
pub fn connect(key: &str) -> Result<(), String> {
    request_with(key, VIEWER, json!({})).and_then(|text| data(&text))?;
    set_generic_password(SERVICE, API_KEY_ACCOUNT, key.as_bytes()).map_err(|err| format!("Keychain: {err}"))
}

pub fn disconnect() {
    let Ok(_gate) = auth_transition_lock().lock() else { return };
    AUTH_GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Ok(_guard) = auth_lock().try_lock() {
        delete_credentials();
        return;
    }
    DISCONNECT_PENDING.store(true, Ordering::Release);
    thread::spawn(|| {
        let Ok(_guard) = auth_lock().lock() else { return };
        let Ok(_gate) = auth_transition_lock().lock() else { return };
        delete_credentials();
        DISCONNECT_PENDING.store(false, Ordering::Release);
    });
}

fn delete_credentials() {
    let _ = delete_generic_password(SERVICE, API_KEY_ACCOUNT);
    let _ = delete_generic_password(SERVICE, OAUTH_ACCOUNT);
}

fn legacy_key() -> Result<String, String> {
    let bytes = get_generic_password(SERVICE, API_KEY_ACCOUNT).map_err(|_| "No Linear credentials in the Keychain".to_owned())?;
    String::from_utf8(bytes).map_err(|_| "The Linear key in the Keychain is not text".to_owned())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

// ---- Sync --------------------------------------------------------------------------------

/// When the assigned issues were last fetched and the fetch in flight, if any.
#[derive(Default)]
pub struct Linear {
    last: Option<Instant>,
    pending: Option<Receiver<Result<Vec<Issue>, String>>>,
}

impl Linear {
    /// Whether the next fetch is due: none yet, or the last one was ten minutes ago. A failed fetch counts,
    /// so a dead network or a revoked credential is retried at the same slow pace instead of in a loop.
    pub fn due(&self) -> bool {
        self.pending.is_none() && self.last.is_none_or(|at| at.elapsed() >= SYNC_EVERY)
    }

    /// Forgets the schedule and any fetch in flight; the next check fetches at once.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Fetches on a background thread, then has `target` call `selector` on the main thread.
    pub fn fetch(&mut self, target: &AnyObject, selector: Sel) {
        let (sender, receiver) = mpsc::channel();
        self.last = Some(Instant::now());
        self.pending = Some(receiver);
        let target = target as *const AnyObject as usize;
        thread::spawn(move || {
            let issues = request(ASSIGNED, json!({})).and_then(|text| parse_issues(&text));
            if sender.send(issues).is_ok() {
                let target = target as *const AnyObject;
                let _: () = unsafe { msg_send![target, performSelectorOnMainThread: selector, withObject: std::ptr::null::<AnyObject>(), waitUntilDone: false] };
            }
        });
    }

    /// The finished fetch, if there is one.
    pub fn take(&mut self) -> Option<Result<Vec<Issue>, String>> {
        let result = self.pending.as_ref()?.try_recv().ok()?;
        self.pending = None;
        Some(result)
    }

    /// Moves the issue behind a task's `ext` to its team's completed state. Failures only go to stderr.
    pub fn complete(ext: &str) {
        let Some(identifier) = ext.strip_prefix(EXT_PREFIX).map(str::to_owned) else { return };
        thread::spawn(move || {
            if let Err(err) = set_completed(&identifier) {
                eprintln!("notchpad: could not complete {identifier} in Linear: {err}");
            }
        });
    }
}

fn set_completed(identifier: &str) -> Result<(), String> {
    let lookup = request(COMPLETED_STATE, json!({"id": identifier}))?;
    let (issue, state) = parse_completed_state(&lookup)?;
    let reply = request(SET_STATE, json!({"id": issue, "state": state}))?;
    match data(&reply)?["issueUpdate"]["success"].as_bool() {
        Some(true) => Ok(()),
        _ => Err("Linear did not accept the update".to_owned()),
    }
}

// ---- HTTP --------------------------------------------------------------------------------

fn request(query: &str, variables: Value) -> Result<String, String> {
    let generation = AUTH_GENERATION.load(Ordering::Acquire);
    let auth = access_token()?;
    let oauth = oauth_tokens()?.is_some();
    let reply = request_with(&auth, query, variables.clone())?;
    if oauth && graphql_auth_failure(&reply) {
        return request_with(&format!("Bearer {}", refresh_oauth(generation)?), query, variables);
    }
    Ok(reply)
}

fn graphql_auth_failure(reply: &str) -> bool {
    let lower = reply.to_ascii_lowercase();
    lower.contains("unauthorized") || lower.contains("forbidden") || lower.contains("invalid token") || lower.contains("authentication")
}

fn request_with(auth: &str, query: &str, variables: Value) -> Result<String, String> {
    post(ENDPOINT, auth, &json!({"query": query, "variables": variables}).to_string())
}

fn post(url: &str, auth: &str, body: &str) -> Result<String, String> {
    curl(url, &[&format!("Authorization: {auth}"), "Content-Type: application/json"], body)
}

fn post_form(url: &str, body: &str) -> Result<String, String> {
    curl(url, &["Content-Type: application/x-www-form-urlencoded"], body)
}

fn curl(url: &str, headers: &[&str], body: &str) -> Result<String, String> {
    let mut curl = Command::new("curl")
        .args(["--silent", "--show-error", "--max-time", "20", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("curl: {err}"))?;
    let mut config = format!("url = {}\nrequest = \"POST\"\n", quote(url));
    for header in headers { config.push_str(&format!("header = {}\n", quote(header))); }
    config.push_str(&format!("data = {}\n", quote(body)));
    curl.stdin.take().ok_or("curl has no stdin")?.write_all(config.as_bytes()).map_err(|err| err.to_string())?;
    let output = curl.wait_with_output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.lines().next().unwrap_or("curl failed").to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A curl config that POSTs `body` to `url`; credentials are passed in `auth` only in stdin.
fn curl_config(url: &str, auth: &str, body: &str) -> String {
    format!("url = {}\nrequest = \"POST\"\nheader = {}\nheader = \"Content-Type: application/json\"\ndata = {}\n", quote(url), quote(&format!("Authorization: {auth}")), quote(body))
}

/// `text` as a curl config string: double-quoted, with `\\`, `"` and line breaks escaped.
fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r");
    format!("\"{escaped}\"")
}

// ---- Parsing -----------------------------------------------------------------------------

/// The `data` of a GraphQL reply, or the first error Linear reported.
fn data(json: &str) -> Result<Value, String> {
    let mut reply: Value = serde_json::from_str(json).map_err(|_| "Linear sent a reply that is not JSON".to_owned())?;
    if let Some(message) = reply["errors"][0]["message"].as_str() {
        return Err(message.to_owned());
    }
    Ok(reply["data"].take())
}

pub fn parse_issues(json: &str) -> Result<Vec<Issue>, String> {
    let nodes = data(json)?["viewer"]["assignedIssues"]["nodes"].take();
    let nodes = nodes.as_array().ok_or("Linear's reply has no issues list")?;
    let text = |node: &Value, field: &str| node[field].as_str().unwrap_or_default().to_owned();
    Ok(nodes.iter().filter(|node| node["identifier"].as_str().is_some_and(|id| !id.is_empty())).map(|node| Issue { identifier: text(node, "identifier"), title: text(node, "title"), due: text(node, "dueDate") }).collect())
}

/// The issue's own id and the id of its team's first completed state.
fn parse_completed_state(json: &str) -> Result<(String, String), String> {
    let issue = data(json)?["issue"].take();
    let id = issue["id"].as_str().ok_or("Linear did not find the issue")?;
    let state = issue["team"]["states"]["nodes"][0]["id"].as_str().ok_or("The issue's team has no completed state")?;
    Ok((id.to_owned(), state.to_owned()))
}

/// Adds a task for each issue that has none yet and returns how many it added.
pub fn import(tasks: &mut Vec<Task>, issues: &[Issue], today: &str) -> usize {
    let before = tasks.len();
    for issue in issues {
        let ext = format!("{EXT_PREFIX}{}", issue.identifier);
        if tasks.iter().any(|task| task.ext == ext) { continue; }
        let text = issue.title.replace(['\t', '\n', '\r'], " ");
        let day = if issue.due.is_empty() { today } else { &issue.due };
        tasks.push(Task { text, day: day.to_owned(), ext, ..Task::default() });
    }
    tasks.len() - before
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    const REPLY: &str = r#"{"data":{"viewer":{"assignedIssues":{"nodes":[
        {"id":"u1","identifier":"ENG-1","title":"Fix login","dueDate":"2026-11-12"},
        {"id":"u2","identifier":"ENG-2","title":"Write\tdocs","dueDate":null}]}}}}"#;

    fn issue(identifier: &str, due: &str) -> Issue {
        Issue { identifier: identifier.to_owned(), title: format!("Issue {identifier}"), due: due.to_owned() }
    }

    #[test]
    fn pkce_uses_the_rfc_sha256_challenge() {
        assert_eq!(url_token(&sha256(b"abc")), "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0");
    }

    #[test]
    fn callback_rejects_wrong_state_before_accepting_the_real_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = AtomicBool::new(false);
        let server = thread::spawn(move || receive_callback_until(listener, "right", &cancel, CALLBACK_TIMEOUT).unwrap());
        for state in ["wrong", "right"] {
            let mut stream = TcpStream::connect(address).unwrap();
            let _ = write!(stream, "GET /oauth/callback?code=abc&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if state == "wrong" { assert!(response.contains("invalid")); }
        }
        assert_eq!(server.join().unwrap(), "abc");
    }

    #[test]
    fn token_refresh_rotation_preserves_old_refresh_token_when_omitted() {
        let old = TokenSet { access: "old".into(), refresh: "refresh".into(), expires_at: 0 };
        let token = parse_token_response(r#"{"access_token":"new","expires_in":60}"#, Some(&old)).unwrap();
        assert_eq!(token, TokenSet { access: "new".into(), refresh: "refresh".into(), expires_at: now() + 60 });
    }

    #[test]
    fn callback_rejects_duplicate_and_bad_percent_parameters() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let server = thread::spawn(move || receive_callback_until(listener, "right", &worker_cancel, CALLBACK_TIMEOUT));
        for query in [
            "code=abc&state=right&state=right",
            "code=%ZZ&state=right",
            "code=abc&state=wrong",
        ] {
            let mut stream = TcpStream::connect(address).unwrap();
            let _ = write!(stream, "GET /oauth/callback?{query} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            assert!(response.contains("callback") || response.contains("state"));
        }
        cancel.store(true, Ordering::Release);
        assert_eq!(server.join().unwrap(), Err("Linear authorization canceled".to_owned()));
    }

    #[test]
    fn callback_timeout_and_cancel_are_prompt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let started = Instant::now();
        let server = thread::spawn(move || receive_callback_until(listener, "state", &worker_cancel, Duration::from_millis(20)));
        assert_eq!(server.join().unwrap(), Err("Timed out waiting for Linear authorization".to_owned()));
        assert!(started.elapsed() < Duration::from_secs(1));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let server = thread::spawn(move || receive_callback_until(listener, "state", &worker_cancel, CALLBACK_TIMEOUT));
        thread::sleep(Duration::from_millis(20));
        cancel.store(true, Ordering::Release);
        assert_eq!(server.join().unwrap(), Err("Linear authorization canceled".to_owned()));
    }

    #[test]
    fn oauth_start_is_rejected_while_disconnect_cleanup_is_pending() {
        let generation = AUTH_GENERATION.load(Ordering::Acquire);
        DISCONNECT_PENDING.store(true, Ordering::Release);
        assert!(reserve_oauth_start().is_err());
        DISCONNECT_PENDING.store(false, Ordering::Release);
        assert!(reserve_oauth_start().is_ok());
        AUTH_GENERATION.store(generation, Ordering::Release);
    }

    #[test]
    fn refresh_lock_serializes_concurrent_refresh_attempts() {
        let guard = auth_lock().lock().unwrap();
        let completed = Arc::new(AtomicBool::new(false));
        let worker_completed = Arc::clone(&completed);
        let worker = thread::spawn(move || {
            let _guard = auth_lock().lock().unwrap();
            worker_completed.store(true, Ordering::Release);
        });
        thread::sleep(Duration::from_millis(20));
        assert!(!completed.load(Ordering::Acquire));
        drop(guard);
        worker.join().unwrap();
        assert!(completed.load(Ordering::Acquire));
    }

    #[test]
    fn disconnected_generation_rejects_late_token_storage() {
        let generation = AUTH_GENERATION.load(Ordering::Acquire);
        AUTH_GENERATION.store(generation.wrapping_add(1), Ordering::Release);
        let token = TokenSet { access: "access".into(), refresh: "refresh".into(), expires_at: now() + 60 };
        assert!(save_oauth_locked(&token, generation).is_err());
        AUTH_GENERATION.store(generation, Ordering::Release);
    }

    #[test]
    fn oauth_configuration_names_exact_redirect_uri() {
        assert!(oauth_configuration_error().contains(REDIRECT_URI));
    }

    #[test]
    fn oauth_token_forms_use_client_id_and_no_client_secret() {
        let authorization = form(&[
            ("grant_type", "authorization_code"),
            ("code", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", "client"),
            ("code_verifier", "verifier"),
        ]);
        let refresh = form(&[("grant_type", "refresh_token"), ("refresh_token", "refresh"), ("client_id", "client")]);
        assert!(authorization.contains("client_id=client"));
        assert!(authorization.contains("code_verifier=verifier"));
        assert!(refresh.contains("client_id=client"));
        assert!(!authorization.contains("client_secret"));
        assert!(!refresh.contains("client_secret"));
    }

    #[test]
    fn assigned_issues_are_parsed_with_missing_due_dates_empty() {
        let issues = parse_issues(REPLY).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0], Issue { identifier: "ENG-1".into(), title: "Fix login".into(), due: "2026-11-12".into() });
        assert_eq!(issues[1].due, "");
    }

    #[test]
    fn errors_and_garbage_are_reported_not_parsed() {
        assert_eq!(parse_issues(r#"{"errors":[{"message":"Authentication required"}]}"#), Err("Authentication required".to_owned()));
        assert!(parse_issues("<html>").is_err());
        assert!(parse_issues(r#"{"data":{"viewer":null}}"#).is_err());
        assert_eq!(parse_issues(r#"{"data":{"viewer":{"assignedIssues":{"nodes":[]}}}}"#), Ok(Vec::new()));
    }

    #[test]
    fn import_keys_tasks_by_identifier_and_places_them_by_due_date() {
        let mut tasks = Vec::new();
        let added = import(&mut tasks, &parse_issues(REPLY).unwrap(), "2026-11-09");
        assert_eq!(added, 2);
        assert_eq!((tasks[0].ext.as_str(), tasks[0].day.as_str(), tasks[0].text.as_str()), ("linear:ENG-1", "2026-11-12", "Fix login"));
        assert_eq!((tasks[1].day.as_str(), tasks[1].text.as_str()), ("2026-11-09", "Write docs"));
    }

    #[test]
    fn import_skips_issues_that_already_have_a_task_even_a_finished_one() {
        let mut tasks = vec![Task { done: true, ext: "linear:ENG-1".into(), ..Task::default() }, Task { text: "Own".into(), ..Task::default() }];
        let issues = [issue("ENG-1", ""), issue("ENG-3", ""), issue("ENG-3", "")];
        assert_eq!(import(&mut tasks, &issues, "2026-11-09"), 1);
        assert_eq!(import(&mut tasks, &issues, "2026-11-09"), 0);
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].done);
    }

    #[test]
    fn completed_state_comes_from_the_issues_team() {
        let reply = r#"{"data":{"issue":{"id":"uuid-1","team":{"states":{"nodes":[{"id":"done-state"},{"id":"other"}]}}}}}"#;
        assert_eq!(parse_completed_state(reply), Ok(("uuid-1".to_owned(), "done-state".to_owned())));
        assert!(parse_completed_state(r#"{"data":{"issue":null}}"#).is_err());
        assert!(parse_completed_state(r#"{"data":{"issue":{"id":"u","team":{"states":{"nodes":[]}}}}}"#).is_err());
    }

    /// Runs the real `curl` against a one-shot server on the loopback interface (no outside network).
    #[test]
    fn a_post_reaches_the_server_with_header_and_body_intact() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/graphql", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&request).into_owned();
                let Some((head, body)) = text.split_once("\r\n\r\n") else { continue };
                let length = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                if body.len() >= length { break; }
            }
            let reply = r#"{"data":{"ok":true}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        let body = json!({"query": "q \"x\"", "variables": {"t": "a\nb\\c"}}).to_string();
        assert_eq!(post(&url, "test-key", &body).unwrap(), r#"{"data":{"ok":true}}"#);
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /graphql"));
        assert!(request.contains("Authorization: test-key\r\n"));
        assert!(request.ends_with(&body));
    }

    #[test]
    fn curl_config_carries_the_key_and_escapes_the_body() {
        let body = json!({"query": "q \"x\"", "variables": {"t": "a\nb\\c"}}).to_string();
        let config = curl_config(ENDPOINT, "test-key", &body);
        assert!(config.contains("header = \"Authorization: test-key\"\n"));
        assert!(config.contains("url = \"https://api.linear.app/graphql\"\n"));
        assert!(config.lines().all(|line| line.starts_with("url") || line.starts_with("request") || line.starts_with("header") || line.starts_with("data")));
        let data_line = config.lines().find(|l| l.starts_with("data = ")).unwrap();
        assert!(data_line.contains(r#"\"query\":\"q \\\"x\\\"\"#));
    }
}
