//! VocaTray — 트레이 아이콘에 마우스를 올리면 영단어 카드가 뜬다 (PLAN.md §5).
//!
//! 창은 두 개다. `hover`(마우스를 올리면 뜨는 읽기 전용 카드)와 `click`(아이콘을 클릭하면 뜨는,
//! 버튼 있는 카드). 둘 다 처음부터 숨겨 둔 채 만들어 두고 보였다 숨겼다만 한다.
//! 카드를 띄우는 순서: Rust가 `card` 이벤트로 단어를 보냄 → 화면이 그리고 높이를 재서
//! `card_ready` 호출 → Rust가 창 크기·위치를 맞추고 보여 줌. 내용을 다 그린 뒤에 보여 주므로
//! 이전 단어가 잠깐 비치지 않는다.
//!
//! 외운 단어 창(`words`)과 퀴즈 창(`quiz`)은 트레이 메뉴에서 열 때 만들고 닫으면 없앤다.

mod gdrive;
mod picker;
mod placement;
mod store;
mod sync;
mod wordbook;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItem, MenuItemBuilder, SubmenuBuilder,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

use picker::Picker;
use placement::{Area, place_card};
use store::{Progress, QuizStats};
use wordbook::{Word, WordBook};

/// 마우스가 아이콘 위에 이만큼 머물러야 카드를 띄운다 — 지나가기만 해서는 카드가 뜨지 않게.
const HOVER_DELAY: Duration = Duration::from_millis(300);
/// 호버 중 커서가 아직 아이콘 위에 있는지 확인하는 간격.
const HOVER_POLL: Duration = Duration::from_millis(100);
/// 클릭 카드가 포커스를 잃고 닫힌 직후의 아이콘 클릭은 "닫기"로 본다 (다시 열지 않음).
const REOPEN_GUARD: Duration = Duration::from_millis(300);
/// 카드 너비와 아이콘과의 간격 (논리 픽셀) — 너비는 tauri.conf.json의 창 너비와 같게.
const CARD_WIDTH: f64 = 240.0;
const CARD_GAP: f64 = 6.0;
/// 단어장을 고른 적이 없을 때 쓰는 단어장.
const DEFAULT_BOOK: &str = "middle";
/// [몰라요]를 누른 단어는 이만큼 다른 단어를 본 뒤에 한 번 더 나온다.
const AGAIN_GAP: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Card {
    Hover,
    Click,
}

