//! 사용자 데이터: 기기 ID와 이 기기의 학습 기록 파일 (PLAN.md §6.3).
//!
//! 형식은 처음부터 동기화할 수 있게 기기별 파일(`progress.<기기ID>.json`)이다.
//! 다른 기기 기록은 `Progress::merge`로 내 기록에 합친다 (§6.4).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `device.json` — 로컬 전용, 절대 동기화하지 않는다 (두 기기가 같은 ID를 쓰게 되므로).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_id: String,
    /// 구글 드라이브에 올린 내 기록 파일의 ID — 드라이브는 같은 이름 파일을 여러 개 허용하므로 기억해 둔다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_file_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamped<T> {
    pub value: T,
    pub at: String,
}

/// 되돌리기도 지우지 않고 `memorized: false`로 남긴다 — 동기화 때 옛 "외웠음"이 되살아나지 않게.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordRecord {
    pub memorized: bool,
    pub at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_book: Option<Stamped<String>>,
    #[serde(default)]
    pub words: BTreeMap<String, WordRecord>,
}

impl Progress {
    pub fn new(device: &str) -> Self {
        Self { device: device.to_string(), ..Self::default() }
    }

    /// 아무 기록도 없다 (새로 시작했거나 파일이 지워지거나 깨졌다)
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.selected_book.is_none()
    }

    pub fn is_memorized(&self, key: &str) -> bool {
        self.words.get(key).is_some_and(|r| r.memorized)
    }

    pub fn set_memorized(&mut self, key: &str, memorized: bool, at: String) {
        self.words.insert(key.to_string(), WordRecord { memorized, at });
    }

    pub fn selected_book(&self) -> Option<&str> {
        self.selected_book.as_ref().map(|s| s.value.as_str())
    }

    pub fn select_book(&mut self, id: &str, at: String) {
        self.selected_book = Some(Stamped { value: id.to_string(), at });
    }

    /// 다른 기기 기록을 합친다 — 단어마다 `at`이 더 최근인 기록이 이기고, 시각이 같으면 "외웠음"이 이긴다.
    /// 단어장 선택도 최근 것이 이긴다. 합친 결과가 달라졌으면 true.
    /// 어느 기기에서 어떤 순서로 합쳐도 결과가 같다 → 기기마다 합친 기록이 결국 같아진다.
    pub fn merge(&mut self, other: &Progress) -> bool {
        let mut changed = false;
        for (key, theirs) in &other.words {
            let take = match self.words.get(key) {
                Some(mine) => theirs.at > mine.at || (theirs.at == mine.at && theirs.memorized && !mine.memorized),
                None => true,
            };
            if take {
                self.words.insert(key.clone(), theirs.clone());
                changed = true;
            }
        }
        if let Some(theirs) = &other.selected_book {
            // 시각이 같으면 값이 큰 쪽 — 기기마다 다른 쪽을 고르지 않게 아무 기준이나 하나로 정한다.
            let take = self.selected_book.as_ref().is_none_or(|mine| {
                (theirs.at.as_str(), theirs.value.as_str()) > (mine.at.as_str(), mine.value.as_str())
            });
            if take {
                self.selected_book = Some(theirs.clone());
                changed = true;
            }
        }
        changed
    }
}

/// 기록 시각 — UTC, 밀리초까지 (예: `2026-10-27T12:03:00.123Z`).
/// 자릿수가 늘 같아서 문자열 비교가 곧 시각 비교다 (`merge`가 이것에 기댄다).
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 기기 ID를 읽고, 없거나 깨져 있으면 새로 만든다.
pub fn load_or_create_device(dir: &Path) -> io::Result<Device> {
    let path = dir.join("device.json");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(device) = serde_json::from_str::<Device>(&text) {
            return Ok(device);
        }
        fs::rename(&path, path.with_extension("json.bak"))?;
    }
    let device = Device { device_id: new_device_id(), drive_file_id: None };
    save_device(dir, &device)?;
    Ok(device)
}

pub fn save_device(dir: &Path, device: &Device) -> io::Result<()> {
    write_atomic(&dir.join("device.json"), &serde_json::to_vec_pretty(device)?)
}

pub fn progress_path(dir: &Path, device_id: &str) -> PathBuf {
    dir.join(format!("progress.{device_id}.json"))
}

/// 이 기기의 기록. 없으면 빈 기록, 깨져 있으면 `.bak`으로 옮기고 빈 기록으로 시작한다
/// (파일 하나가 깨졌다고 앱이 아예 안 켜지면 안 되므로).
pub fn load_progress(dir: &Path, device_id: &str) -> io::Result<Progress> {
    let path = progress_path(dir, device_id);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Progress::new(device_id)),
        Err(e) => return Err(e),
    };
    match serde_json::from_str::<Progress>(&text) {
        Ok(progress) => Ok(progress),
        Err(_) => {
            fs::rename(&path, path.with_extension("json.bak"))?;
            Ok(Progress::new(device_id))
        }
    }
}

pub fn save_progress(dir: &Path, progress: &Progress) -> io::Result<()> {
    let path = progress_path(dir, &progress.device);
    write_atomic(&path, &serde_json::to_vec_pretty(progress)?)
}

/// 임시 파일에 다 쓴 뒤 바꿔 끼운다 → 저장 중에 강제 종료돼도 원래 파일은 멀쩡하다.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

