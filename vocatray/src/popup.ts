// 옵션 "창을 트레이 위에 작게 열기"로 뜬 창 — 제목 표시줄이 없어서 Esc로 닫는다 (포커스를 잃어도 닫힌다)
import { invoke } from "@tauri-apps/api/core";

if (location.hash === "#popup") {
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") invoke("close_popup");
  });
}