impl Card {
    fn label(self) -> &'static str {
        match self {
            Card::Hover => "hover",
            Card::Click => "click",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CardPayload {
    card: Card,
    seq: u64,
    /// `None`이면 단어장을 다 외운 것
    word: Option<Word>,
    /// [◀ 이전]으로 돌아온 단어는 이미 외웠을 수 있다 → [외웠어요] 자리에 [못 외웠어요]
    memorized: bool,
    has_previous: bool,
    book_title: String,
    remaining: usize,
    total: usize,
}

struct AppState {
    books: Vec<WordBook>,
    data_dir: PathBuf,
    progress: Progress,
    picker: Picker,
    /// 호버 카드에 마지막으로 뜬 단어 — 클릭 카드는 이 단어를 대상으로 한다 (§5.3).
    current: Option<String>,
    /// 바로 전 단어 — [◀ 이전]으로 한 번 돌아갈 수 있다 ([외웠어요]를 잘못 눌렀을 때).
    previous: Option<String>,
    /// [◀ 이전]으로 돌아가기 전에 보던 단어 — [다음]을 누르면 이 단어로 돌아온다.
    upcoming: Option<String>,
    tray_rect: Option<Rect>,
    hovering: bool,
    /// 호버를 시작할 때마다 1씩 — 감시 스레드가 그사이 새로 시작된 호버의 것인지 가린다.
    hover_generation: u64,
    /// 카드 내용 번호 — 더 새 내용이 나간 뒤 늦게 온 `card_ready`는 무시한다.
    seq: u64,
    click_visible: bool,
    click_hidden_at: Option<Instant>,
}

impl AppState {
    fn book(&self) -> &WordBook {
        let selected = self.progress.selected_book().unwrap_or(DEFAULT_BOOK);
        self.books
            .iter()
            .find(|b| b.id == selected)
            .unwrap_or(&self.books[0])
    }

    /// 선택한 단어장 − 외운 단어
    fn candidates(&self) -> Vec<String> {
        self.book()
            .words
            .iter()
            .map(Word::key)
            .filter(|key| !self.progress.is_memorized(key))
            .collect()
    }

    fn advance(&mut self) {
        let candidates = self.candidates();
        self.previous = self.current.take();
        self.current = match self.upcoming.take() {
            Some(key) if candidates.contains(&key) => Some(key),
            _ => self.picker.next(&candidates),
        };
    }

    fn back(&mut self) {
        if let Some(previous) = self.previous.take() {
            self.upcoming = self.current.take();
            self.current = Some(previous);
        }
    }

    /// 단어장이 바뀌었을 때 — 처음부터 새로 고른다.
    fn reset_words(&mut self) {
        self.picker.reset();
        self.current = None;
        self.previous = None;
        self.upcoming = None;
    }

    fn payload(&mut self, card: Card) -> CardPayload {
        self.seq += 1;
        let book = self.book();
        let word = self
            .current
            .as_ref()
            .and_then(|key| book.words.iter().find(|w| &w.key() == key))
            .cloned();
        CardPayload {
            card,
            seq: self.seq,
            word,
            memorized: self.current.as_ref().is_some_and(|key| self.progress.is_memorized(key)),
            has_previous: self.previous.is_some(),
            book_title: book.title.clone(),
            remaining: self.candidates().len(),
            total: book.words.len(),
        }
    }

    fn save(&self) {
        if let Err(e) = store::save_progress(&self.data_dir, &self.progress) {
            eprintln!("학습 기록 저장 실패: {e}");
        }
    }
}

/// 트레이 메뉴의 단어장 체크 항목. `AppState` 밖에 두는 이유: 메뉴 항목이 들어 있으면 `AppState`를 쓰는
/// 단위 테스트가 Windows 공용 컨트롤 v6을 찾다가 실행조차 안 된다 (테스트 실행 파일엔 그 매니페스트가 없다).
struct BookItems(Vec<CheckMenuItem<tauri::Wry>>);

/// 트레이 메뉴의 동기화 줄 — 동기화 스레드가 글자를 바꾼다.
struct SyncMenu {
    status: MenuItem<tauri::Wry>,
    sync_now: MenuItem<tauri::Wry>,
    account: MenuItem<tauri::Wry>,
}

/// 상태 잠금. 잠근 채로 창 API를 부르지 않는다 — 창 API는 메인 스레드를 기다릴 수 있다.
fn lock(app: &AppHandle) -> MutexGuard<'_, AppState> {
    app.state::<Mutex<AppState>>()
        .inner()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn present(app: &AppHandle, card: Card) {
    let payload = lock(app).payload(card);
    if let Err(e) = app.emit("card", payload) {
        eprintln!("카드 이벤트 전송 실패: {e}");
    }
}

fn hide(app: &AppHandle, card: Card) {
    if let Some(window) = app.get_webview_window(card.label()) {
        let _ = window.hide();
    }
}

fn on_tray_event(app: &AppHandle, event: TrayIconEvent) {
    match event {
        // tray-icon이 Leave를 놓쳤다면 다음 진입은 Enter가 아니라 Move로 온다 — 둘 다 호버 시작으로 본다.
        TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. } => {
            let generation = {
                let mut s = lock(app);
                s.tray_rect = Some(rect);
                if s.hovering || s.click_visible {
                    return;
                }
                s.hovering = true;
                s.hover_generation += 1;
                s.hover_generation
            };
            let app = app.clone();
            std::thread::spawn(move || watch_hover(app, generation));
        }
        TrayIconEvent::Leave { .. } => leave_hover(app),
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => {
            {
                let mut s = lock(app);
                s.tray_rect = Some(rect);
                s.hovering = false;
                // 클릭 카드가 방금 포커스를 잃고 닫혔다면 이번 클릭은 "닫기"였다.
                if s.click_hidden_at.is_some_and(|t| t.elapsed() < REOPEN_GUARD) {
                    return;
                }
                if s.current.is_none() {
                    s.advance();
                }
            }
            hide(app, Card::Hover);
            present(app, Card::Click);
        }
        _ => {}
    }
}

/// 호버 한 번을 지켜본다: 커서가 아이콘 위에 0.3초 머물면 카드를 띄우고, 떠나면 닫는다.
/// Leave 이벤트만 믿지 않는 이유: tray-icon은 아이콘 위에 멈췄다가 빠르게 빠져나가면
/// Leave를 보내지 않을 때가 있다.
fn watch_hover(app: AppHandle, generation: u64) {
    let started = Instant::now();
    let mut shown = false;
    loop {
        std::thread::sleep(HOVER_POLL);
        let rect = {
            let s = lock(&app);
            if !s.hovering || s.hover_generation != generation {
                return;
            }
            s.tray_rect
        };
        let inside = match (rect, app.cursor_position()) {
            (Some(rect), Ok(cursor)) => icon_area(rect).contains(cursor.x, cursor.y),
            _ => false,
        };
        if !inside {
            leave_hover(&app);
            return;
        }
        if !shown && started.elapsed() >= HOVER_DELAY {
            shown = true;
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || show_hover(&handle, generation));
        }
    }
}

