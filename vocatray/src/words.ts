// 외운 단어 창 — 최근에 외운 것부터, 검색, [못 외웠어요]로 되돌리기
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { $, el, type Word } from "./word";

type MemorizedWord = { key: string; at: string; word: Word };

const search = $<HTMLInputElement>("search");
const list = $("list");
const empty = $("empty");
let words: MemorizedWord[] = [];

const meaning = (w: Word) => w.senses.map((s) => `${s.pos} ${s.ko}`).join(" · ");
const day = (at: string) => new Date(at).toLocaleDateString("ko-KR", { month: "numeric", day: "numeric" });

function render() {
  const q = search.value.trim().toLowerCase();
  const shown = q ? words.filter((m) => m.key.includes(q) || meaning(m.word).includes(q)) : words;
  $("count").textContent = `${words.length}개`;
  empty.hidden = shown.length > 0;
  empty.textContent = words.length === 0 ? "아직 외운 단어가 없어요." : "찾는 단어가 없어요.";
  list.replaceChildren(
    ...shown.map((m) => {
      const head = el("div", "head", "");
      head.append(el("span", "w", m.word.word), el("span", "phon", m.word.phonetic), el("span", "date", day(m.at)));
      const text = el("div", "text", "");
      text.append(head, el("div", "ko", meaning(m.word)));
      const forget = el("button", "btn", "못 외웠어요");
      forget.addEventListener("click", () => invoke("forget_word", { key: m.key }));
      const row = el("li", "row", "");
      row.append(text, forget);
      return row;
    }),
  );
}

async function load() {
  words = await invoke<MemorizedWord[]>("memorized_words");
  render();
}

search.addEventListener("input", render);
// 카드·퀴즈·다른 컴퓨터에서 기록이 바뀌면 다시 읽는다
listen("progress", load);
load();
