//! 자동 업데이트 — GitHub 릴리스의 `latest.json`을 보고 새 버전이 있으면 물어본 뒤 설치한다.
//!
//! 늘 켜 두는 앱이라 켤 때 한 번(30초 뒤)과 그 뒤 하루에 한 번 확인한다. 트레이 메뉴로 바로 확인할 수도 있다.
//! 설치 파일은 tauri.conf.json의 공개 키로 서명을 확인한다 — 서명 키는 `~/.tauri/vocatray.key` (PLAN.md Phase 5).
//! Windows에서는 설치 프로그램이 돌기 시작하면 앱이 꺼지고, 설치가 끝나면 설치 프로그램이 다시 켠다.

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// 자동 확인 — 설치판에서만 (개발판이 설치판으로 바뀌어 버리면 안 되므로)
pub fn start(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            check(&app, false);
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// 트레이 메뉴 "업데이트 확인…" — 새 버전이 없거나 확인에 실패해도 알려 준다.
pub fn check_now(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || check(&app, true));
}

fn check(app: &AppHandle, manual: bool) {
    let found = tauri::async_runtime::block_on(async { app.updater()?.check().await });
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            if manual {
                let version = app.package_info().version.to_string();
                message(app, MessageDialogKind::Info, &format!("지금 쓰는 {version}이 최신 버전이에요."));
            }
            return;
        }
        Err(e) => {
            eprintln!("업데이트 확인 실패: {e}");
            if manual {
                message(app, MessageDialogKind::Warning, "업데이트를 확인하지 못했어요. 인터넷 연결을 확인해 주세요.");
            }
            return;
        }
    };
    let asked = app
        .dialog()
        .message(format!(
            "새 버전 {}이 나왔어요 (지금 {}).\n\n지금 업데이트할까요? 잠깐 꺼졌다가 다시 켜지고, 학습 기록은 그대로예요.",
            update.version, update.current_version
        ))
        .title("VocaTray 업데이트")
        .buttons(MessageDialogButtons::OkCancelCustom("지금 업데이트".into(), "나중에".into()))
        .blocking_show();
    if !asked {
        return;
    }
    // 성공하면 설치 프로그램이 앱을 끄므로 여기로 돌아오지 않는다
    if let Err(e) = tauri::async_runtime::block_on(update.download_and_install(|_, _| {}, || {})) {
        eprintln!("업데이트 설치 실패: {e}");
        message(app, MessageDialogKind::Error, "업데이트를 내려받지 못했어요. 나중에 다시 해 볼게요.");
    }
}

fn message(app: &AppHandle, kind: MessageDialogKind, text: &str) {
    app.dialog().message(text).title("VocaTray 업데이트").kind(kind).blocking_show();
}