fn leave_hover(app: &AppHandle) {
    lock(app).hovering = false;
    hide(app, Card::Hover);
}

fn show_hover(app: &AppHandle, generation: u64) {
    {
        let mut s = lock(app);
        if !s.hovering || s.hover_generation != generation || s.click_visible {
            return;
        }
        // 호버는 현재 단어를 다시 보여 주기만 한다 — 다음 단어는 [외웠어요]·[다음]을 눌러야 나온다.
        if s.current.is_none() {
            s.advance();
        }
    }
    present(app, Card::Hover);
}

fn icon_area(rect: Rect) -> Area {
    // 트레이 이벤트의 위치·크기는 물리 픽셀이라 배율 값은 쓰이지 않는다.
    let pos = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    Area { x: pos.x, y: pos.y, w: size.width, h: size.height }
}

fn position_near_tray(app: &AppHandle, window: &WebviewWindow, rect: Rect, height: f64) {
    let icon = icon_area(rect);
    let Ok(Some(monitor)) = app.monitor_from_point(icon.x + icon.w / 2.0, icon.y + icon.h / 2.0)
    else {
        return;
    };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let work = Area {
        x: work.position.x as f64,
        y: work.position.y as f64,
        w: work.size.width as f64,
        h: work.size.height as f64,
    };
    let (w, h) = (CARD_WIDTH * scale, height * scale);
    let (x, y) = place_card(icon, w, h, work, CARD_GAP * scale);
    // 위치를 먼저 옮긴다 — 배율이 다른 모니터로 가면 창 크기가 다시 계산되므로 크기는 그다음에.
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    let _ = window.set_size(PhysicalSize::new(w.round() as u32, h.round() as u32));
}

/// 화면이 새 내용을 다 그린 뒤 부른다 — 그 높이로 창 크기·위치를 맞추고 보여 준다.
#[tauri::command]
fn card_ready(app: AppHandle, card: Card, seq: u64, height: f64) {
    let rect = {
        let s = lock(&app);
        // 그사이 더 새 내용이 나갔거나, 마우스가 이미 아이콘을 떠났으면 보여 주지 않는다.
        if seq != s.seq || (card == Card::Hover && (!s.hovering || s.click_visible)) {
            return;
        }
        s.tray_rect
    };
    let Some(window) = app.get_webview_window(card.label()) else {
        return;
    };
    if let Some(rect) = rect {
        position_near_tray(&app, &window, rect, height);
    }
    let _ = window.show();
    if card == Card::Click {
        let _ = window.set_focus();
        lock(&app).click_visible = true;
    }
}

#[tauri::command]
fn memorize(app: AppHandle) {
    {
        let mut s = lock(&app);
        if let Some(key) = s.current.clone() {
            s.progress.set_memorized(&key, true, store::now());
            s.save();
        }
        s.advance();
    }
    progress_changed(&app);
    present(&app, Card::Click);
}

/// [외웠어요] 자리의 [못 외웠어요] — [◀ 이전]으로 돌아온 단어를 되돌리고 그 단어에 머문다.
#[tauri::command]
fn unmemorize(app: AppHandle) {
    let key = lock(&app).current.clone();
    if let Some(key) = key {
        forget_word(app.clone(), key);
    }
    present(&app, Card::Click);
}

