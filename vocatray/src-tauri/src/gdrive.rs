//! 구글 연동 (PLAN.md §6.4): 구글 로그인과 드라이브 앱 전용 숨김 폴더(`appDataFolder`) 읽고 쓰기.
//! 다른 서비스로 바꾸려면 이 파일만 바꾸면 된다.
//!
//! 로그인은 데스크톱 앱 방식 — 기본 브라우저로 구글 로그인 화면을 열고, 결과를 `127.0.0.1:<임의 포트>`에서
//! 받는다 (PKCE). 받은 refresh token은 OS 자격 증명 저장소에만 둔다.
//! 개인정보처리방침(full-fish.com/privacy/vocatray)에 적은 대로 범위는 `drive.appdata` 하나만 쓴다.

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

const SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
/// 브라우저에서 로그인을 기다리는 최대 시간
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const KEYRING_SERVICE: &str = "com.vocatray.desktop";
const KEYRING_USER: &str = "google-refresh-token";

type Response = ureq::http::Response<ureq::Body>;

/// 개발자가 Google Cloud에서 받은 데스크톱 앱 클라이언트 (PLAN.md §8 Phase 2).
/// 설치형 앱의 클라이언트 비밀값은 구글도 비밀로 보지 않는다 — 보안은 PKCE가 맡는다.
const CLIENT_JSON: &str = include_str!("../google-client.json");

#[derive(Deserialize)]
struct ClientFile {
    installed: Client,
}

#[derive(Clone, Deserialize)]
struct Client {
    client_id: String,
    client_secret: String,
}

fn client() -> Client {
    // 형식은 테스트(client_file_is_valid)로 확인한다.
    serde_json::from_str::<ClientFile>(CLIENT_JSON).expect("google-client.json 형식 오류").installed
}

#[derive(Debug)]
pub enum Error {
    /// 로그인이 풀렸다 (권한 취소·만료) — 다시 로그인해야 한다.
    Unauthorized,
    /// 드라이브에 그 파일이 없다.
    NotFound,
    /// 그 밖의 실패 (오프라인, 서버 오류, 로그인 취소 등) — 다음에 다시 시도한다.
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unauthorized => write!(f, "로그인이 만료됨"),
            Error::NotFound => write!(f, "파일이 없음"),
            Error::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::StatusCode(404) => Error::NotFound,
            e => Error::Failed(e.to_string()),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Failed(e.to_string())
    }
}

impl From<keyring::Error> for Error {
    fn from(e: keyring::Error) -> Self {
        Error::Failed(format!("자격 증명 저장소: {e}"))
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub id: String,
    pub name: String,
    pub modified_time: String,
}

/// 로그인한 상태 — access token은 1시간짜리라 만료 1분 전에 refresh token으로 새로 받는다.
pub struct Session {
    agent: ureq::Agent,
    client: Client,
    refresh_token: String,
    access: Option<(String, Instant)>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT)).build().into()
}

fn keyring_entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
}

