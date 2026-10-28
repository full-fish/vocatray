//! 동기화 스레드 (PLAN.md §6.4)
//! - 확인(다른 기기 기록 받아 합치기 + 내 기록 올리기): 앱을 켤 때, 로그인했을 때, 메뉴 [지금 동기화]를 눌렀을 때.
//!   주기적으로는 확인하지 않는다. 실패하면 1분부터 30분까지 간격을 늘려 가며 다시 시도한다.
//! - 내 기록이 바뀌면(외웠어요·단어장 변경) 1시간 모아서 올리기만 한다.
//!   그 전에 앱을 끄면 로컬엔 이미 저장돼 있으므로 다음에 켤 때 확인하면서 올린다.
//!
//! 네트워크는 이 스레드에서만 쓴다 → 인터넷이 느리거나 끊겨도 트레이·카드는 멈추지 않는다.
//!
//! 드라이브 API 할당량(2026-05~): 목록 조회 100단위, 올리기 50단위, 받기 200단위.
//! 목록 조회는 파일 이름과 수정 시각만 받는다 — 내용은 수정 시각이 바뀐 파일만 받는다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::gdrive::{self, Error, Session};
use crate::store::{self, Device, Progress};

/// 처음 바뀐 뒤 이만큼 모아서 한 번에 올린다 — [외웠어요]를 계속 눌러도 1시간에 한 번.
const UPLOAD_DELAY: Duration = Duration::from_secs(60 * 60);
/// 실패하면 이 간격부터 두 배씩, 최대 30분까지 늘려 다시 시도한다 (켤 때 아직 인터넷이 안 잡힌 경우 등).
const RETRY_FIRST: Duration = Duration::from_secs(60);
const RETRY_MAX: Duration = Duration::from_secs(30 * 60);

pub enum Command {
    /// 내 기록이 바뀌었다
    Changed,
    /// 메뉴의 "지금 동기화"
    SyncNow,
    /// 메뉴의 "구글 계정 연결" / "구글 계정 연결 끊기"
    ToggleAccount,
}

struct Handle(Sender<Command>);

pub fn notify(app: &AppHandle, command: Command) {
    if let Some(handle) = app.try_state::<Handle>() {
        let _ = handle.0.send(command);
    }
}

pub fn start(app: AppHandle, data_dir: PathBuf, device: Device) {
    let (tx, rx) = mpsc::channel();
    app.manage(Handle(tx));
    let worker = Worker { app, data_dir, device, session: None, seen: HashMap::new(), uploaded: None, restored: false };
    std::thread::spawn(move || worker.run(rx));
}

struct Worker {
    app: AppHandle,
    data_dir: PathBuf,
    device: Device,
    session: Option<Session>,
    /// 다른 기기 파일 ID → 마지막으로 받은 수정 시각 (바뀐 파일만 다시 받는다)
    seen: HashMap<String, String>,
    /// 마지막으로 올린 내 기록 — 그대로면 다시 올리지 않는다
    uploaded: Option<Vec<u8>>,
    /// 앱을 켜거나 로그인한 뒤 첫 확인을 했는지 — 그때 이 컴퓨터의 기록이 비어 있으면(파일이 지워졌거나 깨짐)
    /// 드라이브의 내 파일을 받아 되살린다. 빈 기록으로 드라이브 사본을 덮어쓰지 않게.
    restored: bool,
}