#[tauri::command]
fn next_word(app: AppHandle) {
    lock(&app).advance();
    present(&app, Card::Click);
}

/// [몰라요] — 다음 단어로 넘어가되, 이 단어를 `AGAIN_GAP`개 뒤에 한 번 더 보여 준다.
#[tauri::command]
fn dont_know(app: AppHandle) {
    {
        let mut s = lock(&app);
        if let Some(key) = s.current.clone() {
            s.picker.again(&key, AGAIN_GAP);
        }
        s.advance();
    }
    present(&app, Card::Click);
}

#[tauri::command]
fn previous_word(app: AppHandle) {
    lock(&app).back();
    present(&app, Card::Click);
}

#[derive(Serialize)]
struct MemorizedWord {
    key: String,
    at: String,
    word: Word,
    quiz: QuizStats,
}

/// 외운 단어 — 최근에 외운 것부터. 외운 기록은 단어장과 상관없이 단어마다 하나라
/// 모든 단어장에서 찾는다 (같은 단어가 여러 단어장에 있으면 지금 단어장의 뜻).
#[tauri::command]
fn memorized_words(app: AppHandle) -> Vec<MemorizedWord> {
    memorized(&lock(&app))
}

/// 퀴즈 한 문제 — 외운 단어 전체에서 가중치대로 뽑는다 (picker::pick_weighted). 외운 단어가 없으면 `None`.
#[tauri::command]
fn quiz_word(app: AppHandle, book: Option<String>) -> Option<MemorizedWord> {
    let mut words = {
        let s = lock(&app);
        let mut words = memorized(&s);
        // 단어장을 고르면 그 단어장에 있는 외운 단어만 (`None`이면 전체)
        if let Some(b) = book.and_then(|id| s.books.iter().find(|b| b.id == id)) {
            let keys: HashSet<String> = b.words.iter().map(Word::key).collect();
            words.retain(|m| keys.contains(&m.key));
        }
        words
    };
    let streaks: Vec<u32> = words.iter().map(|m| m.quiz.streak).collect();
    let i = picker::pick_weighted(&streaks, fastrand::f64())?;
    Some(words.swap_remove(i))
}

#[derive(Serialize)]
struct BookInfo {
    id: String,
    title: String,
}

