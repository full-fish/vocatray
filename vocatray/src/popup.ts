// 옵션 "창을 트레이 위에 작게 열기"로 뜬 창 — 제목 표시줄이 없어서 Esc로 닫는다 (포커스를 잃어도 닫힌다)
import { invoke } from "@tauri-apps/api/core";

const popup = location.hash === "#popup";

if (popup) {
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") invoke("close_popup");
  });
}

/** 퀴즈, 통계 작은 창 — 내용을 다 그릴 때마다 부른다. Rust가 창을 내용 높이에 맞춰 보여 준다 (아래가 비지 않게).
 *  처음엔 숨긴 창이라 애니메이션 프레임을 기다리지 않고 바로 잰다 (main.ts와 같은 이유) */
export function fitPopup() {
  if (!popup) return;
  const page = document.querySelector<HTMLElement>(".page")!;
  // 창 높이(100vh)를 잠깐 풀어서 내용만큼의 높이를 잰다
  page.style.height = "auto";
  const height = Math.ceil(page.getBoundingClientRect().height);
  page.style.height = "";
  invoke("fit_popup", { height });
}