impl Worker {
    fn run(mut self, rx: Receiver<Command>) {
        self.session = Session::restore();
        if self.session.is_none() {
            self.report("동기화 꺼짐", false);
        }
        let mut next_check = Some(Instant::now()); // 켤 때 한 번
        let mut next_upload: Option<Instant> = None;
        let mut retry = RETRY_FIRST;
        loop {
            // 할 일이 없으면 명령이 올 때까지 그냥 기다린다
            let message = match next_check.into_iter().chain(next_upload).min() {
                Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match message {
                Ok(Command::Changed) => {
                    next_upload.get_or_insert(Instant::now() + UPLOAD_DELAY);
                }
                Ok(Command::SyncNow) => next_check = Some(Instant::now()),
                Ok(Command::ToggleAccount) => {
                    self.toggle_account();
                    next_check = Some(Instant::now());
                }
                Err(RecvTimeoutError::Timeout) => {
                    let check = next_check.is_some_and(|due| Instant::now() >= due);
                    // 확인할 때도 내 기록을 올리므로 둘 다 비운다
                    next_check = None;
                    next_upload = None;
                    if self.sync(check) {
                        retry = RETRY_FIRST;
                    } else {
                        next_check = Some(Instant::now() + retry);
                        retry = (retry * 2).min(RETRY_MAX);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn toggle_account(&mut self) {
        if let Some(session) = self.session.take() {
            session.logout();
            self.report("동기화 꺼짐", false);
            return;
        }
        self.report("브라우저에서 구글 로그인 중…", true);
        match Session::login() {
            Ok(session) => {
                self.session = Some(session);
                self.seen.clear();
                self.uploaded = None;
                self.restored = false;
            }
            Err(e) => self.report(&format!("연결 안 됨 · {e}"), false),
        }
    }

    /// `check`: 다른 기기 기록도 확인한다. 아니면 내 기록만 올린다.
    /// 다시 시도해야 하면 false (네트워크·서버 문제). 로그인이 없거나 만료됐으면 다시 시도하지 않는다.
    fn sync(&mut self, check: bool) -> bool {
        if self.session.is_none() {
            return true;
        }
        eprintln!("동기화: {}", if check { "확인" } else { "올리기" });
        match self.sync_once(check) {
            Ok(()) => self.report(&format!("동기화됨 · {}", chrono::Local::now().format("%H:%M")), false),
            Err(Error::Unauthorized) => {
                gdrive::forget();
                self.session = None;
                self.report("로그인 만료 · 다시 연결해 주세요", false);
            }
            Err(e) => {
                eprintln!("동기화 실패: {e}");
                self.report("동기화 끊김 · 자동으로 다시 시도해요", false);
                return false;
            }
        }
        true
    }

    fn sync_once(&mut self, check: bool) -> Result<(), Error> {
        let session = self.session.as_mut().expect("로그인했을 때만 부른다");
        let mine = format!("progress.{}.json", self.device.device_id);
        // 올리기만 할 때는 목록을 보지 않는다 — 내 파일 ID를 아직 모를 때만 본다.
        let listed = check || self.device.drive_file_id.is_none();
        let files = if listed { session.list()? } else { Vec::new() };

        // 1) 다른 기기 기록 — 수정 시각이 바뀐 파일만 받아 합친다 (다른 기기 파일은 읽기만 한다).
        //    내 파일은 켠 뒤 첫 확인 때 이 컴퓨터의 기록이 비어 있을 때만 받는다 (드라이브 사본으로 되살리기).
        let restore = !self.restored && crate::progress_snapshot(&self.app).is_empty();
        let mut remote = Vec::new();
        let progress_files = files.iter().filter(|f| f.name.starts_with("progress.") && f.name.ends_with(".json"));
        for file in progress_files {
            if (file.name == mine && !restore) || self.seen.get(&file.id) == Some(&file.modified_time) {
                continue;
            }
            match serde_json::from_slice::<Progress>(&session.download(&file.id)?) {
                Ok(progress) => {
                    remote.push(progress);
                    self.seen.insert(file.id.clone(), file.modified_time.clone());
                }
                // 깨진 파일은 이번엔 건너뛰고 다음에 다시 읽는다 (§6.4 규칙 5)
                Err(e) => eprintln!("동기화: {} 건너뜀 — {e}", file.name),
            }
        }
        if !remote.is_empty() {
            crate::merge_remote(&self.app, &remote);
        }
        if listed {
            self.restored = true;
        }

        // 2) 내 기록 — 바뀌었을 때만 올린다
        let bytes = serde_json::to_vec_pretty(&crate::progress_snapshot(&self.app)).expect("기록 직렬화");
        if self.uploaded.as_deref() == Some(&bytes[..]) {
            return Ok(());
        }
        // 내 파일 ID: 기억해 둔 것 → 이름으로 찾기 → 새로 만들기 (같은 이름 파일이 여러 개 생기지 않게)
        let known = self.device.drive_file_id.clone().or_else(|| files.iter().find(|f| f.name == mine).map(|f| f.id.clone()));
        let id = match known {
            Some(id) => id,
            None => session.create(&mine)?,
        };
        let id = match session.upload(&id, &bytes) {
            Ok(()) => id,
            // 드라이브에서 지워졌다 (숨겨진 앱 데이터 삭제 등) → 새로 만든다
            Err(Error::NotFound) => {
                let id = session.create(&mine)?;
                session.upload(&id, &bytes)?;
                id
            }
            Err(e) => return Err(e),
        };
        self.uploaded = Some(bytes);
        if self.device.drive_file_id.as_deref() != Some(id.as_str()) {
            self.device.drive_file_id = Some(id);
            if let Err(e) = store::save_device(&self.data_dir, &self.device) {
                eprintln!("동기화: 기기 정보 저장 실패: {e}");
            }
        }
        Ok(())
    }

    fn report(&self, status: &str, busy: bool) {
        crate::show_sync_status(&self.app, status, self.session.is_some(), busy);
    }
}