/// 퀴즈 창의 단어장 고르기 목록
#[tauri::command]
fn book_list(app: AppHandle) -> Vec<BookInfo> {
    lock(&app).books.iter().map(|b| BookInfo { id: b.id.clone(), title: b.title.clone() }).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BookStat {
    title: String,
    total: usize,
    memorized: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    books: Vec<BookStat>,
    /// 지금 외운 단어마다 외운 시각 — 날짜별로 묶는 건 화면이 한다 (내 컴퓨터 시간대 기준)
    memorized_at: Vec<String>,
    quiz_asked: u32,
    quiz_correct: u32,
}

/// 통계 창
#[tauri::command]
fn stats(app: AppHandle) -> Stats {
    let s = lock(&app);
    let books = s
        .books
        .iter()
        .map(|b| BookStat {
            title: b.title.clone(),
            total: b.words.len(),
            memorized: b.words.iter().filter(|w| s.progress.is_memorized(&w.key())).count(),
        })
        .collect();
    let records = s.progress.words.values();
    Stats {
        books,
        memorized_at: records.clone().filter(|r| r.memorized).map(|r| r.at.clone()).collect(),
        quiz_asked: records.clone().filter_map(|r| r.quiz.as_ref()).map(|q| q.value.asked).sum(),
        quiz_correct: records.filter_map(|r| r.quiz.as_ref()).map(|q| q.value.correct).sum(),
    }
}

/// 퀴즈 채점. 틀리면 그 단어의 가중치를 처음으로 되돌리고 "못 외웠어요"로 바꿔 카드에 다시 나오게 한다.
#[tauri::command]
fn quiz_answer(app: AppHandle, key: String, correct: bool) {
    {
        let mut s = lock(&app);
        let at = store::now();
        s.progress.record_quiz(&key, correct, at.clone());
        if !correct {
            s.progress.set_memorized(&key, false, at);
        }
        s.save();
    }
    progress_changed(&app);
}

fn memorized(s: &AppState) -> Vec<MemorizedWord> {
    let mut index: HashMap<String, &Word> = HashMap::new();
    for word in std::iter::once(s.book()).chain(&s.books).flat_map(|b| &b.words) {
        index.entry(word.key()).or_insert(word);
    }
    let mut words: Vec<MemorizedWord> = s
        .progress
        .words
        .iter()
        .filter(|(_, record)| record.memorized)
        .filter_map(|(key, record)| {
            let word = (*index.get(key)?).clone();
            let quiz = s.progress.quiz_stats(key);
            Some(MemorizedWord { key: key.clone(), at: record.at.clone(), word, quiz })
        })
        .collect();
    words.sort_by(|a, b| b.at.cmp(&a.at));
    words
}

/// 외운 단어를 "못 외웠어요"로 되돌린다 — 외운 단어 창·클릭 카드에서 부른다.
#[tauri::command]
fn forget_word(app: AppHandle, key: String) {
    {
        let mut s = lock(&app);
        if !s.progress.is_memorized(&key) {
            return;
        }
        s.progress.set_memorized(&key, false, store::now());
        s.save();
    }
    progress_changed(&app);
}

/// 내 기록이 바뀌었다 — 동기화 스레드에 알리고, 열려 있는 외운 단어 창이 다시 읽게 한다.
fn progress_changed(app: &AppHandle) {
    sync::notify(app, sync::Command::Changed);
    let _ = app.emit("progress", ());
}

#[tauri::command]
fn hide_click(app: AppHandle) {
    hide(&app, Card::Click);
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "quit" => return app.exit(0),
        "account" => return sync::notify(app, sync::Command::ToggleAccount),
        "sync-now" => return sync::notify(app, sync::Command::SyncNow),
        "words" => return open_window(app, "words", "외운 단어", (380.0, 520.0)),
        "quiz" => return open_window(app, "quiz", "퀴즈", (340.0, 400.0)),
        "stats" => return open_window(app, "stats", "통계", (340.0, 420.0)),
        "autostart" => return toggle_autostart(app),
        _ => {}
    }
    let Some(book_id) = id.strip_prefix("book:") else {
        return;
    };
    {
        let mut s = lock(app);
        s.progress.select_book(book_id, store::now());
        s.save();
        s.reset_words();
    }
    check_book(app, book_id);
    sync::notify(app, sync::Command::Changed);
}

/// 외운 단어 창·퀴즈 창 — 열 때 만들고 닫으면 없앤다 (늘 켜 두는 앱이라 안 쓰는 창을 메모리에 두지 않는다).
fn open_window(app: &AppHandle, label: &'static str, title: &'static str, (width, height): (f64, f64)) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    // 메뉴 이벤트(메인 스레드)에서 창을 만들면 Windows에서 멈출 수 있다 (WebView2) → 다른 스레드에서 만든다.
    let app = app.clone();
    std::thread::spawn(move || {
        let url = WebviewUrl::App(format!("{label}.html").into());
        let built = WebviewWindowBuilder::new(&app, label, url)
            .title(format!("VocaTray · {title}"))
            .inner_size(width, height)
            .center()
            .build();
        if let Err(e) = built {
            eprintln!("{title} 창을 열 수 없음: {e}");
        }
    });
}

/// 트레이 메뉴의 "시작 시 자동 실행" 체크
struct AutostartItem(CheckMenuItem<tauri::Wry>);

/// 이 컴퓨터 설정이라 (Windows는 레지스트리 Run 키) 동기화하지 않는다.
/// 지금 실행 중인 프로그램 경로를 등록한다 → 설치한 앱에서 켜야 한다.
fn toggle_autostart(app: &AppHandle) {
    let autolaunch = app.autolaunch();
    let result = if autolaunch.is_enabled().unwrap_or(false) { autolaunch.disable() } else { autolaunch.enable() };
    if let Err(e) = result {
        eprintln!("자동 실행 설정 실패: {e}");
    }
    // 체크 메뉴는 누르면 스스로 뒤집힌다 — 실패했을 수도 있으니 실제 상태로 다시 맞춘다.
    if let Some(item) = app.try_state::<AutostartItem>() {
        let _ = item.0.set_checked(autolaunch.is_enabled().unwrap_or(false));
    }
}