impl Session {
    /// 전에 로그인해 둔 게 있으면 그걸로 시작한다.
    pub fn restore() -> Option<Session> {
        match keyring_entry().and_then(|e| e.get_password()) {
            Ok(refresh_token) => Some(Session { agent: agent(), client: client(), refresh_token, access: None }),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                eprintln!("동기화: 저장된 로그인을 읽을 수 없음: {e}");
                None
            }
        }
    }

    /// 기본 브라우저로 구글 로그인을 하고, 받은 토큰을 자격 증명 저장소에 저장한다. 최대 5분 기다린다.
    pub fn login() -> Result<Session, Error> {
        let client = client();
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let verifier = random_token()?;
        let state = random_token()?;
        let url = url::Url::parse_with_params(
            AUTH_URL,
            &[
                ("client_id", client.client_id.as_str()),
                ("redirect_uri", &redirect_uri),
                ("response_type", "code"),
                ("scope", SCOPE),
                ("code_challenge", &pkce_challenge(&verifier)),
                ("code_challenge_method", "S256"),
                ("state", &state),
                // 다시 연결할 때도 refresh token을 받도록
                ("access_type", "offline"),
                ("prompt", "consent"),
            ],
        )
        .expect("고정된 주소");
        open::that(url.as_str()).map_err(|e| Error::Failed(format!("브라우저를 열 수 없음: {e}")))?;
        let code = wait_for_code(&listener, &state)?;

        let agent = agent();
        let tokens: TokenResponse = agent
            .post(TOKEN_URL)
            .send_form([
                ("code", code.as_str()),
                ("client_id", &client.client_id),
                ("client_secret", &client.client_secret),
                ("redirect_uri", &redirect_uri),
                ("grant_type", "authorization_code"),
                ("code_verifier", &verifier),
            ])?
            .body_mut()
            .read_json()?;
        // 로그인 화면에서 드라이브 권한 체크를 풀고 허용할 수도 있다.
        if !tokens.scope.split(' ').any(|s| s == SCOPE) {
            return Err(Error::Failed("드라이브 권한을 허용해야 동기화할 수 있어요".into()));
        }
        let refresh_token =
            tokens.refresh_token.ok_or_else(|| Error::Failed("구글이 refresh token을 주지 않음".into()))?;
        keyring_entry()?.set_password(&refresh_token)?;
        let access = Some((tokens.access_token, expires_at(tokens.expires_in)));
        Ok(Session { agent, client, refresh_token, access })
    }

    /// 연결 끊기: 구글에 토큰 폐기를 요청하고(실패해도 넘어간다) 자격 증명 저장소에서 지운다.
    pub fn logout(self) {
        if let Err(e) = self.agent.post(REVOKE_URL).send_form([("token", self.refresh_token.as_str())]) {
            eprintln!("동기화: 토큰 폐기 요청 실패 (무시): {e}");
        }
        forget();
    }

    fn access_token(&mut self) -> Result<String, Error> {
        if let Some((token, until)) = &self.access
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let response = self.agent.post(TOKEN_URL).send_form([
            ("client_id", self.client.client_id.as_str()),
            ("client_secret", &self.client.client_secret),
            ("refresh_token", &self.refresh_token),
            ("grant_type", "refresh_token"),
        ]);
        let tokens: TokenResponse = match response {
            Ok(mut r) => r.body_mut().read_json()?,
            // invalid_grant — 사용자가 권한을 취소했거나 토큰이 만료됐다
            Err(ureq::Error::StatusCode(400 | 401)) => return Err(Error::Unauthorized),
            Err(e) => return Err(e.into()),
        };
        self.access = Some((tokens.access_token.clone(), expires_at(tokens.expires_in)));
        Ok(tokens.access_token)
    }

    /// 드라이브 요청 결과를 정리한다 — 401이면 access token을 버려 다음 요청 때 새로 받게 한다.
    fn check(&mut self, result: Result<Response, ureq::Error>) -> Result<Response, Error> {
        if let Err(ureq::Error::StatusCode(401)) = result {
            self.access = None;
        }
        Ok(result?)
    }

    /// 앱 전용 폴더의 파일 목록
    pub fn list(&mut self) -> Result<Vec<RemoteFile>, Error> {
        #[derive(Deserialize)]
        struct FileList {
            files: Vec<RemoteFile>,
        }
        let auth = format!("Bearer {}", self.access_token()?);
        let result = self
            .agent
            .get(FILES_URL)
            .query("spaces", "appDataFolder")
            .query("fields", "files(id,name,modifiedTime)")
            .query("pageSize", "1000")
            .header("Authorization", &auth)
            .call();
        let list: FileList = self.check(result)?.body_mut().read_json()?;
        Ok(list.files)
    }

    pub fn download(&mut self, id: &str) -> Result<Vec<u8>, Error> {
        let auth = format!("Bearer {}", self.access_token()?);
        let result = self.agent.get(format!("{FILES_URL}/{id}")).query("alt", "media").header("Authorization", &auth).call();
        Ok(self.check(result)?.body_mut().read_to_vec()?)
    }

    /// 앱 전용 폴더에 빈 파일을 만들고 ID를 돌려준다 — 내용은 `upload`로 올린다.
    pub fn create(&mut self, name: &str) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Created {
            id: String,
        }
        let auth = format!("Bearer {}", self.access_token()?);
        let result = self
            .agent
            .post(FILES_URL)
            .query("fields", "id")
            .header("Authorization", &auth)
            .send_json(json!({ "name": name, "parents": ["appDataFolder"], "mimeType": "application/json" }));
        let created: Created = self.check(result)?.body_mut().read_json()?;
        Ok(created.id)
    }

    /// 파일 내용을 바꾼다. 드라이브에서 지워졌으면 `Error::NotFound`.
    pub fn upload(&mut self, id: &str, bytes: &[u8]) -> Result<(), Error> {
        let auth = format!("Bearer {}", self.access_token()?);
        let result = self
            .agent
            .patch(format!("{UPLOAD_URL}/{id}"))
            .query("uploadType", "media")
            .header("Authorization", &auth)
            .content_type("application/json")
            .send(bytes);
        self.check(result)?;
        Ok(())
    }
}

/// 저장해 둔 로그인을 지운다 (구글에 폐기 요청은 하지 않음 — 이미 무효인 토큰일 때).
pub fn forget() {
    match keyring_entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => eprintln!("동기화: 저장된 로그인을 지울 수 없음: {e}"),
    }
}

fn expires_at(expires_in: u64) -> Instant {
    Instant::now() + Duration::from_secs(expires_in.saturating_sub(60))
}

