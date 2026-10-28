// 퀴즈 창 — 외운 단어를 섞어 하나씩, 뜻은 눌러야 보인다. [틀렸어요]는 "못 외웠어요"로 되돌린다.
import { invoke } from "@tauri-apps/api/core";
import { $, renderWord, type Word } from "./word";

type MemorizedWord = { key: string; at: string; word: Word };

const question = $("question");
const answer = $("answer");
const reveal = $("reveal");
const grade = $("grade");
const finished = $("finished");
let deck: MemorizedWord[] = [];
let current: MemorizedWord | undefined;
let right = 0;
let wrong = 0;

function shuffle<T>(items: T[]) {
  for (let i = items.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [items[i], items[j]] = [items[j], items[i]];
  }
  return items;
}

function show() {
  current = deck.pop();
  question.hidden = !current;
  answer.hidden = true;
  reveal.hidden = false;
  grade.hidden = true;
  // 다 풀면 결과, 처음부터 외운 단어가 없으면 안내
  finished.hidden = current !== undefined || right + wrong === 0;
  $("empty").hidden = current !== undefined || right + wrong > 0;
  $("finished-title").textContent = `한 바퀴 다 풀었어요! 맞음 ${right} · 틀림 ${wrong}`;
  $("score").hidden = !current;
  $("score").textContent = `맞음 ${right} · 틀림 ${wrong} · 남은 문제 ${deck.length + 1}`;
  if (current) renderWord(current.word);
}

async function start() {
  deck = shuffle(await invoke<MemorizedWord[]>("memorized_words"));
  right = 0;
  wrong = 0;
  show();
}

reveal.addEventListener("click", () => {
  reveal.hidden = true;
  answer.hidden = false;
  grade.hidden = false;
});
$("right").addEventListener("click", () => {
  right++;
  show();
});
$("wrong").addEventListener("click", async () => {
  wrong++;
  if (current) await invoke("forget_word", { key: current.key });
  show();
});
$("again").addEventListener("click", start);
start();
