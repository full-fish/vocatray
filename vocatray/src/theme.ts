// 테마 — 트레이 메뉴 옵션 › 테마에서 고른다 (settings.json, 이 컴퓨터 설정). 색은 styles.css의 [data-theme]이 정한다.
// 빈 값은 기본 (시스템 밝기를 따라감). 새 창이 처음 그릴 때 다른 테마가 비치지 않게 지난 값을 localStorage에서 바로 쓴다
// (모든 창이 같은 localStorage를 쓰고, 카드 창은 늘 떠 있어서 테마를 바꾸면 바로 적어 둔다).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

function apply(theme: string) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("theme", theme);
}

apply(localStorage.getItem("theme") ?? "");
invoke<string>("theme").then(apply);
listen<string>("theme", (e) => apply(e.payload));