/// 체크 메뉴는 누를 때마다 스스로 뒤집히므로, 고른 단어장만 체크되게 다시 맞춘다.
fn check_book(app: &AppHandle, book_id: &str) {
    let Some(items) = app.try_state::<BookItems>() else {
        return;
    };
    let id = format!("book:{book_id}");
    for item in &items.0 {
        let _ = item.set_checked(item.id().as_ref() == id);
    }
}

/// 동기화 스레드가 올릴 내 기록
fn progress_snapshot(app: &AppHandle) -> Progress {
    lock(app).progress.clone()
}

/// 다른 기기 기록을 내 기록에 합친다 (동기화 스레드에서 부름). 단어장이 바뀌었으면 메뉴 체크와 다음 단어도 맞춘다.
fn merge_remote(app: &AppHandle, remote: &[Progress]) {
    let book_changed = {
        let mut s = lock(app);
        let before = s.book().id.clone();
        let mut changed = false;
        for progress in remote {
            changed |= s.progress.merge(progress);
        }
        if !changed {
            return;
        }
        s.save();
        let after = s.book().id.clone();
        (after != before).then(|| {
            s.reset_words();
            after
        })
    };
    // 열려 있는 외운 단어 창이 다시 읽게 (동기화 스레드가 곧 올리므로 Changed는 보내지 않는다)
    let _ = app.emit("progress", ());
    if let Some(book) = book_changed {
        check_book(app, &book);
    }
}

fn show_sync_status(app: &AppHandle, status: &str, connected: bool, busy: bool) {
    let Some(menu) = app.try_state::<SyncMenu>() else {
        return;
    };
    let _ = menu.status.set_text(status);
    let _ = menu.sync_now.set_enabled(connected && !busy);
    let _ = menu.account.set_text(if connected { "구글 계정 연결 끊기" } else { "구글 계정 연결…" });
    let _ = menu.account.set_enabled(!busy);
}