/// 컴퓨터 이름 + 임의 4글자 (예: `desktop-ab12cd-3f2a`) — 사람이 알아보기 쉽게.
fn new_device_id() -> String {
    let host: String = gethostname::gethostname()
        .to_string_lossy()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let host = host.trim_matches('-');
    let host = if host.is_empty() { "device" } else { host };
    format!("{host}-{:04x}", fastrand::u16(..))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트마다 비어 있는 임시 폴더 — 끝나면 지운다.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("vocatray-test-{:016x}", fastrand::u64(..)));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn device_id_is_created_once_and_reused() {
        let dir = TempDir::new();
        let first = load_or_create_device(&dir.0).unwrap();
        let second = load_or_create_device(&dir.0).unwrap();
        assert_eq!(first, second);
        assert!(first.device_id.len() > 5);
    }

    #[test]
    fn missing_progress_starts_empty() {
        let dir = TempDir::new();
        let progress = load_progress(&dir.0, "pc-1").unwrap();
        assert_eq!(progress, Progress::new("pc-1"));
    }

    #[test]
    fn progress_round_trips_and_leaves_no_temp_file() {
        let dir = TempDir::new();
        let mut progress = Progress::new("pc-1");
        progress.set_memorized("change", true, now());
        progress.select_book("middle", now());
        save_progress(&dir.0, &progress).unwrap();

        assert_eq!(load_progress(&dir.0, "pc-1").unwrap(), progress);
        assert!(!progress_path(&dir.0, "pc-1").with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_progress_is_moved_to_bak() {
        let dir = TempDir::new();
        let path = progress_path(&dir.0, "pc-1");
        fs::write(&path, "{ not json").unwrap();

        let progress = load_progress(&dir.0, "pc-1").unwrap();
        assert_eq!(progress, Progress::new("pc-1"));
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn unmemorize_keeps_a_record_instead_of_deleting() {
        let mut progress = Progress::new("pc-1");
        progress.set_memorized("apple", true, now());
        progress.set_memorized("apple", false, now());
        assert!(!progress.is_memorized("apple"));
        assert!(progress.words.contains_key("apple"));
    }

    const T1: &str = "2026-10-27T10:00:00.000Z";
    const T2: &str = "2026-10-27T11:00:00.000Z";

    #[test]
    fn merge_keeps_words_memorized_on_either_device() {
        let mut pc = Progress::new("pc");
        pc.set_memorized("apple", true, T1.into());
        let mut mac = Progress::new("mac");
        mac.set_memorized("banana", true, T2.into());

        assert!(pc.merge(&mac));
        assert!(pc.is_memorized("apple") && pc.is_memorized("banana"));
        assert_eq!(pc.device, "pc");
    }

    #[test]
    fn merge_newer_undo_wins_and_older_memorized_does_not_come_back() {
        let mut pc = Progress::new("pc");
        pc.set_memorized("apple", true, T1.into());
        let mut mac = pc.clone();
        mac.set_memorized("apple", false, T2.into()); // 맥에서 되돌림

        assert!(pc.merge(&mac));
        assert!(!pc.is_memorized("apple"));
        // 반대로 맥이 PC의 옛 기록을 합쳐도 되돌린 상태가 유지된다
        assert!(!mac.merge(&pc));
        assert!(!mac.is_memorized("apple"));
    }

    #[test]
    fn merge_same_time_prefers_memorized() {
        let mut a = Progress::new("a");
        a.set_memorized("apple", false, T1.into());
        let mut b = Progress::new("b");
        b.set_memorized("apple", true, T1.into());

        let mut a2 = a.clone();
        a2.merge(&b);
        let mut b2 = b.clone();
        b2.merge(&a);
        assert!(a2.is_memorized("apple") && b2.is_memorized("apple"));
    }

    #[test]
    fn merge_selected_book_newest_wins_and_is_order_independent() {
        let mut a = Progress::new("a");
        a.select_book("middle", T1.into());
        let mut b = Progress::new("b");
        b.select_book("high", T2.into());
        let mut c = Progress::new("c");
        c.select_book("elementary", T2.into()); // b와 같은 시각

        let mut x = a.clone();
        x.merge(&b);
        x.merge(&c);
        let mut y = c.clone();
        y.merge(&a);
        y.merge(&b);
        assert_eq!(x.selected_book, y.selected_book);
        assert_eq!(x.selected_book(), Some("high"));
    }

    #[test]
    fn merge_reports_no_change_when_nothing_new() {
        let mut pc = Progress::new("pc");
        pc.set_memorized("apple", true, T2.into());
        pc.select_book("middle", T2.into());
        let mut old = Progress::new("mac");
        old.set_memorized("apple", true, T1.into());
        old.select_book("high", T1.into());

        let before = pc.clone();
        assert!(!pc.merge(&old));
        assert_eq!(pc, before);
    }

    #[test]
    fn device_json_without_drive_file_id_still_loads() {
        let dir = TempDir::new();
        fs::write(dir.0.join("device.json"), r#"{ "deviceId": "pc-1" }"#).unwrap();
        let device = load_or_create_device(&dir.0).unwrap();
        assert_eq!(device, Device { device_id: "pc-1".into(), drive_file_id: None });
    }

    #[test]
    fn file_format_matches_plan() {
        let mut progress = Progress::new("pc-1");
        progress.select_book("middle", "2026-10-27T11:00:00.000Z".into());
        progress.set_memorized("change", true, "2026-10-27T12:03:00.000Z".into());
        let json: serde_json::Value = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["device"], "pc-1");
        assert_eq!(json["selectedBook"]["value"], "middle");
        assert_eq!(json["words"]["change"]["memorized"], true);
    }
}
