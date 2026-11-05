// 외운 단어 창 — 최근에 외운 것부터, 검색, [못 외웠어요]로 되돌리기.
// 탭: 전체 / 진짜 외운 단어(퀴즈 5번 연속 맞힘) / 그냥 외운 단어. 전체에선 진짜 외운 단어를 맨 위에 다른 색으로
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./popup";
import { $, el, type Word } from "./word";

type MemorizedWord = { key: string; at: string; word: Word; quiz: { asked: number; correct: number }; mastered: boolean };
type Tab = "all" | "mastered" | "plain";

const search = $<HTMLInputElement>("search");
const list = $("list");
const empty = $("empty");
const tabs = [...$("tabs").querySelectorAll<HTMLButtonElement>("[data-tab]")];
const TABS: Record<Tab, { label: string; has: (m: MemorizedWord) => boolean; empty: string }> = {
  all: { label: "전체", has: () => true, empty: "아직 외운 단어가 없어요." },
  mastered: { label: "진짜 외운 단어", has: (m) => m.mastered, empty: "퀴즈를 5번 연속 맞힌 단어가 여기에 모여요." },
  plain: { label: "그냥 외운 단어", has: (m) => !m.mastered, empty: "그냥 외운 단어가 없어요. 모두 진짜 외웠어요!" },
};
let words: MemorizedWord[] = [];
// 작은 창은 포커스를 잃을 때마다 닫히므로 고른 탭을 기억해 둔다
let tab: Tab = (["mastered", "plain"] as const).find((t) => t === localStorage.getItem("words-tab")) ?? "all";

const meaning = (w: Word) => w.senses.map((s) => `${s.pos} ${s.ko}`).join(" · ");
const day = (at: string) => new Date(at).toLocaleDateString("ko-KR", { month: "numeric", day: "numeric" });

function render() {
  for (const button of tabs) {
    const t = button.dataset.tab as Tab;
    button.setAttribute("aria-selected", String(t === tab));
    button.replaceChildren(TABS[t].label, el("span", "count", String(words.filter(TABS[t].has).length)));
  }
  const q = search.value.trim().toLowerCase();
  // 전체 탭은 진짜 외운 단어를 맨 위로 — 그 안에서는 최근에 외운 순서 그대로
  const ordered = tab === "all" ? [...words.filter((m) => m.mastered), ...words.filter((m) => !m.mastered)] : words.filter(TABS[tab].has);
  const shown = q ? ordered.filter((m) => m.key.includes(q) || meaning(m.word).includes(q)) : ordered;
  empty.hidden = shown.length > 0;
  empty.textContent = ordered.length === 0 ? TABS[tab].empty : "찾는 단어가 없어요.";
  list.replaceChildren(
    ...shown.map((m) => {
      const head = el("div", "head", "");
      head.append(el("span", "w", m.word.word), el("span", "phon", m.word.phonetic), el("span", "date", day(m.at)));
      // 퀴즈에 나온 적 있는 단어만 — 맞힌 횟수/나온 횟수
      if (m.quiz.asked > 0) head.append(el("span", "stat", `퀴즈 ${m.quiz.correct}/${m.quiz.asked}`));
      const text = el("div", "text", "");
      text.append(head, el("div", "ko", meaning(m.word)));
      const forget = el("button", "btn", "못 외웠어요");
      forget.addEventListener("click", () => invoke("forget_word", { key: m.key }));
      const row = el("li", m.mastered ? "row mastered" : "row", "");
      row.append(text, forget);
      return row;
    }),
  );
}

for (const button of tabs) {
  button.addEventListener("click", () => {
    tab = button.dataset.tab as Tab;
    localStorage.setItem("words-tab", tab);
    render();
  });
}

async function load() {
  words = await invoke<MemorizedWord[]>("memorized_words");
  render();
}

search.addEventListener("input", render);
// 카드·퀴즈·다른 컴퓨터에서 기록이 바뀌면 다시 읽는다
listen("progress", load);
load();
