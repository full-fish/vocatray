import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { $, renderWord, speak, type Word } from "./word";

type CardPayload = {
  card: "hover" | "click";
  seq: number;
  word: Word | null;
  memorized: boolean;
  hasPrevious: boolean;
  bookTitle: string;
  remaining: number;
  total: number;
};

const mode = location.hash === "#click" ? "click" : "hover";
document.body.dataset.mode = mode;

const card = $("card");
const entry = $("entry");
const done = $("done");
const actions = $("actions");
const prev = $<HTMLButtonElement>("prev");
const memorize = $("memorize");
const forget = $("forget");
const next = $("next");
const dontKnow = $("dontknow");
let word: Word | null = null;

function render(p: CardPayload) {
  entry.hidden = p.word === null;
  done.hidden = p.word !== null;
  // 다 외웠어도 바로 전 단어로는 돌아갈 수 있다
  actions.hidden = mode === "hover" || (p.word === null && !p.hasPrevious);
  prev.disabled = !p.hasPrevious;
  // [◀]로 돌아온 단어를 이미 외웠으면 [외웠어요] 자리에 [못 외웠어요]
  memorize.hidden = p.word === null || p.memorized;
  forget.hidden = p.word === null || !p.memorized;
  dontKnow.hidden = p.word === null || p.memorized;
  next.hidden = p.word === null;
  word = p.word;
  $("meta").textContent = `${p.bookTitle} · 남은 단어 ${p.remaining} / ${p.total}`;
  if (p.word) renderWord(p.word);
}

// 내용을 다 그린 뒤 높이를 재서 알려 준다 — Rust가 그 크기로 창을 맞추고 보여 준다.
// 숨긴 창에서는 타이머·애니메이션 프레임이 멈출 수 있어서 기다리지 않고 바로 잰다.
listen<CardPayload>("card", ({ payload }) => {
  if (payload.card !== mode) return;
  render(payload);
  const height = Math.ceil(card.getBoundingClientRect().height);
  invoke("card_ready", { card: mode, seq: payload.seq, height });
});

prev.addEventListener("click", () => invoke("previous_word"));
memorize.addEventListener("click", () => invoke("memorize"));
forget.addEventListener("click", () => invoke("unmemorize"));
dontKnow.addEventListener("click", () => invoke("dont_know"));
next.addEventListener("click", () => invoke("next_word"));
$("speak").addEventListener("click", () => word && speak(word.word));
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") invoke("hide_click");
});