/// 추측할 수 없는 임의 문자열 43자 (PKCE verifier · state용, RFC 7636)
fn random_token() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| Error::Failed(format!("난수 생성 실패: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 브라우저가 `127.0.0.1`로 돌아올 때까지 기다려 인증 코드를 꺼낸다.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, Error> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(result) = handle_request(stream, state)? {
                    return result;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err(Error::Failed("로그인을 기다리다 시간이 지났어요".into()));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// 요청 하나에 답한다. 로그인 결과가 아닌 요청(favicon 등)이면 `None`.
fn handle_request(mut stream: TcpStream, state: &str) -> Result<Option<Result<String, Error>>, Error> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // 나머지 헤더까지 읽어 둬야 연결을 닫을 때 브라우저가 오류를 띄우지 않는다.
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let result = parse_redirect(&request_line, state);
    let (status, body) = match &result {
        None => ("404 Not Found", String::new()),
        Some(Ok(_)) => ("200 OK", result_page("연결됐어요", "이 탭을 닫고 VocaTray로 돌아가세요.")),
        Some(Err(e)) => ("200 OK", result_page("연결하지 못했어요", &format!("{e} — 트레이 메뉴에서 다시 시도해 주세요."))),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    Ok(result)
}

fn result_page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>VocaTray</title>\
         <body style=\"font-family:'Segoe UI','Malgun Gothic',sans-serif;padding:48px\">\
         <h2>VocaTray — {title}</h2><p>{message}</p></body>"
    )
}

/// 요청 첫 줄(`GET /?state=..&code=.. HTTP/1.1`)에서 로그인 결과를 꺼낸다.
/// state가 다르면 우리가 시작한 로그인이 아니므로 무시한다(`None`).
fn parse_redirect(request_line: &str, state: &str) -> Option<Result<String, Error>> {
    let target = request_line.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    let params: HashMap<_, _> = url.query_pairs().collect();
    if params.get("state").map(|s| s.as_ref()) != Some(state) {
        return None;
    }
    if let Some(code) = params.get("code") {
        return Some(Ok(code.to_string()));
    }
    let message = match params.get("error").map(|e| e.as_ref()) {
        Some("access_denied") => "로그인을 취소했어요".to_string(),
        Some(e) => format!("구글 로그인 오류: {e}"),
        None => "구글 로그인 결과를 알 수 없어요".to_string(),
    };
    Some(Err(Error::Failed(message)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_file_is_valid() {
        let client = client();
        assert!(client.client_id.ends_with(".apps.googleusercontent.com"));
        assert!(!client.client_secret.is_empty());
    }

    #[test]
    fn pkce_challenge_is_base64url_sha256() {
        // 기댓값은 파이썬 hashlib.sha256 + base64.urlsafe_b64encode(= 제거)로 따로 계산했다.
        let verifier = "dBjftJeZ4CVP-mJ92IZ8lKCZoMd0XB2u4F7y6nY8ZXXy3tfgaVnfVCeUo0Y";
        assert_eq!(pkce_challenge(verifier), "ZmUsRi6TUac0GizQI-d6F-8C0D6twqdswtSa6OO_b7k");
    }

    #[test]
    fn random_tokens_are_long_and_different() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn redirect_with_code_is_accepted() {
        let line = "GET /?state=abc&code=4%2F0Ab_xyz&scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.appdata HTTP/1.1\r\n";
        assert_eq!(parse_redirect(line, "abc").unwrap().unwrap(), "4/0Ab_xyz");
    }

    #[test]
    fn redirect_with_denied_consent_is_an_error() {
        let line = "GET /?error=access_denied&state=abc HTTP/1.1\r\n";
        let err = parse_redirect(line, "abc").unwrap().unwrap_err();
        assert_eq!(err.to_string(), "로그인을 취소했어요");
    }

    #[test]
    fn unrelated_requests_are_ignored() {
        assert!(parse_redirect("GET /favicon.ico HTTP/1.1\r\n", "abc").is_none());
        assert!(parse_redirect("GET /?state=other&code=x HTTP/1.1\r\n", "abc").is_none());
        assert!(parse_redirect("", "abc").is_none());
    }

    #[test]
    fn file_list_response_parses() {
        let json = r#"{ "files": [ { "id": "1AbC", "name": "progress.pc-3f2a.json", "modifiedTime": "2026-10-27T12:00:00.000Z" } ] }"#;
        #[derive(Deserialize)]
        struct FileList {
            files: Vec<RemoteFile>,
        }
        let list: FileList = serde_json::from_str(json).unwrap();
        assert_eq!(list.files[0].name, "progress.pc-3f2a.json");
        assert_eq!(list.files[0].modified_time, "2026-10-27T12:00:00.000Z");
    }
}