fn on_window_event(window: &Window, event: &WindowEvent) {
    match event {
        // 카드 창은 닫지 않고 숨기기만 한다 (Alt+F4 등) — 닫히면 다시 띄울 창이 없다.
        // 외운 단어 창·퀴즈 창은 그냥 닫힌다.
        WindowEvent::CloseRequested { api, .. } if matches!(window.label(), "hover" | "click") => {
            api.prevent_close();
            let _ = window.hide();
        }
        WindowEvent::Focused(false) if window.label() == Card::Click.label() => {
            let _ = window.hide();
            let mut s = lock(window.app_handle());
            s.click_visible = false;
            s.click_hidden_at = Some(Instant::now());
        }
        _ => {}
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let (books, selected) = {
        let s = lock(app);
        let books: Vec<(String, String)> = s
            .books
            .iter()
            .map(|b| (b.id.clone(), format!("{} ({}단어)", b.title, b.words.len())))
            .collect();
        (books, s.book().id.clone())
    };
    let mut submenu = SubmenuBuilder::new(app, "단어장");
    let mut book_items = Vec::new();
    for (id, label) in books {
        let item = CheckMenuItemBuilder::with_id(format!("book:{id}"), label)
            .checked(id == selected)
            .build(app)?;
        submenu = submenu.item(&item);
        book_items.push(item);
    }
    let status = MenuItemBuilder::with_id("sync-status", "동기화 확인 중…").enabled(false).build(app)?;
    let sync_now = MenuItemBuilder::with_id("sync-now", "지금 동기화").enabled(false).build(app)?;
    let account = MenuItemBuilder::with_id("account", "구글 계정 연결…").enabled(false).build(app)?;
    let autostart = CheckMenuItemBuilder::with_id("autostart", "시작 시 자동 실행")
        .checked(app.autolaunch().is_enabled().unwrap_or(false))
        .build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&submenu.build()?)
        .text("words", "외운 단어…")
        .text("quiz", "퀴즈…")
        .text("stats", "통계…")
        .separator()
        .item(&status)
        .item(&sync_now)
        .item(&account)
        .separator()
        .item(&autostart)
        .text("quit", "종료")
        .build()?;
    app.manage(BookItems(book_items));
    app.manage(SyncMenu { status, sync_now, account });
    app.manage(AutostartItem(autostart));

    // 툴팁은 넣지 않는다 — Windows 기본 툴팁이 호버 카드와 겹쳐 뜬다.
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("번들 아이콘 없음"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| on_tray_event(tray.app_handle(), event))
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 두 번째로 실행하면 아무것도 하지 않고 끝난다 — 트레이 아이콘은 항상 하나.
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            card_ready,
            memorize,
            unmemorize,
            next_word,
            previous_word,
            hide_click,
            memorized_words,
            forget_word,
            quiz_word,
            quiz_answer,
            book_list,
            dont_know,
            stats
        ])
        .on_window_event(on_window_event)
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let device = store::load_or_create_device(&data_dir)?;
            let progress = store::load_progress(&data_dir, &device.device_id)?;
            app.manage(Mutex::new(AppState {
                books: wordbook::bundled()?,
                data_dir: data_dir.clone(),
                progress,
                picker: Picker::new(),
                current: None,
                previous: None,
                upcoming: None,
                tray_rect: None,
                hovering: false,
                hover_generation: 0,
                seq: 0,
                click_visible: false,
                click_hidden_at: None,
            }));
            // 자동 실행 등록은 이름만 보고 "켜짐"으로 판단한다 → 설치판은 켤 때마다 지금 exe 경로로 다시 등록한다
            // (개발판에서 켰거나 다른 위치에 다시 설치했으면 옛 경로가 실행되므로).
            #[cfg(not(debug_assertions))]
            if app.autolaunch().is_enabled().unwrap_or(false) {
                let _ = app.autolaunch().enable();
            }
            build_tray(app.handle())?;
            sync::start(app.handle().clone(), data_dir, device);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(words: &[&str]) -> AppState {
        let words = words
            .iter()
            .map(|w| Word { word: w.to_string(), phonetic: String::new(), senses: Vec::new(), examples: Vec::new() })
            .collect();
        AppState {
            books: vec![WordBook { id: DEFAULT_BOOK.into(), title: "테스트".into(), source: String::new(), words }],
            data_dir: PathBuf::new(),
            progress: Progress::new("pc"),
            picker: Picker::with_rng(fastrand::Rng::with_seed(1)),
            current: None,
            previous: None,
            upcoming: None,
            tray_rect: None,
            hovering: false,
            hover_generation: 0,
            seq: 0,
            click_visible: false,
            click_hidden_at: None,
        }
    }

    /// [외웠어요] — 지금 단어를 외우고 다음 단어로
    fn memorize(s: &mut AppState) -> String {
        let key = s.current.clone().unwrap();
        s.progress.set_memorized(&key, true, store::now());
        s.advance();
        key
    }

    #[test]
    fn back_returns_to_mistakenly_memorized_word_then_next_resumes() {
        let mut s = state(&["a", "b", "c", "d"]);
        s.advance();
        let first = memorize(&mut s);
        let second = s.current.clone();

        s.back();
        assert_eq!(s.current.as_deref(), Some(first.as_str()));
        let card = s.payload(Card::Click);
        assert!(card.memorized, "[못 외웠어요]가 떠야 한다");
        assert!(!card.has_previous, "한 번만 돌아간다");

        s.progress.set_memorized(&first, false, store::now()); // [못 외웠어요]
        assert!(!s.payload(Card::Click).memorized);
        s.advance(); // [다음]
        assert_eq!(s.current, second, "돌아가기 전에 보던 단어로 온다");
        assert_eq!(s.previous.as_deref(), Some(first.as_str()));
    }

    #[test]
    fn back_works_after_memorizing_the_last_word() {
        let mut s = state(&["a"]);
        s.advance();
        memorize(&mut s);
        assert_eq!(s.current, None, "다 외웠다");
        assert!(s.payload(Card::Click).has_previous);

        s.back();
        assert_eq!(s.current.as_deref(), Some("a"));
        assert_eq!(s.candidates().len(), 0);
        s.progress.set_memorized("a", false, store::now());
        assert_eq!(s.candidates(), ["a"]);
    }

    #[test]
    fn changing_book_forgets_previous_word() {
        let mut s = state(&["a", "b"]);
        s.advance();
        s.advance();
        s.reset_words();
        s.back();
        assert_eq!(s.current, None);
    }
}
